//! The restore drill of Story 9.13: a Hub with a snapshot, batches across days, a fork and an
//! erased player, rebuilt on a fresh Hub.

use std::sync::atomic::AtomicBool;

use arena_store::{
    BatchFork, ChangeOp, ChangeRecord, PendingBatch, PlayerKeyRow, Row, RowKey, RowValue, Store,
    Table, encode_batch,
};

use super::*;
use crate::tests::{T0, seeded_with_device};

const SCHEMA: u32 = 84;

struct Backup {
    _dir: tempfile::TempDir,
    snapshot: PathBuf,
    batches: Vec<PathBuf>,
    keys: Vec<PlayerKeyRow>,
    /// The checksum of the source's rows at the last uploaded change.
    expected_checksum: String,
    /// The snapshot's last `seq`.
    snapshot_seq: i64,
    parent: String,
    child: String,
    work: PathBuf,
}

fn text(s: &str) -> RowValue {
    RowValue::Text(s.to_owned())
}

/// A player row of the schema, read back from a scratch database.
fn player_row(id: &str) -> Row {
    let dir = tempfile::tempdir().unwrap();
    let mut scratch = Store::open(dir.path().join("scratch.db"), T0).unwrap();
    scratch
        .write_tx(|tx| {
            tx.execute(
                "INSERT INTO players (player_id, name_enc, phone_enc, username, password_hash,
                                      phone_lookup, tags, rev, created_at, updated_at)
                 VALUES (?1, 'enc-name', 'enc-phone', 'user-' || lower(?1), 'hash', ?1, '[]',
                         1, 0, 0)",
                [id],
            )
            .map_err(arena_store::db_error)?;
            Ok(())
        })
        .unwrap();
    scratch.rows(Table::Players).unwrap().remove(0)
}

fn erased(mut row: Row) -> Row {
    for col in ["name_enc", "phone_enc", "password_hash", "phone_lookup"] {
        row.insert(col.into(), text(""));
    }
    row.insert("username".into(), text("erased"));
    row.insert("erased_at".into(), RowValue::Integer(T0));
    row.insert("rev".into(), RowValue::Integer(2));
    row
}

fn change(
    template: &ChangeRecord,
    epoch: &str,
    seq: i64,
    table: Table,
    op: ChangeOp,
    row: &Row,
) -> ChangeRecord {
    ChangeRecord {
        epoch: epoch.to_owned(),
        seq,
        tbl: table.name().to_owned(),
        row_id: RowKey::from_row(table, row).unwrap().row_id(),
        op,
        row: rmp_serde::to_vec_named(row).unwrap(),
        intent_kind: "test".to_owned(),
        ..template.clone()
    }
}

fn write_batch(dir: &Path, rows: Vec<ChangeRecord>, fork: Option<BatchFork>) -> PathBuf {
    let (epoch, first, last) = (rows[0].epoch.clone(), rows[0].seq, rows[rows.len() - 1].seq);
    let bytes = encode_batch(
        SCHEMA,
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

/// The Hub: seeded, snapshotted; then P1..P3 join, a setting changes, P2 is erased, the Hub is
/// replaced at that point and the new epoch carries on, while the old one wrote two more rows.
async fn backup() -> Backup {
    let (a, source) = seeded_with_device().await;
    let log = source.changes().unwrap();
    let template = log.last().unwrap().clone();
    let parent = template.epoch.clone();
    let child = "9:01JNEWHUB".to_owned();
    let snapshot_seq = template.seq;

    let dir = tempfile::tempdir().unwrap();
    let snap = arena_store::build_snapshot_file(source.path(), a.path(), "2027-01-15").unwrap();
    assert_eq!(snap.last_seq, snapshot_seq);
    let snapshot = dir.path().join("snapshot.db.zst");
    fs::copy(snap.path(), &snapshot).unwrap();

    let setting = |value: &str| -> Row {
        let mut row = source
            .rows(Table::Settings)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        row.insert("value".into(), text(value));
        row
    };
    let (p1, p2, p3) = (player_row("P1"), player_row("P2"), player_row("P3"));
    let n = snapshot_seq;
    let (ins, upd) = (ChangeOp::Insert, ChangeOp::Update);
    let day1 = vec![
        change(&template, &parent, n + 1, Table::Players, ins, &p1),
        change(&template, &parent, n + 2, Table::Players, ins, &p2),
        change(&template, &parent, n + 3, Table::Players, ins, &p3),
    ];
    let day2 = vec![
        change(
            &template,
            &parent,
            n + 4,
            Table::Settings,
            upd,
            &setting("one"),
        ),
        change(
            &template,
            &parent,
            n + 5,
            Table::Players,
            upd,
            &erased(p2.clone()),
        ),
        change(
            &template,
            &parent,
            n + 6,
            Table::Settings,
            upd,
            &setting("two"),
        ),
    ];
    // Written by the replaced Hub after the fork point, and uploaded: never part of the state.
    let orphans = vec![
        change(
            &template,
            &parent,
            n + 7,
            Table::Settings,
            upd,
            &setting("orphaned"),
        ),
        change(
            &template,
            &parent,
            n + 8,
            Table::Settings,
            upd,
            &setting("orphaned too"),
        ),
    ];
    let day3 = vec![
        change(
            &template,
            &child,
            n + 7,
            Table::Settings,
            upd,
            &setting("three"),
        ),
        change(
            &template,
            &child,
            n + 8,
            Table::Settings,
            upd,
            &setting("four"),
        ),
    ];

    let kept = dir.path().join("changes");
    fs::create_dir_all(&kept).unwrap();
    let fork = BatchFork {
        parent_epoch: parent.clone(),
        parent_seq: n + 6,
    };
    let batches = vec![
        write_batch(&kept, day1.clone(), None),
        write_batch(&kept, day2.clone(), None),
        write_batch(&kept, orphans, None),
        write_batch(&kept, day3.clone(), Some(fork)),
    ];

    // The oracle: the whole log, minus what the fork left out, replayed into an empty database.
    let mut all = log;
    all.extend(day1);
    all.extend(day2);
    all.extend(day3);
    let oracle_dir = tempfile::tempdir().unwrap();
    let mut oracle = Store::open(oracle_dir.path().join("oracle.db"), T0).unwrap();
    oracle
        .write_tx_mut(|tx| {
            let forks = [EpochFork {
                epoch: child.clone(),
                parent_epoch: parent.clone(),
                parent_seq: n + 6,
            }];
            replay_history(all, &forks, None, tx)?;
            Ok(())
        })
        .unwrap();
    let expected_checksum = state_checksum(&oracle).unwrap();

    let keys = ["P1", "P2", "P3", "GHOST"]
        .iter()
        .map(|id| PlayerKeyRow {
            player_id: (*id).to_owned(),
            dek_wrapped: format!("wrapped-{id}"),
        })
        .collect();
    let work = dir.path().join("rebuild.db");
    Backup {
        _dir: dir,
        snapshot,
        batches,
        keys,
        expected_checksum,
        snapshot_seq,
        parent,
        child,
        work,
    }
}

fn run_rebuild(b: &Backup, cancel: &AtomicBool) -> Result<Rebuilt, RebuildStop> {
    rebuild(
        &b.work,
        &RebuildSource {
            snapshot: Some(&b.snapshot),
            batches: &b.batches,
            keys: &b.keys,
        },
        T0,
        cancel,
        &mut |_, _, _| {},
    )
}

fn count(path: &Path, sql: &str) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row(sql, [], |r| r.get(0))
        .unwrap()
}

#[tokio::test]
async fn the_drill_rebuilds_the_hubs_rows_keeps_the_erasure_and_leaves_the_orphans_out() {
    let b = backup().await;
    let done = run_rebuild(&b, &AtomicBool::new(false)).unwrap();
    let n = b.snapshot_seq;

    assert_eq!(done.batches, 4);
    assert_eq!(
        done.applied, 8,
        "3 + 3 + 2; the two orphans are not applied"
    );
    assert_eq!(done.orphaned, 2);
    assert_eq!(
        done.last,
        Some(Resume {
            epoch: b.child.clone(),
            seq: n + 8
        })
    );
    assert_eq!(
        done.first_applied,
        Some(Resume {
            epoch: b.parent.clone(),
            seq: n + 1
        })
    );

    // The branch rows equal the source's at the last uploaded seq, orphans excluded.
    let store = Store::open(&b.work, T0).unwrap();
    assert_eq!(state_checksum(&store).unwrap(), b.expected_checksum);
    assert_eq!(done.checksum, b.expected_checksum);
    let settings = store.rows(Table::Settings).unwrap();
    assert!(
        settings.iter().all(
            |r| !matches!(r.get("value"), Some(RowValue::Text(v)) if v.starts_with("orphaned"))
        ),
        "an orphaned row reached the state"
    );
    drop(store);

    // The log is whole and the orphans are listed apart.
    assert_eq!(count(&b.work, "SELECT COUNT(*) FROM orphaned_changes"), 2);
    assert_eq!(
        count(
            &b.work,
            &format!("SELECT MAX(seq) FROM changes WHERE epoch = '{}'", b.child)
        ),
        n + 8
    );

    // The erased player stays erased: their key is not restored; neither is a key for no one.
    assert_eq!(done.keys_restored, 2);
    assert_eq!(done.keys_dropped, 2);
    let keys: Vec<String> = Connection::open(&b.work)
        .unwrap()
        .prepare("SELECT player_id FROM player_keys ORDER BY player_id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(keys, ["P1", "P3"]);
    assert_eq!(
        count(
            &b.work,
            "SELECT erased_at FROM players WHERE player_id = 'P2'"
        ),
        T0
    );

    // Uploading continues after what was replayed; the paired device is listed to pair again.
    assert_eq!(
        count(&b.work, "SELECT last_seq_uploaded FROM upload_cursor"),
        n + 8
    );
    assert!(done.pair_again >= 1);
    assert_eq!(
        done.pair_again,
        count(&b.work, &format!("SELECT COUNT(*) FROM {PAIR_AGAIN_TABLE}"))
    );
    assert!(
        count(
            &b.work,
            &format!("SELECT COUNT(*) FROM {PAIR_AGAIN_TABLE} WHERE tbl = 'devices'")
        ) >= 1
    );
    assert!(!PathBuf::from(format!("{}-wal", b.work.display())).exists());
}

#[tokio::test]
async fn a_cancel_leaves_no_database_behind() {
    let b = backup().await;
    let cancel = AtomicBool::new(true);
    assert!(matches!(
        run_rebuild(&b, &cancel),
        Err(RebuildStop::Cancelled)
    ));
    assert!(!b.work.exists());

    // Cancelled part-way through the batches.
    let cancel = AtomicBool::new(false);
    let out = rebuild(
        &b.work,
        &RebuildSource {
            snapshot: Some(&b.snapshot),
            batches: &b.batches,
            keys: &b.keys,
        },
        T0,
        &cancel,
        &mut |stage, done, _| {
            if stage == RebuildStage::Replaying && done == 2 {
                cancel.store(true, Ordering::SeqCst);
            }
        },
    );
    assert!(matches!(out, Err(RebuildStop::Cancelled)));
    assert!(!b.work.exists());
}

#[tokio::test]
async fn a_missing_batch_and_a_damaged_one_stop_the_rebuild() {
    let mut b = backup().await;
    let hole = b.batches.remove(0);
    let Err(RebuildStop::Failed(e)) = run_rebuild(&b, &AtomicBool::new(false)) else {
        panic!("a hole must fail");
    };
    assert!(e.message.contains("seq gap"), "{}", e.message);
    assert!(!b.work.exists());

    b.batches.insert(0, hole.clone());
    let mut bytes = fs::read(&hole).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    fs::write(&hole, bytes).unwrap();
    let Err(RebuildStop::Failed(e)) = run_rebuild(&b, &AtomicBool::new(false)) else {
        panic!("a damaged batch must fail");
    };
    assert_eq!(e.code, ErrorCode::RestoreCorrupt);
}

#[tokio::test]
async fn progress_counts_batches_and_then_the_checks() {
    let b = backup().await;
    let mut seen: Vec<(RebuildStage, u32, u32)> = Vec::new();
    rebuild(
        &b.work,
        &RebuildSource {
            snapshot: Some(&b.snapshot),
            batches: &b.batches,
            keys: &b.keys,
        },
        T0,
        &AtomicBool::new(false),
        &mut |s, d, t| seen.push((s, d, t)),
    )
    .unwrap();
    assert_eq!(seen.first(), Some(&(RebuildStage::Replaying, 0, 4)));
    assert!(seen.contains(&(RebuildStage::Replaying, 4, 4)));
    let last = seen.last().unwrap();
    assert_eq!(last.0, RebuildStage::Checking);
    assert_eq!(last.1, last.2, "the checks end complete");
}

#[tokio::test]
async fn a_backup_without_a_snapshot_replays_from_the_first_change() {
    let (_a, source) = seeded_with_device().await;
    let log = source.changes().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut batches = Vec::new();
    for chunk in log.chunks(3) {
        batches.push(write_batch(dir.path(), chunk.to_vec(), None));
    }
    let work = dir.path().join("rebuild.db");
    let done = rebuild(
        &work,
        &RebuildSource {
            snapshot: None,
            batches: &batches,
            keys: &[],
        },
        T0,
        &AtomicBool::new(false),
        &mut |_, _, _| {},
    )
    .unwrap();
    assert_eq!(done.applied as usize, log.len());
    assert_eq!(done.checksum, state_checksum(&source).unwrap());
}
