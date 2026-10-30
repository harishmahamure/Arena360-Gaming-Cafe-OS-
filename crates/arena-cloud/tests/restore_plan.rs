//! `POST /v1/restore/plan` (Story 9.12): the checks and the plan against an in-memory listing of
//! the data bucket.
#![allow(dead_code)]
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use arena_cloud::auth::AuthState;
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::gaps::GapCounts;
use arena_cloud::health::Reachability;
use arena_cloud::presign::Presigner;
use arena_cloud::store::{MemoryStore, ObjectStore, S3Config};
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
}

/// An owner's branch with a Hub paired to it.
struct Cafe {
    branch: String,
    tenant: String,
    hub: String,
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

/// A heartbeat body: the Hub says it uploaded `epoch` through `last`.
fn hb(epoch: &str, last: i64) -> Value {
    json!({"epoch": epoch, "version": "0.2.0", "schema_version": 12, "last_seq_uploaded": last})
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

    async fn beat(&self, token: &str, body: Value) -> Reply {
        let req = Request::builder()
            .method("POST")
            .uri("/v1/heartbeat")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(body.to_string()))
            .unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    /// A heartbeat at `epoch` that says the Hub uploaded through `last`.
    async fn beat_at(&self, c: &Cafe, epoch: &str, last: i64) -> Reply {
        self.beat(&c.token, hb(epoch, last)).await
    }

    /// Puts an empty object for every batch in `ranges`.
    async fn hold(&self, c: &Cafe, epoch: &str, ranges: &[(i64, i64)]) {
        for (first, last) in ranges {
            self.keys
                .put(
                    &format!(
                        "tenants/{}/branches/{}/changes/{epoch}/{first:012}-{last:012}.zst",
                        c.tenant, c.branch
                    ),
                    vec![1],
                )
                .await
                .unwrap();
        }
    }

    fn gaps(&self, c: &Cafe) -> GapCounts {
        self.st.gap_metrics.counts(&c.tenant, &c.branch)
    }

    async fn view(&self, c: &Cafe) -> arena_store::control::HubUploadView {
        self.db.hub_upload_view(&c.hub).await.unwrap()
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
        let hub = self
            .db
            .active_hub(&branch)
            .await
            .unwrap()
            .expect("a paired hub")
            .id;
        let tenant = self.db.branch(&branch).await.unwrap().unwrap().tenant_id;
        Cafe {
            branch,
            tenant,
            hub,
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

    /// Lets the heartbeat rate limit (one per 5 s per Hub) pass.
    fn tick(&self) {
        self.now.fetch_add(10_000, Ordering::SeqCst);
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
    let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    Reply { status, body }
}

impl Harness {
    /// A paid branch whose first Hub checked in, with a replacement Hub (`01HUBINSTANCE2`) paired
    /// on top of it. Returns the replacement's cafe handle.
    async fn replacement(&self) -> (Cafe, Cafe) {
        let first = self.paid_cafe().await;
        self.tick();
        assert_eq!(self.beat_at(&first, "1:HUBA", 0).await.status, 200);
        let r = self
            .call(
                "POST",
                "/v1/wizard/install-hub/code",
                Some(json!({})),
                &first.access,
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
                    "platform": "windows",
                    "hub_instance_id": "01HUBINSTANCE2",
                    "version": "0.1.0",
                })
                .to_string(),
            ))
            .unwrap();
        req.extensions_mut().insert(ConnectInfo(SocketAddr::new(
            "198.51.100.5".parse().unwrap(),
            40000,
        )));
        let r = reply(self.app.clone().oneshot(req).await.unwrap()).await;
        assert_eq!(r.status, 200, "{}", r.body);
        let token = r.body["token"].as_str().unwrap().to_owned();
        let hub = self.db.active_hub(&first.branch).await.unwrap().unwrap().id;
        let second = Cafe {
            branch: first.branch.clone(),
            tenant: first.tenant.clone(),
            hub,
            token,
            access: first.access.clone(),
        };
        (first, second)
    }
}

const INSTANCE2: &str = "01HUBINSTANCE2";
const AGE_KEY: &str = "age1v0w7nljdjmtc27xp7rt4zyyrmryhr67skkwxemc2slu9x09dsgjq7dr5j4";

impl Harness {
    async fn plan(&self, token: &str, body: Value) -> Reply {
        let req = Request::builder()
            .method("POST")
            .uri("/v1/restore/plan")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(body.to_string()))
            .unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    /// The owner approved the replacement's key request and the Hub took the key.
    async fn release_key(&self, c: &Cafe) {
        use arena_store::control::{KeyDecision, NewAdminAudit, NewCommand};
        self.db
            .put_key_request("R1", &c.branch, INSTANCE2, AGE_KEY, None, T0)
            .await
            .unwrap();
        let owner = self.db.owner_by_phone(PHONE_A).await.unwrap().unwrap().id;
        let command = NewCommand {
            branch_id: &c.branch,
            idempotency_key: "K1",
            kind: "backup.restore",
            schema_version: 1,
            payload: "{}",
            issued_by: &owner,
            group_id: None,
            elevated_by: Some(&owner),
            now: T0,
        };
        let audit = NewAdminAudit {
            id: "A1",
            at: T0,
            actor_kind: "system",
            actor_id: &owner,
            action: "kek.release",
            target_kind: "branch",
            target_id: &c.branch,
            reason: None,
            summary: "{}",
            request_id: "REQ",
        };
        self.db
            .apply_key_release(&KeyDecision {
                command: &command,
                request_id: "R1",
                approve: true,
                wrapped_kek: Some("V3JhcHBlZA=="),
                audit: &audit,
            })
            .await
            .unwrap();
        self.db
            .fetch_key_release(&c.branch, INSTANCE2, T0)
            .await
            .unwrap();
    }

    async fn put(&self, c: &Cafe, rest: &str) {
        self.keys
            .put(
                &format!("tenants/{}/branches/{}/{rest}", c.tenant, c.branch),
                vec![1],
            )
            .await
            .unwrap();
    }
}

fn body() -> Value {
    json!({"hub_instance_id": INSTANCE2})
}

#[tokio::test]
async fn a_released_replacement_gets_the_snapshot_and_keys_then_the_batches_after_it() {
    let h = Harness::new().await;
    let (_first, c) = h.replacement().await;
    h.release_key(&c).await;
    h.put(&c, "snapshots/2026-10-01.db.zst").await;
    h.put(&c, "snapshots/2026-10-29.db.zst").await;
    h.put(&c, "keys/player-keys.bin").await;
    for (epoch, a, b) in [("1:HUBA", 1, 9), ("1:HUBA", 10, 19), ("2:HUBB", 1, 4)] {
        h.hold(&c, epoch, &[(a, b)]).await;
    }
    // Another branch's objects never appear.
    let other = format!(
        "tenants/{}/branches/OTHER/changes/1:HUBA/{:012}-{:012}.zst",
        c.tenant, 1, 3
    );
    h.keys.put(&other, vec![1]).await.unwrap();

    let r = h.plan(&c.token, body()).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["snapshot"]["date"], "2026-10-29");
    assert!(
        r.body["snapshot"]["url"]
            .as_str()
            .unwrap()
            .contains("2026-10-29")
    );
    assert!(
        r.body["keys_url"]
            .as_str()
            .unwrap()
            .contains("player-keys.bin")
    );
    assert_eq!(r.body["batches_pending"], true);
    assert_eq!(r.body["batches"].as_array().unwrap().len(), 0);
    assert_eq!(r.body["expires_in_s"], 600);

    let r = h
        .plan(
            &c.token,
            json!({"hub_instance_id": INSTANCE2, "after": {"epoch": "1:HUBA", "seq": 9}}),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["batches_pending"], false);
    let got: Vec<(String, i64, i64)> = r.body["batches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            let url = b["url"].as_str().unwrap();
            assert!(url.contains(&c.branch) && !url.contains("OTHER"), "{url}");
            (
                b["epoch"].as_str().unwrap().to_owned(),
                b["first"].as_i64().unwrap(),
                b["last"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(got, [("1:HUBA".into(), 10, 19), ("2:HUBB".into(), 1, 4)]);
}

#[tokio::test]
async fn without_a_snapshot_every_batch_is_listed_at_once() {
    let h = Harness::new().await;
    let (_, c) = h.replacement().await;
    h.release_key(&c).await;
    h.hold(&c, "1:HUBA", &[(1, 9), (10, 19)]).await;
    let r = h.plan(&c.token, body()).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["batches_pending"], false);
    assert_eq!(r.body["batches"].as_array().unwrap().len(), 2);
    assert!(r.body.get("snapshot").is_none() && r.body.get("keys_url").is_none());
}

#[tokio::test]
async fn the_plan_is_refused_without_the_key_a_paid_tier_or_a_valid_request() {
    let h = Harness::new().await;
    let (_, c) = h.replacement().await;
    let r = h.plan(&c.token, body()).await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (403, Some("KEY_NOT_RELEASED"))
    );
    h.db.put_key_request("R1", &c.branch, INSTANCE2, AGE_KEY, None, T0)
        .await
        .unwrap();
    let r = h.plan(&c.token, body()).await;
    assert_eq!(
        r.body["code"], "KEY_NOT_RELEASED",
        "pending is not released"
    );

    // A lapsed (Free) branch.
    h.release_key(&c).await;
    h.now.store(T0 + 40 * DAY, Ordering::SeqCst);
    let r = h.plan(&c.token, body()).await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (403, Some("FEATURE_NOT_ENTITLED"))
    );
    assert!(r.body.get("snapshot").is_none() && r.body.get("batches").is_none());
}

#[tokio::test]
async fn a_foreign_instance_a_bad_position_and_a_bad_token_are_refused() {
    let h = Harness::new().await;
    let (_, c) = h.replacement().await;
    h.release_key(&c).await;
    let r = h
        .plan(&c.token, json!({"hub_instance_id": "someone-else"}))
        .await;
    assert_eq!(r.body["code"], "VALIDATION_FAILED");
    let r = h
        .plan(
            &c.token,
            json!({"hub_instance_id": INSTANCE2, "after": {"epoch": "x", "seq": 1}}),
        )
        .await;
    assert_eq!(r.body["field"].as_str(), Some("after"));
    assert_eq!(r.body["code"], "VALIDATION_FAILED");
    let r = h.plan("hub_nope.nope", body()).await;
    assert_eq!(r.status, 401);
}

#[tokio::test]
async fn a_hub_that_replaces_nobody_is_forbidden_and_no_bucket_is_unavailable() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let r = h
        .plan(&a.token, json!({"hub_instance_id": "01HUBINSTANCE"}))
        .await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (403, Some("FORBIDDEN"))
    );

    let h = Harness::build(false).await;
    let (_, c) = h.replacement().await;
    h.release_key(&c).await;
    let r = h.plan(&c.token, body()).await;
    assert_eq!(r.status, 503, "{}", r.body);
}

#[tokio::test]
async fn plans_are_limited_per_branch() {
    let h = Harness::new().await;
    let (_, c) = h.replacement().await;
    h.release_key(&c).await;
    for _ in 0..60 {
        assert_eq!(h.plan(&c.token, body()).await.status, 200);
    }
    let r = h.plan(&c.token, body()).await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (429, Some("RATE_LIMITED"))
    );
}
