//! The restore drill and the privacy proof of Story 9.27 (release-blocking).
//!
//! One seeded branch with 30 days of activity (players, wallet top-ups, a setting that changes
//! every day), a fork (the Hub was replaced on day 27, its old epoch wrote two more rows) and two
//! erased players (one before the snapshot, one after). It is restored two ways onto a new PC:
//! from a local `.a360` export (`stage_export_checked` + `apply_pending`) and from the cloud
//! backup (`rebuild` + `stage_rebuilt` + `apply_pending`). Both must open a new epoch, hold the
//! source's rows (`state_checksum`) and keep the erased players erased. A scanner then reads
//! every file the drill produced, and the captured log text, for the seeded names, phones and
//! emails. `export_one_gigabyte_benchmark` (arena-store) writes the budget side of the report.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock};

use arena_core::key_backup::{open_key_table, seal_key_table};
use arena_core::players::{KEK_LEN, open_field_of};
use arena_core::{HubStatus, LeaseInfo, ManualClock, Role, SnapshotReader, Writer, status_channel};
use arena_proto::{
    Actor, ActorKind, BranchBootstrap, CloudRestoreFacts, ErrorCode, ExportHeader, Intent,
    IntentPayload, PaymentMethod, PaymentRow, PlayerErase, PlayerForm, RestorePosition,
    SettingsSet, ShiftOpen, Tier, WalletTopup,
};
use arena_store::{
    BatchFork, ChangeRecord, HubIdentity, LATEST_SCHEMA_VERSION, LeaseRow, META_HUB_INSTANCE_ID,
    PendingBatch, PlayerKeyRow, PromoteInput, RowValue, Store, Table, apply_pending,
    build_standby_snapshot, decode_batch, encode_batch, insert_hub_identity, list_player_keys,
    mark_restore_pending, set_meta, stage_export_checked, stage_rebuilt, write_export, write_lease,
};
use rusqlite::Connection;

use crate::tests::T0;
use crate::{EpochFork, MonthBuilder, RebuildSource, apply_replicated, rebuild, state_checksum};

const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const KEK: [u8; KEK_LEN] = [5; KEK_LEN];
/// Name, phone and email of the seeded players. None of it may appear in plaintext anywhere.
const PEOPLE: [(&str, &str, &str); 6] = [
    ("Asha Rao", "+919876543210", "asha.rao@example.com"),
    ("Ravi Menon", "+919811122233", "ravi.menon@example.com"),
    ("Meera Iyer", "+919822233344", "meera.iyer@example.com"),
    ("Kabir Shah", "+919833344455", "kabir.shah@example.com"),
    ("Zoya Khan", "+919844455566", "zoya.khan@example.com"),
    ("Dhruv Patel", "+919855566677", "dhruv.patel@example.com"),
];
/// Who is erased and on which day.
const ERASED: [(usize, i64); 2] = [(1, 8), (3, 26)];
/// The living players, who get wallet top-ups.
const LIVING: [usize; 4] = [0, 2, 4, 5];
const DAYS: i64 = 30;
const SNAPSHOT_DAY: i64 = 20;
/// The Hub is replaced after this day: later changes belong to the child epoch.
const FORK_DAY: i64 = 27;
/// The restore happens a day after the last activity.
const RESTORE_AT: i64 = T0 + (DAYS + 1) * DAY_MS;
const PARENT: &str = "1:01JHUB";
const CHILD: &str = "2:01JNEWHUB";

struct Drill {
    _dir: tempfile::TempDir,
    /// The source Hub's database (closed).
    path: PathBuf,
    branch_id: String,
    ids: Vec<String>,
    /// The checksum of the source's rows at the last change.
    checksum: String,
    log: Vec<ChangeRecord>,
    snapshot: PathBuf,
    snapshot_seq: i64,
    fork_seq: i64,
    /// The key table as the cloud holds it (backed up on day 26, before the second erasure).
    keys: Vec<PlayerKeyRow>,
    /// A standby's snapshot of the main Hub on day 26, before the second erasure.
    standby_snapshot: PathBuf,
    standby_tip: (String, i64),
    export: PathBuf,
    batches: Vec<PathBuf>,
    gst: Vec<u8>,
}

fn owner() -> Actor {
    Actor {
        kind: ActorKind::Owner,
        id: "drill".into(),
    }
}

fn intent(at: i64, payload: IntentPayload, base_rev: Option<i64>) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: payload.kind(),
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: at,
        base_rev,
        actor: if matches!(payload, IntentPayload::BranchBootstrap(_)) {
            Actor {
                kind: ActorKind::System,
                id: "drill-seed".into(),
            }
        } else {
            owner()
        },
        elevated_by: None,
        reason: None,
        payload,
    }
}

fn tip(path: &Path) -> i64 {
    Store::open_reader(path)
        .unwrap()
        .changes()
        .unwrap()
        .last()
        .map_or(0, |c| c.seq)
}

fn epoch_counter(epoch: &str) -> i64 {
    epoch.split(':').next().unwrap().parse().unwrap()
}

fn write_batch(dir: &Path, rows: Vec<ChangeRecord>, fork: Option<BatchFork>) -> PathBuf {
    let (epoch, first, last) = (rows[0].epoch.clone(), rows[0].seq, rows[rows.len() - 1].seq);
    let bytes = encode_batch(
        LATEST_SCHEMA_VERSION,
        &PendingBatch {
            epoch: epoch.clone(),
            first,
            last,
            fork,
            rows,
        },
    )
    .unwrap();
    let path = dir.join(format!(
        "{}-{first:012}-{last:012}.zst",
        epoch.replace(':', "@")
    ));
    fs::write(&path, bytes).unwrap();
    path
}

async fn seed() -> Drill {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source").join("branch.db");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut store = Store::open(&path, T0).unwrap();
    let lease = LeaseRow {
        counter: 1,
        hub_instance_id: "01JHUB".into(),
        provisional: true,
        renewed_at: T0,
        expires_at: i64::MAX,
    };
    store.write_tx(|tx| write_lease(tx, &lease)).unwrap();
    let mut status = HubStatus::opened(store.schema_version().unwrap(), "01JHUB");
    status.role = Role::Primary;
    status.lease = Some(LeaseInfo::from(&lease));
    let (_tx, rx) = status_channel(status);
    let clock = Arc::new(ManualClock::new(T0));
    let writer = Writer::spawn(store, clock.clone(), rx).unwrap();
    let go = |at: i64, payload, base_rev| {
        let writer = writer.clone();
        clock.set(at);
        async move {
            writer
                .submit_intent(intent(at, payload, base_rev))
                .await
                .unwrap()
        }
    };

    go(
        T0,
        IntentPayload::BranchBootstrap(BranchBootstrap {
            name: "Drill Arena".into(),
            branch_code: "DRL01".into(),
            currency: "INR".into(),
            tz: "Asia/Kolkata".into(),
            day_start_min: 360,
            fy_start_month: 4,
            onboarding: None,
        }),
        None,
    )
    .await;
    go(
        T0,
        IntentPayload::ShiftOpen(ShiftOpen { float_minor: 1_000 }),
        None,
    )
    .await;

    let mut ids: Vec<String> = Vec::new();
    let mut snapshot: Option<(PathBuf, i64)> = None;
    let mut keys = Vec::new();
    let mut standby: Option<(PathBuf, (String, i64))> = None;
    let mut fork_seq = 0;
    let out = dir.path().join("artefacts");
    fs::create_dir_all(&out).unwrap();
    for day in 0..DAYS {
        let at = T0 + day * DAY_MS;
        if let Some((i, _)) = PEOPLE.iter().enumerate().find(|(i, _)| *i as i64 == day) {
            let (name, phone, email) = PEOPLE[i];
            let id = ulid::Ulid::generate().to_string();
            let create = arena_core::players::seal(
                &KEK,
                &PlayerForm {
                    username: format!("user{}", i + 1),
                    password: "secret1".into(),
                    phone: phone.into(),
                    name: name.into(),
                    email: email.into(),
                    dob: String::new(),
                    notes: String::new(),
                    tags: Vec::new(),
                    consent: true,
                    policy_version: "1".into(),
                },
                &id,
                "$argon2id$stub".into(),
            )
            .unwrap();
            go(at, IntentPayload::PlayerCreate(create), None).await;
            ids.push(id);
        }
        // The same setting changes every day: 30 days of rows for the log.
        go(
            at,
            IntentPayload::SettingsSet(SettingsSet {
                key: "idle_lock_minutes".into(),
                value: (1 + day).to_string(),
            }),
            Some(1 + day),
        )
        .await;
        if day >= 6 {
            let who = &ids[LIVING[usize::try_from(day).unwrap() % LIVING.len()]];
            let amount = 100 * (day + 1);
            go(
                at,
                IntentPayload::WalletTopup(WalletTopup {
                    player_id: who.clone(),
                    amount_minor: amount,
                    payments: vec![PaymentRow {
                        method: PaymentMethod::Cash,
                        amount_minor: amount,
                        reference: None,
                    }],
                }),
                None,
            )
            .await;
        }
        if day == SNAPSHOT_DAY {
            let file = arena_store::build_snapshot_file(&path, &out, "2027-02-04").unwrap();
            let kept = dir.path().join("snapshot.db.zst");
            fs::copy(file.path(), &kept).unwrap();
            snapshot = Some((kept, file.last_seq));
        }
        for (who, erase_day) in ERASED {
            if day != erase_day {
                continue;
            }
            if who == 3 {
                // On day 26 the key backup and the standby's snapshot still hold the player.
                keys = Store::open_reader(&path)
                    .unwrap()
                    .read_with(list_player_keys)
                    .unwrap();
                let snap = dir.path().join("standby-snapshot.db");
                let at_tip = build_standby_snapshot(&path, &snap).unwrap().unwrap();
                standby = Some((snap, at_tip));
            }
            go(
                at,
                IntentPayload::PlayerErased(PlayerErase {
                    player_id: ids[who].clone(),
                    wallet: None,
                    note: None,
                }),
                None,
            )
            .await;
        }
        if day == FORK_DAY {
            fork_seq = tip(&path);
        }
    }
    writer.close().await;

    let store = Store::open(&path, T0).unwrap();
    let log = store.changes().unwrap();
    assert_eq!(log[0].epoch, PARENT);
    let checksum = state_checksum(&store).unwrap();
    let branch_id = match &store.rows(Table::Branch).unwrap()[0]["branch_id"] {
        RowValue::Text(t) => t.clone(),
        other => panic!("{other:?}"),
    };
    drop(store);

    // The local export, taken at the end.
    let export = write_export(&path, &out, "DRL01-drill.a360", RESTORE_AT - DAY_MS)
        .unwrap()
        .path;

    // The cloud: the snapshot of day 20, then the log after it in batches, the old epoch's two
    // rows written after the fork, and the new epoch carrying on from the fork.
    let (snapshot, snapshot_seq) = snapshot.unwrap();
    let batch_dir = dir.path().join("changes");
    fs::create_dir_all(&batch_dir).unwrap();
    let after: Vec<ChangeRecord> = log
        .iter()
        .filter(|c| c.seq > snapshot_seq)
        .cloned()
        .collect();
    let (parent_rows, child_rows): (Vec<_>, Vec<_>) =
        after.into_iter().partition(|c| c.seq <= fork_seq);
    assert!(!parent_rows.is_empty() && !child_rows.is_empty());
    let mut batches = Vec::new();
    for chunk in parent_rows.chunks(9) {
        batches.push(write_batch(&batch_dir, chunk.to_vec(), None));
    }
    let template = log.iter().rev().find(|c| c.tbl == "settings").unwrap();
    let orphans: Vec<ChangeRecord> = (1..=2)
        .map(|n| {
            let mut row: arena_store::Row = rmp_serde::from_slice(&template.row).unwrap();
            row.insert("value".into(), RowValue::Text(format!("orphaned {n}")));
            ChangeRecord {
                epoch: PARENT.into(),
                seq: fork_seq + n,
                row: rmp_serde::to_vec_named(&row).unwrap(),
                ..template.clone()
            }
        })
        .collect();
    batches.push(write_batch(&batch_dir, orphans, None));
    let fork = BatchFork {
        parent_epoch: PARENT.into(),
        parent_seq: fork_seq,
    };
    for (i, chunk) in child_rows.chunks(9).enumerate() {
        let rows = chunk
            .iter()
            .map(|c| ChangeRecord {
                epoch: CHILD.into(),
                ..c.clone()
            })
            .collect();
        batches.push(write_batch(
            &batch_dir,
            rows,
            (i == 0).then(|| fork.clone()),
        ));
    }

    // The yearly GST archive, as the Hub builds it.
    let gst = SnapshotReader::open(&path)
        .unwrap()
        .gst_archive_file("FY2026-27", RESTORE_AT)
        .unwrap()
        .expect("the branch exists")
        .bytes;

    let (standby_snapshot, standby_tip) = standby.unwrap();
    Drill {
        _dir: dir,
        path,
        branch_id,
        ids,
        checksum,
        log,
        snapshot,
        snapshot_seq,
        fork_seq,
        keys,
        standby_snapshot,
        standby_tip,
        export,
        batches,
        gst,
    }
}

/// A new PC that was paired to replace this branch's Hub and waits in "Restore this branch".
fn new_pc(root: &Path, name: &str, branch_id: &str) -> PathBuf {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("branch.db");
    let mut store = Store::open(&path, T0).unwrap();
    store
        .write_tx(|tx| {
            set_meta(tx, META_HUB_INSTANCE_ID, "01JNEWPC")?;
            insert_hub_identity(
                tx,
                &HubIdentity {
                    tenant_id: "T1".into(),
                    branch_id: branch_id.into(),
                    hub_id: "H2".into(),
                    tier: Tier::Premium,
                    paired_at: T0,
                },
            )?;
            mark_restore_pending(tx, "Drill Arena")
        })
        .unwrap();
    path
}

struct Restored {
    /// The new PC restored from the export.
    from_export: PathBuf,
    export_epoch: String,
    /// The new PC rebuilt from the cloud backup.
    from_cloud: PathBuf,
    cloud_epoch: String,
    /// The rebuilt file the cloud path staged (kept for the scan).
    rebuilt: PathBuf,
    facts: CloudRestoreFacts,
}

/// Restores `d` onto two new PCs. The cloud key backup is the one of day 26.
fn restore_both(d: &Drill, root: &Path) -> Restored {
    // From a local export. The cloud key backup has every living key and the second erased
    // player's was still in it on day 26, but the file was made after that erasure.
    let pc = new_pc(root, "pc-export", &d.branch_id);
    let live: Vec<String> = d.keys.iter().map(|k| k.player_id.clone()).collect();
    stage_export_checked(&pc, &d.export, "owner", Some(live)).unwrap();
    let out = apply_pending(&pc, RESTORE_AT).unwrap().unwrap();
    let export_epoch = out.restore.new_epoch.clone();

    // From the cloud backup, the way the Hub's Rebuilder does it: the sealed key backup is opened
    // with the branch key and the staged files are replayed into a new database.
    let sealed = seal_key_table(&KEK, &d.keys).unwrap();
    let keys = open_key_table(&KEK, &sealed).unwrap();
    let rebuilt = root.join("rebuilt.db");
    let done = rebuild(
        &rebuilt,
        &RebuildSource {
            snapshot: Some(&d.snapshot),
            batches: &d.batches,
            keys: &keys,
        },
        RESTORE_AT,
        &AtomicBool::new(false),
        &mut |_, _, _| {},
    )
    .map_err(|e| format!("{e:?}"))
    .unwrap();
    assert_eq!(done.checksum, d.checksum, "the rebuild matches the source");
    let pos = |r: &Option<crate::Resume>| {
        r.as_ref().map(|r| RestorePosition {
            epoch: r.epoch.clone(),
            seq: r.seq,
        })
    };
    let facts = CloudRestoreFacts {
        snapshot_date: Some("2027-02-04".into()),
        first: pos(&done.first_applied),
        last: pos(&done.last),
        batches: done.batches,
        applied: done.applied,
        orphaned: done.orphaned,
        keys_restored: done.keys_restored,
    };
    let pc = new_pc(root, "pc-cloud", &d.branch_id);
    stage_rebuilt(&pc, &rebuilt, facts.clone(), RESTORE_AT).unwrap();
    let out = apply_pending(&pc, RESTORE_AT).unwrap().unwrap();
    assert_eq!(out.restore.cloud, Some(facts.clone()));
    Restored {
        from_export: root.join("pc-export").join("branch.db"),
        export_epoch,
        from_cloud: pc,
        cloud_epoch: out.restore.new_epoch,
        rebuilt,
        facts,
    }
}

fn count(path: &Path, sql: &str) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row(sql, [], |r| r.get(0))
        .unwrap()
}

/// The restored database holds the source's rows, a new epoch above `above`, and the erased
/// players have no key, a stamp and nothing readable.
fn check_restored(d: &Drill, db: &Path, new_epoch: &str, above: i64) {
    let store = Store::open(db, RESTORE_AT + 1000).unwrap();
    assert_eq!(
        state_checksum(&store).unwrap(),
        d.checksum,
        "{}: rows differ from the source",
        db.display()
    );
    let lease = store.lease().unwrap().unwrap();
    assert!(lease.counter > above, "the new epoch is above {above}");
    assert!(lease.provisional);
    assert_eq!(
        format!("{}:{}", lease.counter, lease.hub_instance_id),
        new_epoch
    );
    assert!(store.restore_pending().unwrap().is_none());
    drop(store);

    for (who, _) in ERASED {
        let id = &d.ids[who];
        assert_eq!(
            count(
                db,
                &format!("SELECT COUNT(*) FROM player_keys WHERE player_id = '{id}'")
            ),
            0,
            "{id} still has a key"
        );
        let row: (String, String, Option<String>, String, String, Option<i64>) =
            Connection::open(db)
                .unwrap()
                .query_row(
                    "SELECT name_enc, phone_enc, email_enc, phone_lookup, username, erased_at
                       FROM players WHERE player_id = ?1",
                    [id],
                    |r| {
                        Ok((
                            r.get(0)?,
                            r.get(1)?,
                            r.get(2)?,
                            r.get(3)?,
                            r.get(4)?,
                            r.get(5)?,
                        ))
                    },
                )
                .unwrap();
        assert_eq!(row.0, "");
        assert_eq!(row.1, "");
        assert_eq!(row.2, None);
        assert_eq!(row.3, "");
        assert_eq!(row.4, "erased");
        assert!(row.5.is_some(), "{id} has no erased_at");
        assert_eq!(
            open_field_of(&KEK, id, None, "name", "").unwrap_err().code,
            ErrorCode::KeyErased
        );
    }
    // The living players are still readable with the branch key.
    for who in LIVING {
        let id = &d.ids[who];
        let (wrapped, name_enc): (String, String) = Connection::open(db)
            .unwrap()
            .query_row(
                "SELECT k.dek_wrapped, p.name_enc FROM players p
                   JOIN player_keys k ON k.player_id = p.player_id WHERE p.player_id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            arena_core::players::open_name(&KEK, id, &wrapped, &name_enc).unwrap(),
            PEOPLE[who].0
        );
    }
    assert_eq!(count(db, "SELECT COUNT(*) FROM player_keys"), 4);
}

#[tokio::test]
async fn the_branch_restores_from_an_export_and_from_the_cloud_with_the_same_rows_and_erased_players_erased()
 {
    let d = seed().await;
    assert!(
        d.log.len() > 60,
        "30 days of activity: {} changes",
        d.log.len()
    );
    let root = tempfile::tempdir().unwrap();
    let r = restore_both(&d, root.path());

    // Both reach a new epoch: the export above the source's epoch 1, the cloud above the child 2.
    check_restored(&d, &r.from_export, &r.export_epoch, 1);
    check_restored(&d, &r.from_cloud, &r.cloud_epoch, 2);

    // The cloud path replayed through the fork: the old epoch's two late rows are set aside.
    assert_eq!(r.facts.orphaned, 2);
    assert_eq!(
        r.facts.keys_restored, 4,
        "the second erased player's key is dropped"
    );
    assert_eq!(
        r.facts.last,
        Some(RestorePosition {
            epoch: CHILD.into(),
            seq: d.log.last().unwrap().seq
        })
    );
    assert_eq!(r.facts.snapshot_date.as_deref(), Some("2027-02-04"));
    assert!(d.fork_seq > d.snapshot_seq);
    assert_eq!(epoch_counter(&r.cloud_epoch), 3);
    assert_eq!(epoch_counter(&r.export_epoch), 2);

    let erased = ERASED.len();
    let restore = |from: &str, epoch: &str, orphaned: i64| {
        serde_json::json!({
            "from": from,
            "new_epoch": epoch,
            "epoch_above_source": true,
            "rows_equal": true,
            "erased_players": erased,
            "erased_without_key": erased,
            "erased_pii_empty": erased,
            "living_readable": LIVING.len(),
            "orphaned": orphaned,
        })
    };
    record(
        "restore.json",
        &serde_json::json!({
            "days": DAYS,
            "changes": d.log.len(),
            "fork": true,
            "restores": [
                restore("export", &r.export_epoch, 0),
                restore("cloud", &r.cloud_epoch, r.facts.orphaned),
            ],
        }),
    );
}

#[tokio::test]
async fn a_player_erased_after_the_export_is_erased_before_the_writer_opens_when_the_cloud_key_backup_is_known()
 {
    let d = seed().await;
    let root = tempfile::tempdir().unwrap();
    // Erased after the export: the file holds Meera's key, the cloud key backup does not.
    let meera = d.ids[2].clone();
    let live: Vec<String> = d
        .keys
        .iter()
        .map(|k| k.player_id.clone())
        .filter(|id| *id != meera)
        .collect();
    let pc = new_pc(root.path(), "pc-late", &d.branch_id);
    stage_export_checked(&pc, &d.export, "owner", Some(live)).unwrap();
    let out = apply_pending(&pc, RESTORE_AT).unwrap().unwrap();
    assert_eq!(out.restore.erasures_reapplied, 1);
    assert_eq!(
        count(
            &pc,
            &format!("SELECT COUNT(*) FROM player_keys WHERE player_id = '{meera}'")
        ),
        0
    );
    assert!(
        count(
            &pc,
            &format!("SELECT erased_at FROM players WHERE player_id = '{meera}'")
        ) > 0
    );
    assert_eq!(count(&pc, "SELECT COUNT(*) FROM player_keys"), 3);
    // The players erased before the export stay erased and nobody else is touched.
    assert_eq!(
        count(
            &pc,
            "SELECT COUNT(*) FROM players WHERE erased_at IS NOT NULL"
        ),
        3
    );

    // Offline, on Free or with an unreadable backup: the restore is as before.
    let pc = new_pc(root.path(), "pc-offline", &d.branch_id);
    stage_export_checked(&pc, &d.export, "owner", None).unwrap();
    let out = apply_pending(&pc, RESTORE_AT).unwrap().unwrap();
    let offline_reapplied = out.restore.erasures_reapplied;
    assert_eq!(offline_reapplied, 0);
    assert_eq!(count(&pc, "SELECT COUNT(*) FROM player_keys"), 4);
    record(
        "late-erasure.json",
        &serde_json::json!({
            "erasures_reapplied": 1,
            "keys_left": 3,
            "offline_erasures_reapplied": offline_reapplied,
            "offline_keys_left": 4,
        }),
    );
}

#[tokio::test]
async fn an_erased_player_has_no_key_on_the_standby_once_it_is_promoted() {
    let d = seed().await;
    let root = tempfile::tempdir().unwrap();
    // The standby took its snapshot on day 26, before the erasure, so it holds the key.
    let mut standby = Store::open(root.path().join("standby.db"), T0).unwrap();
    standby.load_standby_snapshot(&d.standby_snapshot).unwrap();
    let victim = d.ids[3].clone();
    assert_eq!(
        standby
            .replication_key_states(std::slice::from_ref(&victim))
            .unwrap()[0]
            .1
            .as_deref()
            .map(|_| true),
        Some(true)
    );
    // It follows the rows, including the erasure; the message that removes the key is lost.
    let (epoch, seq) = d.standby_tip.clone();
    let rest: Vec<ChangeRecord> = d
        .log
        .iter()
        .filter(|c| c.seq > seq && c.seq <= d.fork_seq)
        .cloned()
        .collect();
    assert!(rest.iter().any(|c| c.intent_kind == "player.erased"));
    let _ = epoch;
    standby
        .write_tx_mut(|tx| apply_replicated(rest, &[], tx))
        .unwrap();
    assert_eq!(
        count(
            standby.path(),
            &format!("SELECT COUNT(*) FROM player_keys WHERE player_id = '{victim}'")
        ),
        1,
        "the key was left behind"
    );
    standby
        .write_tx(|tx| set_meta(tx, META_HUB_INSTANCE_ID, "01JSTBY"))
        .unwrap();

    // The standby becomes main.
    let outcome = standby
        .promote_to_main(&PromoteInput {
            now_ms: RESTORE_AT,
            lease_ttl_ms: 10_000,
            claimed_epoch: None,
        })
        .unwrap();
    assert_eq!(outcome.erasures_reapplied, 2, "both erased players");
    let db = standby.path().to_path_buf();
    for (who, _) in ERASED {
        assert_eq!(
            count(
                &db,
                &format!(
                    "SELECT COUNT(*) FROM player_keys WHERE player_id = '{}'",
                    d.ids[who]
                )
            ),
            0
        );
    }
    assert_eq!(count(&db, "SELECT COUNT(*) FROM player_keys"), 4);
    assert!(outcome.counter > epoch_counter(&d.standby_tip.0));
    record(
        "promote.json",
        &serde_json::json!({
            "erased_players": ERASED.len(),
            "erased_without_key": ERASED.len(),
            "erasures_reapplied": outcome.erasures_reapplied,
            "stale_key_before_promote": true,
        }),
    );
}

// ---- the privacy scan ----

/// One thing the drill produced, with its bytes as a person could read them (compressed files
/// are inflated; databases are read as they lie on disk).
struct Artefact {
    name: String,
    bytes: Vec<u8>,
}

fn raw(name: &str, path: &Path) -> Artefact {
    Artefact {
        name: name.to_owned(),
        bytes: fs::read(path).unwrap(),
    }
}

fn inflated(name: &str, bytes: &[u8]) -> Artefact {
    Artefact {
        name: name.to_owned(),
        bytes: zstd::decode_all(bytes).unwrap(),
    }
}

/// The seeded names, phones and emails (a phone also without its country code).
fn needles() -> Vec<String> {
    PEOPLE
        .iter()
        .flat_map(|(name, phone, email)| {
            [
                (*name).to_owned(),
                (*phone).to_owned(),
                phone.trim_start_matches("+91").to_owned(),
                (*email).to_owned(),
            ]
        })
        .collect()
}

/// `"<artefact>: <needle>"` for every seeded value found in plaintext in an artefact or the log.
fn scan(artefacts: &[Artefact], log: &str) -> Vec<String> {
    let mut found = Vec::new();
    let needles = needles();
    for a in artefacts {
        for n in &needles {
            if a.bytes.windows(n.len()).any(|w| w == n.as_bytes()) {
                found.push(format!("{}: {n}", a.name));
            }
        }
    }
    for n in &needles {
        if log.contains(n.as_str()) {
            found.push(format!("log: {n}"));
        }
    }
    found
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Every log line of every test in this process, from one global subscriber installed once.
fn captured_log() -> Capture {
    static LOG: OnceLock<Capture> = OnceLock::new();
    let log = LOG.get_or_init(Capture::default);
    let _ = tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_writer(log.clone())
            .with_max_level(tracing::Level::TRACE)
            .finish(),
    );
    log.clone()
}

/// Every artefact the drill produced.
fn artefacts(d: &Drill, r: &Restored) -> Vec<Artefact> {
    let mut all = vec![
        raw("source database", &d.path),
        inflated("snapshot", &fs::read(&d.snapshot).unwrap()),
        raw("key backup", &{
            let sealed = seal_key_table(&KEK, &d.keys).unwrap();
            let p = d._dir.path().join("player-keys.bin");
            fs::write(&p, sealed).unwrap();
            p
        }),
        inflated("GST archive", &d.gst),
        raw("rebuilt database", &r.rebuilt),
        raw("database restored from the export", &r.from_export),
        raw("database restored from the cloud", &r.from_cloud),
    ];
    let wal = d.path.with_file_name("branch.db-wal");
    if fs::metadata(&wal).is_ok_and(|m| m.len() > 0) {
        all.push(raw("source WAL", &wal));
    }
    // The export: header as written, then the body inflated.
    let bytes = fs::read(&d.export).unwrap();
    let (_, at) = ExportHeader::decode(&bytes).unwrap();
    all.push(Artefact {
        name: "export header".into(),
        bytes: bytes[..at].to_vec(),
    });
    all.push(inflated("export body", &bytes[at..]));
    for b in &d.batches {
        all.push(inflated(
            &format!("batch {}", b.file_name().unwrap().to_string_lossy()),
            &fs::read(b).unwrap(),
        ));
    }
    // The audit cache, built by the audit builder's own code from the snapshot and the batches.
    let scratch = tempfile::tempdir().unwrap();
    let forks = vec![EpochFork {
        epoch: CHILD.into(),
        parent_epoch: PARENT.into(),
        parent_seq: d.fork_seq,
    }];
    let mut month = MonthBuilder::open(
        scratch.path(),
        "2027-03",
        Some(&fs::read(&d.snapshot).unwrap()),
        forks,
        T0,
    )
    .unwrap();
    for b in &d.batches {
        month
            .apply(&decode_batch(&fs::read(b).unwrap()).unwrap())
            .unwrap();
    }
    let built = month.finish().unwrap();
    assert_eq!(
        built.checksum, d.checksum,
        "the audit cache has the same rows"
    );
    assert_eq!(built.orphaned, 2);
    all.push(inflated("audit cache", &built.bytes));
    all
}

#[tokio::test]
async fn no_seeded_name_phone_or_email_is_in_plaintext_in_any_artefact_or_the_log() {
    let log = captured_log();
    let d = seed().await;
    let root = tempfile::tempdir().unwrap();
    let r = restore_both(&d, root.path());
    for id in &d.ids {
        tracing::info!(player_id = %id, "drill: player restored");
    }
    let all = artefacts(&d, &r);
    let logged = String::from_utf8_lossy(&log.0.lock().unwrap()).into_owned();
    assert!(
        all.iter().all(|a| !a.bytes.is_empty()),
        "every artefact was filled: {:?}",
        all.iter()
            .filter(|a| a.bytes.is_empty())
            .map(|a| &a.name)
            .collect::<Vec<_>>()
    );
    assert!(logged.contains(&d.ids[0]), "the log carries player ids");
    assert_eq!(scan(&all, &logged), Vec::<String>::new());

    let mut kinds: Vec<&str> = all.iter().map(|a| kind_of(&a.name)).collect();
    kinds.sort_unstable();
    kinds.dedup();
    record(
        "privacy.json",
        &serde_json::json!({
            "artefacts": all.len(),
            "kinds": kinds,
            "seeded_values": needles().len(),
            "findings": 0,
            "log_bytes": logged.len(),
            "log_player_ids": d.ids.iter().filter(|id| logged.contains(id.as_str())).count(),
        }),
    );

    // The scanner itself works: it names the file a seeded phone appears in.
    let mut leaked = all;
    let batch = leaked
        .iter_mut()
        .find(|a| a.name.starts_with("batch "))
        .unwrap();
    batch.bytes.extend_from_slice(PEOPLE[2].1.as_bytes());
    let name = batch.name.clone();
    assert_eq!(
        scan(&leaked, &logged),
        vec![format!("{name}: {}", PEOPLE[2].1)]
            .into_iter()
            .chain([format!("{name}: {}", PEOPLE[2].1.trim_start_matches("+91"))])
            .collect::<Vec<_>>()
    );
    let leaky_log = format!("{logged}\nsigned in {}", PEOPLE[0].2);
    assert_eq!(scan(&[], &leaky_log), vec![format!("log: {}", PEOPLE[0].2)]);
}

// ---- the result files of the release report ----

/// Writes `value` as `<ARENA_RESTORE_DRILL_OUT>/<file>` when the variable is set (the release job
/// sets it); `scripts/restore-drill-report.mjs` reads the files and fails the release without
/// one. Only called after the test's assertions held.
fn record(file: &str, value: &serde_json::Value) {
    let Some(dir) = std::env::var_os("ARENA_RESTORE_DRILL_OUT").filter(|v| !v.is_empty()) else {
        return;
    };
    let dir = PathBuf::from(dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(file), serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

/// What kind of artefact a scanned file is, for the report.
fn kind_of(name: &str) -> &'static str {
    if name.starts_with("batch ") {
        "batch"
    } else if name == "snapshot" {
        "snapshot"
    } else if name == "key backup" {
        "key backup"
    } else if name == "GST archive" {
        "GST archive"
    } else if name == "audit cache" {
        "audit cache"
    } else if name.starts_with("export ") {
        "export"
    } else {
        "database"
    }
}
