//! The cloud backup retention sweep (Story 9.15): the planner matrix, the tier-change grace and
//! the sweep against an in-memory data bucket and a local `gz_control`.

use std::sync::Arc;

use arena_cloud::presign::Presigner;
use arena_cloud::retention::{GRACE_MS, observe, plan_branch, sweep_once};
use arena_cloud::store::{MemoryStore, S3Config};
use arena_proto::Tier;
use arena_store::control::{ControlDb, RetentionState};

const DAY: i64 = 24 * 60 * 60 * 1000;
/// 2027-01-15 00:00 UTC.
const NOW: i64 = 1_799_971_200_000;
const P: &str = "tenants/t1/branches/b1/";

fn obj(rel: &str, age_days: i64) -> (String, i64) {
    (format!("{P}{rel}"), NOW - age_days * DAY)
}

fn plan(objects: &[(String, i64)], days: Option<u32>) -> Vec<String> {
    let mut v: Vec<String> = plan_branch(P, objects, days, NOW)
        .into_iter()
        .map(|k| k.strip_prefix(P).unwrap().to_owned())
        .collect();
    v.sort();
    v
}

/// The date `age` days before `NOW` as `yyyy-mm-dd`.
fn date(age: i64) -> String {
    chrono::DateTime::from_timestamp_millis(NOW - age * DAY)
        .unwrap()
        .format("%Y-%m-%d")
        .to_string()
}

#[test]
fn old_rollups_and_batches_go_and_recent_ones_stay() {
    let old = format!("rollups/{}.json", date(400));
    let new = format!("rollups/{}.json", date(100));
    let objects = [
        obj(&old, 400),
        obj(&new, 100),
        obj("changes/1:H/000000000001-000000000010.zst", 400),
        obj("changes/1:H/000000000011-000000000020.zst", 100),
    ];
    assert_eq!(
        plan(&objects, Some(365)),
        vec!["changes/1:H/000000000001-000000000010.zst".to_owned(), old]
    );
    // Premium keeps all of it.
    assert!(plan(&objects, Some(1095)).is_empty());
}

#[test]
fn keys_state_and_everything_else_are_never_touched() {
    let objects = [
        obj("keys/player-keys.bin", 5000),
        obj("state/latest.json", 5000),
        obj("audit-cache/2020-01.zst", 5000),
        obj("rollups/not-a-date.json", 5000),
        obj("snapshots/garbage.zst", 5000),
        (
            "tenants/t1/branches/b2/rollups/2000-01-01.json".to_owned(),
            0,
        ),
    ];
    assert!(plan(&objects, Some(365)).is_empty());
    assert!(plan(&objects, None).is_empty());
}

#[test]
fn the_snapshot_a_retained_batch_restores_from_is_kept() {
    let s400 = format!("snapshots/{}.db.zst", date(400));
    let s380 = format!("snapshots/{}.db.zst", date(380));
    let s200 = format!("snapshots/{}.db.zst", date(200));
    let objects = [
        obj(&s400, 400),
        obj(&s380, 380),
        obj(&s200, 200),
        obj("changes/1:H/000000000001-000000000010.zst", 300),
    ];
    // Retention 365: the oldest retained batch is 300 days old; the newest snapshot that ended
    // before it is the 380-day one. The 400-day one is surplus; the 200-day one is within retention.
    assert_eq!(plan(&objects, Some(365)), vec![s400]);
}

#[test]
fn with_no_retained_batch_the_newest_old_snapshot_is_kept() {
    let s500 = format!("snapshots/{}.db.zst", date(500));
    let s400 = format!("snapshots/{}.db.zst", date(400));
    let objects = [
        obj(&s500, 500),
        obj(&s400, 400),
        obj("changes/1:H/000000000001-000000000010.zst", 450),
    ];
    assert_eq!(
        plan(&objects, Some(365)),
        vec!["changes/1:H/000000000001-000000000010.zst".to_owned(), s500]
    );
}

#[test]
fn gst_archives_go_only_eight_years_after_the_year_ends() {
    // NOW is 2027-01-15. FY2017-18 ends in 2018 at the latest: deletable from 2027-01-01.
    // FY2018-19 ends in 2019: deletable from 2028-01-01.
    let objects = [
        obj("gst-archive/FY2017-18.zst", 1),
        obj("gst-archive/FY2018-19.zst", 1),
        obj("gst-archive/FY2018.zst", 1),
        obj("gst-archive/FY2019.zst", 1),
        obj("gst-archive/FY2017-99.zst", 1),
    ];
    // Any tier, and after a lapse or never paid (no retention): the same.
    for days in [None, Some(365), Some(1095)] {
        assert_eq!(
            plan(&objects, days),
            vec![
                "gst-archive/FY2017-18.zst".to_owned(),
                "gst-archive/FY2018.zst".to_owned()
            ],
            "{days:?}"
        );
    }
}

#[test]
fn a_branch_that_never_paid_keeps_its_batches_and_snapshots() {
    let objects = [
        obj("changes/1:H/000000000001-000000000010.zst", 4000),
        obj(&format!("snapshots/{}.db.zst", date(4000)), 4000),
        obj(&format!("rollups/{}.json", date(4000)), 4000),
    ];
    assert!(plan(&objects, None).is_empty());
}

fn state(paid: u32, prev: u32, changed_at: i64) -> RetentionState {
    RetentionState {
        paid_days: paid,
        prev_days: prev,
        changed_at,
    }
}

#[test]
fn a_move_to_a_shorter_plan_keeps_the_longer_retention_for_30_days() {
    let premium = state(1095, 1095, NOW - 400 * DAY);
    let (days, save) = observe(Tier::Basic, Some(premium), NOW);
    assert_eq!(days, Some(1095));
    let saved = save.unwrap();
    assert_eq!(
        (saved.paid_days, saved.prev_days, saved.changed_at),
        (365, 1095, NOW)
    );
    // Day 29: still graced. Day 30: the new retention applies.
    assert_eq!(
        observe(Tier::Basic, Some(saved), NOW + 29 * DAY),
        (Some(1095), None)
    );
    assert_eq!(
        observe(Tier::Basic, Some(saved), NOW + 30 * DAY),
        (Some(365), None)
    );
    assert_eq!(GRACE_MS, 30 * DAY);
}

#[test]
fn a_move_to_a_longer_plan_applies_at_once() {
    let basic = state(365, 365, NOW - 400 * DAY);
    let (days, save) = observe(Tier::Premium, Some(basic), NOW);
    assert_eq!(days, Some(1095));
    assert_eq!(save.unwrap().paid_days, 1095);
}

#[test]
fn a_lapsed_branch_keeps_the_last_paid_retention() {
    let basic = state(365, 365, NOW - 900 * DAY);
    assert_eq!(observe(Tier::Free, Some(basic), NOW), (Some(365), None));
    // A branch that never paid has no retention.
    assert_eq!(observe(Tier::Free, None, NOW), (None, None));
    // A first sight of a paid branch records it.
    let (days, save) = observe(Tier::Basic, None, NOW);
    assert_eq!(days, Some(365));
    assert_eq!(save, Some(state(365, 365, NOW)));
    // Unchanged: nothing to save.
    assert_eq!(
        observe(Tier::Basic, Some(state(365, 365, 1)), NOW),
        (Some(365), None)
    );
}

async fn seeded(dir: &std::path::Path) -> ControlDb {
    let db = ControlDb::open_local(&dir.join("gz_control.db"))
        .await
        .unwrap();
    db.migrate(NOW).await.unwrap();
    let c = rusqlite::Connection::open(dir.join("gz_control.db")).unwrap();
    c.execute_batch(
        "PRAGMA foreign_keys = OFF;
         INSERT INTO tenants (id, owner_id, name, created_at) VALUES ('t1', 'o1', 'Pixel', 1);
         INSERT INTO branches (id, tenant_id, name, city, country, currency, timezone,
           gst_registered, gstin, branch_code, code_locked_at, created_at, updated_at)
           VALUES ('b1', 't1', 'Pixel Arena', 'Pune', 'IN', 'INR', 'Asia/Kolkata', 0, NULL,
           'PAG', NULL, 1, 1),
           ('b2', 't1', 'Free Arena', 'Pune', 'IN', 'INR', 'Asia/Kolkata', 0, NULL,
           'FRE', NULL, 1, 1);
         INSERT INTO entitlements (branch_id, tenant_id, tier, source, addons_json, issued_at,
           until, updated_at) VALUES ('b1', 't1', 'basic', 'paid', '[]', 1, NULL, 1),
           ('b2', 't1', 'free', 'free', '[]', 1, NULL, 1);",
    )
    .unwrap();
    db
}

fn presigner(mem: &Arc<MemoryStore>) -> Presigner {
    Presigner::new(&S3Config {
        endpoint: "http://127.0.0.1:9000".into(),
        bucket: "arena-data".into(),
        access_key_id: "AKID".into(),
        secret_access_key: "SAK".into(),
    })
    .with_memory_listing(Arc::clone(mem))
}

fn put(mem: &MemoryStore, branch: &str, rel: &str, age_days: i64) {
    mem.put_at(
        &format!("tenants/t1/branches/{branch}/{rel}"),
        b"x".to_vec(),
        NOW - age_days * DAY,
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_sweep_deletes_per_branch_and_remembers_the_retention() {
    let dir = tempfile::tempdir().unwrap();
    let db = seeded(dir.path()).await;
    let mem = Arc::new(MemoryStore::new());
    put(&mem, "b1", "changes/1:H/000000000001-000000000010.zst", 400);
    put(&mem, "b1", "changes/1:H/000000000011-000000000020.zst", 10);
    put(&mem, "b1", "keys/player-keys.bin", 4000);
    put(&mem, "b1", "state/latest.json", 4000);
    put(&mem, "b1", "gst-archive/FY2017-18.zst", 1);
    put(&mem, "b1", "gst-archive/FY2020-21.zst", 1);
    // A Free branch that never paid: only the GST archive rule applies.
    put(
        &mem,
        "b2",
        "changes/1:H/000000000001-000000000010.zst",
        4000,
    );
    put(&mem, "b2", "gst-archive/FY2017-18.zst", 1);
    let store = presigner(&mem);

    let out = sweep_once(&db, &store, NOW).await.unwrap();
    assert_eq!((out.branches, out.deleted, out.failed), (2, 3, 0));
    assert_eq!(
        mem.keys(),
        vec![
            "tenants/t1/branches/b1/changes/1:H/000000000011-000000000020.zst",
            "tenants/t1/branches/b1/gst-archive/FY2020-21.zst",
            "tenants/t1/branches/b1/keys/player-keys.bin",
            "tenants/t1/branches/b1/state/latest.json",
            "tenants/t1/branches/b2/changes/1:H/000000000001-000000000010.zst",
        ]
    );
    let rows = db.retention_branches(NOW).await.unwrap();
    assert_eq!(rows[0].state, Some(state(365, 365, NOW)));
    assert_eq!(rows[1].state, None);

    // Run again: nothing more to do.
    let again = sweep_once(&db, &store, NOW + DAY).await.unwrap();
    assert_eq!(again.deleted, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_downgrade_is_graced_then_applied_by_the_sweep() {
    let dir = tempfile::tempdir().unwrap();
    let db = seeded(dir.path()).await;
    // b1 was Premium when the sweep last saw it; it is Basic now.
    db.save_branch_retention("b1", state(1095, 1095, NOW - 500 * DAY))
        .await
        .unwrap();
    let mem = Arc::new(MemoryStore::new());
    put(&mem, "b1", "changes/1:H/000000000001-000000000010.zst", 500);
    let store = presigner(&mem);

    assert_eq!(sweep_once(&db, &store, NOW).await.unwrap().deleted, 0);
    assert_eq!(
        sweep_once(&db, &store, NOW + 29 * DAY)
            .await
            .unwrap()
            .deleted,
        0
    );
    assert_eq!(
        sweep_once(&db, &store, NOW + 30 * DAY)
            .await
            .unwrap()
            .deleted,
        1
    );
}
