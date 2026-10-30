//! The on-demand audit builder (Story 8.36): job queue API, replay from a snapshot and batches,
//! forks, failures, the tenant limit and the 24-hour expiry, against an in-memory data bucket.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use arena_cloud::audit_build::{AuditBuilder, Requested, cache_key, request_build};
use arena_cloud::audit_query::AuditCache;
use arena_cloud::auth::AuthState;
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
use arena_cloud::presign::Presigner;
use arena_cloud::presign::{Epoch, batch_key, branch_prefix, snapshot_key};
use arena_cloud::store::{MemoryStore, ObjectStore, S3Config};
use arena_cloud::{Clock, app_with_auth, auth_state};
use arena_proto::{AuditBuildState, Tier};
use arena_replay::state_checksum;
use arena_store::control::{CompGrant, ControlDb, LATEST_CONTROL_VERSION, NewBackupCode};
use arena_store::{
    BatchFork, ChangeOp, ChangeRecord, PendingBatch, Row, RowKey, RowValue, Store, Table,
    build_snapshot_file, encode_batch, insert_change, upsert_row,
};
use axum::body::{Body, to_bytes};
use axum::extract::connect_info::ConnectInfo;
use axum::http::{Request, header};
use serde_json::{Value, json};
use tokio::sync::watch;
use tower::ServiceExt;

const ORIGIN: &str = "https://start.arena360.cloud";
const PEPPER: &str = "test-pepper-0123456789abcdef0123456789";
const T0: i64 = 1_800_000_000_000;
const DAY: i64 = 24 * 60 * 60 * 1000;
const PHONE_A: &str = "+919876543210";
const PHONE_B: &str = "+919876543211";

struct Harness {
    app: axum::Router,
    db: Arc<ControlDb>,
    st: Arc<AuthState>,
    now: Arc<AtomicI64>,
    keys: Arc<MemoryStore>,
    _dir: tempfile::TempDir,
}

struct Reply {
    status: u16,
    body: Value,
    retry_after: Option<String>,
}

impl Reply {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or("")
    }
}

/// An owner's branch with a Hub paired to it.
struct Cafe {
    branch: String,
    tenant: String,
    #[allow(dead_code)]
    token: String,
    access: String,
}

fn config() -> AuthConfig {
    AuthConfig {
        pepper: PEPPER.as_bytes().to_vec(),
        web_origins: vec![ORIGIN.to_owned()],
        sms: SmsProviderConfig::Fake,
        sms_global_per_min: 10_000,
        mfa_key: MfaKey::from_bytes([0xa5; 32]),
        licence: arena_cloud::licence::LicenceSigner::dev(),
        billing_enabled: false,
    }
}

fn presigner(keys: &Arc<MemoryStore>) -> Presigner {
    Presigner::new(&S3Config {
        endpoint: "http://127.0.0.1:9000".into(),
        bucket: "arena-data".into(),
        access_key_id: "AKID-SECRET".into(),
        secret_access_key: "SAK-SECRET".into(),
    })
    .with_memory_listing(Arc::clone(keys))
}

fn router(st: &Arc<AuthState>) -> axum::Router {
    let (_tx, rx) = watch::channel(Reachability {
        reachable: true,
        checked_at: None,
    });
    app_with_auth(
        Role::All,
        true,
        rx,
        LATEST_CONTROL_VERSION,
        Some(Arc::clone(st)),
    )
}

impl Harness {
    async fn new() -> Self {
        Self::build(true).await
    }

    async fn build(with_bucket: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = ControlDb::open_local(&dir.path().join("gz_control.db"))
            .await
            .unwrap();
        db.migrate(T0).await.unwrap();
        let db = Arc::new(db);
        let now = Arc::new(AtomicI64::new(T0));
        let tick = Arc::clone(&now);
        let clock: Clock = Arc::new(move || tick.load(Ordering::SeqCst));
        let st = auth_state(Arc::clone(&db), &config(), clock).unwrap();
        let keys = Arc::new(MemoryStore::new());
        if with_bucket {
            assert!(st.presigner.set(presigner(&keys)).is_ok());
        }
        let app = router(&st);
        Self {
            app,
            db,
            st,
            now,
            keys,
            _dir: dir,
        }
    }

    async fn call(&self, method: &str, path: &str, body: Option<Value>, access: &str) -> Reply {
        let mut req = Request::builder().method(method).uri(path);
        if !access.is_empty() {
            req = req.header("cookie", format!("arena_access={access}"));
        }
        let req = match body {
            Some(b) => req
                .header("content-type", "application/json")
                .body(Body::from(b.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    async fn sign_up(&self, phone: &str, cafe_name: &str) -> String {
        let s = self
            .call(
                "POST",
                "/v1/signup/start",
                Some(json!({
                    "name": "Asha Rao",
                    "phone": phone,
                    "email": format!("owner{}@example.in", &phone[phone.len() - 2..]),
                    "cafe_name": cafe_name,
                    "city": "Pune",
                })),
                "",
            )
            .await;
        assert_eq!(s.status, 202, "{}", s.body);
        let id = s.body["challenge_id"].as_str().unwrap().to_owned();
        let text = self
            .st
            .sms
            .fake()
            .unwrap()
            .sent()
            .last()
            .unwrap()
            .text
            .clone();
        let req = Request::builder()
            .method("POST")
            .uri("/v1/signup/verify")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"challenge_id": id, "code": &text[..6]}).to_string(),
            ))
            .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status().as_u16(), 200);
        res.headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .find_map(|c| {
                c.to_str()
                    .unwrap()
                    .strip_prefix("arena_access=")
                    .map(|v| v.split(';').next().unwrap().to_owned())
            })
            .unwrap()
    }

    async fn complete_setup(&self, phone: &str) {
        let id = self.db.owner_by_phone(phone).await.unwrap().unwrap().id;
        assert!(
            self.db
                .set_owner_password(&id, "$argon2id$x", T0)
                .await
                .unwrap()
        );
        assert!(self.db.start_owner_mfa(&id, b"ct", b"n", T0).await.unwrap());
        let codes: Vec<NewBackupCode> = (0..10)
            .map(|i| NewBackupCode {
                id: format!("{id}-b{i}"),
                code_hash: format!("h{i}"),
            })
            .collect();
        assert!(
            self.db
                .confirm_owner_mfa(&id, b"n", 1, &codes, T0)
                .await
                .unwrap()
        );
        assert!(self.db.ack_backup_codes(&id, T0).await.unwrap());
    }

    /// A signed-up owner whose branch has a Hub paired (`peer` is the Hub's address).
    async fn cafe(&self, phone: &str, name: &str, peer: &str) -> Cafe {
        let access = self.sign_up(phone, name).await;
        self.complete_setup(phone).await;
        let r = self
            .call(
                "PUT",
                "/v1/wizard/cafe",
                Some(json!({
                    "cafe_name": name,
                    "city": "Pune",
                    "country": "IN",
                    "currency": "INR",
                    "timezone": "Asia/Kolkata",
                    "gst_registered": false,
                })),
                &access,
            )
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
        let branch = r.body["branch"]["id"].as_str().unwrap().to_owned();
        let r = self
            .call(
                "PUT",
                "/v1/wizard/tier",
                Some(json!({"tier": "free"})),
                &access,
            )
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
        let r = self
            .call(
                "POST",
                "/v1/wizard/install-hub/code",
                Some(json!({})),
                &access,
            )
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
        let code = r.body["code"].as_str().unwrap().to_owned();
        let mut req = Request::builder()
            .method("POST")
            .uri("/v1/pair")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "code": code,
                    "platform": "android",
                    "hub_instance_id": "01HUBINSTANCE",
                    "version": "0.1.0",
                })
                .to_string(),
            ))
            .unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::new(peer.parse().unwrap(), 40000)));
        let r = reply(self.app.clone().oneshot(req).await.unwrap()).await;
        assert_eq!(r.status, 200, "{}", r.body);
        let token = r.body["token"].as_str().unwrap().to_owned();
        let tenant = self.db.branch(&branch).await.unwrap().unwrap().tenant_id;
        Cafe {
            branch,
            tenant,
            token,
            access,
        }
    }

    /// A paid branch with a Hub paired to it.
    async fn paid_cafe(&self) -> Cafe {
        let a = self.cafe(PHONE_A, "Pixel Arena", "198.51.100.4").await;
        self.upgrade(&a.branch, Tier::Basic, T0 + 30 * DAY).await;
        a
    }

    async fn get(&self, c: &Cafe, path: &str) -> Reply {
        self.call("GET", path, None, &c.access).await
    }

    /// Moves the cloud's clock on.
    fn advance(&self, ms: i64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }
    async fn upgrade(&self, branch: &str, tier: Tier, until: i64) {
        self.db
            .grant_comp(&CompGrant {
                branch_id: branch,
                tier,
                until,
                audit_id: &format!("audit-{branch}-{tier:?}-{until}"),
                operator: "ops",
                reason: "beta cafe",
                details_json: "{}",
                now: T0,
            })
            .await
            .unwrap();
    }
}

async fn reply(res: axum::response::Response) -> Reply {
    let status = res.status().as_u16();
    let retry_after = res
        .headers()
        .get(header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    Reply {
        status,
        body,
        retry_after,
    }
}

const MONTH: &str = "2027-01";
const BRANCH_ID: &str = "HUBBRANCH";

fn text(s: &str) -> RowValue {
    RowValue::Text(s.to_owned())
}

fn branch_row() -> Row {
    Row::from([
        ("branch_id".to_owned(), text(BRANCH_ID)),
        ("name".to_owned(), text("Pixel Arena")),
        ("branch_code".to_owned(), text("PXA01")),
        ("currency".to_owned(), text("INR")),
        ("tz".to_owned(), text("Asia/Kolkata")),
        ("day_start_min".to_owned(), RowValue::Integer(360)),
        ("fy_start_month".to_owned(), RowValue::Integer(4)),
        ("rev".to_owned(), RowValue::Integer(1)),
        ("created_at".to_owned(), RowValue::Integer(T0)),
        ("updated_at".to_owned(), RowValue::Integer(T0)),
    ])
}

fn setting_row(key: &str, value: &str) -> Row {
    Row::from([
        ("branch_id".to_owned(), text(BRANCH_ID)),
        ("key".to_owned(), text(key)),
        ("value".to_owned(), text(value)),
        ("rev".to_owned(), RowValue::Integer(1)),
        ("updated_at".to_owned(), RowValue::Integer(T0)),
    ])
}

/// Change `seq` of `epoch`: the branch row at seq 1 of epoch 1, a setting after it.
fn change(epoch: &str, seq: i64, day: &str) -> ChangeRecord {
    let (table, op, row) = if epoch == "1:HUBA" && seq == 1 {
        (Table::Branch, ChangeOp::Insert, branch_row())
    } else {
        (
            Table::Settings,
            ChangeOp::Insert,
            setting_row(&format!("k{epoch}-{seq}"), &format!("v{seq}")),
        )
    };
    ChangeRecord {
        epoch: epoch.to_owned(),
        seq,
        tbl: table.name().to_owned(),
        row_id: RowKey::from_row(table, &row).unwrap().row_id(),
        op,
        row: rmp_serde::to_vec_named(&row).unwrap(),
        intent_id: format!("I{epoch}{seq}"),
        intent_kind: "settings.set".to_owned(),
        actor_kind: "staff".to_owned(),
        actor_id: "S1".to_owned(),
        elevated_by_kind: None,
        elevated_by_id: None,
        elevated_by_mode: None,
        elevated_by_verifier_version: None,
        at: T0 + seq,
        business_day: Some(day.to_owned()),
        fy: Some("FY2026-27".to_owned()),
        reason: None,
    }
}

fn changes(epoch: &str, from: i64, to: i64) -> Vec<ChangeRecord> {
    (from..=to)
        .map(|s| change(epoch, s, "2027-01-10"))
        .collect()
}

/// A Hub database holding `log`, as the Hub would: rows and change rows.
fn hub(dir: &std::path::Path, name: &str, log: &[ChangeRecord]) -> Store {
    let mut store = Store::open(dir.join(name), T0).unwrap();
    for c in log {
        let table = Table::from_name(&c.tbl).unwrap();
        let row: Row = rmp_serde::from_slice(&c.row).unwrap();
        store
            .write_tx(|tx| {
                upsert_row(tx, table, &row)?;
                insert_change(tx, c)
            })
            .unwrap();
    }
    store
}

/// Puts an object in the in-memory bucket (a memory put never waits).
fn put(keys: &MemoryStore, key: &str, bytes: Vec<u8>) {
    futures_util::FutureExt::now_or_never(keys.put(key, bytes))
        .expect("a memory put is ready")
        .unwrap();
}

fn remove(keys: &MemoryStore, key: &str) {
    futures_util::FutureExt::now_or_never(keys.delete(key))
        .expect("a memory delete is ready")
        .unwrap();
}

/// Puts the batches of `rows` (one epoch) in the bucket, `size` rows each; the first batch
/// carries `fork`. Returns their keys.
fn put_batches(
    keys: &MemoryStore,
    prefix: &str,
    rows: &[ChangeRecord],
    size: usize,
    fork: Option<BatchFork>,
) -> Vec<String> {
    let mut out = Vec::new();
    for (i, chunk) in rows.chunks(size).enumerate() {
        let epoch = chunk[0].epoch.clone();
        let (first, last) = (chunk[0].seq, chunk[chunk.len() - 1].seq);
        let batch = PendingBatch {
            epoch: epoch.clone(),
            first,
            last,
            fork: if i == 0 { fork.clone() } else { None },
            rows: chunk.to_vec(),
        };
        let key = batch_key(prefix, &Epoch::parse(&epoch).unwrap(), first, last);
        put(keys, &key, encode_batch(12, &batch).unwrap());
        out.push(key);
    }
    out
}

fn builder(h: &Harness, scratch: &std::path::Path) -> AuditBuilder {
    let tick = Arc::clone(&h.now);
    let clock: Clock = Arc::new(move || tick.load(Ordering::SeqCst));
    AuditBuilder::new(
        Arc::clone(&h.db),
        presigner(&h.keys),
        clock,
        scratch.to_path_buf(),
    )
}

fn build_path(c: &Cafe) -> String {
    format!("/v1/branches/{}/audit/build", c.branch)
}

async fn request(h: &Harness, c: &Cafe, month: &str) -> Reply {
    h.call(
        "POST",
        &build_path(c),
        Some(json!({"month": month})),
        &c.access,
    )
    .await
}

/// The rebuilt month in the bucket, opened.
fn rebuilt(h: &Harness, c: &Cafe, dir: &std::path::Path) -> Store {
    let prefix = branch_prefix(&c.tenant, &c.branch);
    let (bytes, _) = h
        .keys
        .read(&cache_key(&prefix, MONTH))
        .expect("a cache object");
    let path = dir.join("rebuilt.db");
    std::fs::write(&path, zstd::decode_all(&bytes[..]).unwrap()).unwrap();
    Store::open(&path, T0).unwrap()
}

#[tokio::test]
async fn asking_for_a_month_queues_one_job_and_a_repeat_reuses_it() {
    let h = Harness::new().await;
    let c = h.paid_cafe().await;
    let r = request(&h, &c, MONTH).await;
    assert_eq!(r.status, 202, "{}", r.body);
    assert_eq!(r.body["code"], "AUDIT_BUILDING");
    assert_eq!(r.body["state"], "queued");
    assert_eq!(r.body["progress"], 0);
    assert_eq!(r.body["month"], MONTH);
    let again = request(&h, &c, MONTH).await;
    assert_eq!(again.status, 202);
    let job = h.db.builder_job(&c.branch, MONTH).await.unwrap().unwrap();
    assert_eq!((job.state, job.attempt), (AuditBuildState::Queued, 1));
    assert!(job.requested_by.starts_with("owner:"));
    assert_eq!(h.db.builder_queue_depth().await.unwrap(), 1);
    let status = h
        .get(&c, &format!("{}?month={MONTH}", build_path(&c)))
        .await;
    assert_eq!(status.status, 202);
    assert_eq!(status.body["code"], "AUDIT_BUILDING");
}

#[tokio::test]
async fn a_snapshot_and_the_batches_after_it_rebuild_the_hubs_database() {
    let h = Harness::new().await;
    let c = h.paid_cafe().await;
    let prefix = branch_prefix(&c.tenant, &c.branch);
    let dir = tempfile::tempdir().unwrap();

    // The Hub snapshots after its seventh change and goes on to the twelfth.
    let log = changes("1:HUBA", 1, 12);
    let at_snapshot = hub(dir.path(), "snap.db", &log[..7]);
    let snap = build_snapshot_file(at_snapshot.path(), dir.path(), "2026-12-31").unwrap();
    put(
        &h.keys,
        &snapshot_key(&prefix, "2026-12-31"),
        snap.read().unwrap(),
    );
    let batches = put_batches(&h.keys, &prefix, &log, 5, None);
    // The first batch is inside the snapshot: it is never read.
    put(&h.keys, &batches[0], b"never read".to_vec());
    let full = hub(dir.path(), "hub.db", &log);

    assert_eq!(request(&h, &c, MONTH).await.status, 202);
    let scratch = tempfile::tempdir().unwrap();
    let job = builder(&h, scratch.path())
        .run_one()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.state, AuditBuildState::Ready, "{:?}", job.error);
    assert_eq!(job.progress, 100);
    assert_eq!(job.key.as_deref(), Some(cache_key(&prefix, MONTH).as_str()));
    assert_eq!(job.built_at, Some(T0));

    let audit = rebuilt(&h, &c, dir.path());
    assert_eq!(
        state_checksum(&audit).unwrap(),
        state_checksum(&full).unwrap(),
        "the rebuilt database matches the Hub's at the same seq"
    );
    assert_eq!(audit.changes().unwrap().len(), log.len());
    assert_eq!(
        std::fs::read_dir(scratch.path()).unwrap().count(),
        0,
        "the temporary database is deleted"
    );

    // Ready and fresh: a repeat reuses it and starts nothing.
    let ready = request(&h, &c, MONTH).await;
    assert_eq!(ready.status, 200, "{}", ready.body);
    assert_eq!(ready.body["state"], "ready");
    assert_eq!(ready.body["progress"], 100);
    assert!(ready.body.get("code").is_none());
    let job = h.db.builder_job(&c.branch, MONTH).await.unwrap().unwrap();
    assert_eq!(job.attempt, 2, "request, claim; the repeat adds none");
}

#[tokio::test]
async fn with_no_snapshot_the_first_batch_starts_the_replay() {
    let h = Harness::new().await;
    let c = h.paid_cafe().await;
    let prefix = branch_prefix(&c.tenant, &c.branch);
    let dir = tempfile::tempdir().unwrap();
    let log = changes("1:HUBA", 1, 9);
    put_batches(&h.keys, &prefix, &log, 4, None);
    let full = hub(dir.path(), "hub.db", &log);

    request(&h, &c, MONTH).await;
    let scratch = tempfile::tempdir().unwrap();
    let job = builder(&h, scratch.path())
        .run_one()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.state, AuditBuildState::Ready, "{:?}", job.error);
    let audit = rebuilt(&h, &c, dir.path());
    assert_eq!(
        state_checksum(&audit).unwrap(),
        state_checksum(&full).unwrap()
    );
}

#[tokio::test]
async fn rows_above_a_fork_point_are_listed_apart_and_never_replayed() {
    let h = Harness::new().await;
    let c = h.paid_cafe().await;
    let prefix = branch_prefix(&c.tenant, &c.branch);
    let dir = tempfile::tempdir().unwrap();
    // Epoch 1 wrote eight changes; the replacement Hub's epoch 2 forked after the sixth.
    let first = changes("1:HUBA", 1, 8);
    let second = changes("2:HUBB", 7, 9);
    put_batches(&h.keys, &prefix, &first, 4, None);
    put_batches(
        &h.keys,
        &prefix,
        &second,
        4,
        Some(BatchFork {
            parent_epoch: "1:HUBA".to_owned(),
            parent_seq: 6,
        }),
    );
    let mut kept = first[..6].to_vec();
    kept.extend(second.clone());
    let full = hub(dir.path(), "hub.db", &kept);

    request(&h, &c, MONTH).await;
    let scratch = tempfile::tempdir().unwrap();
    let job = builder(&h, scratch.path())
        .run_one()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.state, AuditBuildState::Ready, "{:?}", job.error);
    let audit = rebuilt(&h, &c, dir.path());
    assert_eq!(
        state_checksum(&audit).unwrap(),
        state_checksum(&full).unwrap()
    );
    let orphans: Vec<i64> = {
        let conn = rusqlite::Connection::open(audit.path()).unwrap();
        let mut stmt = conn
            .prepare("SELECT seq FROM orphaned_changes ORDER BY seq")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(orphans, vec![7, 8]);
}

#[tokio::test]
async fn a_missing_batch_fails_the_job_and_a_new_request_starts_another_attempt() {
    let h = Harness::new().await;
    let c = h.paid_cafe().await;
    let prefix = branch_prefix(&c.tenant, &c.branch);
    let log = changes("1:HUBA", 1, 12);
    let keys = put_batches(&h.keys, &prefix, &log, 4, None);
    let (middle, _) = h.keys.read(&keys[1]).unwrap();
    remove(&h.keys, &keys[1]);

    request(&h, &c, MONTH).await;
    let scratch = tempfile::tempdir().unwrap();
    let b = builder(&h, scratch.path());
    let job = b.run_one().await.unwrap().unwrap();
    assert_eq!(job.state, AuditBuildState::Failed);
    assert!(
        job.error.as_deref().unwrap_or("").contains("seq gap"),
        "{:?}",
        job.error
    );
    assert!(h.keys.read(&cache_key(&prefix, MONTH)).is_none());

    let status = h
        .get(&c, &format!("{}?month={MONTH}", build_path(&c)))
        .await;
    assert_eq!(status.status, 502, "{}", status.body);
    assert_eq!(status.code(), "AUDIT_BUILD_FAILED");

    // The gap is filled; asking again is a new attempt.
    put(&h.keys, &keys[1], middle);
    let again = request(&h, &c, MONTH).await;
    assert_eq!(again.status, 202, "{}", again.body);
    assert_eq!(again.body["state"], "queued");
    let job = b.run_one().await.unwrap().unwrap();
    assert_eq!(job.state, AuditBuildState::Ready, "{:?}", job.error);
    assert_eq!(job.attempt, 4, "request, claim, request, claim");
}

#[tokio::test]
async fn a_batch_that_fails_its_checksum_fails_the_job() {
    let h = Harness::new().await;
    let c = h.paid_cafe().await;
    let prefix = branch_prefix(&c.tenant, &c.branch);
    let log = changes("1:HUBA", 1, 8);
    let keys = put_batches(&h.keys, &prefix, &log, 4, None);
    let (mut bytes, _) = h.keys.read(&keys[1]).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xff;
    put(&h.keys, &keys[1], bytes);

    request(&h, &c, MONTH).await;
    let scratch = tempfile::tempdir().unwrap();
    let job = builder(&h, scratch.path())
        .run_one()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.state, AuditBuildState::Failed);
    assert!(
        job.error
            .as_deref()
            .unwrap_or("")
            .contains("failed its check"),
        "{:?}",
        job.error
    );
}

#[tokio::test]
async fn a_tenant_over_its_build_limit_is_told_to_wait() {
    let h = Harness::new().await;
    let c = h.paid_cafe().await;
    for month in [
        "2026-07", "2026-08", "2026-09", "2026-10", "2026-11", "2026-12",
    ] {
        assert_eq!(request(&h, &c, month).await.status, 202, "{month}");
    }
    let seventh = request(&h, &c, MONTH).await;
    assert_eq!(seventh.status, 429, "{}", seventh.body);
    assert_eq!(seventh.code(), "RATE_LIMITED");
    assert!(seventh.retry_after.is_some());
    assert!(h.db.builder_job(&c.branch, MONTH).await.unwrap().is_none());
    // A month already queued is still answered: it starts nothing.
    assert_eq!(request(&h, &c, "2026-07").await.status, 202);
    assert_eq!(h.db.builder_queue_depth().await.unwrap(), 6);
    // An hour later the window has moved on (the owner's session has not, so ask directly).
    h.advance(61 * 60 * 1000);
    let later = ask(&h, &c).await;
    assert!(matches!(later, Requested::Queued(_)), "{later:?}");
}

#[tokio::test]
async fn only_the_owner_of_a_paid_branch_may_ask_for_a_real_month() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let b = h.cafe(PHONE_B, "Level Up", "198.51.100.5").await;

    // Free uploads nothing to build from.
    let free = request(&h, &b, MONTH).await;
    assert_eq!(free.status, 403);
    assert_eq!(free.code(), "FEATURE_NOT_ENTITLED");
    // Another cafe's branch is not found.
    let other = h
        .call(
            "POST",
            &build_path(&b),
            Some(json!({"month": MONTH})),
            &a.access,
        )
        .await;
    assert_eq!(other.status, 404);
    // Not a month, or one that has not happened yet.
    for bad in ["2027-13", "nope", "2027-1", "2027-02"] {
        let r = request(&h, &a, bad).await;
        assert_eq!(r.status, 422, "{bad}: {}", r.body);
        assert_eq!(r.body["field"], "month");
    }
    let all = h
        .call(
            "POST",
            "/v1/branches/all/audit/build",
            Some(json!({"month": MONTH})),
            &a.access,
        )
        .await;
    assert_eq!(all.status, 422);
    let anon = h
        .call("POST", &build_path(&a), Some(json!({"month": MONTH})), "")
        .await;
    assert_eq!(anon.status, 401);
    assert!(h.db.builder_job(&a.branch, MONTH).await.unwrap().is_none());
    assert_eq!(h.db.builder_queue_depth().await.unwrap(), 0);
}

/// Asks the queue directly, as the owner at the current time (a session lapses over a day).
async fn ask(h: &Harness, c: &Cafe) -> Requested {
    request_build(
        &h.db,
        &c.tenant,
        &c.branch,
        MONTH,
        "owner:x",
        None,
        h.now.load(Ordering::SeqCst),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn a_built_month_expires_after_24_hours_and_is_built_again_on_request() {
    let h = Harness::new().await;
    let c = h.paid_cafe().await;
    let prefix = branch_prefix(&c.tenant, &c.branch);
    put_batches(&h.keys, &prefix, &changes("1:HUBA", 1, 6), 3, None);
    request(&h, &c, MONTH).await;
    let scratch = tempfile::tempdir().unwrap();
    let b = builder(&h, scratch.path());
    assert_eq!(
        b.run_one().await.unwrap().unwrap().state,
        AuditBuildState::Ready
    );
    let key = cache_key(&prefix, MONTH);
    assert!(h.keys.read(&key).is_some());

    // The owner's session lapses over a day, so these ask the queue directly.
    h.advance(23 * 60 * 60 * 1000);
    assert_eq!(b.sweep_expired().await.unwrap(), 0, "still inside 24 hours");
    assert!(matches!(ask(&h, &c).await, Requested::Existing(_)));

    h.advance(61 * 60 * 1000);
    assert_eq!(b.sweep_expired().await.unwrap(), 1);
    assert!(h.keys.read(&key).is_none(), "the cache object is gone");
    let again = ask(&h, &c).await;
    assert!(matches!(again, Requested::Queued(_)), "{again:?}");
    assert_eq!(
        b.run_one().await.unwrap().unwrap().state,
        AuditBuildState::Ready
    );
    assert!(h.keys.read(&key).is_some());
}

#[tokio::test]
async fn a_build_stops_at_the_end_of_its_month() {
    let h = Harness::new().await;
    let c = h.paid_cafe().await;
    let prefix = branch_prefix(&c.tenant, &c.branch);
    let dir = tempfile::tempdir().unwrap();
    // Changes 9 to 12 were written in February.
    let mut log = changes("1:HUBA", 1, 8);
    log.extend((9..=12).map(|s| change("1:HUBA", s, "2027-02-02")));
    put_batches(&h.keys, &prefix, &log, 4, None);
    let january = hub(dir.path(), "hub.db", &log[..8]);

    request(&h, &c, MONTH).await;
    let scratch = tempfile::tempdir().unwrap();
    let job = builder(&h, scratch.path())
        .run_one()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.state, AuditBuildState::Ready, "{:?}", job.error);
    let audit = rebuilt(&h, &c, dir.path());
    assert_eq!(
        state_checksum(&audit).unwrap(),
        state_checksum(&january).unwrap()
    );
}

#[tokio::test]
async fn a_dead_builders_job_is_taken_again_after_the_stale_window() {
    let h = Harness::new().await;
    let c = h.paid_cafe().await;
    let prefix = branch_prefix(&c.tenant, &c.branch);
    put_batches(&h.keys, &prefix, &changes("1:HUBA", 1, 4), 4, None);
    request(&h, &c, MONTH).await;
    let now = h.now.load(Ordering::SeqCst);
    let claimed =
        h.db.claim_builder_job(now, now - 15 * 60 * 1000)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(claimed.state, AuditBuildState::Building);
    // Nobody finishes it; a second builder finds nothing while it is fresh.
    let scratch = tempfile::tempdir().unwrap();
    let b = builder(&h, scratch.path());
    assert!(b.run_one().await.unwrap().is_none());
    h.advance(16 * 60 * 1000);
    let job = b.run_one().await.unwrap().unwrap();
    assert_eq!(job.state, AuditBuildState::Ready, "{:?}", job.error);
    // The dead builder's late write is refused.
    assert!(
        !h.db
            .set_builder_progress(&c.branch, MONTH, claimed.attempt, 50, now)
            .await
            .unwrap()
    );
}

// ---- Story 8.37: searches over a built month, and the support path ----

fn with_cache(h: &Harness, dir: &std::path::Path, max_bytes: u64) {
    assert!(
        h.st.audit_cache
            .set(AuditCache::new(dir.to_path_buf(), max_bytes))
            .is_ok()
    );
}

fn audit_path(c: &Cafe, query: &str) -> String {
    format!("/v1/branches/{}/audit?month={MONTH}&{query}", c.branch)
}

/// `n` January changes: every tenth is the system's `sessions.start`, the rest the staff
/// member's `settings.set`.
fn mixed_log(n: i64) -> Vec<ChangeRecord> {
    changes("1:HUBA", 1, n)
        .into_iter()
        .map(|mut c| {
            if c.seq % 10 == 0 {
                c.actor_kind = "system".to_owned();
                c.actor_id = "hub".to_owned();
                c.intent_kind = "sessions.start".to_owned();
            }
            c
        })
        .collect()
}

/// Queues and builds `month` of `c`'s branch.
async fn build_month(h: &Harness, c: &Cafe, month: &str, scratch: &std::path::Path) {
    assert!(matches!(
        request_build(
            &h.db,
            &c.tenant,
            &c.branch,
            month,
            "owner:x",
            None,
            h.now.load(Ordering::SeqCst)
        )
        .await
        .unwrap(),
        Requested::Queued(_)
    ));
    let job = builder(h, scratch).run_one().await.unwrap().unwrap();
    assert_eq!(job.state, AuditBuildState::Ready, "{:?}", job.error);
}

fn seqs(page: &Value, key: &str) -> Vec<i64> {
    page[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["seq"].as_i64().unwrap())
        .collect()
}

#[tokio::test]
async fn a_built_month_answers_the_catalogue_queries_fifty_rows_a_page_from_its_local_copy() {
    let h = Harness::new().await;
    let c = h.paid_cafe().await;
    let prefix = branch_prefix(&c.tenant, &c.branch);
    let cache = tempfile::tempdir().unwrap();
    with_cache(&h, cache.path(), u64::MAX);
    put_batches(&h.keys, &prefix, &mixed_log(60), 20, None);
    let scratch = tempfile::tempdir().unwrap();
    build_month(&h, &c, MONTH, scratch.path()).await;

    let first = h.get(&c, &audit_path(&c, "q=recent")).await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(first.body["month"], MONTH);
    assert_eq!(first.body["built_at"], T0);
    assert_eq!(seqs(&first.body, "items").len(), 50);
    assert_eq!(seqs(&first.body, "items")[0], 60, "newest first");
    assert!(seqs(&first.body, "orphaned").is_empty());
    let item = &first.body["items"][1];
    assert_eq!(item["module"], "settings");
    assert_eq!(item["actor"]["kind"], "staff");
    assert_eq!(item["actor"]["id"], "S1");
    assert!(
        !first.body.to_string().contains("\"v59\""),
        "no row image leaves the cache"
    );
    let cursor = first.body["next_cursor"].as_str().unwrap().to_owned();

    // The object may go; the local copy answers.
    remove(&h.keys, &cache_key(&prefix, MONTH));
    let second = h
        .get(&c, &audit_path(&c, &format!("q=recent&cursor={cursor}")))
        .await;
    assert_eq!(second.status, 200, "{}", second.body);
    assert_eq!(seqs(&second.body, "items").len(), 10);
    assert_eq!(seqs(&second.body, "items")[9], 1);
    assert!(second.body["next_cursor"].is_null());

    let system = h
        .get(&c, &audit_path(&c, "q=actor&actor_kind=system"))
        .await;
    assert_eq!(seqs(&system.body, "items"), vec![60, 50, 40, 30, 20, 10]);
    assert_eq!(
        system.body["items"][0]["actor"]["name"],
        "hub.audit.actor.system"
    );
    let nobody = h
        .get(
            &c,
            &audit_path(&c, "q=actor&actor_kind=staff&actor_id=nobody"),
        )
        .await;
    assert!(seqs(&nobody.body, "items").is_empty());
    let module = h.get(&c, &audit_path(&c, "q=module&module=sessions")).await;
    assert_eq!(seqs(&module.body, "items").len(), 6);
    let intent = h
        .get(&c, &audit_path(&c, "q=intent&intent=sessions.start"))
        .await;
    assert_eq!(seqs(&intent.body, "items").len(), 6);
    let row_id = first.body["items"][1]["row_id"].as_str().unwrap();
    let entity = h
        .get(
            &c,
            &audit_path(&c, &format!("q=entity&tbl=settings&row_id={row_id}")),
        )
        .await;
    assert_eq!(seqs(&entity.body, "items"), vec![59]);
    let range = h
        .get(
            &c,
            &audit_path(&c, &format!("q=time&from={}&to={}", T0 + 10, T0 + 20)),
        )
        .await;
    assert_eq!(seqs(&range.body, "items").len(), 10, "to is exclusive");
}

#[tokio::test]
async fn any_other_query_shape_is_unsupported_and_a_bad_value_is_a_validation_error() {
    let h = Harness::new().await;
    let c = h.paid_cafe().await;
    for query in [
        "q=sql&text=select+1",
        "q=recent&limit=500",
        "q=recent&where=1%3D1",
        "q=recent&actor_kind=staff",
        "text=hello",
    ] {
        let r = h.get(&c, &audit_path(&c, query)).await;
        assert_eq!((r.status, r.code()), (400, "UNSUPPORTED"), "{query}");
    }
    let r = h.get(&c, &audit_path(&c, "q=actor")).await;
    assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));
    let r = h
        .get(
            &c,
            &format!("/v1/branches/{}/audit?q=recent&month=soon", c.branch),
        )
        .await;
    assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));
}

#[tokio::test]
async fn a_month_that_is_not_ready_answers_its_build_state() {
    let h = Harness::new().await;
    let c = h.paid_cafe().await;
    let cache = tempfile::tempdir().unwrap();
    with_cache(&h, cache.path(), u64::MAX);
    let never = h.get(&c, &audit_path(&c, "q=recent")).await;
    assert_eq!((never.status, never.code()), (404, "NOT_FOUND"));

    request(&h, &c, MONTH).await;
    let queued = h.get(&c, &audit_path(&c, "q=recent")).await;
    assert_eq!(queued.status, 202, "{}", queued.body);
    assert_eq!(queued.body["code"], "AUDIT_BUILDING");
    assert_eq!(queued.body["state"], "queued");

    let now = h.now.load(Ordering::SeqCst);
    let job =
        h.db.claim_builder_job(now, now - 15 * 60 * 1000)
            .await
            .unwrap()
            .unwrap();
    h.db.set_builder_progress(&c.branch, MONTH, job.attempt, 40, now)
        .await
        .unwrap();
    let building = h.get(&c, &audit_path(&c, "q=recent")).await;
    assert_eq!(building.status, 202);
    assert_eq!(building.body["state"], "building");
    assert_eq!(building.body["progress"], 40);

    h.db.fail_builder_job(&c.branch, MONTH, job.attempt, "a batch is missing", now)
        .await
        .unwrap();
    let failed = h.get(&c, &audit_path(&c, "q=recent")).await;
    assert_eq!((failed.status, failed.code()), (502, "AUDIT_BUILD_FAILED"));
}

#[tokio::test]
async fn orphaned_rows_come_back_in_their_own_list_on_the_first_page() {
    let h = Harness::new().await;
    let c = h.paid_cafe().await;
    let prefix = branch_prefix(&c.tenant, &c.branch);
    let cache = tempfile::tempdir().unwrap();
    with_cache(&h, cache.path(), u64::MAX);
    put_batches(&h.keys, &prefix, &changes("1:HUBA", 1, 8), 4, None);
    put_batches(
        &h.keys,
        &prefix,
        &changes("2:HUBB", 7, 9),
        4,
        Some(BatchFork {
            parent_epoch: "1:HUBA".to_owned(),
            parent_seq: 6,
        }),
    );
    let scratch = tempfile::tempdir().unwrap();
    build_month(&h, &c, MONTH, scratch.path()).await;

    let page = h.get(&c, &audit_path(&c, "q=recent")).await;
    assert_eq!(page.status, 200, "{}", page.body);
    assert_eq!(
        seqs(&page.body, "items").len(),
        9,
        "six kept, three of epoch 2"
    );
    let orphans = page.body["orphaned"].as_array().unwrap();
    assert!(orphans.iter().all(|o| o["epoch"] == "1:HUBA"));
    assert_eq!(seqs(&page.body, "orphaned"), vec![8, 7]);
    let none = h.get(&c, &audit_path(&c, "q=module&module=money")).await;
    assert!(
        seqs(&none.body, "orphaned").is_empty(),
        "orphans follow the filters"
    );
}

#[tokio::test]
async fn a_month_whose_object_is_gone_is_queued_again() {
    let h = Harness::new().await;
    let c = h.paid_cafe().await;
    let prefix = branch_prefix(&c.tenant, &c.branch);
    let cache = tempfile::tempdir().unwrap();
    with_cache(&h, cache.path(), u64::MAX);
    put_batches(&h.keys, &prefix, &changes("1:HUBA", 1, 6), 3, None);
    let scratch = tempfile::tempdir().unwrap();
    build_month(&h, &c, MONTH, scratch.path()).await;
    remove(&h.keys, &cache_key(&prefix, MONTH));

    let r = h.get(&c, &audit_path(&c, "q=recent")).await;
    assert_eq!(r.status, 202, "{}", r.body);
    assert_eq!(r.body["code"], "AUDIT_BUILDING");
    let job = h.db.builder_job(&c.branch, MONTH).await.unwrap().unwrap();
    assert_eq!(job.state, AuditBuildState::Queued);
    assert_eq!(job.attempt, 3, "request, claim, queued again");
    // The builder makes it again and the search answers.
    let built = builder(&h, scratch.path())
        .run_one()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(built.state, AuditBuildState::Ready);
    let r = h.get(&c, &audit_path(&c, "q=recent")).await;
    assert_eq!(r.status, 200, "{}", r.body);
}

#[tokio::test]
async fn the_local_cache_drops_the_least_recently_used_month_and_fetches_it_again() {
    let h = Harness::new().await;
    let c = h.paid_cafe().await;
    let prefix = branch_prefix(&c.tenant, &c.branch);
    let cache = tempfile::tempdir().unwrap();
    // Room for no month but the one in use.
    with_cache(&h, cache.path(), 1);
    put_batches(&h.keys, &prefix, &changes("1:HUBA", 1, 6), 3, None);
    let scratch = tempfile::tempdir().unwrap();
    build_month(&h, &c, MONTH, scratch.path()).await;
    build_month(&h, &c, "2026-12", scratch.path()).await;
    let files = || {
        std::fs::read_dir(cache.path().join(&c.branch))
            .map(|d| d.count())
            .unwrap_or(0)
    };

    let january = h.get(&c, &audit_path(&c, "q=recent")).await;
    assert_eq!(january.status, 200, "{}", january.body);
    assert_eq!(files(), 1);
    std::thread::sleep(std::time::Duration::from_millis(30));
    let december = h
        .get(
            &c,
            &format!("/v1/branches/{}/audit?month=2026-12&q=recent", c.branch),
        )
        .await;
    assert_eq!(december.status, 200, "{}", december.body);
    assert_eq!(files(), 1, "January was evicted for December");

    let again = h.get(&c, &audit_path(&c, "q=recent")).await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(seqs(&again.body, "items").len(), 6, "fetched from R2 again");
    assert_eq!(files(), 1);
}

const STAFF_TOKEN: &str = "staff-secret-token";

fn staff_tokens(h: &Harness) {
    let spec = format!("asha:{}", arena_cloud::admin::token_hash_hex(STAFF_TOKEN));
    assert!(
        h.st.admin_tokens
            .set(arena_cloud::admin::AdminTokens::parse(&spec).unwrap())
            .is_ok()
    );
}

async fn admin(h: &Harness, method: &str, path: &str, body: Option<Value>, token: &str) -> Reply {
    let mut req = Request::builder().method(method).uri(path);
    if !token.is_empty() {
        req = req.header("authorization", format!("Bearer {token}"));
    }
    let req = match body {
        Some(b) => req
            .header("content-type", "application/json")
            .body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .unwrap();
    reply(h.app.clone().oneshot(req).await.unwrap()).await
}

#[tokio::test]
async fn the_support_path_needs_a_staff_credential_and_a_reason_and_is_audited() {
    let h = Harness::new().await;
    let c = h.paid_cafe().await;
    let prefix = branch_prefix(&c.tenant, &c.branch);
    let cache = tempfile::tempdir().unwrap();
    with_cache(&h, cache.path(), u64::MAX);
    put_batches(&h.keys, &prefix, &changes("1:HUBA", 1, 6), 3, None);
    let build = format!("/v1/admin/branches/{}/audit/build", c.branch);
    let search = format!(
        "/v1/admin/branches/{}/audit?month={MONTH}&q=recent&reason=refund+dispute",
        c.branch
    );

    // No credentials configured: the API is not there.
    let r = admin(&h, "GET", &search, None, STAFF_TOKEN).await;
    assert_eq!((r.status, r.code()), (404, "NOT_FOUND"));

    staff_tokens(&h);
    for token in ["", "wrong"] {
        let r = admin(&h, "GET", &search, None, token).await;
        assert_eq!((r.status, r.code()), (401, "AUTH_FAILED"), "{token:?}");
    }
    // An owner's session is not a staff credential.
    let r = h.get(&c, &search).await;
    assert_eq!(r.status, 401);
    assert!(h.db.admin_audit_rows().await.unwrap().is_empty());

    // A reason is required, on both calls.
    let no_reason = format!(
        "/v1/admin/branches/{}/audit?month={MONTH}&q=recent",
        c.branch
    );
    let r = admin(&h, "GET", &no_reason, None, STAFF_TOKEN).await;
    assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));
    assert_eq!(r.body["field"], "reason");
    let r = admin(
        &h,
        "POST",
        &build,
        Some(json!({"month": MONTH, "reason": "  "})),
        STAFF_TOKEN,
    )
    .await;
    assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));
    assert!(h.db.admin_audit_rows().await.unwrap().is_empty());

    // Searching before a build is a recorded call that finds nothing.
    let r = admin(&h, "GET", &search, None, STAFF_TOKEN).await;
    assert_eq!(r.status, 404, "{}", r.body);
    // The clock is fixed: move it on so the rows order by time, not by their random ids.
    h.advance(1);
    let r = admin(
        &h,
        "POST",
        &build,
        Some(json!({"month": MONTH, "reason": "refund dispute"})),
        STAFF_TOKEN,
    )
    .await;
    assert_eq!((r.status, r.code()), (202, "AUDIT_BUILDING"), "{}", r.body);
    let job = h.db.builder_job(&c.branch, MONTH).await.unwrap().unwrap();
    assert_eq!(job.requested_by, "admin:asha");
    assert_eq!(job.reason.as_deref(), Some("refund dispute"));

    let scratch = tempfile::tempdir().unwrap();
    let built = builder(&h, scratch.path())
        .run_one()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(built.state, AuditBuildState::Ready);
    h.advance(1);
    let r = admin(&h, "GET", &search, None, STAFF_TOKEN).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(seqs(&r.body, "items").len(), 6);

    let rows = h.db.admin_audit_rows().await.unwrap();
    let actions: Vec<&str> = rows.iter().map(|r| r.action.as_str()).collect();
    assert_eq!(actions, vec!["audit.search", "audit.build", "audit.search"]);
    for row in &rows {
        assert_eq!(row.actor_kind, "admin");
        assert_eq!(row.actor_id, "asha");
        assert_eq!(row.target_kind, "branch");
        assert_eq!(row.target_id, c.branch);
        assert_eq!(row.reason.as_deref(), Some("refund dispute"));
        let summary: Value = serde_json::from_str(&row.summary).unwrap();
        assert_eq!(summary["month"], MONTH);
    }
    let summary: Value = serde_json::from_str(&rows[2].summary).unwrap();
    assert_eq!(summary["query"]["q"], "recent");
    assert!(summary["query"].get("reason").is_none());

    // Past 24 hours the cache is stale: the support search queues it again.
    h.advance(25 * 60 * 60 * 1000);
    let r = admin(&h, "GET", &search, None, STAFF_TOKEN).await;
    assert_eq!((r.status, r.code()), (202, "AUDIT_BUILDING"), "{}", r.body);
    let job = h.db.builder_job(&c.branch, MONTH).await.unwrap().unwrap();
    assert_eq!(job.state, AuditBuildState::Queued);
    assert_eq!(job.requested_by, "admin:asha");
}

// ---- Story 8.42: a busy month's first audit results ----

/// A busy branch month (`ARENA_AUDIT_ROWS` changes, 30,000 by default; the release rig runs the
/// month of a busy café) asked for the first time: the owner's request, the builder replaying the
/// batches and the first page of the audit answer, in `ARENA_AUDIT_FIRST_MS` (10 s) together.
#[tokio::test]
async fn a_busy_month_asked_for_the_first_time_shows_its_first_results_in_under_ten_seconds() {
    let env = |name: &str, default: i64| -> i64 {
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    let rows = env("ARENA_AUDIT_ROWS", 30_000);
    let budget_ms = env("ARENA_AUDIT_FIRST_MS", 10_000);
    let h = Harness::new().await;
    let c = h.paid_cafe().await;
    let prefix = branch_prefix(&c.tenant, &c.branch);
    let cache = tempfile::tempdir().unwrap();
    with_cache(&h, cache.path(), u64::MAX);
    let log = mixed_log(rows);
    put_batches(&h.keys, &prefix, &log, arena_store::BATCH_MAX_ROWS, None);
    let scratch = tempfile::tempdir().unwrap();

    let started = std::time::Instant::now();
    let queued = request(&h, &c, MONTH).await;
    assert_eq!(queued.status, 202, "{}", queued.body);
    let job = builder(&h, scratch.path())
        .run_one()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.state, AuditBuildState::Ready, "{:?}", job.error);
    let built_ms = i64::try_from(started.elapsed().as_millis()).unwrap();
    let first = h.get(&c, &audit_path(&c, "q=recent")).await;
    assert_eq!(first.status, 200, "{}", first.body);
    let first_ms = i64::try_from(started.elapsed().as_millis()).unwrap();
    assert_eq!(seqs(&first.body, "items").len(), 50);
    assert_eq!(seqs(&first.body, "items")[0], rows, "newest first");

    if let Ok(dir) = std::env::var("ARENA_REMOTE_OUT") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join("audit.json"),
            serde_json::to_vec_pretty(&json!({
                "rows": rows,
                "build_ms": built_ms,
                "first_results_ms": first_ms,
                "budget_ms": budget_ms,
            }))
            .unwrap(),
        )
        .unwrap();
    }
    assert!(first_ms < budget_ms, "{first_ms} ms for {rows} rows");
}
