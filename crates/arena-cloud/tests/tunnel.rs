//! The Cloudflare Tunnel of an entitled branch (Story 8.39): the heartbeat creates the tunnel and
//! the Access application once, hands the credential out once (and again when the Hub holds
//! none), answers `on` while it is held, and reverses everything when the flag goes away. The
//! provider is the recorded fake.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use arena_cloud::auth::AuthState;
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
use arena_cloud::tunnel::{FakeOp, FakeTunnels};
use arena_cloud::{Clock, app_with_auth, auth_state};
use arena_proto::Tier;
use arena_store::control::{
    CompGrant, ControlDb, LATEST_CONTROL_VERSION, NewBackupCode, TunnelRowState,
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

struct Harness {
    app: axum::Router,
    db: Arc<ControlDb>,
    st: Arc<AuthState>,
    now: Arc<AtomicI64>,
    dir: tempfile::TempDir,
}

struct Reply {
    status: u16,
    body: Value,
}

/// An owner's branch with a Hub paired to it.
struct Cafe {
    branch: String,
    token: String,
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
        let app = router(&st);
        Self {
            app,
            db,
            st,
            now,
            dir,
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

    /// A signed-up owner whose branch has a Hub of `platform` paired. A Windows Hub pairs only
    /// on a paid branch, so the branch is upgraded to `tier` first.
    async fn cafe(&self, platform: &str, tier: Tier) -> Cafe {
        let access = self.sign_up(PHONE_A, "Pixel Arena").await;
        self.complete_setup(PHONE_A).await;
        let r = self
            .call(
                "PUT",
                "/v1/wizard/cafe",
                Some(json!({
                    "cafe_name": "Pixel Arena",
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
        if tier != Tier::Free {
            self.upgrade(&branch, tier).await;
        }
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
                    "platform": platform,
                    "hub_instance_id": "01HUBINSTANCE",
                    "version": "0.1.0",
                })
                .to_string(),
            ))
            .unwrap();
        req.extensions_mut().insert(ConnectInfo(SocketAddr::new(
            "198.51.100.4".parse().unwrap(),
            40000,
        )));
        let r = reply(self.app.clone().oneshot(req).await.unwrap()).await;
        assert_eq!(r.status, 200, "{}", r.body);
        Cafe {
            branch,
            token: r.body["token"].as_str().unwrap().to_owned(),
        }
    }

    /// A heartbeat; `held` is whether the Hub says it has the credential (`None`: no report).
    async fn beat(&self, c: &Cafe, held: Option<bool>) -> Reply {
        // The heartbeat rate limit is one per 5 s per Hub.
        self.now.fetch_add(10_000, Ordering::SeqCst);
        let mut body = json!({
            "epoch": "3:HUBA",
            "version": "0.2.0",
            "schema_version": 12,
            "last_seq_uploaded": 0,
        });
        if let Some(held) = held {
            body["tunnel"] = json!({"has_credential": held});
        }
        let req = Request::builder()
            .method("POST")
            .uri("/v1/heartbeat")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", c.token))
            .body(Body::from(body.to_string()))
            .unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    /// Sets the branch's comp entitlement (the latest grant wins).
    async fn upgrade(&self, branch: &str, tier: Tier) {
        let n = self.now.fetch_add(1, Ordering::SeqCst);
        self.db
            .grant_comp(&CompGrant {
                branch_id: branch,
                tier,
                until: T0 + 30 * DAY,
                audit_id: &format!("audit-{branch}-{tier:?}-{n}"),
                operator: "ops",
                reason: "beta cafe",
                details_json: "{}",
                now: T0,
            })
            .await
            .unwrap();
    }

    fn fake(&self) -> &FakeTunnels {
        self.st
            .tunnels
            .get()
            .and_then(|t| t.fake())
            .expect("the recorded fake")
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

fn hostname(c: &Cafe) -> String {
    format!("{}.tunnel.arena360.cloud", c.branch)
}

fn credential_of(r: &Reply) -> String {
    r.body["tunnel"]["credential"]
        .as_str()
        .expect("a credential")
        .to_owned()
}

#[tokio::test]
async fn the_first_sight_of_the_flag_creates_the_tunnel_once_and_grants_the_credential() {
    let h = Harness::new().await;
    let c = h.cafe("windows", Tier::Premium).await;
    let r = h.beat(&c, Some(false)).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["tunnel"]["state"], "grant");
    assert_eq!(r.body["tunnel"]["hostname"], hostname(&c));
    assert!(!credential_of(&r).is_empty());
    assert_eq!(h.fake().created(), 1);
    assert_eq!(h.fake().credentials_issued(), 1);
    let row = h.db.branch_tunnel(&c.branch).await.unwrap().expect("a row");
    assert_eq!(row.hostname, hostname(&c));
    assert_eq!(row.state, TunnelRowState::Ready);
    assert!(row.delivered_at.is_some());
    assert_eq!(
        row.tunnel_id,
        h.fake().provisioned(&c.branch).unwrap().tunnel_id
    );
}

#[tokio::test]
async fn a_hub_that_holds_the_credential_gets_on_and_nothing_more_is_made_or_sent() {
    let h = Harness::new().await;
    let c = h.cafe("windows", Tier::Premium).await;
    let first = h.beat(&c, Some(false)).await;
    let secret = credential_of(&first);
    for _ in 0..3 {
        let r = h.beat(&c, Some(true)).await;
        assert_eq!(
            r.body["tunnel"],
            json!({"state": "on", "hostname": hostname(&c)})
        );
        assert!(!r.body.to_string().contains(&secret));
    }
    assert_eq!((h.fake().created(), h.fake().credentials_issued()), (1, 1));
}

#[tokio::test]
async fn a_hub_that_lost_its_credential_is_granted_a_fresh_one_without_a_new_tunnel() {
    let h = Harness::new().await;
    let c = h.cafe("windows", Tier::Premium).await;
    let first = h.beat(&c, Some(false)).await;
    // A replaced PC: the same Hub id, no credential file.
    let again = h.beat(&c, Some(false)).await;
    assert_eq!(again.body["tunnel"]["state"], "grant");
    assert_ne!(credential_of(&again), credential_of(&first));
    // A Hub that sends no report (older build) is not handed the credential on every beat.
    let unreported = h.beat(&c, None).await;
    assert_eq!(unreported.body["tunnel"]["state"], "on");
    assert_eq!(h.fake().created(), 1);
    assert_eq!(h.fake().credentials_issued(), 2);
}

#[tokio::test]
async fn a_cloud_failure_never_tells_a_hub_that_holds_its_credential_to_stop() {
    let h = Harness::new().await;
    let c = h.cafe("windows", Tier::Premium).await;
    h.fake().fail(FakeOp::Credential, 2);
    // Nothing was delivered yet, and the credential cannot be fetched: a Hub without one is told
    // `off`, a Hub that holds one is told nothing at all.
    let r = h.beat(&c, Some(false)).await;
    assert_eq!(r.body["tunnel"], json!({"state": "off"}));
    let r = h.beat(&c, Some(true)).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(r.body.get("tunnel").is_none(), "{}", r.body);
}

#[tokio::test]
async fn a_provider_error_on_create_answers_off_and_the_next_heartbeat_retries() {
    let h = Harness::new().await;
    let c = h.cafe("windows", Tier::Premium).await;
    h.fake().fail(FakeOp::Ensure, 1);
    let r = h.beat(&c, Some(false)).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["tunnel"], json!({"state": "off"}));
    assert!(h.db.branch_tunnel(&c.branch).await.unwrap().is_none());
    let r = h.beat(&c, Some(false)).await;
    assert_eq!(r.body["tunnel"]["state"], "grant");
    assert_eq!(h.fake().created(), 1);
}

#[tokio::test]
async fn a_credential_error_leaves_the_row_undelivered_and_the_retry_delivers_it() {
    let h = Harness::new().await;
    let c = h.cafe("windows", Tier::Premium).await;
    h.fake().fail(FakeOp::Credential, 1);
    let r = h.beat(&c, Some(false)).await;
    assert_eq!(r.body["tunnel"], json!({"state": "off"}));
    let row = h.db.branch_tunnel(&c.branch).await.unwrap().expect("a row");
    assert_eq!(row.delivered_at, None);
    // Even a Hub that says it holds a credential gets a fresh one: none was ever delivered.
    let r = h.beat(&c, Some(true)).await;
    assert_eq!(r.body["tunnel"]["state"], "grant");
    assert_eq!(h.fake().created(), 1);
    assert!(
        h.db.branch_tunnel(&c.branch)
            .await
            .unwrap()
            .unwrap()
            .delivered_at
            .is_some()
    );
}

#[tokio::test]
async fn removing_the_flag_deletes_the_tunnel_and_its_row_and_answers_off() {
    let h = Harness::new().await;
    let c = h.cafe("windows", Tier::Premium).await;
    h.beat(&c, Some(false)).await;
    // Basic has no tunnel flag.
    h.upgrade(&c.branch, Tier::Basic).await;
    let r = h.beat(&c, Some(true)).await;
    assert_eq!(r.body["tunnel"], json!({"state": "off"}));
    assert_eq!(h.fake().deleted(), 1);
    assert!(h.fake().live().is_empty());
    assert!(h.db.branch_tunnel(&c.branch).await.unwrap().is_none());
    // Nothing more to delete, nothing created.
    let r = h.beat(&c, Some(false)).await;
    assert_eq!(r.body["tunnel"], json!({"state": "off"}));
    assert_eq!((h.fake().created(), h.fake().deleted()), (1, 1));
}

#[tokio::test]
async fn a_delete_error_keeps_the_row_deleting_and_is_retried() {
    let h = Harness::new().await;
    let c = h.cafe("windows", Tier::Premium).await;
    h.beat(&c, Some(false)).await;
    h.upgrade(&c.branch, Tier::Basic).await;
    h.fake().fail(FakeOp::Delete, 1);
    let r = h.beat(&c, Some(true)).await;
    assert_eq!(r.body["tunnel"], json!({"state": "off"}));
    let row = h.db.branch_tunnel(&c.branch).await.unwrap().expect("kept");
    assert_eq!(row.state, TunnelRowState::Deleting);
    assert_eq!(h.fake().live().len(), 1);
    let r = h.beat(&c, Some(false)).await;
    assert_eq!(r.body["tunnel"], json!({"state": "off"}));
    assert!(h.db.branch_tunnel(&c.branch).await.unwrap().is_none());
    assert!(h.fake().live().is_empty());
}

#[tokio::test]
async fn a_flag_that_returns_while_the_delete_is_pending_finishes_the_delete_then_creates_anew() {
    let h = Harness::new().await;
    let c = h.cafe("windows", Tier::Premium).await;
    h.beat(&c, Some(false)).await;
    h.upgrade(&c.branch, Tier::Basic).await;
    h.fake().fail(FakeOp::Delete, 1);
    h.beat(&c, Some(true)).await;
    h.upgrade(&c.branch, Tier::Premium).await;
    // The delete is retried first; it works now, so the same beat creates a new tunnel.
    let r = h.beat(&c, Some(true)).await;
    assert_eq!(r.body["tunnel"]["state"], "grant");
    assert_eq!((h.fake().created(), h.fake().deleted()), (2, 1));
    let row = h.db.branch_tunnel(&c.branch).await.unwrap().unwrap();
    assert_eq!(row.state, TunnelRowState::Ready);
}

#[tokio::test]
async fn an_android_hub_with_the_flag_is_unsupported_and_nothing_is_created() {
    let h = Harness::new().await;
    let c = h.cafe("android", Tier::Premium).await;
    let r = h.beat(&c, Some(false)).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["tunnel"], json!({"state": "unsupported_platform"}));
    assert_eq!(h.fake().created(), 0);
    assert!(h.db.branch_tunnel(&c.branch).await.unwrap().is_none());
}

#[tokio::test]
async fn a_tunnel_left_for_a_branch_whose_hub_is_android_is_deleted() {
    let h = Harness::new().await;
    let c = h.cafe("android", Tier::Premium).await;
    let provider = h.st.tunnels.get().unwrap();
    let made = provider.ensure(&c.branch, &hostname(&c)).await.unwrap();
    h.db.insert_branch_tunnel(&arena_store::control::NewBranchTunnel {
        branch_id: &c.branch,
        hostname: &hostname(&c),
        tunnel_id: &made.tunnel_id,
        access_app_id: &made.access_app_id,
        now: T0,
    })
    .await
    .unwrap();
    let r = h.beat(&c, Some(true)).await;
    assert_eq!(r.body["tunnel"], json!({"state": "unsupported_platform"}));
    assert_eq!(h.fake().deleted(), 1);
    assert!(h.db.branch_tunnel(&c.branch).await.unwrap().is_none());
}

#[tokio::test]
async fn a_branch_without_the_flag_gets_off_and_a_tunnel_is_never_made() {
    let h = Harness::new().await;
    let basic = h.cafe("windows", Tier::Basic).await;
    let r = h.beat(&basic, Some(false)).await;
    assert_eq!(r.body["tunnel"], json!({"state": "off"}));
    assert_eq!(h.fake().created(), 0);
}

#[tokio::test]
async fn the_credential_is_never_stored_by_the_cloud() {
    let h = Harness::new().await;
    let c = h.cafe("windows", Tier::Premium).await;
    let secret = credential_of(&h.beat(&c, Some(false)).await);
    assert!(!everything_stored(&h).contains(&secret));
}

/// Every value of every table of the control database, as text.
fn everything_stored(h: &Harness) -> String {
    let conn = rusqlite::Connection::open(h.dir.path().join("gz_control.db")).unwrap();
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut out = String::new();
    for table in tables {
        let mut stmt = conn.prepare(&format!("SELECT * FROM \"{table}\"")).unwrap();
        let cols = stmt.column_count();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for i in 0..cols {
                out.push_str(&format!("{:?} ", row.get_ref(i).unwrap()));
            }
        }
    }
    out
}
