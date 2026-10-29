//! Owner commands (Story 8.18): the catalogue, validation, fan-out, entitlement, schema range,
//! rate limit and the owner list, against the real router.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use arena_cloud::auth::AuthState;
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
use arena_cloud::presign::Presigner;
use arena_cloud::store::{MemoryStore, S3Config};
use arena_cloud::{Clock, app_with_auth, auth_state};
use arena_proto::Tier;
use arena_store::control::{CompGrant, ControlDb, LATEST_CONTROL_VERSION, NewBackupCode};
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
        Cafe {
            branch,
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

const K1: &str = "01JBQ6W3Y8Z9X0A1B2C3D4E5F6";
const K2: &str = "01JBQ6W3Y8Z9X0A1B2C3D4E5F7";
const K3: &str = "01JBQ6W3Y8Z9X0A1B2C3D4E5F8";

fn rate() -> Value {
    json!({"group_id": "g1", "price_per_hour_minor": 6000})
}

fn post(branch: &str, key: &str, kind: &str, payload: Value) -> Value {
    json!({"branch_id": branch, "idempotency_key": key, "kind": kind, "payload": payload})
}

impl Harness {
    async fn send(&self, c: &Cafe, body: Value) -> Reply {
        self.call("POST", "/v1/commands", Some(body), &c.access)
            .await
    }

    async fn get(&self, c: &Cafe, path: &str) -> Reply {
        self.call("GET", path, None, &c.access).await
    }

    fn advance(&self, ms: i64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }

    async fn extra_branch(&self, c: &Cafe, name: &str, tier: Option<Tier>) -> String {
        let r = self
            .call(
                "POST",
                "/v1/branches",
                Some(json!({"name": name})),
                &c.access,
            )
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
        let id = r.body["id"].as_str().unwrap().to_owned();
        if let Some(t) = tier {
            self.upgrade(&id, t, T0 + 30 * DAY).await;
        }
        id
    }

    fn rows(&self) -> i64 {
        rusqlite::Connection::open(self._dir.path().join("gz_control.db"))
            .unwrap()
            .query_row("SELECT COUNT(*) FROM commands", [], |r| r.get(0))
            .unwrap()
    }
}

#[tokio::test]
async fn an_owner_kind_is_queued_once_per_key() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let r = h.send(&a, post(&a.branch, K1, "rate.upsert", rate())).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let c = &r.body["commands"][0];
    assert_eq!(c["branch_id"], a.branch.as_str());
    assert_eq!(c["kind"], "rate.upsert");
    assert_eq!(c["status"], "pending");
    assert_eq!(c["queued_at"], T0);
    assert!(c["group_id"].is_null());
    assert_eq!(r.body["skipped"].as_array().unwrap().len(), 0);

    let again = h.send(&a, post(&a.branch, K1, "rate.upsert", rate())).await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(again.body["commands"][0]["id"], c["id"]);
    assert_eq!(h.rows(), 1, "nothing new was inserted");

    let stored =
        h.db.command(c["id"].as_i64().unwrap())
            .await
            .unwrap()
            .unwrap();
    let owner = h.db.owner_by_phone(PHONE_A).await.unwrap().unwrap();
    assert_eq!(stored.issued_by, owner.id);
    assert_eq!(stored.schema_version, 1);
}

#[tokio::test]
async fn base_rev_rides_in_the_stored_payload() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let mut body = post(&a.branch, K1, "rate.upsert", rate());
    body["base_rev"] = json!(4);
    let r = h.send(&a, body).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let stored =
        h.db.command(r.body["commands"][0]["id"].as_i64().unwrap())
            .await
            .unwrap()
            .unwrap();
    let p: Value = serde_json::from_str(&stored.payload).unwrap();
    assert_eq!(p["base_rev"], 4);
    assert_eq!(p["price_per_hour_minor"], 6000);
}

#[tokio::test]
async fn kinds_outside_the_catalogue_or_reserved_are_unsupported() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    for kind in [
        "hub.revoke",
        "backup.restore",
        "pricing.set",
        "staff.upsert",
        "branch.go_live",
    ] {
        let r = h.send(&a, post(&a.branch, K1, kind, json!({}))).await;
        assert_eq!(r.status, 400, "{kind}: {}", r.body);
        assert_eq!(r.code(), "UNSUPPORTED", "{kind}");
    }
    assert_eq!(h.rows(), 0);
}

#[tokio::test]
async fn a_bad_key_or_payload_is_refused() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let r = h
        .send(&a, post(&a.branch, "not-a-ulid", "rate.upsert", rate()))
        .await;
    assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));
    let bad = json!({"group_id": "g1"});
    let r = h.send(&a, post(&a.branch, K1, "rate.upsert", bad)).await;
    assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));
    let r = h
        .send(&a, post(&a.branch, K1, "backup.now", json!({"x": 1})))
        .await;
    assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));
    let r = h
        .send(&a, post(&a.branch, K1, "backup.now", json!({})))
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(h.rows(), 1);
}

#[tokio::test]
async fn only_the_owners_paid_branches_are_valid_targets() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let free = h.extra_branch(&a, "Free Arena", None).await;
    let r = h.send(&a, post(&free, K1, "rate.upsert", rate())).await;
    assert_eq!((r.status, r.code()), (403, "FEATURE_NOT_ENTITLED"));

    let b = h.cafe(PHONE_B, "Other Arena", "198.51.100.5").await;
    h.upgrade(&b.branch, Tier::Basic, T0 + 30 * DAY).await;
    let r = h.send(&a, post(&b.branch, K1, "rate.upsert", rate())).await;
    assert_eq!((r.status, r.code()), (404, "NOT_FOUND"));
    let r = h.send(&a, post("nope", K1, "rate.upsert", rate())).await;
    assert_eq!((r.status, r.code()), (404, "NOT_FOUND"));
    assert_eq!(h.rows(), 0);

    let body = post(&a.branch, K1, "rate.upsert", rate());
    let signed_out = h.call("POST", "/v1/commands", Some(body), "").await;
    assert_eq!(signed_out.status, 401);
}

#[tokio::test]
async fn all_fans_out_to_every_entitled_branch_under_one_group() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let two = h.extra_branch(&a, "Arena Two", Some(Tier::Premium)).await;
    let free = h.extra_branch(&a, "Free Arena", None).await;

    let r = h.send(&a, post("all", K1, "rate.upsert", rate())).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let cmds = r.body["commands"].as_array().unwrap();
    assert_eq!(cmds.len(), 2);
    let ids: Vec<&str> = cmds
        .iter()
        .map(|c| c["branch_id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&a.branch.as_str()) && ids.contains(&two.as_str()));
    assert!(
        cmds.iter()
            .all(|c| c["group_id"] == K1 && c["status"] == "pending")
    );
    let skipped = r.body["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["branch_id"], free.as_str());
    assert_eq!(skipped[0]["code"], "FEATURE_NOT_ENTITLED");

    let mut keys = Vec::new();
    for c in cmds {
        let row =
            h.db.command(c["id"].as_i64().unwrap())
                .await
                .unwrap()
                .unwrap();
        assert_ne!(row.idempotency_key, K1);
        assert!(ulid::Ulid::from_string(&row.idempotency_key).is_ok());
        keys.push(row.idempotency_key);
    }
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), 2, "a key per branch");

    let again = h.send(&a, post("all", K1, "rate.upsert", rate())).await;
    assert_eq!(again.body["commands"], r.body["commands"]);
    assert_eq!(h.rows(), 2);
}

#[tokio::test]
async fn all_with_no_entitled_branch_is_refused() {
    let h = Harness::new().await;
    let a = h.cafe(PHONE_A, "Pixel Arena", "198.51.100.4").await;
    let r = h.send(&a, post("all", K1, "rate.upsert", rate())).await;
    assert_eq!((r.status, r.code()), (403, "FEATURE_NOT_ENTITLED"));
}

#[tokio::test]
async fn a_branch_that_runs_an_older_command_schema_needs_an_update() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let two = h.extra_branch(&a, "Arena Two", Some(Tier::Basic)).await;
    let hub = h.db.active_hub(&a.branch).await.unwrap().unwrap();
    assert_eq!(h.db.branch_command_schema(&a.branch).await.unwrap(), None);

    h.db.record_command_schema(&hub.id, 0).await.unwrap();
    let r = h.send(&a, post(&a.branch, K1, "rate.upsert", rate())).await;
    assert_eq!((r.status, r.code()), (409, "BRANCH_UPDATE_REQUIRED"));

    let r = h.send(&a, post("all", K2, "rate.upsert", rate())).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["commands"].as_array().unwrap().len(), 1);
    assert_eq!(r.body["commands"][0]["branch_id"], two.as_str());
    assert_eq!(r.body["skipped"][0]["branch_id"], a.branch.as_str());
    assert_eq!(r.body["skipped"][0]["code"], "BRANCH_UPDATE_REQUIRED");

    h.db.record_command_schema(&hub.id, 1).await.unwrap();
    let r = h.send(&a, post(&a.branch, K1, "rate.upsert", rate())).await;
    assert_eq!(r.status, 200, "{}", r.body);
}

#[tokio::test]
async fn the_heartbeat_reports_the_hubs_command_schema() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/heartbeat")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", a.token))
        .body(Body::from(
            json!({"epoch": "3:HUBA", "version": "0.2.0", "schema_version": 12,
                   "last_seq_uploaded": 0, "command_schema": 1})
            .to_string(),
        ))
        .unwrap();
    let r = reply(h.app.clone().oneshot(req).await.unwrap()).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        h.db.branch_command_schema(&a.branch).await.unwrap(),
        Some(1)
    );
}

#[tokio::test]
async fn a_tenant_over_its_post_limit_is_rate_limited() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    for i in 0..60u128 {
        let key = ulid::Ulid::from(i + 1).to_string();
        let r = h
            .send(&a, post(&a.branch, &key, "backup.now", json!({})))
            .await;
        assert_eq!(r.status, 200, "{i}: {}", r.body);
    }
    let key = ulid::Ulid::from(1000u128).to_string();
    let r = h
        .send(&a, post(&a.branch, &key, "backup.now", json!({})))
        .await;
    assert_eq!((r.status, r.code()), (429, "RATE_LIMITED"));
    assert!(r.retry_after.is_some());
}

#[tokio::test]
async fn the_owner_lists_the_tenants_commands_newest_first() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let two = h.extra_branch(&a, "Arena Two", Some(Tier::Basic)).await;
    let first = h.send(&a, post(&a.branch, K1, "rate.upsert", rate())).await;
    h.send(&a, post("all", K2, "backup.now", json!({}))).await;
    h.advance(1_000);
    let last = h.send(&a, post(&two, K3, "backup.now", json!({}))).await;
    let id_of = |r: &Reply| r.body["commands"][0]["id"].as_i64().unwrap();

    let list = h.get(&a, "/v1/commands").await;
    assert_eq!(list.status, 200, "{}", list.body);
    let ids: Vec<i64> = list.body["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids.len(), 4);
    assert!(ids.windows(2).all(|w| w[0] > w[1]), "newest first: {ids:?}");
    assert_eq!(ids[0], id_of(&last));
    assert_eq!(*ids.last().unwrap(), id_of(&first));
    assert!(list.body["next_before"].is_null());

    let by_group = h.get(&a, &format!("/v1/commands?group={K2}")).await;
    let grouped = by_group.body["commands"].as_array().unwrap();
    assert_eq!(grouped.len(), 2);
    assert!(
        grouped
            .iter()
            .all(|c| c["group_id"] == K2 && c["kind"] == "backup.now")
    );
    let by_branch = h.get(&a, &format!("/v1/commands?branch={two}")).await;
    assert_eq!(by_branch.body["commands"].as_array().unwrap().len(), 2);
    let none = h.get(&a, "/v1/commands?status=applied").await;
    assert_eq!(none.body["commands"].as_array().unwrap().len(), 0);

    let page = h.get(&a, "/v1/commands?limit=3").await;
    assert_eq!(page.body["commands"].as_array().unwrap().len(), 3);
    let before = page.body["next_before"].as_i64().unwrap();
    let rest = h
        .get(&a, &format!("/v1/commands?limit=3&before={before}"))
        .await;
    assert_eq!(rest.body["commands"].as_array().unwrap().len(), 1);
    assert!(rest.body["next_before"].is_null());

    let bad = h.get(&a, "/v1/commands?status=done").await;
    assert_eq!((bad.status, bad.code()), (422, "VALIDATION_FAILED"));
}

#[tokio::test]
async fn the_list_shows_state_error_and_done_time_and_never_another_cafes_rows() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let b = h.cafe(PHONE_B, "Other Arena", "198.51.100.5").await;
    h.upgrade(&b.branch, Tier::Basic, T0 + 30 * DAY).await;
    let mine = h.send(&a, post(&a.branch, K1, "rate.upsert", rate())).await;
    let theirs = h.send(&b, post(&b.branch, K2, "rate.upsert", rate())).await;
    assert_eq!(theirs.status, 200, "{}", theirs.body);

    h.advance(5_000);
    let id = mine.body["commands"][0]["id"].as_i64().unwrap();
    let error = serde_json::to_string(&json!({"code": "VALIDATION_FAILED", "message": "no group"}))
        .unwrap();
    h.db.resolve_command(&a.branch, id, None, Some(&error), T0 + 5_000)
        .await
        .unwrap();

    let list = h.get(&a, "/v1/commands").await;
    let cmds = list.body["commands"].as_array().unwrap();
    assert_eq!(cmds.len(), 1, "only this cafe's rows");
    assert_eq!(cmds[0]["status"], "rejected");
    assert_eq!(cmds[0]["error"]["code"], "VALIDATION_FAILED");
    assert_eq!(cmds[0]["queued_at"], T0);
    assert_eq!(cmds[0]["done_at"], T0 + 5_000);

    let rejected = h.get(&a, "/v1/commands?status=rejected").await;
    assert_eq!(rejected.body["commands"].as_array().unwrap().len(), 1);
    let path = format!("/v1/commands?branch={}", b.branch);
    let theirs_only = h.get(&a, &path).await;
    assert_eq!(theirs_only.body["commands"].as_array().unwrap().len(), 0);

    let signed_out = h.call("GET", "/v1/commands", None, "").await;
    assert_eq!(signed_out.status, 401);
}

#[tokio::test]
async fn the_hub_still_fetches_its_mailbox_from_the_same_path() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let r = h.send(&a, post(&a.branch, K1, "rate.upsert", rate())).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let req = Request::builder()
        .method("GET")
        .uri("/v1/commands?after=0&epoch=3:HUBA")
        .header("authorization", format!("Bearer {}", a.token))
        .body(Body::empty())
        .unwrap();
    let fetched = reply(h.app.clone().oneshot(req).await.unwrap()).await;
    assert_eq!(fetched.status, 200, "{}", fetched.body);
    assert_eq!(fetched.body["commands"][0]["kind"], "rate.upsert");
    assert_eq!(fetched.body["commands"][0]["idempotency_key"], K1);
}
