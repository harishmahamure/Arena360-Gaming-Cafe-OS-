use super::*;
use std::collections::{HashMap, HashSet};

use arena_core::status_channel;
use arena_proto::RestorePlanSnapshot;
use arena_store::{ChangeOp, ChangeRecord, PendingBatch, Store, encode_batch, insert_change};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::cloud::CloudEndpoint;

const T0: i64 = 1_800_000_000_000;

#[derive(Default)]
struct Mem(Mutex<Option<Vec<u8>>>);

impl Keystore for Mem {
    fn put(&self, secret: &[u8]) -> std::io::Result<()> {
        *self.0.lock().unwrap() = Some(secret.to_vec());
        Ok(())
    }
    fn get(&self) -> std::io::Result<Option<Vec<u8>>> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn delete(&self) -> std::io::Result<()> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

fn change(epoch: &str, seq: i64) -> ChangeRecord {
    ChangeRecord {
        epoch: epoch.into(),
        seq,
        tbl: "settings".into(),
        row_id: format!("k{seq}"),
        op: ChangeOp::Insert,
        row: vec![0x80, seq as u8, 0xc4, 0x01],
        intent_id: format!("I{seq}"),
        intent_kind: "settings.set".into(),
        actor_kind: "staff".into(),
        actor_id: "S1".into(),
        elevated_by_kind: None,
        elevated_by_id: None,
        elevated_by_mode: None,
        elevated_by_verifier_version: None,
        at: T0 + seq,
        business_day: None,
        fy: None,
        reason: None,
    }
}

fn batch_bytes(epoch: &str, first: i64, last: i64) -> Vec<u8> {
    encode_batch(
        85,
        &PendingBatch {
            epoch: epoch.into(),
            first,
            last,
            fork: None,
            rows: (first..=last).map(|s| change(epoch, s)).collect(),
        },
    )
    .unwrap()
}

/// A packed snapshot whose last change is `(epoch, last)`.
fn snapshot_bytes(epoch: &str, last: i64) -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("branch.db");
    let mut store = Store::open(&path, T0).unwrap();
    store
        .write_tx(|tx| {
            for s in 1..=last {
                insert_change(tx, &change(epoch, s))?;
            }
            Ok(())
        })
        .unwrap();
    let out = tempfile::tempdir().unwrap();
    let file = arena_store::build_snapshot_file(&path, out.path(), "2026-10-29").unwrap();
    file.read().unwrap()
}

/// What the fake cloud and bucket hold and what they were asked.
#[derive(Default)]
struct World {
    snapshot: Mutex<Option<(String, String)>>,
    keys: Mutex<Option<String>>,
    /// `(epoch, first, last, object name)` as the plan lists them.
    batches: Mutex<Vec<(String, i64, i64, String)>>,
    objects: Mutex<HashMap<String, Vec<u8>>>,
    gets: Mutex<Vec<String>>,
    plans: Mutex<Vec<serde_json::Value>>,
    /// Answer the next N object GETs with 500.
    fail_gets: Mutex<usize>,
    /// Answer these objects 403 once (an expired link).
    expire_once: Mutex<HashSet<String>>,
    /// Answer the plan with this refusal.
    refuse: Mutex<Option<(u16, &'static str)>>,
}

impl World {
    fn add_batch(&self, epoch: &str, first: i64, last: i64) {
        let name = format!("b-{epoch}-{first}-{last}");
        self.objects
            .lock()
            .unwrap()
            .insert(name.clone(), batch_bytes(epoch, first, last));
        self.batches
            .lock()
            .unwrap()
            .push((epoch.into(), first, last, name));
    }

    fn gets_of(&self, name: &str) -> usize {
        self.gets
            .lock()
            .unwrap()
            .iter()
            .filter(|g| *g == name)
            .count()
    }
}

fn counter(epoch: &str) -> i64 {
    epoch.split(':').next().unwrap().parse().unwrap()
}

fn plan_body(w: &World, port: u16, body: &serde_json::Value) -> String {
    let url = |name: &str| format!("http://127.0.0.1:{port}/obj/{name}");
    let after = body.get("after").filter(|a| !a.is_null());
    let snapshot = w.snapshot.lock().unwrap().clone();
    let mut plan = RestorePlan {
        expires_in_s: 600,
        ..RestorePlan::default()
    };
    if let Some((date, name)) = &snapshot {
        plan.snapshot = Some(RestorePlanSnapshot {
            date: date.clone(),
            url: url(name),
        });
    }
    plan.keys_url = w.keys.lock().unwrap().as_deref().map(url);
    if snapshot.is_some() && after.is_none() {
        plan.batches_pending = true;
    } else {
        let mut list = w.batches.lock().unwrap().clone();
        list.sort_by_key(|(e, f, _, _)| (counter(e), *f));
        for (epoch, first, last, name) in list {
            let keep = match after {
                None => true,
                Some(a) => {
                    let (ae, aseq) = (a["epoch"].as_str().unwrap(), a["seq"].as_i64().unwrap());
                    counter(&epoch) > counter(ae) || (epoch == ae && last > aseq)
                }
            };
            if keep {
                plan.batches.push(RestorePlanBatch {
                    epoch,
                    first,
                    last,
                    url: url(&name),
                });
            }
        }
    }
    serde_json::to_string(&plan).unwrap()
}

async fn serve(world: Arc<World>) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut raw = Vec::new();
            let mut buf = [0u8; 8192];
            let body_at = loop {
                let n = sock.read(&mut buf).await.unwrap_or(0);
                raw.extend_from_slice(&buf[..n]);
                if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&raw[..at]).to_ascii_lowercase();
                    let len = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    while raw.len() < at + 4 + len {
                        let n = sock.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        raw.extend_from_slice(&buf[..n]);
                    }
                    break at + 4;
                }
                if n == 0 {
                    break raw.len();
                }
            };
            let text = String::from_utf8_lossy(&raw[..body_at]).into_owned();
            let line = text.lines().next().unwrap_or("").to_owned();
            let body = String::from_utf8_lossy(&raw[body_at..]).into_owned();
            let (status, reply): (u16, Vec<u8>) = if line.starts_with("POST /v1/restore/plan") {
                let json: serde_json::Value = serde_json::from_str(&body).unwrap();
                world.plans.lock().unwrap().push(json.clone());
                let refusal = *world.refuse.lock().unwrap();
                match refusal {
                    Some((status, code)) => (
                        status,
                        serde_json::json!({"code": code, "message": "no"})
                            .to_string()
                            .into_bytes(),
                    ),
                    None => (200, plan_body(&world, port, &json).into_bytes()),
                }
            } else if let Some(name) = line
                .strip_prefix("GET /obj/")
                .and_then(|r| r.split(' ').next())
            {
                world.gets.lock().unwrap().push(name.to_owned());
                let failing = {
                    let mut fail = world.fail_gets.lock().unwrap();
                    let failing = *fail > 0;
                    *fail = fail.saturating_sub(1);
                    failing
                };
                let expired = world.expire_once.lock().unwrap().remove(name);
                if failing {
                    (500, b"{}".to_vec())
                } else if expired {
                    (403, b"expired".to_vec())
                } else {
                    let found = world.objects.lock().unwrap().get(name).cloned();
                    match found {
                        Some(bytes) => (200, bytes),
                        None => (404, b"missing".to_vec()),
                    }
                }
            } else {
                (404, Vec::new())
            };
            let head = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/octet-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                reply.len()
            );
            let _ = sock.write_all(head.as_bytes()).await;
            let _ = sock.write_all(&reply).await;
        }
    });
    port
}

struct Fx {
    world: Arc<World>,
    port: u16,
    dir: tempfile::TempDir,
    status: watch::Sender<HubStatus>,
    fetcher: RestoreFetcher,
}

fn fetcher(port: u16, dir: &Path, status: &watch::Sender<HubStatus>) -> RestoreFetcher {
    let cloud =
        CloudClient::new(&CloudEndpoint::parse(&format!("http://127.0.0.1:{port}"), true).unwrap())
            .unwrap();
    let token = Arc::new(Mem(Mutex::new(Some(b"hub_x.y".to_vec()))));
    RestoreFetcher::new(cloud, token, "01INSTANCE".into(), dir, status.subscribe())
        .with_retry(Duration::from_millis(10))
}

async fn fx() -> Fx {
    let world = Arc::new(World::default());
    let port = serve(Arc::clone(&world)).await;
    let dir = tempfile::tempdir().unwrap();
    let (status, _rx) = status_channel(HubStatus::opened(85, "01INSTANCE"));
    status.send_modify(|s| {
        s.restore_pending = Some("Pixel Arena".into());
        s.restore_key_released = true;
        s.tier = Some(Tier::Basic);
    });
    let fetcher = fetcher(port, dir.path(), &status);
    Fx {
        world,
        port,
        dir,
        status,
        fetcher,
    }
}

/// A backup with a snapshot at `1:H`/5, the key backup and batches 1..5, 6..10 and 11..12.
fn standard_backup(w: &World) {
    w.objects
        .lock()
        .unwrap()
        .insert("snap".into(), snapshot_bytes("1:H", 5));
    *w.snapshot.lock().unwrap() = Some(("2026-10-29".into(), "snap".into()));
    w.objects.lock().unwrap().insert("keys".into(), vec![9; 40]);
    *w.keys.lock().unwrap() = Some("keys".into());
    w.add_batch("1:H", 1, 5);
    w.add_batch("1:H", 6, 10);
    w.add_batch("1:H", 11, 12);
}

async fn finished(f: &RestoreFetcher) -> RestoreFetchView {
    for _ in 0..400 {
        if let Some(v) = f.view()
            && v.phase != RestoreFetchPhase::Downloading
        {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the download never finished: {:?}", f.view());
}

fn staged(dir: &Path, rel: &str) -> bool {
    dir.join(STAGING_DIR).join(rel).exists()
}

#[tokio::test]
async fn a_complete_backup_is_asked_for_in_two_steps_checked_and_staged() {
    let fx = fx().await;
    standard_backup(&fx.world);
    fx.fetcher.start().unwrap();
    let v = finished(&fx.fetcher).await;
    assert_eq!(v.phase, RestoreFetchPhase::Ready, "{v:?}");
    let ready = v.ready.unwrap();
    assert_eq!(
        (
            ready.snapshot_date.as_deref(),
            ready.batches,
            ready.last_seq,
            ready.partial
        ),
        (Some("2026-10-29"), 2, 12, false)
    );
    // snapshot + keys + the two batches after seq 5
    assert_eq!((v.done, v.total, v.percent), (4, 4, 100));
    assert!(v.bytes > 40);

    let plans = fx.world.plans.lock().unwrap().clone();
    assert_eq!(
        plans[0],
        serde_json::json!({"hub_instance_id": "01INSTANCE"})
    );
    assert_eq!(
        plans[1]["after"],
        serde_json::json!({"epoch": "1:H", "seq": 5})
    );
    assert!(staged(fx.dir.path(), "snapshot.db.zst"));
    assert!(staged(fx.dir.path(), "player-keys.bin"));
    assert!(staged(
        fx.dir.path(),
        "changes/1@H/000000000006-000000000010.zst"
    ));
    assert!(staged(
        fx.dir.path(),
        "changes/1@H/000000000011-000000000012.zst"
    ));
    assert!(!staged(
        fx.dir.path(),
        "changes/1@H/000000000001-000000000005.zst"
    ));
    let manifest: StagedManifest = serde_json::from_str(
        &std::fs::read_to_string(fx.dir.path().join(STAGING_DIR).join(MANIFEST_FILE)).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest.batches.len(), 2);
    assert!(manifest.keys && !manifest.partial);
    assert_eq!(manifest.snapshot_seq, 5);
}

#[tokio::test]
async fn finished_files_are_not_fetched_again_after_a_restart() {
    let fx = fx().await;
    standard_backup(&fx.world);
    fx.fetcher.start().unwrap();
    assert_eq!(finished(&fx.fetcher).await.phase, RestoreFetchPhase::Ready);
    let before = fx.world.gets.lock().unwrap().len();
    assert_eq!(before, 4);

    // A new process: no view yet, the same staging folder.
    let again = fetcher(fx.port, fx.dir.path(), &fx.status);
    assert_eq!(again.view(), None);
    again.start().unwrap();
    let v = finished(&again).await;
    assert_eq!((v.phase, v.done, v.total), (RestoreFetchPhase::Ready, 4, 4));
    assert_eq!(
        fx.world.gets.lock().unwrap().len(),
        before,
        "nothing downloaded twice"
    );
}

#[tokio::test]
async fn a_dropped_connection_is_retried_and_an_expired_link_is_resigned() {
    let fx = fx().await;
    standard_backup(&fx.world);
    *fx.world.fail_gets.lock().unwrap() = 3;
    fx.world
        .expire_once
        .lock()
        .unwrap()
        .insert("b-1:H-11-12".into());
    fx.fetcher.start().unwrap();
    let v = finished(&fx.fetcher).await;
    assert_eq!(v.phase, RestoreFetchPhase::Ready, "{v:?}");
    // Once an object arrived it is not asked for again.
    assert_eq!(fx.world.gets_of("keys"), 1);
    assert_eq!(fx.world.gets_of("b-1:H-6-10"), 1);
    assert_eq!(
        fx.world.gets_of("b-1:H-11-12"),
        2,
        "refused once, then read"
    );
    let plans = fx.world.plans.lock().unwrap().clone();
    assert!(
        plans.len() >= 3,
        "the plan was asked again for new links: {plans:?}"
    );
    assert_eq!(
        plans.last().unwrap()["after"],
        serde_json::json!({"epoch": "1:H", "seq": 5})
    );
}

#[tokio::test]
async fn a_missing_batch_stops_with_a_gap_and_the_owner_can_stop_before_it() {
    let fx = fx().await;
    // No snapshot: batches 1..5, 6..10 (gone from the bucket), 11..15.
    fx.world.add_batch("1:H", 1, 5);
    fx.world.add_batch("1:H", 6, 10);
    fx.world.add_batch("1:H", 11, 15);
    fx.world.objects.lock().unwrap().remove("b-1:H-6-10");
    fx.fetcher.start().unwrap();
    let v = finished(&fx.fetcher).await;
    assert_eq!(v.phase, RestoreFetchPhase::Gap, "{v:?}");
    let gap = v.gap.unwrap();
    assert_eq!(
        (
            gap.epoch.as_str(),
            gap.from_seq,
            gap.to_seq,
            gap.last_good_seq,
            gap.lost
        ),
        ("1:H", 6, 10, 5, 10)
    );
    assert!(staged(
        fx.dir.path(),
        "changes/1@H/000000000001-000000000005.zst"
    ));
    assert!(!staged(
        fx.dir.path(),
        "changes/1@H/000000000011-000000000015.zst"
    ));
    assert!(
        !staged(fx.dir.path(), MANIFEST_FILE),
        "nothing is ready yet"
    );

    fx.fetcher.restore_partial().unwrap();
    let v = fx.fetcher.view().unwrap();
    assert_eq!(v.phase, RestoreFetchPhase::Ready);
    let ready = v.ready.unwrap();
    assert_eq!((ready.batches, ready.last_seq, ready.partial), (1, 5, true));
    assert!(staged(fx.dir.path(), MANIFEST_FILE));
    assert!(
        fx.fetcher.restore_partial().is_err(),
        "the choice is made once"
    );
}

#[tokio::test]
async fn a_damaged_batch_is_a_gap_and_later_files_from_an_earlier_try_are_dropped() {
    let fx = fx().await;
    fx.world.add_batch("1:H", 1, 5);
    fx.world.add_batch("1:H", 6, 10);
    fx.world.add_batch("1:H", 11, 15);
    let mut bad = batch_bytes("1:H", 6, 10);
    let n = bad.len();
    bad[n - 3] ^= 0xff;
    fx.world
        .objects
        .lock()
        .unwrap()
        .insert("b-1:H-6-10".into(), bad);
    // A file from an earlier attempt, after the bad one.
    let later = fx
        .dir
        .path()
        .join(STAGING_DIR)
        .join("changes/1@H/000000000011-000000000015.zst");
    std::fs::create_dir_all(later.parent().unwrap()).unwrap();
    std::fs::write(&later, batch_bytes("1:H", 11, 15)).unwrap();

    fx.fetcher.start().unwrap();
    let v = finished(&fx.fetcher).await;
    assert_eq!(v.phase, RestoreFetchPhase::Gap, "{v:?}");
    let gap = v.gap.unwrap();
    assert_eq!(
        (gap.from_seq, gap.to_seq, gap.last_good_seq, gap.lost),
        (6, 10, 5, 10)
    );
    assert!(!staged(
        fx.dir.path(),
        "changes/1@H/000000000006-000000000010.zst"
    ));
    assert!(!staged(
        fx.dir.path(),
        "changes/1@H/000000000006-000000000010.zst.part"
    ));
    fx.fetcher.restore_partial().unwrap();
    assert!(!later.exists());
}

#[tokio::test]
async fn a_hole_in_the_listing_is_a_gap_too() {
    let fx = fx().await;
    standard_backup(&fx.world);
    // Batch 6..10 is not in the bucket listing at all.
    fx.world
        .batches
        .lock()
        .unwrap()
        .retain(|(_, first, _, _)| *first != 6);
    fx.fetcher.start().unwrap();
    let v = finished(&fx.fetcher).await;
    assert_eq!(v.phase, RestoreFetchPhase::Gap, "{v:?}");
    let gap = v.gap.unwrap();
    assert_eq!((gap.from_seq, gap.to_seq, gap.last_good_seq), (6, 10, 5));
    assert_eq!(gap.lost, 5 + 2, "the hole and the one batch after it");
    fx.fetcher.restore_partial().unwrap();
    let ready = fx.fetcher.view().unwrap().ready.unwrap();
    assert_eq!(
        (ready.batches, ready.last_seq),
        (0, 5),
        "back to the snapshot"
    );
}

#[test]
fn the_hole_check_covers_chains_epochs_and_a_log_that_starts_late() {
    let b = |epoch: &str, first, last| RestorePlanBatch {
        epoch: epoch.into(),
        first,
        last,
        url: String::new(),
    };
    let list = [b("1:H", 6, 10), b("1:H", 11, 12), b("2:H", 3, 4)];
    let pos = ("1:H".to_owned(), 5);
    assert_eq!(
        first_hole(&list, Some(&pos)),
        None,
        "a new epoch starts where it forked"
    );
    assert_eq!(
        first_hole(&[b("1:H", 8, 10)], Some(&pos)),
        Some((0, "1:H".into(), 6, 7))
    );
    assert_eq!(
        first_hole(&[b("1:H", 4, 10)], Some(&pos)),
        None,
        "an overlap is fine"
    );
    assert_eq!(
        first_hole(&[b("1:H", 2, 3)], None),
        Some((0, "1:H".into(), 1, 1))
    );
    assert_eq!(first_hole(&[], None), None);
}

#[tokio::test]
async fn the_cloud_refusing_the_plan_ends_the_attempt_with_its_code() {
    let fx = fx().await;
    *fx.world.refuse.lock().unwrap() = Some((403, "KEY_NOT_RELEASED"));
    fx.fetcher.start().unwrap();
    let v = finished(&fx.fetcher).await;
    assert_eq!(
        (v.phase, v.error.as_deref()),
        (RestoreFetchPhase::Failed, Some("KEY_NOT_RELEASED"))
    );
    // Asking again after the owner fixed it starts over.
    *fx.world.refuse.lock().unwrap() = None;
    standard_backup(&fx.world);
    fx.fetcher.start().unwrap();
    assert_eq!(finished(&fx.fetcher).await.phase, RestoreFetchPhase::Ready);
}

#[tokio::test]
async fn start_is_refused_unless_the_hub_waits_with_the_key_on_a_paid_tier() {
    let fx = fx().await;
    let code = |f: &RestoreFetcher| f.start().unwrap_err().code;
    fx.status.send_modify(|s| s.restore_key_released = false);
    assert_eq!(code(&fx.fetcher), ErrorCode::KeyNotReleased);
    fx.status.send_modify(|s| {
        s.restore_key_released = true;
        s.tier = Some(Tier::Free);
    });
    assert_eq!(code(&fx.fetcher), ErrorCode::FeatureNotEntitled);
    fx.status.send_modify(|s| {
        s.tier = Some(Tier::Basic);
        s.restore_pending = None;
    });
    assert_eq!(code(&fx.fetcher), ErrorCode::NotFound);
    fx.status.send_modify(|s| {
        s.restore_pending = Some("B".into());
        s.pairing = PairingState::Revoked;
    });
    assert_eq!(code(&fx.fetcher), ErrorCode::TokenRevoked);
    assert_eq!(fx.fetcher.view(), None);
    assert!(fx.world.plans.lock().unwrap().is_empty());
}

#[tokio::test]
async fn cancel_removes_the_staging_folder_and_leaves_the_hub_waiting() {
    let fx = fx().await;
    standard_backup(&fx.world);
    fx.fetcher.start().unwrap();
    assert_eq!(finished(&fx.fetcher).await.phase, RestoreFetchPhase::Ready);
    assert!(staged(fx.dir.path(), "snapshot.db.zst"));
    fx.fetcher.cancel();
    assert_eq!(fx.fetcher.view(), None);
    assert!(!fx.dir.path().join(STAGING_DIR).exists());
    assert!(fx.status.borrow().restore_pending.is_some());
    // Cancelling when nothing runs is harmless.
    fx.fetcher.cancel();
}
