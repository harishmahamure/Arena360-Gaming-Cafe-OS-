//! Owner sign-up with a phone one-time code (Story 2.4), in process: the full app router over
//! a local `gz_control`, the recorded fake SMS and a hand-moved clock. Plus one end-to-end run
//! of `arena-cloud --role all --dev` and the Gupshup client against a local stub.

use std::io::{Read, Write as _};
use std::net::{TcpListener as StdListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use arena_cloud::auth::{AuthState, token_hash};
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::health::Reachability;
use arena_cloud::messaging::{GupshupConfig, GupshupSms, Sms};
use arena_cloud::{Clock, app_with_auth, auth_state};
use arena_proto::MfaState;
use arena_store::control::ControlDb;
use axum::body::{Body, to_bytes};
use axum::http::{Request, header};
use serde_json::{Value, json};
use tokio::sync::watch;
use tower::ServiceExt;

const ORIGIN: &str = "https://start.arena360.cloud";
const PEPPER: &str = "test-pepper-0123456789abcdef0123456789";
const T0: i64 = 1_800_000_000_000;
const PHONE: &str = "+919876543210";

struct Harness {
    app: axum::Router,
    db: Arc<ControlDb>,
    st: Arc<AuthState>,
    now: Arc<AtomicI64>,
    _dir: tempfile::TempDir,
}

struct Reply {
    status: u16,
    headers: axum::http::HeaderMap,
    body: Value,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    fn cookies(&self) -> Vec<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap().to_owned())
            .collect()
    }

    /// The value of the cookie `name` set by this reply.
    fn cookie(&self, name: &str) -> Option<String> {
        self.cookies().iter().find_map(|c| {
            let first = c.split(';').next()?;
            let (k, v) = first.split_once('=')?;
            (k == name && !v.is_empty()).then(|| v.to_owned())
        })
    }

    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or("")
    }
}

fn start_body(phone: &str) -> Value {
    json!({
        "name": "Asha Rao",
        "phone": phone,
        "email": "asha.rao@example.in",
        "cafe_name": "Level Up Gaming",
        "city": "Pune",
    })
}

impl Harness {
    async fn new(dev: bool) -> Self {
        Self::with_global_cap(dev, 60).await
    }

    async fn with_global_cap(dev: bool, sms_global_per_min: u64) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = ControlDb::open_local(&dir.path().join("gz_control.db"))
            .await
            .unwrap();
        db.migrate(T0).await.unwrap();
        let db = Arc::new(db);
        let now = Arc::new(AtomicI64::new(T0));
        let clock_now = Arc::clone(&now);
        let clock: Clock = Arc::new(move || clock_now.load(Ordering::SeqCst));
        let cfg = AuthConfig {
            pepper: PEPPER.as_bytes().to_vec(),
            web_origins: vec![ORIGIN.to_owned()],
            sms: SmsProviderConfig::Fake,
            sms_global_per_min,
            mfa_key: arena_cloud::crypto::MfaKey::from_bytes([9; 32]),
            licence: arena_cloud::licence::LicenceSigner::dev(),
            billing_enabled: false,
            escrow_key: None,
        };
        let st = auth_state(Arc::clone(&db), &cfg, clock).unwrap();
        let (_tx, rx) = watch::channel(Reachability {
            reachable: true,
            checked_at: None,
        });
        let app = app_with_auth(
            Role::All,
            dev,
            rx,
            arena_store::control::LATEST_CONTROL_VERSION,
            Some(Arc::clone(&st)),
        );
        Self {
            app,
            db,
            st,
            now,
            _dir: dir,
        }
    }

    fn advance(&self, ms: i64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }

    fn sent(&self) -> Vec<arena_cloud::messaging::SentSms> {
        self.st.sms.fake().unwrap().sent()
    }

    /// The code in the newest SMS.
    fn last_code(&self) -> String {
        let text = self.sent().last().unwrap().text.clone();
        text[..6].to_owned()
    }

    async fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> Reply {
        let mut req = Request::builder().method(method).uri(path);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let req = match body {
            Some(b) => req
                .header("content-type", "application/json")
                .body(Body::from(b.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let headers = res.headers().clone();
        let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| panic!("not JSON: {}", String::from_utf8_lossy(&bytes)))
        };
        Reply {
            status,
            headers,
            body,
        }
    }

    async fn post(&self, path: &str, body: Value) -> Reply {
        self.call("POST", path, Some(body), &[]).await
    }

    async fn start(&self, phone: &str) -> Reply {
        self.post("/v1/signup/start", start_body(phone)).await
    }

    async fn verify(&self, challenge_id: &str, code: &str) -> Reply {
        self.post(
            "/v1/signup/verify",
            json!({"challenge_id": challenge_id, "code": code}),
        )
        .await
    }

    async fn resend(&self, challenge_id: &str) -> Reply {
        self.post("/v1/signup/resend", json!({"challenge_id": challenge_id}))
            .await
    }

    /// Start + verify with the right code.
    async fn sign_up(&self, phone: &str) -> Reply {
        let s = self.start(phone).await;
        assert_eq!(s.status, 202, "{}", s.body);
        let id = s.body["challenge_id"].as_str().unwrap().to_owned();
        self.verify(&id, &self.last_code()).await
    }

    async fn session(&self, access: &str) -> Reply {
        let cookie = format!("arena_access={access}");
        self.call("GET", "/v1/session", None, &[("cookie", &cookie)])
            .await
    }

    async fn refresh(&self, refresh: &str) -> Reply {
        let cookie = format!("arena_refresh={refresh}");
        self.call("POST", "/v1/session/refresh", None, &[("cookie", &cookie)])
            .await
    }
}

fn wrong(code: &str) -> String {
    let n: u32 = code.parse().unwrap();
    format!("{:06}", (n + 1) % 1_000_000)
}

#[tokio::test]
async fn an_invalid_field_is_named_and_sends_nothing() {
    let h = Harness::new(false).await;
    for (field, value) in [
        ("phone", json!("+91123")),
        ("phone", json!("+915876543210")),
        ("phone", json!("9876543210")),
        ("name", json!("   ")),
        ("name", json!("x".repeat(81))),
        ("email", json!("asha@example")),
        ("cafe_name", json!("")),
        ("city", json!("c".repeat(81))),
    ] {
        let mut body = start_body(PHONE);
        body[field] = value;
        let r = h.post("/v1/signup/start", body).await;
        assert_eq!(r.status, 422, "{field}: {}", r.body);
        assert_eq!(r.code(), "VALIDATION_FAILED");
        assert_eq!(r.body["field"], field);
    }
    assert!(h.sent().is_empty());
}

#[tokio::test]
async fn start_sends_one_code_and_creates_no_owner() {
    let h = Harness::new(false).await;
    let r = h.start(PHONE).await;
    assert_eq!(r.status, 202, "{}", r.body);
    assert_eq!(r.body["expires_in_s"], 300);
    assert_eq!(r.body["resend_after_s"], 30);
    assert_eq!(r.body["challenge_id"].as_str().unwrap().len(), 26);
    let sent = h.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, PHONE);
    let code = &sent[0].text[..6];
    assert!(code.bytes().all(|b| b.is_ascii_digit()));
    assert_eq!(
        sent[0].text,
        format!("{code} is your Arena 360 code. It expires in 5 minutes.")
    );
    assert_eq!(h.db.owner_count().await.unwrap(), 0);
    assert!(r.cookies().is_empty());
    // The stored hash is not the code.
    let id = r.body["challenge_id"].as_str().unwrap();
    let c = h.db.otp_challenge(id).await.unwrap().unwrap();
    assert_ne!(c.code_hash, code);
    assert!(!c.code_hash.contains(code));
}

#[tokio::test]
async fn the_right_code_creates_a_pending_owner_and_signs_in() {
    let h = Harness::new(false).await;
    let s = h.start(PHONE).await;
    let id = s.body["challenge_id"].as_str().unwrap().to_owned();
    h.advance(4 * 60_000);
    let r = h.verify(&id, &h.last_code()).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body, json!({"next": "account_setup"}));

    let cookies = r.cookies();
    assert_eq!(cookies.len(), 2, "{cookies:?}");
    let access = cookies
        .iter()
        .find(|c| c.starts_with("arena_access="))
        .unwrap();
    let refresh = cookies
        .iter()
        .find(|c| c.starts_with("arena_refresh="))
        .unwrap();
    for (c, path, age) in [
        (access, "Path=/v1;", "Max-Age=900;"),
        (refresh, "Path=/v1/session/refresh;", "Max-Age=2592000;"),
    ] {
        assert!(c.contains(path) && c.contains(age), "{c}");
        assert!(
            c.contains("HttpOnly") && c.contains("Secure") && c.contains("SameSite=Lax"),
            "{c}"
        );
        assert!(
            !c.to_ascii_lowercase().contains("domain="),
            "host-only: {c}"
        );
    }
    let access = r.cookie("arena_access").unwrap();
    assert_eq!(access.len(), 64);

    let owner = h.db.owner_by_phone(PHONE).await.unwrap().unwrap();
    assert_eq!(owner.mfa_state, MfaState::Pending);
    assert_eq!(
        (
            owner.name.as_str(),
            owner.email.as_str(),
            owner.cafe_name.as_str(),
            owner.city.as_str()
        ),
        ("Asha Rao", "asha.rao@example.in", "Level Up Gaming", "Pune")
    );
    assert!(
        h.db.otp_challenge(&id)
            .await
            .unwrap()
            .unwrap()
            .consumed_at
            .is_some()
    );
    // Only hashes are stored.
    let live =
        h.db.owner_session_by_access(&token_hash(&access), h.st.now())
            .await
            .unwrap()
            .unwrap();
    assert_eq!(live.owner.id, owner.id);
    assert!(
        h.db.owner_session_by_access(&access, h.st.now())
            .await
            .unwrap()
            .is_none()
    );

    let me = h.session(&access).await;
    assert_eq!(me.status, 200, "{}", me.body);
    assert_eq!(
        me.body,
        json!({"owner": {
            "name": "Asha Rao",
            "cafe_name": "Level Up Gaming",
            "city": "Pune",
            "mfa_state": "pending",
            "has_password": false,
            "backup_codes_acked": false,
            "sms_fallback": false,
        }})
    );
    let none = h.call("GET", "/v1/session", None, &[]).await;
    assert_eq!((none.status, none.code()), (401, "AUTH_FAILED"));
    let bad = h.session("00").await;
    assert_eq!((bad.status, bad.code()), (401, "AUTH_FAILED"));
    // The access token lasts 15 minutes.
    h.advance(15 * 60_000);
    assert_eq!(h.session(&access).await.status, 401);
}

#[tokio::test]
async fn wrong_codes_count_down_and_the_fifth_voids_the_challenge() {
    let h = Harness::new(false).await;
    let s = h.start(PHONE).await;
    let id = s.body["challenge_id"].as_str().unwrap().to_owned();
    let code = h.last_code();
    for _ in 0..4 {
        let r = h.verify(&id, &wrong(&code)).await;
        assert_eq!((r.status, r.code()), (400, "OTP_INVALID"), "{}", r.body);
    }
    let r = h.verify(&id, &wrong(&code)).await;
    assert_eq!((r.status, r.code()), (400, "OTP_EXPIRED"));
    let r = h.verify(&id, &code).await;
    assert_eq!((r.status, r.code()), (400, "OTP_EXPIRED"));
    assert!(r.cookies().is_empty());
    assert_eq!(h.db.owner_count().await.unwrap(), 0);
    // Not a code at all: a validation error, not counted.
    let r = h.verify(&id, "12ab").await;
    assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));
    assert_eq!(r.body["field"], "code");
}

#[tokio::test]
async fn an_expired_or_used_code_never_verifies() {
    let h = Harness::new(false).await;
    let s = h.start(PHONE).await;
    let id = s.body["challenge_id"].as_str().unwrap().to_owned();
    let code = h.last_code();
    h.advance(5 * 60_000);
    let r = h.verify(&id, &code).await;
    assert_eq!((r.status, r.code()), (400, "OTP_EXPIRED"));
    assert_eq!(h.db.owner_count().await.unwrap(), 0);

    let s = h.start(PHONE).await;
    let id = s.body["challenge_id"].as_str().unwrap().to_owned();
    let code = h.last_code();
    assert_eq!(h.verify(&id, &code).await.status, 200);
    let again = h.verify(&id, &code).await;
    assert_eq!((again.status, again.code()), (400, "OTP_EXPIRED"));
    assert!(again.cookies().is_empty());
    let unknown = h.verify("01J00000000000000000000000", &code).await;
    assert_eq!(unknown.code(), "OTP_EXPIRED");
    let resend = h.resend(&id).await;
    assert_eq!(resend.code(), "OTP_EXPIRED");
}

#[tokio::test]
async fn a_pending_owner_signs_in_again_without_a_new_row() {
    let h = Harness::new(false).await;
    let first = h.sign_up(PHONE).await;
    assert_eq!(first.status, 200);
    let owner = h.db.owner_by_phone(PHONE).await.unwrap().unwrap();

    let mut other = start_body(PHONE);
    other["name"] = json!("Somebody Else");
    let s = h.post("/v1/signup/start", other).await;
    assert_eq!(s.status, 202);
    let keys: Vec<_> = s.body.as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, ["challenge_id", "expires_in_s", "resend_after_s"]);
    let r = h
        .verify(s.body["challenge_id"].as_str().unwrap(), &h.last_code())
        .await;
    assert_eq!(r.body, json!({"next": "account_setup"}));
    assert_eq!(h.db.owner_count().await.unwrap(), 1);
    let me = h.session(&r.cookie("arena_access").unwrap()).await;
    assert_eq!(me.body["owner"]["name"], "Asha Rao");
    assert_eq!(h.db.owner_by_phone(PHONE).await.unwrap().unwrap(), owner);
}

#[tokio::test]
async fn an_enrolled_owner_gets_no_session_from_the_code() {
    let h = Harness::new(false).await;
    h.sign_up(PHONE).await;
    let owner = h.db.owner_by_phone(PHONE).await.unwrap().unwrap();
    h.db.set_owner_mfa_state(&owner.id, MfaState::Enrolled)
        .await
        .unwrap();

    let new_phone = h.start("+14155550100").await;
    let s = h.start(PHONE).await;
    // The same answer as for a phone without an account.
    assert_eq!(s.status, new_phone.status);
    assert_eq!(s.body["expires_in_s"], new_phone.body["expires_in_s"]);
    assert_eq!(s.body["resend_after_s"], new_phone.body["resend_after_s"]);
    assert_eq!(s.headers.len(), new_phone.headers.len());

    let r = h
        .verify(s.body["challenge_id"].as_str().unwrap(), &h.last_code())
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(r.body, json!({"next": "sign_in"}));
    assert!(r.cookies().is_empty());
    assert_eq!(h.db.owner_count().await.unwrap(), 1);
}

#[tokio::test]
async fn the_sixth_send_in_an_hour_is_rate_limited_and_sends_nothing() {
    let h = Harness::new(false).await;
    let mut first_id = String::new();
    for i in 0..4 {
        let r = h.start(PHONE).await;
        assert_eq!(r.status, 202, "{}", r.body);
        if i == 0 {
            first_id = r.body["challenge_id"].as_str().unwrap().to_owned();
        }
        h.advance(60_000);
    }
    // A resend counts too (the 5th send).
    assert_eq!(h.resend(&first_id).await.status, 202);
    assert_eq!(h.sent().len(), 5);
    h.advance(31_000);
    for r in [h.start(PHONE).await, h.resend(&first_id).await] {
        assert_eq!((r.status, r.code()), (429, "RATE_LIMITED"), "{}", r.body);
        let retry: u64 = r.header("retry-after").unwrap().parse().unwrap();
        assert!(retry > 0 && retry <= 3600, "{retry}");
        // The first send was 4 min 31 s ago in the minute bucket ending 3 min 31 s ago: the
        // bucket leaves the hour in 56 min 29 s.
        assert_eq!(retry, 3600 - 211);
    }
    assert_eq!(h.sent().len(), 5);
    // Another phone is not affected.
    assert_eq!(h.start("+14155550100").await.status, 202);
    // One second early it is still refused; once the oldest bucket leaves, one more passes.
    h.advance((3600 - 212) * 1000);
    assert_eq!(h.start(PHONE).await.status, 429);
    h.advance(1000);
    assert_eq!(h.start(PHONE).await.status, 202);
    assert_eq!(h.start(PHONE).await.status, 429);
}

#[tokio::test]
async fn a_resend_waits_30_s_then_replaces_the_code() {
    let h = Harness::new(false).await;
    let s = h.start(PHONE).await;
    let id = s.body["challenge_id"].as_str().unwrap().to_owned();
    let old = h.last_code();
    assert_eq!(h.verify(&id, &wrong(&old)).await.code(), "OTP_INVALID");
    h.advance(10_000);
    let early = h.resend(&id).await;
    assert_eq!((early.status, early.code()), (429, "RATE_LIMITED"));
    let retry: u64 = early.header("retry-after").unwrap().parse().unwrap();
    assert!(retry > 0 && retry <= 30, "{retry}");
    assert_eq!(retry, 20);
    assert_eq!(h.sent().len(), 1);

    h.advance(20_000);
    let r = h.resend(&id).await;
    assert_eq!(r.status, 202, "{}", r.body);
    assert_eq!(r.body["challenge_id"], id.as_str());
    assert_eq!(h.sent().len(), 2);
    let new = h.last_code();
    let c = h.db.otp_challenge(&id).await.unwrap().unwrap();
    assert_eq!(c.attempts, 0);
    assert_eq!(c.expires_at, h.st.now() + 300_000);
    if new != old {
        assert_eq!(h.verify(&id, &old).await.code(), "OTP_INVALID");
    }
    // The new code works for 5 minutes from the resend.
    h.advance(4 * 60_000 + 50_000);
    assert_eq!(h.verify(&id, &new).await.status, 200);
}

#[tokio::test]
async fn a_void_challenge_drops_the_form_and_cannot_be_resent() {
    let h = Harness::new(false).await;
    let s = h.start(PHONE).await;
    let id = s.body["challenge_id"].as_str().unwrap().to_owned();
    let code = h.last_code();
    for _ in 0..5 {
        h.verify(&id, &wrong(&code)).await;
    }
    assert_eq!(
        h.db.otp_challenge(&id).await.unwrap().unwrap().payload,
        None
    );
    h.advance(30_000);
    let r = h.resend(&id).await;
    assert_eq!((r.status, r.code()), (400, "OTP_EXPIRED"));
    assert_eq!(h.sent().len(), 1);
    // A new start works.
    assert_eq!(h.start(PHONE).await.status, 202);
}

#[tokio::test]
async fn an_sms_outage_answers_503_and_is_not_counted() {
    let h = Harness::new(false).await;
    let fake = h.st.sms.fake().unwrap();
    fake.set_failing(true);
    for _ in 0..6 {
        let r = h.start(PHONE).await;
        assert_eq!((r.status, r.code()), (503, "INTERNAL"), "{}", r.body);
        assert_eq!(r.body["message"], arena_cloud::api::INTERNAL_MESSAGE);
    }
    let counted = |b: Vec<(i64, u64)>| b.iter().map(|b| b.1).sum::<u64>();
    assert_eq!(
        counted(
            h.db.rate_buckets(&format!("otp_send:{PHONE}"))
                .await
                .unwrap()
        ),
        0
    );
    assert_eq!(counted(h.db.rate_buckets("sms:global").await.unwrap()), 0);
    fake.set_failing(false);
    let s = h.start(PHONE).await;
    assert_eq!(s.status, 202);
    let id = s.body["challenge_id"].as_str().unwrap().to_owned();
    let code = h.last_code();
    // A failed resend keeps the challenge and its current code.
    h.advance(30_000);
    fake.set_failing(true);
    assert_eq!(h.resend(&id).await.status, 503);
    fake.set_failing(false);
    assert_eq!(h.verify(&id, &code).await.status, 200);
    let buckets =
        h.db.rate_buckets(&format!("otp_send:{PHONE}"))
            .await
            .unwrap();
    assert_eq!(buckets.iter().map(|b| b.1).sum::<u64>(), 1);
}

#[tokio::test]
async fn refresh_rotates_and_reuse_revokes_the_family() {
    let h = Harness::new(false).await;
    let r = h.sign_up(PHONE).await;
    let access1 = r.cookie("arena_access").unwrap();
    let refresh1 = r.cookie("arena_refresh").unwrap();

    h.advance(60_000);
    let r2 = h.refresh(&refresh1).await;
    assert_eq!(r2.status, 200, "{}", r2.body);
    assert_eq!(r2.body["owner"]["mfa_state"], "pending");
    let access2 = r2.cookie("arena_access").unwrap();
    let refresh2 = r2.cookie("arena_refresh").unwrap();
    assert_ne!(access2, access1);
    assert_ne!(refresh2, refresh1);
    assert_eq!(h.session(&access2).await.status, 200);
    let live =
        h.db.owner_session_by_access(&token_hash(&access2), h.st.now())
            .await
            .unwrap()
            .unwrap();
    let family =
        h.db.owner_session_family(&live.session.family_id)
            .await
            .unwrap();
    assert_eq!(family.len(), 2);
    assert!(family[0].rotated_at.is_some());

    // The rotated session's access token stops working at once.
    assert_eq!(h.session(&access1).await.status, 401);
    // The rotated token again at once (a second tab refreshing too): 401, nothing revoked and
    // no cookie touched, so the other tab's new cookies stay.
    let race = h.refresh(&refresh1).await;
    assert_eq!((race.status, race.code()), (401, "AUTH_FAILED"));
    assert!(race.cookies().is_empty(), "{:?}", race.cookies());
    assert_eq!(h.session(&access2).await.status, 200);

    // 10 s later it is reuse: 401, the family revoked, the new access fails.
    h.advance(10_000);
    let reuse = h.refresh(&refresh1).await;
    assert_eq!((reuse.status, reuse.code()), (401, "AUTH_FAILED"));
    assert!(
        reuse.cookies().iter().all(|c| c.contains("Max-Age=0")),
        "{:?}",
        reuse.cookies()
    );
    assert_eq!(h.session(&access2).await.status, 401);
    assert_eq!(h.session(&access1).await.status, 401);
    assert_eq!(h.refresh(&refresh2).await.status, 401);
    // No cookie at all.
    let none = h.call("POST", "/v1/session/refresh", None, &[]).await;
    assert_eq!((none.status, none.code()), (401, "AUTH_FAILED"));
}

#[tokio::test]
async fn cors_allows_listed_origins_only() {
    let h = Harness::new(false).await;
    let pre = h
        .call(
            "OPTIONS",
            "/v1/signup/start",
            None,
            &[
                ("origin", ORIGIN),
                ("access-control-request-method", "POST"),
                ("access-control-request-headers", "content-type"),
            ],
        )
        .await;
    assert_eq!(pre.status, 204);
    assert_eq!(pre.header("access-control-allow-origin"), Some(ORIGIN));
    assert_eq!(pre.header("access-control-allow-credentials"), Some("true"));
    assert!(
        pre.header("access-control-allow-methods")
            .unwrap()
            .contains("POST")
    );
    assert!(
        pre.header("access-control-allow-headers")
            .unwrap()
            .contains("content-type")
    );

    let r = h
        .call(
            "POST",
            "/v1/signup/start",
            Some(start_body(PHONE)),
            &[("origin", ORIGIN)],
        )
        .await;
    assert_eq!(r.status, 202);
    assert_eq!(r.header("access-control-allow-origin"), Some(ORIGIN));
    assert_eq!(r.header("access-control-allow-credentials"), Some("true"));
    assert_eq!(
        r.header("access-control-expose-headers"),
        Some("Retry-After")
    );

    // An error envelope carries the headers too, so the page can read it.
    let e = h
        .call("GET", "/v1/session", None, &[("origin", ORIGIN)])
        .await;
    assert_eq!(
        (e.status, e.header("access-control-allow-origin")),
        (401, Some(ORIGIN))
    );

    let evil = "https://start.arena360.cloud.evil.example";
    for origin in [evil, "http://start.arena360.cloud", "null"] {
        let r = h
            .call(
                "POST",
                "/v1/signup/start",
                Some(start_body(PHONE)),
                &[("origin", origin)],
            )
            .await;
        assert_eq!((r.status, r.code()), (403, "FORBIDDEN"), "{origin}");
        assert_eq!(r.header("access-control-allow-origin"), None);
        let pre = h
            .call(
                "OPTIONS",
                "/v1/signup/start",
                None,
                &[
                    ("origin", origin),
                    ("access-control-request-method", "POST"),
                ],
            )
            .await;
        assert_eq!(pre.status, 403);
        assert_eq!(pre.header("access-control-allow-origin"), None);
    }
    // A safe request from an unlisted origin is served but gets no CORS headers.
    let g = h.call("GET", "/healthz", None, &[("origin", evil)]).await;
    assert_eq!(
        (g.status, g.header("access-control-allow-origin")),
        (200, None)
    );
    // Only the listed-origin send went out.
    assert_eq!(h.sent().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn ten_parallel_starts_for_one_phone_send_at_most_five() {
    let h = Harness::new(false).await;
    let replies = futures_util::future::join_all((0..10).map(|_| h.start(PHONE))).await;
    let ok = replies.iter().filter(|r| r.status == 202).count();
    let limited = replies
        .iter()
        .filter(|r| r.status == 429 && r.code() == "RATE_LIMITED")
        .count();
    assert_eq!((ok, limited), (5, 5));
    assert_eq!(h.sent().len(), 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn parallel_resends_of_one_challenge_send_once() {
    let h = Harness::new(false).await;
    let s = h.start(PHONE).await;
    let id = s.body["challenge_id"].as_str().unwrap().to_owned();
    h.advance(30_000);
    let replies = futures_util::future::join_all((0..5).map(|_| h.resend(&id))).await;
    let ok = replies.iter().filter(|r| r.status == 202).count();
    assert_eq!(
        ok,
        1,
        "{:?}",
        replies.iter().map(|r| r.status).collect::<Vec<_>>()
    );
    for r in replies.iter().filter(|r| r.status != 202) {
        assert_eq!((r.status, r.code()), (429, "RATE_LIMITED"));
        assert!(r.header("retry-after").is_some());
    }
    assert_eq!(h.sent().len(), 2);
    // The losers gave their reservations back: 2 sends counted.
    let counted: u64 =
        h.db.rate_buckets(&format!("otp_send:{PHONE}"))
            .await
            .unwrap()
            .iter()
            .map(|b| b.1)
            .sum();
    assert_eq!(counted, 2);
    assert_eq!(h.verify(&id, &h.last_code()).await.status, 200);
}

#[tokio::test]
async fn the_global_sms_cap_limits_sends_across_phones() {
    let h = Harness::with_global_cap(false, 3).await;
    for p in ["+14155550101", "+14155550102", "+14155550103"] {
        assert_eq!(h.start(p).await.status, 202);
    }
    let r = h.start("+14155550104").await;
    assert_eq!((r.status, r.code()), (429, "RATE_LIMITED"), "{}", r.body);
    // The 1 s bucket of the first sends leaves the minute 61 s from now.
    assert_eq!(r.header("retry-after"), Some("61"));
    assert_eq!(h.sent().len(), 3);
    // Refused by the global cap: nothing counted for that phone either.
    assert!(
        h.db.rate_buckets("otp_send:+14155550104")
            .await
            .unwrap()
            .is_empty()
    );
    h.advance(60_000);
    assert_eq!(h.start("+14155550104").await.status, 429);
    h.advance(1_000);
    assert_eq!(h.start("+14155550104").await.status, 202);
}

#[tokio::test]
async fn an_uncertain_send_stays_counted() {
    let h = Harness::new(false).await;
    h.st.sms.fake().unwrap().set_uncertain(true);
    let r = h.start(PHONE).await;
    assert_eq!((r.status, r.code()), (503, "INTERNAL"));
    let counted: u64 =
        h.db.rate_buckets(&format!("otp_send:{PHONE}"))
            .await
            .unwrap()
            .iter()
            .map(|b| b.1)
            .sum();
    assert_eq!(counted, 1);
}

#[tokio::test]
async fn a_repeated_verify_of_a_signed_in_owner_answers_the_same() {
    let h = Harness::new(false).await;
    let s = h.start(PHONE).await;
    let id = s.body["challenge_id"].as_str().unwrap().to_owned();
    let code = h.last_code();
    let first = h.verify(&id, &code).await;
    assert_eq!(first.status, 200);
    let access = first.cookie("arena_access").unwrap();
    let cookie = format!("arena_access={access}");
    let again = h
        .call(
            "POST",
            "/v1/signup/verify",
            Some(json!({"challenge_id": id, "code": code})),
            &[("cookie", &cookie)],
        )
        .await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(again.body, json!({"next": "account_setup"}));
    assert!(again.cookies().is_empty());
    assert_eq!(h.db.owner_count().await.unwrap(), 1);
    // Without that session it is a used code.
    assert_eq!(h.verify(&id, &code).await.code(), "OTP_EXPIRED");
    // Another owner's session does not count.
    let other = h.sign_up("+14155550100").await;
    let other_cookie = format!("arena_access={}", other.cookie("arena_access").unwrap());
    let r = h
        .call(
            "POST",
            "/v1/signup/verify",
            Some(json!({"challenge_id": id, "code": code})),
            &[("cookie", &other_cookie)],
        )
        .await;
    assert_eq!(r.code(), "OTP_EXPIRED");
}

#[tokio::test]
async fn every_v1_response_varies_on_origin() {
    let h = Harness::new(false).await;
    let evil = "https://evil.example";
    for headers in [vec![("origin", evil)], vec![("origin", ORIGIN)], vec![]] {
        let r = h.call("GET", "/v1/session", None, &headers).await;
        let vary: Vec<_> = r
            .headers
            .get_all("vary")
            .iter()
            .map(|v| v.to_str().unwrap().to_owned())
            .collect();
        assert!(
            vary.iter().any(|v| v.contains("Origin")),
            "{headers:?}: {vary:?}"
        );
    }
}

#[tokio::test]
async fn the_dev_outbox_is_served_only_in_dev() {
    let dev = Harness::new(true).await;
    dev.start(PHONE).await;
    let r = dev.call("GET", "/v1/dev/sms", None, &[]).await;
    assert_eq!(r.status, 200);
    let list = r.body.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["to"], PHONE);
    assert!(
        list[0]["text"]
            .as_str()
            .unwrap()
            .ends_with("It expires in 5 minutes.")
    );
    assert_eq!(list[0]["at"], T0);

    let prod = Harness::new(false).await;
    let r = prod.call("GET", "/v1/dev/sms", None, &[]).await;
    assert_eq!((r.status, r.code()), (404, "NOT_FOUND"));
    let r = prod
        .call(
            "POST",
            "/v1/dev/owner-alerts",
            Some(json!({"branch_id": "b", "kind": "test", "params": {}})),
            &[],
        )
        .await;
    assert_eq!(r.status, 404, "{}", r.body);
}

/// A `MakeWriter` into a shared buffer.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
    type Writer = Captured;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[tokio::test]
async fn logs_never_carry_the_submitted_values_the_code_or_tokens() {
    // A process-wide subscriber: a thread-local one misses callsites whose interest another
    // test thread already cached as "never". Every test in this binary then logs here too, and
    // none of them may log personal data either.
    let logs = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(logs.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("the only global subscriber");

    let h = Harness::new(true).await;
    let mut bad = start_body(PHONE);
    bad["email"] = json!("asha.rao@example");
    h.post("/v1/signup/start", bad).await;
    let s = h.start(PHONE).await;
    let id = s.body["challenge_id"].as_str().unwrap().to_owned();
    let code = h.last_code();
    h.verify(&id, &wrong(&code)).await;
    h.resend(&id).await;
    h.advance(30_000);
    h.resend(&id).await;
    let code2 = h.last_code();
    let ok = h.verify(&id, &code2).await;
    let access = ok.cookie("arena_access").unwrap();
    let refresh = ok.cookie("arena_refresh").unwrap();
    h.session(&access).await;
    let r2 = h.refresh(&refresh).await;
    h.refresh(&refresh).await;
    h.st.sms.fake().unwrap().set_failing(true);
    h.start(PHONE).await;
    for _ in 0..6 {
        h.st.sms.fake().unwrap().set_failing(false);
        h.start(PHONE).await;
    }

    // Without timestamps, whose digits could match a 6-digit code by chance.
    let text: String = String::from_utf8(logs.0.lock().unwrap().clone())
        .unwrap()
        .lines()
        .map(|l| {
            let mut v: Value = serde_json::from_str(l).unwrap();
            v.as_object_mut().unwrap().remove("timestamp");
            format!("{v}\n")
        })
        .collect();
    assert!(text.contains("signup code sent"), "{text}");
    assert!(text.contains(&h.st.phone_tag(PHONE)), "{text}");
    let mut secrets = vec![
        "Asha Rao".to_owned(),
        "Asha".to_owned(),
        "9876543210".to_owned(),
        "asha.rao@example".to_owned(),
        "Level Up Gaming".to_owned(),
        "Pune".to_owned(),
        code.clone(),
        wrong(&code),
        code2.clone(),
        access,
        refresh,
        PEPPER.to_owned(),
    ];
    for c in r2.cookies() {
        secrets.push(
            c.split(';')
                .next()
                .unwrap()
                .split('=')
                .nth(1)
                .unwrap()
                .to_owned(),
        );
    }
    for s in secrets.iter().filter(|s| !s.is_empty()) {
        assert!(!text.contains(s.as_str()), "logs contain {s:?}:\n{text}");
    }
}

/// A one-shot HTTP stub: records the raw request and answers `reply`.
fn stub(reply: &'static str) -> (String, std::thread::JoinHandle<String>) {
    let l = StdListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/GatewayAPI/rest", l.local_addr().unwrap());
    let t = std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = s.read(&mut chunk).unwrap();
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf).into_owned();
            if let Some((head, body)) = text.split_once("\r\n\r\n") {
                let len: usize = head
                    .lines()
                    .find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        k.eq_ignore_ascii_case("content-length")
                            .then(|| v.trim().parse().ok())?
                    })
                    .unwrap_or(0);
                if body.len() >= len {
                    break;
                }
            }
            if n == 0 {
                break;
            }
        }
        let res = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
            reply.len()
        );
        s.write_all(res.as_bytes()).unwrap();
        String::from_utf8(buf).unwrap()
    });
    (url, t)
}

fn gupshup(endpoint: String) -> GupshupSms {
    GupshupSms::new(GupshupConfig {
        endpoint,
        userid: "2000123456".into(),
        password: "gs-SECRET-pass".into(),
        sender: "ARNA36".into(),
        dlt_template_id: "1007160000000000001".into(),
    })
    .unwrap()
}

#[tokio::test]
async fn gupshup_posts_the_enterprise_api_form() {
    let (url, t) = stub("success | 919876543210 | 3456789012345678901-123");
    let sms = gupshup(url);
    let text = "123456 is your Arena 360 code. It expires in 5 minutes.";
    sms.send(PHONE, text).await.unwrap();
    let raw = t.join().unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("POST /GatewayAPI/rest HTTP/1.1"), "{head}");
    assert!(
        head.to_ascii_lowercase()
            .contains("content-type: application/x-www-form-urlencoded"),
        "{head}"
    );
    let form: Vec<(String, String)> = body
        .split('&')
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap();
            let dec = |s: &str| {
                let s = s.replace('+', " ");
                let mut out = Vec::new();
                let b = s.as_bytes();
                let mut i = 0;
                while i < b.len() {
                    if b[i] == b'%' {
                        out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
                        i += 3;
                    } else {
                        out.push(b[i]);
                        i += 1;
                    }
                }
                String::from_utf8(out).unwrap()
            };
            (dec(k), dec(v))
        })
        .collect();
    let get = |k: &str| form.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    assert_eq!(get("method"), Some("SendMessage"));
    assert_eq!(get("send_to"), Some("919876543210"));
    assert_eq!(get("msg"), Some(text));
    assert_eq!(get("msg_type"), Some("TEXT"));
    assert_eq!(get("userid"), Some("2000123456"));
    assert_eq!(get("password"), Some("gs-SECRET-pass"));
    assert_eq!(get("auth_scheme"), Some("plain"));
    assert_eq!(get("v"), Some("1.1"));
    assert_eq!(get("format"), Some("text"));
    assert_eq!(get("mask"), Some("ARNA36"));
    assert_eq!(get("dltTemplateId"), Some("1007160000000000001"));
}

#[tokio::test]
async fn a_2xx_reply_without_the_success_token_is_a_failure() {
    for reply in [
        "successful | 919876543210 | 1",
        "<html>success</html>",
        "Success | x",
        "",
    ] {
        let (url, t) = stub(reply);
        let err = gupshup(url)
            .send(PHONE, "123456 is your code")
            .await
            .unwrap_err();
        t.join().unwrap();
        assert!(!err.uncertain, "{reply:?}");
    }
    assert!(arena_cloud::messaging::is_success_reply(
        " success | 91 | 1"
    ));
}

#[tokio::test]
async fn a_gupshup_refusal_is_an_error_without_secrets() {
    let (url, t) = stub("error | 105 | Invalid password gs-SECRET-pass 919876543210");
    let err = gupshup(url)
        .send(PHONE, "123456 is your code")
        .await
        .unwrap_err();
    t.join().unwrap();
    let msg = err.to_string();
    assert!(msg.contains("105"), "{msg}");
    assert!(
        !msg.contains("gs-SECRET-pass") && !msg.contains("9876543210"),
        "{msg}"
    );
    let down = gupshup("http://127.0.0.1:9/GatewayAPI/rest".into())
        .send(PHONE, "x")
        .await
        .unwrap_err()
        .to_string();
    assert!(
        !down.contains("gs-SECRET-pass") && !down.contains("127.0.0.1:9"),
        "{down}"
    );
}

// ── The binary end to end: `--role all --dev` ───────────────────────────────────────────

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// (status, lowercase headers, body).
type RawReply = (u16, Vec<(String, String)>, String);

/// One HTTP/1.1 request; answers (status, headers, body).
fn http(
    port: u16,
    method: &str,
    path: &str,
    body: Option<&Value>,
    cookie: Option<&str>,
) -> Option<RawReply> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    let body = body.map(Value::to_string).unwrap_or_default();
    let mut raw = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
    if !body.is_empty() {
        raw.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\nOrigin: http://localhost:5179\r\n",
            body.len()
        ));
    }
    if let Some(c) = cookie {
        raw.push_str(&format!("Cookie: {c}\r\n"));
    }
    raw.push_str("\r\n");
    raw.push_str(&body);
    s.write_all(raw.as_bytes()).ok()?;
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).into_owned();
    let (head, body) = text.split_once("\r\n\r\n")?;
    let mut lines = head.lines();
    let status = lines.next()?.split(' ').nth(1)?.parse().ok()?;
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    Some((status, headers, body.to_owned()))
}

#[test]
fn the_dev_binary_signs_a_new_owner_up_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let port = StdListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let child = Command::new(env!("CARGO_BIN_EXE_arena-cloud"))
        .env_remove("ARENA_CONTROL_DB_URL")
        .env_remove("ARENA_OTP_PEPPER")
        .env_remove("ARENA_WEB_ORIGINS")
        .env_remove("ARENA_SMS_PROVIDER")
        .env_remove("ARENA_MFA_KEY")
        .args(["--role", "all", "--dev", "--data-dir"])
        .arg(dir.path())
        .args(["--listen", &format!("127.0.0.1:{port}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _running = Running(child);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some((200, _, b)) = http(port, "GET", "/healthz", None, None) {
            let want = format!(
                "\"migration_version\":{}",
                arena_store::control::LATEST_CONTROL_VERSION
            );
            assert!(b.contains(&want), "{b}");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the dev binary did not serve /healthz"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let (status, _, body) = http(
        port,
        "POST",
        "/v1/signup/start",
        Some(&start_body(PHONE)),
        None,
    )
    .unwrap();
    assert_eq!(status, 202, "{body}");
    let id = serde_json::from_str::<Value>(&body).unwrap()["challenge_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, _, outbox) = http(port, "GET", "/v1/dev/sms", None, None).unwrap();
    let outbox: Value = serde_json::from_str(&outbox).unwrap();
    let code = outbox[0]["text"].as_str().unwrap()[..6].to_owned();
    let (status, headers, body) = http(
        port,
        "POST",
        "/v1/signup/verify",
        Some(&json!({"challenge_id": id, "code": code})),
        None,
    )
    .unwrap();
    assert_eq!(status, 200, "{body}");
    let access = headers
        .iter()
        .filter(|(k, _)| k == "set-cookie")
        .find_map(|(_, v)| v.strip_prefix("arena_access="))
        .and_then(|v| v.split(';').next())
        .unwrap()
        .to_owned();
    let (status, _, body) = http(
        port,
        "GET",
        "/v1/session",
        None,
        Some(&format!("arena_access={access}")),
    )
    .unwrap();
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("\"mfa_state\":\"pending\""), "{body}");
}

#[test]
fn production_refuses_to_start_without_the_signup_settings() {
    let out = Command::new(env!("CARGO_BIN_EXE_arena-cloud"))
        .env("ARENA_CONTROL_DB_URL", "libsql://x.turso.io")
        .env("ARENA_CONTROL_DB_TOKEN", "t")
        .env_remove("ARENA_OTP_PEPPER")
        .env_remove("ARENA_WEB_ORIGINS")
        .env_remove("ARENA_SMS_PROVIDER")
        .args(["--role", "control"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(64));
    assert!(String::from_utf8_lossy(&out.stderr).contains("ARENA_OTP_PEPPER"));
}
