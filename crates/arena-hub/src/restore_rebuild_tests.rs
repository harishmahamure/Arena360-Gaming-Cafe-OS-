use super::*;
use std::io;

use arena_core::{ManualClock, status_channel};
use arena_proto::{RestoreFetchView, Tier};
use arena_store::{HubIdentity, Store, set_meta};
use arena_tasks::StagedManifest;

const T: i64 = 1_800_000_000_000;
const BRANCH: &str = "01JBRANCH";
static RESTARTED: AtomicBool = AtomicBool::new(false);

fn restarted() {
    RESTARTED.store(true, Ordering::SeqCst);
}

struct MemKey(Mutex<Option<Vec<u8>>>);

impl Keystore for MemKey {
    fn put(&self, secret: &[u8]) -> io::Result<()> {
        *lock(&self.0) = Some(secret.to_vec());
        Ok(())
    }
    fn get(&self) -> io::Result<Option<Vec<u8>>> {
        Ok(lock(&self.0).clone())
    }
}

struct Fetch(Option<RestoreFetchPhase>);

impl RestoreFetchControl for Fetch {
    fn view(&self) -> Option<RestoreFetchView> {
        self.0.map(|phase| RestoreFetchView {
            phase,
            done: 1,
            total: 1,
            bytes: 0,
            percent: 100,
            retrying: None,
            error: None,
            gap: None,
            ready: None,
        })
    }
    fn start(&self) -> Result<(), UiError> {
        Ok(())
    }
    fn cancel(&self) {}
    fn restore_partial(&self) -> Result<(), UiError> {
        Ok(())
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    db: PathBuf,
    data: PathBuf,
    status: watch::Sender<HubStatus>,
    rebuilder: Rebuilder,
}

/// A waiting Hub whose staging folder holds a snapshot of a one-branch database.
fn fixture(
    phase: Option<RestoreFetchPhase>,
    key: Option<Vec<u8>>,
    tweak: impl FnOnce(&mut HubStatus),
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().to_path_buf();
    let db = data.join("branch.db");
    let mut hub = Store::open(&db, T).unwrap();
    hub.write_tx(|tx| {
        set_meta(tx, arena_store::META_HUB_INSTANCE_ID, "NEWHUB")?;
        arena_store::insert_hub_identity(
            tx,
            &HubIdentity {
                tenant_id: "T1".into(),
                branch_id: BRANCH.into(),
                hub_id: "H2".into(),
                tier: Tier::Basic,
                paired_at: T,
            },
        )?;
        arena_store::mark_restore_pending(tx, "Level Up")?;
        Ok(())
    })
    .unwrap();
    drop(hub);

    // The old Hub's database, snapshotted into the staging folder.
    let old = data.join("old-source.db");
    let mut source = Store::open(&old, T).unwrap();
    source
        .write_tx(|tx| {
            tx.execute(
                "INSERT INTO branch (branch_id, name, branch_code, currency, tz, day_start_min,
                                     fy_start_month, rev, created_at, updated_at)
                 VALUES (?1, 'Level Up', 'LVL01', 'INR', 'Asia/Kolkata', 240, 4, 1, 0, 0)",
                [BRANCH],
            )
            .map_err(arena_store::db_error)?;
            tx.execute(
                "INSERT INTO changes (epoch, seq, tbl, row_id, op, row, intent_id,
                                      intent_kind, actor_kind, actor_id, at)
                 VALUES ('2:OLDHUB', 1, 't', 'r', 'insert', x'00', 'i', 'k', 'system', 'a', 0)",
                [],
            )
            .map_err(arena_store::db_error)?;
            Ok(())
        })
        .unwrap();
    let snap = arena_store::build_snapshot_file(&old, &data, "2027-01-14").unwrap();
    let staging = data.join(RESTORE_STAGING_DIR);
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::copy(snap.path(), staging.join(SNAPSHOT_FILE)).unwrap();
    drop(snap);
    drop(source);
    let manifest = StagedManifest {
        snapshot_date: Some("2027-01-14".into()),
        snapshot_epoch: Some("2:OLDHUB".into()),
        snapshot_seq: 1,
        keys: false,
        batches: vec![],
        partial: false,
    };
    std::fs::write(
        staging.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();

    let mut initial = HubStatus::opened(84, "NEWHUB");
    initial.restore_pending = Some("Level Up".into());
    initial.restore_key_released = true;
    initial.tier = Some(Tier::Basic);
    initial.platform = HubPlatform::Windows;
    tweak(&mut initial);
    let (status, rx) = status_channel(initial);
    let rebuilder = Rebuilder::new(
        Arc::new(Fetch(phase)),
        Arc::new(MemKey(Mutex::new(key))),
        db.clone(),
        data.clone(),
        Arc::new(ManualClock::new(T)),
        rx,
        restarted,
    );
    Fixture {
        _dir: dir,
        db,
        data,
        status,
        rebuilder,
    }
}

fn code(r: Result<(), UiError>) -> ErrorCode {
    r.unwrap_err().code
}

#[tokio::test]
async fn a_rebuild_is_refused_until_the_download_is_complete_and_the_key_released() {
    let ready = Some(RestoreFetchPhase::Ready);
    let key = Some(vec![7; KEK_LEN]);

    let f = fixture(Some(RestoreFetchPhase::Downloading), key.clone(), |_| {});
    assert_eq!(code(f.rebuilder.start()), ErrorCode::NotFound);
    let f = fixture(None, key.clone(), |_| {});
    assert_eq!(code(f.rebuilder.start()), ErrorCode::NotFound);
    let f = fixture(ready, key.clone(), |s| s.restore_key_released = false);
    assert_eq!(code(f.rebuilder.start()), ErrorCode::KeyNotReleased);
    let f = fixture(ready, key.clone(), |s| s.tier = Some(Tier::Free));
    assert_eq!(code(f.rebuilder.start()), ErrorCode::FeatureNotEntitled);
    let f = fixture(ready, key.clone(), |s| s.platform = HubPlatform::Android);
    assert_eq!(code(f.rebuilder.start()), ErrorCode::Unsupported);
    let f = fixture(ready, key.clone(), |s| s.restore_pending = None);
    assert_eq!(code(f.rebuilder.start()), ErrorCode::NotFound);
    let f = fixture(ready, key, |s| s.pairing = PairingState::Revoked);
    assert_eq!(code(f.rebuilder.start()), ErrorCode::TokenRevoked);
    assert!(f.rebuilder.view().is_none());
    assert!(!arena_store::has_pending(&f.db));
}

async fn settled(f: &Fixture) -> RestoreRebuildView {
    for _ in 0..200 {
        if let Some(v) = f.rebuilder.view()
            && matches!(
                v.phase,
                RestoreRebuildPhase::Restarting | RestoreRebuildPhase::Failed
            )
        {
            return v;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("the rebuild did not settle");
}

#[tokio::test]
async fn a_rebuild_stages_the_new_database_and_asks_the_hub_to_restart() {
    RESTARTED.store(false, Ordering::SeqCst);
    let f = fixture(
        Some(RestoreFetchPhase::Ready),
        Some(vec![7; KEK_LEN]),
        |_| {},
    );
    f.rebuilder.start().unwrap();
    assert_eq!(settled(&f).await.phase, RestoreRebuildPhase::Restarting);
    assert!(RESTARTED.load(Ordering::SeqCst));
    assert!(arena_store::has_pending(&f.db));
    assert!(!f.data.join(REBUILD_FILE).exists(), "the work file is gone");
    // The downloaded backup stays until the restart has used the staged file.
    assert!(
        f.data
            .join(RESTORE_STAGING_DIR)
            .join(SNAPSHOT_FILE)
            .exists()
    );
    // Nothing can take the restart back now.
    f.rebuilder.cancel();
    assert_eq!(
        f.rebuilder.view().map(|v| v.phase),
        Some(RestoreRebuildPhase::Restarting)
    );

    // At the next start the swap opens the new epoch and records where the backup came from.
    let out = arena_store::apply_pending(&f.db, T + 1000)
        .unwrap()
        .unwrap();
    let cloud = out.restore.cloud.unwrap();
    assert_eq!(cloud.snapshot_date.as_deref(), Some("2027-01-14"));
    assert_eq!((cloud.batches, cloud.applied, cloud.orphaned), (0, 0, 0));
    assert_eq!(out.restore.new_epoch, "3:NEWHUB");
}

#[tokio::test]
async fn a_rebuild_without_the_branch_key_fails_and_keeps_the_download() {
    let f = fixture(Some(RestoreFetchPhase::Ready), None, |_| {});
    f.rebuilder.start().unwrap();
    let v = settled(&f).await;
    assert_eq!(v.phase, RestoreRebuildPhase::Failed);
    assert_eq!(v.error.as_deref(), Some("KEY_NOT_RELEASED"));
    assert!(!arena_store::has_pending(&f.db));
    assert!(
        f.data
            .join(RESTORE_STAGING_DIR)
            .join(SNAPSHOT_FILE)
            .exists()
    );
    // Cancel clears the failed view and the Hub stays in "Restore this branch".
    f.rebuilder.cancel();
    assert!(f.rebuilder.view().is_none());
    assert!(f.status.borrow().restore_pending.is_some());
}

// Story 9.27: the key backup, opened with the branch key.

fn backup_keys(kek: Option<Vec<u8>>) -> BackupKeys {
    let dir = tempfile::tempdir().unwrap();
    let cloud = arena_tasks::CloudClient::new(
        &arena_tasks::CloudEndpoint::parse("http://127.0.0.1:1", true).unwrap(),
    )
    .unwrap();
    let (status, rx) = status_channel(HubStatus::opened(85, "01INSTANCE"));
    std::mem::forget(status);
    let fetcher = RestoreFetcher::new(
        cloud,
        Arc::new(MemKey(Mutex::new(Some(b"hub_x.y".to_vec())))),
        "01INSTANCE".into(),
        dir.path(),
        rx,
    );
    BackupKeys::new(fetcher, Arc::new(MemKey(Mutex::new(kek))))
}

#[test]
fn the_key_backup_names_its_players_only_for_the_branch_key() {
    let rows = ["P1", "P3"].map(|id| PlayerKeyRow {
        player_id: id.into(),
        dek_wrapped: format!("wrapped-{id}"),
    });
    let sealed = arena_core::key_backup::seal_key_table(&[7; KEK_LEN], &rows).unwrap();
    assert_eq!(
        backup_keys(Some(vec![7; KEK_LEN])).open(&sealed),
        Some(vec!["P1".to_owned(), "P3".to_owned()])
    );
    // Another key, no key, a key of the wrong size and a damaged backup answer nothing.
    assert_eq!(backup_keys(Some(vec![8; KEK_LEN])).open(&sealed), None);
    assert_eq!(backup_keys(None).open(&sealed), None);
    assert_eq!(backup_keys(Some(vec![7; 5])).open(&sealed), None);
    assert_eq!(backup_keys(Some(vec![7; KEK_LEN])).open(b"junk"), None);
}

#[tokio::test]
async fn a_hub_that_cannot_reach_the_cloud_answers_nothing() {
    // The fetcher sees a Hub with no tier yet, so it asks nothing and nobody is erased.
    let keys = backup_keys(Some(vec![7; KEK_LEN]));
    assert_eq!(keys.live_keys().await, None);
}
