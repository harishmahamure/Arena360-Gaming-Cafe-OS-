//! Live view (Story 8.40): `GET /v1/live/{branch}` signs a branch-scoped token of at most five
//! minutes for an entitled branch with a Windows Hub and a ready tunnel, and refuses the rest.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use arena_cloud::auth::AuthState;
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
use arena_cloud::{Clock, app_with_auth, auth_state};
use arena_proto::licence::DEV_LICENCE_VERIFY_KEYS;
use arena_proto::{LiveGrant, Tier, verify_live_token};
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
    _dir: tempfile::TempDir,
}

struct Reply {
    status: u16,
    body: Value,
}

/// An owner's branch with a Hub paired to it.
struct Cafe {
    access: String,
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
            access,
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

impl Harness {
    async fn live(&self, c: &Cafe) -> Reply {
        self.call("GET", &format!("/v1/live/{}", c.branch), None, &c.access)
            .await
    }

    /// A Windows premium branch whose first heartbeat has made the tunnel.
    async fn ready_cafe(&self) -> Cafe {
        let c = self.cafe("windows", Tier::Premium).await;
        let r = self.beat(&c, Some(false)).await;
        assert_eq!(r.body["tunnel"]["state"], "grant", "{}", r.body);
        c
    }
}

#[tokio::test]
async fn an_entitled_branch_gets_a_token_for_that_branch_valid_for_at_most_five_minutes() {
    let h = Harness::new().await;
    let c = h.ready_cafe().await;
    let now = h.now.load(Ordering::SeqCst);
    let r = h.live(&c).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let grant: LiveGrant = serde_json::from_value(r.body).unwrap();
    assert_eq!(grant.url, format!("wss://{}/live", hostname(&c)));
    assert!(grant.expires_at > now && grant.expires_at <= now + 300_000);
    let claims = verify_live_token(&grant.token, DEV_LICENCE_VERIFY_KEYS, &c.branch, now).unwrap();
    assert_eq!(claims.branch_id, c.branch);
    assert!(claims.exp - claims.iat <= 300);
    assert_eq!(claims.exp * 1000, grant.expires_at);
    // Not valid for another branch, and not after it runs out.
    assert!(verify_live_token(&grant.token, DEV_LICENCE_VERIFY_KEYS, "other", now).is_err());
    assert!(
        verify_live_token(
            &grant.token,
            DEV_LICENCE_VERIFY_KEYS,
            &c.branch,
            now + 301_000
        )
        .is_err()
    );
    // A new call is a new token.
    h.now.fetch_add(61_000, Ordering::SeqCst);
    let again: LiveGrant = serde_json::from_value(h.live(&c).await.body).unwrap();
    assert_ne!(again.token, grant.token);
}

#[tokio::test]
async fn a_licence_token_is_not_a_live_token() {
    let h = Harness::new().await;
    let c = h.ready_cafe().await;
    let r = h.beat(&c, Some(true)).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let licence = r.body["licence"].as_str().expect("a licence token");
    let now = h.now.load(Ordering::SeqCst);
    assert!(verify_live_token(licence, DEV_LICENCE_VERIFY_KEYS, &c.branch, now).is_err());
}

#[tokio::test]
async fn a_branch_without_the_tunnel_flag_is_not_entitled() {
    let h = Harness::new().await;
    let c = h.cafe("windows", Tier::Basic).await;
    let r = h.live(&c).await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (403, Some("FEATURE_NOT_ENTITLED"))
    );
    let free = Harness::new().await;
    let c = free.cafe("android", Tier::Free).await;
    let r = free.live(&c).await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (403, Some("FEATURE_NOT_ENTITLED"))
    );
}

#[tokio::test]
async fn an_android_hub_is_platform_unsupported() {
    let h = Harness::new().await;
    let c = h.cafe("android", Tier::Premium).await;
    let r = h.beat(&c, None).await;
    assert_eq!(
        r.body["tunnel"]["state"], "unsupported_platform",
        "{}",
        r.body
    );
    let r = h.live(&c).await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (409, Some("PLATFORM_UNSUPPORTED"))
    );
}

#[tokio::test]
async fn no_tunnel_yet_is_not_ready_and_a_stranger_or_a_signed_out_caller_is_refused() {
    let h = Harness::new().await;
    let c = h.cafe("windows", Tier::Premium).await;
    let r = h.live(&c).await;
    assert_eq!((r.status, r.body["code"].as_str()), (503, Some("NO_HUB")));
    let r = h
        .call("GET", &format!("/v1/live/{}", c.branch), None, "")
        .await;
    assert_eq!(r.status, 401, "{}", r.body);
    let r = h
        .call("GET", "/v1/live/not-a-branch", None, &c.access)
        .await;
    assert_eq!(r.status, 404, "{}", r.body);
    let r = h.call("GET", "/v1/live/all", None, &c.access).await;
    assert_eq!(r.status, 422, "{}", r.body);
}
