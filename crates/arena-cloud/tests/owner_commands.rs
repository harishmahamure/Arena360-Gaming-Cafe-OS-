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
        escrow_key: None,
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
async fn a_credit_reminder_names_only_the_player_and_needs_no_step_up() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let r = h
        .send(
            &a,
            post(&a.branch, K1, "credit.remind", json!({"player_id": "P1"})),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["commands"][0]["kind"], "credit.remind");
    assert_eq!(r.body["commands"][0]["status"], "pending");
    assert_eq!(r.body["commands"][0]["payload"], json!({"player_id": "P1"}));
    for bad in [
        json!({}),
        json!({"player_id": ""}),
        json!({"player_id": "P1", "phone": "+919876543210"}),
        json!({"player_id": "P1", "amount_minor": "5"}),
    ] {
        let r = h
            .send(&a, post(&a.branch, K2, "credit.remind", bad.clone()))
            .await;
        assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"), "{bad}");
    }
    assert_eq!(h.rows(), 1);
}

#[tokio::test]
async fn kinds_outside_the_catalogue_or_reserved_are_unsupported() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    for kind in ["pricing.set", "branch.go_live", "backup.verify"] {
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
    h.db.resolve_command(&a.branch, id, None, Some(&error), "1:HUB", T0 + 5_000)
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

// ---- Story 8.20: the sweeper, expiry and epoch fencing ----

const MIN: i64 = 60 * 1000;
const HUB_A: &str = "1:HUBA";
const HUB_B: &str = "2:HUBB";

impl Harness {
    async fn hub_fetch(&self, c: &Cafe, after: i64, epoch: &str) -> Reply {
        let req = Request::builder()
            .method("GET")
            .uri(format!("/v1/commands?after={after}&epoch={epoch}"))
            .header("authorization", format!("Bearer {}", c.token))
            .body(Body::empty())
            .unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    async fn hub_result(&self, c: &Cafe, id: i64, body: Value) -> Reply {
        let req = Request::builder()
            .method("POST")
            .uri(format!("/v1/commands/{id}/result"))
            .header("authorization", format!("Bearer {}", c.token))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    async fn sweep(&self) -> arena_cloud::command_sweep::SweepOutcome {
        arena_cloud::command_sweep::sweep_once(&self.db, self.now.load(Ordering::SeqCst))
            .await
            .unwrap()
    }

    async fn status_of(&self, c: &Cafe, key: &str) -> String {
        let id = self.id_of(&c.branch, key);
        let list = self.get(c, "/v1/commands?limit=200").await;
        let cmds = list.body["commands"].as_array().unwrap();
        cmds.iter()
            .find(|cmd| cmd["id"] == id)
            .map_or_else(String::new, |cmd| {
                cmd["status"].as_str().unwrap().to_owned()
            })
    }

    fn id_of(&self, branch: &str, key: &str) -> i64 {
        rusqlite::Connection::open(self._dir.path().join("gz_control.db"))
            .unwrap()
            .query_row(
                "SELECT id FROM commands WHERE branch_id = ? AND idempotency_key = ?",
                [branch, key],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// A command row queued `age_ms` ago (the owner's session would not outlive the wait).
    async fn queue_aged(&self, c: &Cafe, key: &str, age_ms: i64) {
        self.db
            .submit_command(&arena_store::control::NewCommand {
                branch_id: &c.branch,
                idempotency_key: key,
                kind: "rate.upsert",
                schema_version: 1,
                payload: &rate().to_string(),
                issued_by: "owner",
                group_id: None,
                elevated_by: None,
                now: self.now.load(Ordering::SeqCst) - age_ms,
            })
            .await
            .unwrap();
    }

    /// Puts the branch fence at `epoch` with `fork_seq` as its fork point and answers how many
    /// commands went back to `pending` (a branch with no fence yet gets it from a first beat).
    async fn set_fence(&self, c: &Cafe, epoch: &str, fork_seq: i64) -> u64 {
        let hub = self.db.active_hub(&c.branch).await.unwrap().unwrap();
        let now = self.now.load(Ordering::SeqCst);
        let Some(current) = self.db.branch_fence_epoch(&c.branch).await.unwrap() else {
            self.db
                .heartbeat(&arena_store::control::Beat {
                    hub_id: &hub.id,
                    branch_id: &c.branch,
                    epoch,
                    version: "0.2.0",
                    schema_version: 12,
                    fork_seq: Some(fork_seq),
                    now,
                })
                .await
                .unwrap();
            return 0;
        };
        match self
            .db
            .claim_branch_fence(&c.branch, &hub.id, &current, epoch, Some(fork_seq), now)
            .await
            .unwrap()
        {
            arena_store::control::FenceMove::Moved { requeued } => requeued,
            other => panic!("the fence did not move: {other:?}"),
        }
    }

    async fn claim(&self, c: &Cafe, body: Value) -> Reply {
        let req = Request::builder()
            .method("POST")
            .uri("/v1/fence/claim")
            .header("authorization", format!("Bearer {}", c.token))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }
}

fn applied(epoch: &str, seq: i64) -> Value {
    json!({"epoch": epoch, "applied_seq": seq})
}

#[tokio::test]
async fn a_fetch_with_no_answer_for_ten_minutes_is_swept_back_and_delivered_again() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let r = h.send(&a, post(&a.branch, K1, "rate.upsert", rate())).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let first = h.hub_fetch(&a, 0, HUB_A).await;
    let id = first.body["commands"][0]["id"].as_i64().unwrap();
    assert_eq!(h.status_of(&a, K1).await, "fetched");

    h.advance(9 * MIN);
    let early = h.sweep().await;
    assert_eq!((early.requeued, early.expired), (0, 0));
    assert_eq!(h.status_of(&a, K1).await, "fetched");

    h.advance(2 * MIN);
    let swept = h.sweep().await;
    assert_eq!((swept.requeued, swept.expired), (1, 0));
    assert_eq!(h.status_of(&a, K1).await, "pending");
    assert_eq!(swept.backlog.len(), 1);
    assert_eq!(swept.backlog[0].pending, 1);
    assert_eq!(swept.backlog[0].fetched, 0);
    assert_eq!(swept.backlog[0].oldest_created_at, T0);

    // The Hub's cursor is already past the row; a re-queued row is still delivered.
    let again = h.hub_fetch(&a, id + 100, HUB_A).await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(again.body["commands"].as_array().unwrap().len(), 1);
    assert_eq!(again.body["commands"][0]["id"], id);

    let ack = h.hub_result(&a, id, applied(HUB_A, 4)).await;
    assert_eq!(ack.body["status"], "applied");
    assert!(h.sweep().await.backlog.is_empty());
}

#[tokio::test]
async fn pending_commands_expire_after_seven_days_and_are_never_fetched() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    h.queue_aged(&a, K1, 7 * DAY + MIN).await;
    h.queue_aged(&a, K2, 6 * DAY).await;
    let swept = h.sweep().await;
    assert_eq!((swept.expired, swept.requeued), (1, 0));
    assert_eq!(h.status_of(&a, K1).await, "expired");
    assert_eq!(h.status_of(&a, K2).await, "pending");

    let list = h.get(&a, "/v1/commands?status=expired").await;
    let cmds = list.body["commands"].as_array().unwrap();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0]["error"]["code"], "COMMAND_EXPIRED");
    assert!(cmds[0]["done_at"].as_i64().is_some());

    let fetched = h.hub_fetch(&a, 0, HUB_A).await;
    let keys: Vec<&str> = fetched.body["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["idempotency_key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, [K2]);
    // A second sweep changes nothing.
    let again = h.sweep().await;
    assert_eq!((again.expired, again.requeued), (0, 0));
}

#[tokio::test]
async fn a_hub_below_the_fence_neither_fetches_nor_answers() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    h.set_fence(&a, HUB_B, 0).await;
    let r = h.send(&a, post(&a.branch, K1, "rate.upsert", rate())).await;
    assert_eq!(r.status, 200, "{}", r.body);

    let old = h.hub_fetch(&a, 0, HUB_A).await;
    assert_eq!((old.status, old.code()), (409, "FENCED"));
    assert!(old.body["message"].as_str().unwrap().contains(HUB_B));
    assert!(old.body.get("commands").is_none());
    assert_eq!(h.status_of(&a, K1).await, "pending");

    let new = h.hub_fetch(&a, 0, HUB_B).await;
    let id = new.body["commands"][0]["id"].as_i64().unwrap();
    assert_eq!(h.status_of(&a, K1).await, "fetched");

    let refused = h.hub_result(&a, id, applied(HUB_A, 5)).await;
    assert_eq!((refused.status, refused.code()), (409, "FENCED"));
    assert_eq!(h.status_of(&a, K1).await, "pending");

    let ok = h.hub_result(&a, id, applied(HUB_B, 1)).await;
    assert_eq!(ok.status, 200, "{}", ok.body);
    assert_eq!(h.status_of(&a, K1).await, "applied");
}

#[tokio::test]
async fn a_fence_move_returns_commands_applied_past_the_fork() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    h.set_fence(&a, HUB_A, 0).await;
    for k in [K1, K2, K3] {
        let r = h.send(&a, post(&a.branch, k, "rate.upsert", rate())).await;
        assert_eq!(r.status, 200, "{}", r.body);
    }
    let page = h.hub_fetch(&a, 0, HUB_A).await;
    let ids: Vec<i64> = page.body["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids.len(), 3);
    h.hub_result(&a, ids[0], applied(HUB_A, 7)).await;
    h.hub_result(&a, ids[1], applied(HUB_A, 12)).await;
    let err = json!({"epoch": HUB_A, "error": {"code": "VALIDATION_FAILED", "message": "no"}});
    h.hub_result(&a, ids[2], err).await;

    assert_eq!(h.set_fence(&a, HUB_B, 9).await, 1);
    assert_eq!(h.status_of(&a, K1).await, "applied");
    assert_eq!(h.status_of(&a, K2).await, "pending");
    assert_eq!(h.status_of(&a, K3).await, "rejected");

    // The new Hub's cursor is past every row; the returned one is delivered anyway.
    let next = h.hub_fetch(&a, ids[2], HUB_B).await;
    let cmds = next.body["commands"].as_array().unwrap();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0]["id"], ids[1]);
    let ack = h.hub_result(&a, ids[1], applied(HUB_B, 10)).await;
    assert_eq!(ack.body["status"], "applied");
}

#[tokio::test]
async fn a_claim_returns_commands_applied_past_its_fork_point() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    h.set_fence(&a, HUB_A, 0).await;
    for k in [K1, K2] {
        let r = h.send(&a, post(&a.branch, k, "rate.upsert", rate())).await;
        assert_eq!(r.status, 200, "{}", r.body);
    }
    let page = h.hub_fetch(&a, 0, HUB_A).await;
    let ids: Vec<i64> = page.body["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_i64().unwrap())
        .collect();
    h.hub_result(&a, ids[0], applied(HUB_A, 7)).await;
    h.hub_result(&a, ids[1], applied(HUB_A, 12)).await;

    let claim = json!({
        "expected": HUB_A,
        "instance_id": "HUBB",
        "reason": "promote",
        "fork": {"parent_epoch": HUB_A, "parent_seq": 9},
    });
    let won = h.claim(&a, claim.clone()).await;
    assert_eq!(won.status, 200, "{}", won.body);
    assert_eq!(won.body["epoch"], HUB_B);
    assert_eq!(h.status_of(&a, K1).await, "applied");
    assert_eq!(h.status_of(&a, K2).await, "pending");

    // The same claim again is stale: the fence is no longer the epoch it expects.
    let again = h.claim(&a, claim).await;
    assert_eq!((again.status, again.code()), (409, "FENCED"));
    assert_eq!(h.status_of(&a, K2).await, "pending");
}

#[tokio::test]
async fn a_fence_moved_during_a_burst_of_100_applies_every_command_exactly_once() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    h.set_fence(&a, HUB_A, 0).await;
    let keys: Vec<String> = (0..100u128)
        .map(|i| ulid::Ulid::from_parts(1_800_000_000_000, i).to_string())
        .collect();
    let payload = rate().to_string();
    for k in &keys {
        h.db.submit_command(&arena_store::control::NewCommand {
            branch_id: &a.branch,
            idempotency_key: k,
            kind: "rate.upsert",
            schema_version: 1,
            payload: &payload,
            issued_by: "owner",
            group_id: None,
            elevated_by: None,
            now: T0,
        })
        .await
        .unwrap();
    }

    // The old Hub applies the first 60 (seq 1..=60) and answers, having fetched 40 more; the fence
    // then moves at fork 30, and the new Hub's database holds only what it was forked at.
    let mut old_applied: Vec<(String, i64)> = Vec::new();
    let mut after = 0;
    'old: loop {
        let page = h.hub_fetch(&a, after, HUB_A).await;
        let cmds = page.body["commands"].as_array().unwrap().clone();
        if cmds.is_empty() {
            break;
        }
        for c in cmds {
            if old_applied.len() == 60 {
                break 'old;
            }
            let seq = i64::try_from(old_applied.len()).unwrap() + 1;
            let id = c["id"].as_i64().unwrap();
            after = id;
            let r = h.hub_result(&a, id, applied(HUB_A, seq)).await;
            assert_eq!(r.status, 200, "{}", r.body);
            old_applied.push((c["idempotency_key"].as_str().unwrap().to_owned(), seq));
        }
    }
    // The 30 applied past the fork plus the 40 fetched and never answered.
    assert_eq!(h.set_fence(&a, HUB_B, 30).await, 70);

    // A late answer from the old Hub is refused.
    let late = h
        .hub_result(&a, h.id_of(&a.branch, &keys[70]), applied(HUB_A, 61))
        .await;
    assert_eq!(late.code(), "FENCED");

    // The new Hub holds the first 30; it applies every other command once, and answers the
    // delivered ones from what it holds.
    let mut held: std::collections::BTreeMap<String, i64> = old_applied
        .iter()
        .filter(|(_, seq)| *seq <= 30)
        .cloned()
        .collect();
    let cursor = h.id_of(&a.branch, &old_applied[29].0);
    let mut applies = 0;
    let mut next_seq = 31;
    loop {
        let page = h.hub_fetch(&a, cursor, HUB_B).await;
        let cmds = page.body["commands"].as_array().unwrap().clone();
        if cmds.is_empty() {
            break;
        }
        for c in cmds {
            let key = c["idempotency_key"].as_str().unwrap().to_owned();
            let seq = *held.entry(key).or_insert_with(|| {
                applies += 1;
                next_seq += 1;
                next_seq - 1
            });
            let r = h
                .hub_result(&a, c["id"].as_i64().unwrap(), applied(HUB_B, seq))
                .await;
            assert_eq!(r.status, 200, "{}", r.body);
        }
    }

    assert_eq!(held.len(), 100);
    assert_eq!(applies, 70);
    let conn = rusqlite::Connection::open(h._dir.path().join("gz_control.db")).unwrap();
    let count = |sql: &str, p: &[&str]| -> i64 {
        conn.query_row(sql, rusqlite::params_from_iter(p), |r| r.get(0))
            .unwrap()
    };
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM commands WHERE status = 'applied'",
            &[]
        ),
        100
    );
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM commands WHERE applied_epoch = ?",
            &[HUB_A]
        ),
        30
    );
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM commands WHERE applied_epoch = ?",
            &[HUB_B]
        ),
        70
    );
}

impl Harness {
    /// The Hub checks in now.
    async fn beat(&self, c: &Cafe) {
        let req = Request::builder()
            .method("POST")
            .uri("/v1/heartbeat")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", c.token))
            .body(Body::from(
                json!({"epoch": "3:HUBA", "version": "0.2.0", "schema_version": 12, "last_seq_uploaded": 0})
                    .to_string(),
            ))
            .unwrap();
        let r = reply(self.app.clone().oneshot(req).await.unwrap()).await;
        assert_eq!(r.status, 200, "{}", r.body);
    }

    async fn preview(&self, c: &Cafe, branch: &str, kind: &str) -> Reply {
        self.call(
            "POST",
            "/v1/commands/preview",
            Some(json!({"branch_id": branch, "kind": kind})),
            &c.access,
        )
        .await
    }
}

#[tokio::test]
async fn the_preview_lists_each_target_with_its_online_state_and_why_one_is_not_offered() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let two = h.extra_branch(&a, "Arena Two", Some(Tier::Premium)).await;
    let free = h.extra_branch(&a, "Free Arena", None).await;
    let hub = h.db.active_hub(&a.branch).await.unwrap().unwrap();
    h.db.record_command_schema(&hub.id, 0).await.unwrap();

    let r = h.preview(&a, "all", "rate.upsert").await;
    assert_eq!(r.status, 200, "{}", r.body);
    let targets = r.body["targets"].as_array().unwrap();
    assert_eq!(targets.len(), 3);
    let of = |id: &str| targets.iter().find(|t| t["branch_id"] == id).unwrap();
    assert_eq!(of(&a.branch)["offered"], false);
    assert_eq!(of(&a.branch)["error"]["code"], "BRANCH_UPDATE_REQUIRED");
    assert_eq!(of(&two)["offered"], true);
    assert!(of(&two).get("error").is_none());
    assert_eq!(of(&free)["offered"], false);
    assert_eq!(of(&free)["error"]["code"], "FEATURE_NOT_ENTITLED");
    assert_eq!(of(&two)["name"], "Arena Two");
    assert_eq!(h.rows(), 0, "a preview queues nothing");
}

#[tokio::test]
async fn the_preview_marks_a_branch_online_only_while_its_hub_is_heard_from() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let heard = |r: &Reply| r.body["targets"][0]["online"].as_bool().unwrap();

    let r = h.preview(&a, &a.branch, "plan.upsert").await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(!heard(&r), "never heard from");
    assert!(r.body["targets"][0].get("last_heartbeat_at").is_none());

    h.beat(&a).await;
    h.advance(60_000);
    let r = h.preview(&a, &a.branch, "plan.upsert").await;
    assert!(heard(&r));
    assert_eq!(r.body["targets"][0]["last_heartbeat_at"], T0);
    h.advance(4 * 60_000);
    assert!(!heard(&h.preview(&a, &a.branch, "plan.upsert").await));
}

#[tokio::test]
async fn the_preview_refuses_unknown_kinds_other_cafes_and_signed_out_callers() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let b = h.cafe(PHONE_B, "Other Arena", "198.51.100.5").await;
    let r = h.preview(&a, &a.branch, "nope").await;
    assert_eq!((r.status, r.code()), (400, "UNSUPPORTED"));
    let r = h.preview(&a, &a.branch, "branch.go_live").await;
    assert_eq!(r.code(), "UNSUPPORTED");
    let r = h.preview(&a, &b.branch, "rate.upsert").await;
    assert_eq!((r.status, r.code()), (404, "NOT_FOUND"));
    let r = h
        .call(
            "POST",
            "/v1/commands/preview",
            Some(json!({"branch_id": a.branch, "kind": "rate.upsert"})),
            "",
        )
        .await;
    assert_eq!(r.status, 401);
}

#[tokio::test]
async fn the_owner_list_carries_the_payload_so_a_retry_can_send_it_again() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let body = json!({
        "branch_id": a.branch, "idempotency_key": K1, "kind": "rate.upsert",
        "payload": rate(), "base_rev": 4,
    });
    let r = h.send(&a, body).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["commands"][0]["payload"]["base_rev"], 4);
    let r = h.get(&a, "/v1/commands").await;
    let p = &r.body["commands"][0]["payload"];
    assert_eq!(p["group_id"], "g1");
    assert_eq!(p["price_per_hour_minor"], 6000);
    assert_eq!(p["base_rev"], 4);
}

#[tokio::test]
async fn only_a_product_may_carry_a_picture_past_the_usual_payload_size() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let image = format!("data:image/jpeg;base64,{}", "A".repeat(200 * 1024 * 4 / 3));
    let product = json!({"name": "Cola", "category": "", "price_minor": 100, "hsn": "",
        "barcode": "", "kitchen": false, "favourite": false, "image": image});
    let r = h
        .send(&a, post(&a.branch, K1, "product.upsert", product.clone()))
        .await;
    assert_eq!(r.status, 200, "{}", r.body);

    let mut rate = rate();
    rate["pad"] = json!("x".repeat(20 * 1024));
    let r = h.send(&a, post(&a.branch, K2, "rate.upsert", rate)).await;
    assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));

    let mut huge = product;
    huge["image"] = json!("A".repeat(300 * 1024));
    let r = h
        .send(&a, post(&a.branch, K3, "product.upsert", huge))
        .await;
    assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));
    assert_eq!(h.rows(), 1);
}

/// Story 8.27: commands waiting for a branch whose Hub tokens are all revoked are rejected with
/// `TOKEN_REVOKED` (the Owner app shows "Device/Hub revoked"); a branch with a live Hub is left alone.
#[tokio::test]
async fn waiting_commands_of_a_branch_with_no_live_hub_token_are_rejected_token_revoked() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let r = h.send(&a, post(&a.branch, K1, "rate.upsert", rate())).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let r = h.send(&a, post(&a.branch, K2, "rate.upsert", rate())).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(h.hub_fetch(&a, 0, HUB_A).await.status, 200);

    let quiet = h.sweep().await;
    assert_eq!(quiet.revoked, 0);
    assert_eq!(h.status_of(&a, K1).await, "fetched");

    rusqlite::Connection::open(h._dir.path().join("gz_control.db"))
        .unwrap()
        .execute(
            "UPDATE hubs SET revoked_at = ?1 WHERE branch_id = ?2",
            rusqlite::params![T0, a.branch],
        )
        .unwrap();
    let swept = h.sweep().await;
    assert_eq!(swept.revoked, 2);
    assert!(swept.backlog.is_empty());
    for key in [K1, K2] {
        assert_eq!(h.status_of(&a, key).await, "rejected");
    }
    let list = h.get(&a, "/v1/commands?status=rejected").await;
    let cmds = list.body["commands"].as_array().unwrap();
    assert_eq!(cmds.len(), 2);
    assert!(cmds.iter().all(|c| c["error"]["code"] == "TOKEN_REVOKED"));
    assert_eq!(h.sweep().await.revoked, 0);
}

#[tokio::test]
async fn the_alert_limits_are_a_settings_key_an_owner_may_set_within_range() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let set = |value: Value| {
        post(
            &a.branch,
            K1,
            "settings.set",
            json!({"key": "owner_alert.thresholds", "value": value}),
        )
    };
    let good = r#"{"cash_variance_minor":null,"large_refund_minor":50000,"large_discount_minor":20000,"large_discount_percent":20}"#;
    for bad in [
        json!(null),
        json!(5),
        json!("nope"),
        json!(r#"{"large_refund_minor":-1,"large_discount_minor":1,"large_discount_percent":20}"#),
        json!(r#"{"large_refund_minor":1,"large_discount_minor":1,"large_discount_percent":101}"#),
    ] {
        let r = h.send(&a, set(bad.clone())).await;
        assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"), "{bad}");
    }
    assert_eq!(h.rows(), 0);
    let r = h.send(&a, set(json!(good))).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(h.rows(), 1);

    // Other keys are still not the owner's to set from here.
    let other = post(
        &a.branch,
        K2,
        "settings.set",
        json!({"key": "idle_lock_minutes", "value": "9"}),
    );
    let r = h.send(&a, other).await;
    assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));
}
