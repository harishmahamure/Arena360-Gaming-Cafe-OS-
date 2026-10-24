//! The owner's password, authenticator app (TOTP) and backup codes (Story 2.5), in process: the
//! full app router over a local `gz_control`, the recorded fake SMS (to sign up) and a
//! hand-moved clock.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use arena_cloud::auth::AuthState;
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
use arena_cloud::passwords::{normalize_backup_code, verify_secret};
use arena_cloud::{Clock, app_with_auth, auth_state, totp};
use arena_proto::MfaState;
use arena_store::control::{ControlDb, LATEST_CONTROL_VERSION};
use axum::body::{Body, to_bytes};
use axum::http::{Request, header};
use serde_json::{Value, json};
use tokio::sync::watch;
use tower::ServiceExt;

const ORIGIN: &str = "https://start.arena360.cloud";
const PEPPER: &str = "test-pepper-0123456789abcdef0123456789";
const T0: i64 = 1_800_000_000_000;
const PHONE: &str = "+919876543210";
const PASSWORD: &str = "Tandoori-Nights-42";
const STEP_MS: i64 = 30_000;

struct Harness {
    app: axum::Router,
    db: Arc<ControlDb>,
    st: Arc<AuthState>,
    now: Arc<AtomicI64>,
    dir: Arc<tempfile::TempDir>,
    /// The signed-in owner's `arena_access` token.
    access: String,
}

struct Reply {
    status: u16,
    headers: axum::http::HeaderMap,
    body: Value,
}

impl Reply {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or("")
    }

    fn field(&self) -> &str {
        self.body["field"].as_str().unwrap_or("")
    }

    fn codes(&self) -> Vec<String> {
        self.body["backup_codes"]
            .as_array()
            .unwrap_or_else(|| panic!("no backup_codes: {}", self.body))
            .iter()
            .map(|c| c.as_str().unwrap().to_owned())
            .collect()
    }
}

fn key_a() -> MfaKey {
    MfaKey::from_bytes([0xa5; 32])
}

/// RFC 4648 base32 (no padding) back to bytes.
fn unbase32(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut buf, mut bits) = (0u32, 0);
    for c in s.bytes() {
        let v = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"
            .iter()
            .position(|&x| x == c)
            .unwrap() as u32;
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    out
}

impl Harness {
    /// A fresh database with one signed-up (`pending`) owner.
    async fn new(dev: bool) -> Self {
        let dir = Arc::new(tempfile::tempdir().unwrap());
        let db = ControlDb::open_local(&dir.path().join("gz_control.db"))
            .await
            .unwrap();
        db.migrate(T0).await.unwrap();
        let mut h = Self::over(
            Arc::new(db),
            dir,
            Arc::new(AtomicI64::new(T0)),
            key_a(),
            dev,
        );
        h.access = h.sign_up().await;
        h
    }

    /// Another app over the same database and clock, with `key`.
    fn with_key(&self, key: MfaKey) -> Self {
        let mut h = Self::over(
            Arc::clone(&self.db),
            Arc::clone(&self.dir),
            Arc::clone(&self.now),
            key,
            true,
        );
        h.access = self.access.clone();
        h
    }

    fn over(
        db: Arc<ControlDb>,
        dir: Arc<tempfile::TempDir>,
        now: Arc<AtomicI64>,
        mfa_key: MfaKey,
        dev: bool,
    ) -> Self {
        let clock_now = Arc::clone(&now);
        let clock: Clock = Arc::new(move || clock_now.load(Ordering::SeqCst));
        let cfg = AuthConfig {
            pepper: PEPPER.as_bytes().to_vec(),
            web_origins: vec![ORIGIN.to_owned()],
            sms: SmsProviderConfig::Fake,
            sms_global_per_min: 10_000,
            mfa_key,
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
            LATEST_CONTROL_VERSION,
            Some(Arc::clone(&st)),
        );
        Self {
            app,
            db,
            st,
            now,
            dir,
            access: String::new(),
        }
    }

    fn now(&self) -> i64 {
        self.now.load(Ordering::SeqCst)
    }

    fn advance(&self, ms: i64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
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

    async fn post(&self, path: &str, body: Option<Value>) -> Reply {
        self.call("POST", path, body, &self.access).await
    }

    /// Signs the owner up with the phone code; answers the access token.
    async fn sign_up(&self) -> String {
        let s = self
            .call(
                "POST",
                "/v1/signup/start",
                Some(json!({
                    "name": "Asha Rao",
                    "phone": PHONE,
                    "email": "asha.rao@example.in",
                    "cafe_name": "Level Up Gaming",
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
        let v = self
            .call(
                "POST",
                "/v1/signup/verify",
                Some(json!({"challenge_id": id, "code": &text[..6]})),
                "",
            )
            .await;
        assert_eq!(v.status, 200, "{}", v.body);
        v.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .find_map(|c| {
                let c = c.to_str().unwrap();
                c.strip_prefix("arena_access=")
                    .map(|v| v.split(';').next().unwrap().to_owned())
            })
            .unwrap()
    }

    async fn sign_in_again(&mut self) {
        let n = self.now();
        let token = format!("relogin-{n}");
        self.db
            .insert_owner_session(&arena_store::control::NewOwnerSession {
                id: format!("s-{n}"),
                owner_id: self.owner_id().await,
                family_id: format!("f-{n}"),
                access_hash: arena_cloud::auth::token_hash(&token),
                access_expires_at: n + 900_000,
                refresh_hash: arena_cloud::auth::token_hash(&format!("refresh-{n}")),
                refresh_expires_at: n + 86_400_000,
                created_at: n,
                device_id: None,
            })
            .await
            .unwrap();
        self.access = token;
    }

    async fn owner_id(&self) -> String {
        self.db.owner_by_phone(PHONE).await.unwrap().unwrap().id
    }

    async fn session(&self) -> Value {
        let r = self.call("GET", "/v1/session", None, &self.access).await;
        assert_eq!(r.status, 200, "{}", r.body);
        r.body["owner"].clone()
    }

    async fn set_password(&self, password: &str) -> Reply {
        self.post(
            "/v1/account/password",
            Some(json!({ "password": password })),
        )
        .await
    }

    /// Starts enrolment; answers the reply and the decoded secret.
    async fn start(&self) -> (Reply, Vec<u8>) {
        let r = self.post("/v1/account/mfa/totp/start", None).await;
        let secret = r.body["secret"].as_str().map(unbase32).unwrap_or_default();
        (r, secret)
    }

    async fn confirm(&self, code: &str) -> Reply {
        self.post(
            "/v1/account/mfa/totp/confirm",
            Some(json!({ "code": code })),
        )
        .await
    }

    async fn regenerate(&self, code: &str) -> Reply {
        self.post(
            "/v1/account/mfa/backup-codes/regenerate",
            Some(json!({ "code": code })),
        )
        .await
    }

    async fn ack(&self) -> Reply {
        self.post("/v1/account/mfa/backup-codes/ack", None).await
    }

    async fn gate(&self) -> Reply {
        self.call("GET", "/v1/dev/setup-gate", None, &self.access)
            .await
    }

    /// The code for the current step (plus `d` steps).
    fn code(&self, secret: &[u8], d: i64) -> String {
        totp::code_at(secret, totp::step_of(self.now()) + d)
    }

    /// Password + start + confirm: answers the secret and the backup codes.
    async fn enrol(&self) -> (Vec<u8>, Vec<String>) {
        assert_eq!(self.set_password(PASSWORD).await.status, 204);
        let (r, secret) = self.start().await;
        assert_eq!(r.status, 200, "{}", r.body);
        let c = self.confirm(&self.code(&secret, 0)).await;
        assert_eq!(c.status, 200, "{}", c.body);
        (secret, c.codes())
    }
}

#[tokio::test]
async fn every_account_route_needs_a_session() {
    let h = Harness::new(true).await;
    for path in [
        "/v1/account/password",
        "/v1/account/mfa/totp/start",
        "/v1/account/mfa/totp/confirm",
        "/v1/account/mfa/backup-codes/ack",
        "/v1/account/mfa/backup-codes/regenerate",
    ] {
        let r = h
            .call(
                "POST",
                path,
                Some(json!({"password": PASSWORD, "code": "000000"})),
                "",
            )
            .await;
        assert_eq!((r.status, r.code()), (401, "AUTH_FAILED"), "{path}");
    }
    let r = h.call("GET", "/v1/dev/setup-gate", None, "").await;
    assert_eq!((r.status, r.code()), (401, "AUTH_FAILED"));
}

#[tokio::test]
async fn the_password_rule_and_a_single_set() {
    let h = Harness::new(true).await;
    let owner = h.session().await;
    assert_eq!(
        owner,
        json!({"name": "Asha Rao", "cafe_name": "Level Up Gaming", "city": "Pune", "mfa_state": "pending", "has_password": false, "backup_codes_acked": false, "sms_fallback": false})
    );

    // Short: 9 characters.
    let r = h.set_password("abcdefghi").await;
    assert_eq!(
        (r.status, r.code(), r.field()),
        (422, "PASSWORD_TOO_SHORT", "password")
    );
    // Common.
    let r = h.set_password("password123").await;
    assert_eq!(
        (r.status, r.code(), r.field()),
        (422, "PASSWORD_COMMON", "password")
    );
    let r = h.set_password("Password123").await;
    assert_eq!(r.code(), "PASSWORD_COMMON");
    let r = h.set_password(&"x".repeat(129)).await;
    assert_eq!(
        (r.status, r.code(), r.field()),
        (422, "VALIDATION_FAILED", "password")
    );
    let id = h.owner_id().await;
    assert_eq!(h.db.owner_password_hash(&id).await.unwrap(), None);

    // Valid: an argon2id PHC string with the OWASP parameters.
    let r = h.set_password(PASSWORD).await;
    assert_eq!(r.status, 204, "{}", r.body);
    assert_eq!(r.body, Value::Null);
    let hash = h.db.owner_password_hash(&id).await.unwrap().unwrap();
    assert!(
        hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
        "{hash}"
    );
    assert!(verify_secret(PASSWORD, &hash));
    assert!(!hash.contains(PASSWORD));
    assert_eq!(h.session().await["has_password"], json!(true));

    // Again: refused, the hash unchanged.
    let r = h.set_password("Another-Good-Password").await;
    assert_eq!((r.status, r.code()), (409, "ALREADY_SET"));
    assert_eq!(h.db.owner_password_hash(&id).await.unwrap().unwrap(), hash);
}

#[tokio::test]
async fn totp_needs_a_password_first() {
    let h = Harness::new(true).await;
    let (r, _) = h.start().await;
    assert_eq!((r.status, r.code()), (403, "PASSWORD_REQUIRED"));
    let r = h.confirm("123456").await;
    assert_eq!((r.status, r.code()), (403, "PASSWORD_REQUIRED"));
    assert_eq!(h.db.owner_mfa(&h.owner_id().await).await.unwrap(), None);
}

#[tokio::test]
async fn start_mints_a_new_secret_each_time_and_the_old_one_stops_working() {
    let h = Harness::new(true).await;
    assert_eq!(h.set_password(PASSWORD).await.status, 204);
    let (r1, s1) = h.start().await;
    assert_eq!(r1.status, 200, "{}", r1.body);
    assert_eq!(r1.headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
    let b32 = r1.body["secret"].as_str().unwrap();
    assert_eq!((s1.len(), b32.len()), (20, 32));
    assert!(!b32.contains('='));
    assert_eq!(
        r1.body["otpauth_uri"].as_str().unwrap(),
        format!(
            "otpauth://totp/Arena%20360:%2B919876543210?secret={b32}&issuer=Arena%20360\
             &algorithm=SHA1&digits=6&period=30"
        )
    );
    let svg = r1.body["qr_svg"].as_str().unwrap();
    assert!(svg.contains("<svg") && svg.contains("</svg>"));

    // The stored secret is ciphertext, not the secret.
    let id = h.owner_id().await;
    let m = h.db.owner_mfa(&id).await.unwrap().unwrap();
    assert_ne!(m.secret_ct, s1);
    assert!(!m.secret_ct.windows(20).any(|w| w == s1.as_slice()));

    let (r2, s2) = h.start().await;
    assert_eq!(r2.status, 200);
    assert_ne!(s1, s2);
    // The first secret's codes fail; the second's work.
    let r = h.confirm(&h.code(&s1, 0)).await;
    assert_eq!(
        (r.status, r.code(), r.field()),
        (400, "TOTP_INVALID", "code")
    );
    let r = h.confirm(&h.code(&s2, 0)).await;
    assert_eq!(r.status, 200, "{}", r.body);
}

#[tokio::test]
async fn confirm_enrols_and_answers_ten_hashed_codes_once() {
    let h = Harness::new(true).await;
    let (secret, codes) = h.enrol().await;
    assert_eq!(codes.len(), 10);
    for c in &codes {
        assert_eq!(c.len(), 11, "{c}");
        assert_eq!(&c[5..6], "-");
        assert!(normalize_backup_code(c).is_some(), "{c}");
    }
    let id = h.owner_id().await;
    let owner = h.db.owner_by_id(&id).await.unwrap().unwrap();
    assert_eq!(owner.mfa_state, MfaState::Enrolled);
    let m = h.db.owner_mfa(&id).await.unwrap().unwrap();
    assert_eq!(m.confirmed_at, Some(h.now()));
    assert_eq!(m.last_step, Some(totp::step_of(h.now())));
    // Only argon2id hashes are stored, one per code.
    let rows = h.db.backup_codes(&id).await.unwrap();
    assert_eq!(rows.len(), 10);
    for c in &codes {
        let n = normalize_backup_code(c).unwrap();
        assert!(rows.iter().all(|r| !r.code_hash.contains(&n)));
        assert_eq!(
            rows.iter()
                .filter(|r| verify_secret(&n, &r.code_hash))
                .count(),
            1,
            "{c}"
        );
    }
    assert_eq!(
        h.session().await,
        json!({"name": "Asha Rao", "cafe_name": "Level Up Gaming", "city": "Pune", "mfa_state": "enrolled", "has_password": true, "backup_codes_acked": false, "sms_fallback": false})
    );

    // Enrolled: no second start or confirm.
    h.advance(STEP_MS);
    let (r, _) = h.start().await;
    assert_eq!((r.status, r.code()), (409, "ALREADY_SET"));
    let r = h.confirm(&h.code(&secret, 0)).await;
    assert_eq!((r.status, r.code()), (409, "ALREADY_SET"));
}

#[tokio::test]
async fn one_step_either_side_is_accepted_but_not_two() {
    for (d, ok) in [(-1, true), (1, true), (-2, false), (2, false)] {
        let h = Harness::new(true).await;
        assert_eq!(h.set_password(PASSWORD).await.status, 204);
        let (_, secret) = h.start().await;
        let r = h.confirm(&h.code(&secret, d)).await;
        assert_eq!(r.status == 200, ok, "step {d}: {}", r.body);
    }
}

#[tokio::test]
async fn a_wrong_code_keeps_the_owner_pending() {
    let h = Harness::new(true).await;
    assert_eq!(h.set_password(PASSWORD).await.status, 204);
    let (_, secret) = h.start().await;
    let right = h.code(&secret, 0);
    let wrong = format!("{:06}", (right.parse::<u32>().unwrap() + 1) % 1_000_000);
    for bad in [wrong.as_str(), "12345", "abcdef", ""] {
        let r = h.confirm(bad).await;
        assert_eq!(
            (r.status, r.code(), r.field()),
            (400, "TOTP_INVALID", "code"),
            "{bad:?}"
        );
    }
    assert_eq!(h.session().await["mfa_state"], json!("pending"));
    assert!(
        h.db.backup_codes(&h.owner_id().await)
            .await
            .unwrap()
            .is_empty()
    );
    // The right code still works afterwards.
    assert_eq!(h.confirm(&right).await.status, 200);
}

#[tokio::test]
async fn regenerate_refuses_a_replayed_step_and_replaces_every_code() {
    let h = Harness::new(true).await;
    let (secret, first) = h.enrol().await;
    let id = h.owner_id().await;
    let old_rows = h.db.backup_codes(&id).await.unwrap();

    // Replay: the step used to confirm is refused, and an earlier one.
    let r = h.regenerate(&h.code(&secret, 0)).await;
    assert_eq!(
        (r.status, r.code(), r.field()),
        (400, "TOTP_INVALID", "code")
    );
    let r = h.regenerate(&h.code(&secret, -1)).await;
    assert_eq!(r.code(), "TOTP_INVALID");
    assert_eq!(h.db.backup_codes(&id).await.unwrap(), old_rows);

    // The next step works: every old code is gone and 10 new ones exist.
    h.advance(STEP_MS);
    let code = h.code(&secret, 0);
    let r = h.regenerate(&code).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
    let second = r.codes();
    assert_eq!(second.len(), 10);
    assert!(second.iter().all(|c| !first.contains(c)));
    let rows = h.db.backup_codes(&id).await.unwrap();
    assert_eq!(rows.len(), 10);
    assert!(rows.iter().all(|r| !old_rows.iter().any(|o| o.id == r.id)));
    for c in &first {
        let n = normalize_backup_code(c).unwrap();
        assert!(!rows.iter().any(|r| verify_secret(&n, &r.code_hash)), "{c}");
    }
    assert_eq!(
        h.db.owner_mfa(&id).await.unwrap().unwrap().last_step,
        Some(totp::step_of(h.now()))
    );

    // The same code again is a replay.
    let r = h.regenerate(&code).await;
    assert_eq!(r.code(), "TOTP_INVALID");
    // A wrong code too.
    h.advance(STEP_MS);
    let right = h.code(&secret, 0);
    let wrong = format!("{:06}", (right.parse::<u32>().unwrap() + 7) % 1_000_000);
    assert_eq!(h.regenerate(&wrong).await.code(), "TOTP_INVALID");
}

#[tokio::test]
async fn ack_and_regenerate_need_an_enrolled_owner() {
    let h = Harness::new(true).await;
    let r = h.ack().await;
    assert_eq!((r.status, r.code()), (403, "FORBIDDEN"));
    let r = h.regenerate("123456").await;
    assert_eq!((r.status, r.code()), (403, "FORBIDDEN"));
    assert_eq!(h.set_password(PASSWORD).await.status, 204);
    h.start().await;
    assert_eq!(h.ack().await.code(), "FORBIDDEN");
    assert_eq!(h.regenerate("123456").await.code(), "FORBIDDEN");
}

#[tokio::test]
async fn the_gate_passes_only_a_complete_setup() {
    let h = Harness::new(true).await;
    let incomplete =
        |r: &Reply| (r.status, r.code().to_owned()) == (403, "SETUP_INCOMPLETE".into());
    // Pending, no password.
    assert!(incomplete(&h.gate().await));
    assert_eq!(h.set_password(PASSWORD).await.status, 204);
    assert!(incomplete(&h.gate().await));
    let (_, secret) = h.start().await;
    assert!(incomplete(&h.gate().await));
    assert_eq!(h.confirm(&h.code(&secret, 0)).await.status, 200);
    // Enrolled, not acked.
    assert!(incomplete(&h.gate().await));
    let r = h.ack().await;
    assert_eq!(r.status, 204, "{}", r.body);
    let r = h.gate().await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body, json!({"setup_complete": true}));
    assert_eq!(h.session().await["backup_codes_acked"], json!(true));
    h.advance(STEP_MS);
    assert_eq!(h.regenerate(&h.code(&secret, 0)).await.status, 200);
    assert!(incomplete(&h.gate().await));
    assert_eq!(h.session().await["backup_codes_acked"], json!(false));
    assert_eq!(h.ack().await.status, 204);
    assert_eq!(h.gate().await.status, 200);
}

#[tokio::test]
async fn ack_needs_an_unused_backup_code() {
    let h = Harness::new(true).await;
    h.enrol().await;
    let id = h.owner_id().await;
    rusqlite::Connection::open(h.dir.path().join("gz_control.db"))
        .unwrap()
        .execute(
            "UPDATE owner_backup_codes SET used_at = 1 WHERE owner_id = ?",
            [&id],
        )
        .unwrap();
    let r = h.ack().await;
    assert_eq!((r.status, r.code()), (403, "FORBIDDEN"));
    assert_eq!(h.session().await["backup_codes_acked"], json!(false));
}

fn wrong_code(right: &str) -> String {
    format!("{:06}", (right.parse::<u32>().unwrap() + 1) % 1_000_000)
}

#[tokio::test]
async fn confirm_allows_five_wrong_codes_per_15_minutes() {
    let mut h = Harness::new(true).await;
    assert_eq!(h.set_password(PASSWORD).await.status, 204);
    let (_, secret) = h.start().await;
    for _ in 0..5 {
        let r = h.confirm(&wrong_code(&h.code(&secret, 0))).await;
        assert_eq!(r.code(), "TOTP_INVALID");
    }
    let r = h.confirm(&h.code(&secret, 0)).await;
    assert_eq!((r.status, r.code()), (429, "RATE_LIMITED"));
    let wait: i64 = r
        .headers
        .get(header::RETRY_AFTER)
        .unwrap()
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(wait > 0 && wait <= 16 * 60, "{wait}");
    assert_eq!(h.session().await["mfa_state"], json!("pending"));
    h.advance(16 * 60_000);
    h.sign_in_again().await;
    let r = h.confirm(&h.code(&secret, 0)).await;
    assert_eq!(r.status, 200, "{}", r.body);
}

#[tokio::test]
async fn regenerate_counts_only_failures_against_the_limit() {
    let mut h = Harness::new(true).await;
    let (secret, _) = h.enrol().await;
    for _ in 0..6 {
        h.advance(STEP_MS);
        assert_eq!(h.regenerate(&h.code(&secret, 0)).await.status, 200);
    }
    for _ in 0..5 {
        let r = h.regenerate(&wrong_code(&h.code(&secret, 0))).await;
        assert_eq!(r.code(), "TOTP_INVALID");
    }
    h.advance(STEP_MS);
    let r = h.regenerate(&h.code(&secret, 0)).await;
    assert_eq!((r.status, r.code()), (429, "RATE_LIMITED"));
    assert!(r.headers.get(header::RETRY_AFTER).is_some());
    h.advance(16 * 60_000);
    h.sign_in_again().await;
    assert_eq!(h.regenerate(&h.code(&secret, 0)).await.status, 200);
}

#[tokio::test]
async fn the_gate_probe_exists_only_in_dev() {
    let h = Harness::new(false).await;
    let r = h.gate().await;
    assert_eq!((r.status, r.code()), (404, "NOT_FOUND"));
}

#[tokio::test]
async fn a_secret_under_another_key_is_an_internal_error() {
    let h = Harness::new(true).await;
    assert_eq!(h.set_password(PASSWORD).await.status, 204);
    let (_, secret) = h.start().await;
    let other = h.with_key(MfaKey::from_bytes([0x5a; 32]));
    let r = other.confirm(&other.code(&secret, 0)).await;
    assert_eq!((r.status, r.code()), (500, "INTERNAL"));
    assert_eq!(r.body["message"], arena_cloud::api::INTERNAL_MESSAGE);
    assert_eq!(other.session().await["mfa_state"], json!("pending"));
    // The right key still opens it.
    assert_eq!(h.confirm(&h.code(&secret, 0)).await.status, 200);
}

#[test]
fn rfc_6238_sha1_vectors() {
    let secret = b"12345678901234567890";
    for (t, want) in [
        (59_i64, 94_287_082_u32),
        (1_111_111_109, 7_081_804),
        (1_111_111_111, 14_050_471),
        (1_234_567_890, 89_005_924),
        (2_000_000_000, 69_279_037),
        (20_000_000_000, 65_353_130),
    ] {
        let step = totp::step_of(t * 1000);
        assert_eq!(totp::hotp(secret, step as u64, 8), want, "T = {t}");
        assert_eq!(
            totp::code_at(secret, step),
            format!("{:06}", want % 1_000_000)
        );
    }
}

#[test]
fn aes_gcm_round_trip_and_a_wrong_key() {
    let s = key_a().seal("owner-1", b"twenty-byte-secret!!").unwrap();
    assert_eq!(
        key_a().open("owner-1", &s.ciphertext, &s.nonce).unwrap(),
        b"twenty-byte-secret!!"
    );
    assert!(
        MfaKey::from_bytes([1; 32])
            .open("owner-1", &s.ciphertext, &s.nonce)
            .is_err()
    );
    assert!(key_a().open("owner-2", &s.ciphertext, &s.nonce).is_err());
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
async fn logs_never_carry_the_password_secret_uri_or_codes() {
    // Process-wide, like the sign-up suite: every test in this binary logs here too.
    let logs = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(logs.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("the only global subscriber");

    let h = Harness::new(true).await;
    h.set_password("password123").await;
    h.set_password("short").await;
    assert_eq!(h.set_password(PASSWORD).await.status, 204);
    h.set_password(PASSWORD).await;
    let (r, s1) = h.start().await;
    let uri1 = r.body["otpauth_uri"].as_str().unwrap().to_owned();
    let b32_1 = r.body["secret"].as_str().unwrap().to_owned();
    let (r, secret) = h.start().await;
    let uri2 = r.body["otpauth_uri"].as_str().unwrap().to_owned();
    let b32_2 = r.body["secret"].as_str().unwrap().to_owned();
    let stale = h.code(&s1, 0);
    h.confirm(&stale).await;
    // A wrong key fails, and says so in the log.
    let other = h.with_key(MfaKey::from_bytes([0x33; 32]));
    other.confirm(&other.code(&secret, 0)).await;
    let c1 = h.code(&secret, 0);
    let first = h.confirm(&c1).await.codes();
    h.regenerate(&c1).await;
    h.advance(STEP_MS);
    let c2 = h.code(&secret, 0);
    let second = h.regenerate(&c2).await.codes();
    h.ack().await;
    h.gate().await;

    let text: String = String::from_utf8(logs.0.lock().unwrap().clone())
        .unwrap()
        .lines()
        .map(|l| {
            let mut v: Value = serde_json::from_str(l).unwrap();
            v.as_object_mut().unwrap().remove("timestamp");
            format!("{v}\n")
        })
        .collect();
    assert!(text.contains("owner password set"), "{text}");
    assert!(text.contains("totp enrolled"), "{text}");
    assert!(text.contains("mfa secret did not decrypt"), "{text}");
    let mut secrets = vec![
        PASSWORD.to_owned(),
        "password123".to_owned(),
        uri1,
        uri2,
        "otpauth://".to_owned(),
        b32_1.clone(),
        b32_2.clone(),
        b32_1[..16].to_owned(),
        b32_2[..16].to_owned(),
        stale,
        c1,
        c2,
    ];
    for c in first.iter().chain(&second) {
        secrets.push(c.clone());
        secrets.push(normalize_backup_code(c).unwrap());
    }
    for s in secrets.iter().filter(|s| !s.is_empty()) {
        assert!(!text.contains(s.as_str()), "logs contain {s:?}:\n{text}");
    }
}

#[tokio::test]
async fn a_refresh_answers_the_owner_even_if_the_mfa_lookup_fails() {
    let mut h = Harness::new(true).await;
    h.enrol().await;
    assert_eq!(h.ack().await.status, 204);
    h.sign_in_again().await;
    let refresh = format!("x; arena_refresh=refresh-{}", h.now());
    rusqlite::Connection::open(h.dir.path().join("gz_control.db"))
        .unwrap()
        .execute_batch("ALTER TABLE owner_mfa RENAME TO owner_mfa_gone")
        .unwrap();
    let r = h.call("POST", "/v1/session/refresh", None, &refresh).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        r.body["owner"],
        json!({"name": "Asha Rao", "cafe_name": "Level Up Gaming", "city": "Pune", "mfa_state": "enrolled", "has_password": true, "backup_codes_acked": false, "sms_fallback": false})
    );
    assert!(
        r.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .any(|c| c.to_str().unwrap().starts_with("arena_access="))
    );
}
