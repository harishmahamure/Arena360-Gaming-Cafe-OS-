use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use arena_cloud::api::signin::SIGNIN_FAILED_MESSAGE;
use arena_cloud::auth::AuthState;
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
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
const EMAIL: &str = "asha.rao@example.in";
const PASSWORD: &str = "Tandoori-Nights-42";
const STEP_MS: i64 = 30_000;
const MINUTE_MS: i64 = 60_000;
const DAY_MS: i64 = 86_400_000;
const ANDROID_UA: &str = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Mobile Safari/537.36";

#[derive(Clone, Default, Debug)]
struct Jar {
    access: Option<String>,
    refresh: Option<String>,
    trusted: Option<String>,
}

impl Jar {
    fn header(&self) -> String {
        [
            ("arena_access", &self.access),
            ("arena_refresh", &self.refresh),
            ("arena_trusted", &self.trusted),
        ]
        .iter()
        .filter_map(|(k, v)| v.as_ref().map(|v| format!("{k}={v}")))
        .collect::<Vec<_>>()
        .join("; ")
    }

    fn absorb(&mut self, r: &Reply) {
        for c in r.set_cookies() {
            let (kv, _) = c.split_once(';').unwrap_or((&c, ""));
            let (k, v) = kv.split_once('=').unwrap();
            let v = (!v.is_empty()).then(|| v.to_owned());
            match k {
                "arena_access" => self.access = v,
                "arena_refresh" => self.refresh = v,
                "arena_trusted" => self.trusted = v,
                _ => {}
            }
        }
    }
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

    fn set_cookies(&self) -> Vec<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|c| c.to_str().unwrap().to_owned())
            .collect()
    }

    fn set_cookie(&self, name: &str) -> Option<String> {
        self.set_cookies()
            .into_iter()
            .find(|c| c.starts_with(&format!("{name}=")))
    }

    fn retry_after(&self) -> Option<u64> {
        self.headers
            .get(header::RETRY_AFTER)
            .map(|v| v.to_str().unwrap().parse().unwrap())
    }

    fn challenge(&self) -> String {
        self.body["challenge_id"]
            .as_str()
            .unwrap_or_else(|| panic!("no challenge: {}", self.body))
            .to_owned()
    }

    fn methods(&self) -> Vec<String> {
        self.body["methods"]
            .as_array()
            .unwrap_or_else(|| panic!("no methods: {}", self.body))
            .iter()
            .map(|m| m.as_str().unwrap().to_owned())
            .collect()
    }

    fn assert_signin_failed(&self) {
        assert_eq!(
            (self.status, self.code()),
            (401, "AUTH_FAILED"),
            "{}",
            self.body
        );
        assert_eq!(self.body["message"], SIGNIN_FAILED_MESSAGE);
        assert!(self.body.get("field").is_none(), "{}", self.body);
    }
}

struct Harness {
    app: axum::Router,
    db: Arc<ControlDb>,
    st: Arc<AuthState>,
    now: Arc<AtomicI64>,
    _dir: tempfile::TempDir,
}

struct Enrolled {
    secret: Vec<u8>,
    codes: Vec<String>,
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
        let clock_now = Arc::clone(&now);
        let clock: Clock = Arc::new(move || clock_now.load(Ordering::SeqCst));
        let cfg = AuthConfig {
            pepper: PEPPER.as_bytes().to_vec(),
            web_origins: vec![ORIGIN.to_owned()],
            sms: SmsProviderConfig::Fake,
            sms_global_per_min: 10_000,
            mfa_key: MfaKey::from_bytes([0xa5; 32]),
        };
        let st = auth_state(Arc::clone(&db), &cfg, clock).unwrap();
        let (_tx, rx) = watch::channel(Reachability {
            reachable: true,
            checked_at: None,
        });
        let app = app_with_auth(
            Role::All,
            true,
            rx,
            LATEST_CONTROL_VERSION,
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

    fn now(&self) -> i64 {
        self.now.load(Ordering::SeqCst)
    }

    fn advance(&self, ms: i64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }

    async fn call(&self, method: &str, path: &str, body: Option<Value>, jar: &Jar) -> Reply {
        self.call_ua(method, path, body, jar, None).await
    }

    async fn call_ua(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        jar: &Jar,
        ua: Option<&str>,
    ) -> Reply {
        let mut req = Request::builder().method(method).uri(path);
        let cookies = jar.header();
        if !cookies.is_empty() {
            req = req.header("cookie", cookies);
        }
        if let Some(ua) = ua {
            req = req.header("user-agent", ua);
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

    fn last_sms_code(&self) -> String {
        let sent = self.st.sms.fake().unwrap().sent();
        sent.last().unwrap().text[..6].to_owned()
    }

    async fn sign_up(&self) -> Jar {
        let s = self
            .call(
                "POST",
                "/v1/signup/start",
                Some(json!({
                    "name": "Asha Rao",
                    "phone": PHONE,
                    "email": EMAIL,
                    "cafe_name": "Level Up Gaming",
                    "city": "Pune",
                })),
                &Jar::default(),
            )
            .await;
        assert_eq!(s.status, 202, "{}", s.body);
        let id = s.challenge();
        let code = self.last_sms_code();
        let v = self
            .call(
                "POST",
                "/v1/signup/verify",
                Some(json!({"challenge_id": id, "code": code})),
                &Jar::default(),
            )
            .await;
        assert_eq!(v.status, 200, "{}", v.body);
        let mut jar = Jar::default();
        jar.absorb(&v);
        jar
    }

    async fn enrol(&self) -> Enrolled {
        let jar = self.sign_up().await;
        let p = self
            .call(
                "POST",
                "/v1/account/password",
                Some(json!({ "password": PASSWORD })),
                &jar,
            )
            .await;
        assert_eq!(p.status, 204, "{}", p.body);
        let s = self
            .call("POST", "/v1/account/mfa/totp/start", None, &jar)
            .await;
        assert_eq!(s.status, 200, "{}", s.body);
        let secret = unbase32(s.body["secret"].as_str().unwrap());
        let c = self
            .call(
                "POST",
                "/v1/account/mfa/totp/confirm",
                Some(json!({ "code": totp::code_at(&secret, totp::step_of(self.now())) })),
                &jar,
            )
            .await;
        assert_eq!(c.status, 200, "{}", c.body);
        let codes = c.body["backup_codes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_str().unwrap().to_owned())
            .collect();
        let a = self
            .call("POST", "/v1/account/mfa/backup-codes/ack", None, &jar)
            .await;
        assert_eq!(a.status, 204);
        self.advance(STEP_MS);
        Enrolled { secret, codes }
    }

    fn totp(&self, secret: &[u8]) -> String {
        totp::code_at(secret, totp::step_of(self.now()))
    }

    async fn signin(&self, login: &str, password: &str, jar: &Jar) -> Reply {
        self.call(
            "POST",
            "/v1/signin",
            Some(json!({ "login": login, "password": password })),
            jar,
        )
        .await
    }

    async fn verify(
        &self,
        challenge: &str,
        method: &str,
        code: &str,
        trust: bool,
        jar: &Jar,
    ) -> Reply {
        self.call_ua(
            "POST",
            "/v1/signin/verify",
            Some(json!({
                "challenge_id": challenge,
                "method": method,
                "code": code,
                "trust_device": trust,
            })),
            jar,
            Some(ANDROID_UA),
        )
        .await
    }

    async fn sign_in_with_totp(&self, e: &Enrolled, trust: bool, jar: &mut Jar) -> Reply {
        let r = self.signin(PHONE, PASSWORD, jar).await;
        assert_eq!(r.status, 200, "{}", r.body);
        let v = self
            .verify(&r.challenge(), "totp", &self.totp(&e.secret), trust, jar)
            .await;
        assert_eq!(v.status, 200, "{}", v.body);
        jar.absorb(&v);
        self.advance(STEP_MS);
        v
    }

    async fn session(&self, jar: &Jar) -> Reply {
        self.call("GET", "/v1/session", None, jar).await
    }

    async fn owner(&self) -> arena_store::control::Owner {
        self.db.owner_by_phone(PHONE).await.unwrap().unwrap()
    }

    fn dummies(&self) -> u64 {
        self.st.dummy_verifies.load(Ordering::SeqCst)
    }

    async fn set_sms_fallback(&self, jar: &Jar, enabled: bool) {
        let r = self
            .call(
                "PUT",
                "/v1/account/security/sms-fallback",
                Some(json!({ "enabled": enabled })),
                jar,
            )
            .await;
        assert_eq!(r.status, 204, "{}", r.body);
    }
}

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

#[tokio::test]
async fn an_unknown_login_runs_the_dummy_hash_and_fails_like_a_wrong_password() {
    let h = Harness::new().await;
    h.enrol().await;
    let before = h.dummies();
    let unknown = h.signin("+919999999999", PASSWORD, &Jar::default()).await;
    unknown.assert_signin_failed();
    assert_eq!(h.dummies(), before + 1);
    let unknown_email = h
        .signin("nobody@example.in", PASSWORD, &Jar::default())
        .await;
    unknown_email.assert_signin_failed();
    assert_eq!(h.dummies(), before + 2);

    let wrong = h.signin(PHONE, "Not-The-Password-1", &Jar::default()).await;
    wrong.assert_signin_failed();
    assert_eq!(h.dummies(), before + 2);
    assert_eq!(wrong.body, unknown.body);
    assert_eq!(h.owner().await.failed_signins, 1);
    assert!(wrong.set_cookies().is_empty());

    let bad = h.signin("12345", PASSWORD, &Jar::default()).await;
    assert_eq!((bad.status, bad.code()), (422, "VALIDATION_FAILED"));
    assert_eq!(bad.body["field"], "login");
}

#[tokio::test]
async fn an_owner_without_a_password_fails_after_the_dummy_hash() {
    let h = Harness::new().await;
    h.sign_up().await;
    let before = h.dummies();
    h.signin(PHONE, PASSWORD, &Jar::default())
        .await
        .assert_signin_failed();
    assert_eq!(h.dummies(), before + 1);
    assert_eq!(h.owner().await.failed_signins, 1);
}

#[tokio::test]
async fn a_pending_owner_with_a_password_gets_a_session_for_account_setup() {
    let h = Harness::new().await;
    let jar = h.sign_up().await;
    let p = h
        .call(
            "POST",
            "/v1/account/password",
            Some(json!({ "password": PASSWORD })),
            &jar,
        )
        .await;
    assert_eq!(p.status, 204);
    let r = h.signin(PHONE, PASSWORD, &Jar::default()).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body, json!({"next": "account_setup"}));
    let mut fresh = Jar::default();
    fresh.absorb(&r);
    let s = h.session(&fresh).await;
    assert_eq!(s.status, 200);
    assert_eq!(s.body["owner"]["mfa_state"], "pending");
    let sec = h.call("GET", "/v1/account/devices", None, &fresh).await;
    assert_eq!((sec.status, sec.code()), (403, "SETUP_INCOMPLETE"));
}

#[tokio::test]
async fn an_enrolled_owner_on_an_untrusted_browser_gets_a_challenge_and_no_session() {
    let h = Harness::new().await;
    h.enrol().await;
    let r = h.signin(PHONE, PASSWORD, &Jar::default()).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.methods(), ["totp", "backup_code"]);
    assert!(r.body.get("next").is_none());
    assert!(r.set_cookies().is_empty());
    let c =
        h.db.signin_challenge(&r.challenge())
            .await
            .unwrap()
            .unwrap();
    assert_eq!(c.expires_at, h.now() + 300_000);

    let by_email = h
        .signin("  Asha.RAO@example.in ", PASSWORD, &Jar::default())
        .await;
    assert_eq!(by_email.status, 200, "{}", by_email.body);
    assert_eq!(by_email.methods(), ["totp", "backup_code"]);
    let spaced = h.signin("98765 43210", PASSWORD, &Jar::default()).await;
    assert_eq!(spaced.status, 200, "{}", spaced.body);
}

#[tokio::test]
async fn totp_with_trust_sets_a_device_and_the_next_sign_in_skips_the_factor() {
    let h = Harness::new().await;
    let e = h.enrol().await;
    let mut jar = Jar::default();
    let v = h.sign_in_with_totp(&e, true, &mut jar).await;
    assert_eq!(v.body, json!({"next": "done"}));
    let trusted = v.set_cookie("arena_trusted").unwrap();
    for attr in [
        "Path=/v1/signin;",
        "Max-Age=2592000;",
        "HttpOnly",
        "Secure",
        "SameSite=Lax",
    ] {
        assert!(trusted.contains(attr), "{trusted}");
    }
    assert!(v.set_cookie("arena_access").is_some() && v.set_cookie("arena_refresh").is_some());
    let o = h.owner().await;
    let devices = h.db.owner_devices(&o.id, h.now()).await.unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].label, "Chrome on Android");
    assert!(!format!("{devices:?}").contains(jar.trusted.as_deref().unwrap()));

    let list = h.call("GET", "/v1/account/devices", None, &jar).await;
    assert_eq!(list.status, 200, "{}", list.body);
    assert_eq!(list.body[0]["label"], "Chrome on Android");
    assert_eq!(list.body[0]["current"], true);

    let again = h
        .signin(
            PHONE,
            PASSWORD,
            &Jar {
                trusted: jar.trusted.clone(),
                ..Jar::default()
            },
        )
        .await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(again.body, json!({"next": "done"}));
    assert!(again.set_cookie("arena_access").is_some());
    let seen = h.db.owner_devices(&o.id, h.now()).await.unwrap();
    assert_eq!(seen[0].last_seen_at, h.now());

    let other = h.signin(PHONE, PASSWORD, &Jar::default()).await;
    assert!(other.body.get("challenge_id").is_some());
    let mut second = Jar::default();
    second.absorb(&again);
    let from_other = h.call("GET", "/v1/account/devices", None, &second).await;
    assert_eq!(from_other.body[0]["current"], true);
}

#[tokio::test]
async fn an_expired_or_revoked_trusted_cookie_asks_for_the_factor_again() {
    let h = Harness::new().await;
    let e = h.enrol().await;
    let mut jar = Jar::default();
    h.sign_in_with_totp(&e, true, &mut jar).await;
    let trusted = Jar {
        trusted: jar.trusted.clone(),
        ..Jar::default()
    };

    let id =
        h.db.owner_devices(&h.owner().await.id, h.now())
            .await
            .unwrap()[0]
            .id
            .clone();
    let del = h
        .call("DELETE", &format!("/v1/account/devices/{id}"), None, &jar)
        .await;
    assert_eq!(del.status, 204);
    assert_eq!(h.session(&jar).await.status, 401);
    let mut other = Jar::default();
    h.sign_in_with_totp(&e, false, &mut other).await;
    let again = h
        .call("DELETE", &format!("/v1/account/devices/{id}"), None, &other)
        .await;
    assert_eq!((again.status, again.code()), (404, "NOT_FOUND"));
    let r = h.signin(PHONE, PASSWORD, &trusted).await;
    assert!(r.body.get("challenge_id").is_some(), "{}", r.body);

    let mut jar2 = Jar::default();
    h.sign_in_with_totp(&e, true, &mut jar2).await;
    h.advance(31 * DAY_MS);
    let r = h
        .signin(
            PHONE,
            PASSWORD,
            &Jar {
                trusted: jar2.trusted.clone(),
                ..Jar::default()
            },
        )
        .await;
    assert!(r.body.get("challenge_id").is_some(), "{}", r.body);
}

#[tokio::test]
async fn a_backup_code_signs_in_once_and_counts_down() {
    let h = Harness::new().await;
    let e = h.enrol().await;
    let r = h.signin(PHONE, PASSWORD, &Jar::default()).await;
    let typed = e.codes[3].to_lowercase().replace('-', " ");
    let v = h
        .verify(
            &r.challenge(),
            "backup_code",
            &typed,
            false,
            &Jar::default(),
        )
        .await;
    assert_eq!(v.status, 200, "{}", v.body);
    assert_eq!(v.body, json!({"next": "done", "backup_codes_remaining": 9}));
    assert!(v.set_cookie("arena_trusted").is_none());
    let mut jar = Jar::default();
    jar.absorb(&v);
    assert_eq!(h.session(&jar).await.status, 200);

    let r = h.signin(PHONE, PASSWORD, &Jar::default()).await;
    h.verify(
        &r.challenge(),
        "backup_code",
        &e.codes[3],
        false,
        &Jar::default(),
    )
    .await
    .assert_signin_failed();
    assert_eq!(h.owner().await.failed_signins, 1);
    let v = h
        .verify(
            &r.challenge(),
            "backup_code",
            &e.codes[4],
            false,
            &Jar::default(),
        )
        .await;
    assert_eq!(v.body["backup_codes_remaining"], 8);
    assert_eq!(h.owner().await.failed_signins, 0);
}

#[tokio::test]
async fn a_totp_step_is_never_accepted_twice_and_a_used_challenge_is_dead() {
    let h = Harness::new().await;
    let e = h.enrol().await;
    let r1 = h.signin(PHONE, PASSWORD, &Jar::default()).await;
    let code = h.totp(&e.secret);
    let ok = h
        .verify(&r1.challenge(), "totp", &code, false, &Jar::default())
        .await;
    assert_eq!(ok.status, 200);
    let r2 = h.signin(PHONE, PASSWORD, &Jar::default()).await;
    h.verify(&r2.challenge(), "totp", &code, false, &Jar::default())
        .await
        .assert_signin_failed();
    h.verify(&r1.challenge(), "totp", &code, false, &Jar::default())
        .await
        .assert_signin_failed();
    h.advance(STEP_MS);
    let prev = totp::code_at(&e.secret, totp::step_of(h.now()) + 1);
    let ok = h
        .verify(&r2.challenge(), "totp", &prev, false, &Jar::default())
        .await;
    assert_eq!(ok.status, 200, "{}", ok.body);
}

#[tokio::test]
async fn sms_is_offered_only_when_the_owner_turned_it_on() {
    let h = Harness::new().await;
    let e = h.enrol().await;
    let r = h.signin(PHONE, PASSWORD, &Jar::default()).await;
    assert!(!r.methods().contains(&"sms".to_owned()));
    let sent_before = h.st.sms.fake().unwrap().sent().len();
    let s = h
        .call(
            "POST",
            "/v1/signin/sms",
            Some(json!({"challenge_id": r.challenge()})),
            &Jar::default(),
        )
        .await;
    assert_eq!((s.status, s.code()), (403, "FORBIDDEN"));
    let v = h
        .verify(&r.challenge(), "sms", "123456", false, &Jar::default())
        .await;
    assert_eq!((v.status, v.code()), (403, "FORBIDDEN"));
    assert_eq!(h.st.sms.fake().unwrap().sent().len(), sent_before);

    let mut jar = Jar::default();
    h.sign_in_with_totp(&e, false, &mut jar).await;
    let s = h.session(&jar).await;
    assert_eq!(s.body["owner"]["sms_fallback"], false);
    h.set_sms_fallback(&jar, true).await;
    assert_eq!(h.session(&jar).await.body["owner"]["sms_fallback"], true);

    let r = h.signin(PHONE, PASSWORD, &Jar::default()).await;
    assert_eq!(r.methods(), ["totp", "backup_code", "sms"]);
    let id = r.challenge();
    let s = h
        .call(
            "POST",
            "/v1/signin/sms",
            Some(json!({"challenge_id": id})),
            &Jar::default(),
        )
        .await;
    assert_eq!(s.status, 202, "{}", s.body);
    assert_eq!(s.body, json!({"resend_after_s": 30}));
    let first = h.last_sms_code();
    let sent = h.st.sms.fake().unwrap().sent();
    assert_eq!(sent.last().unwrap().to, PHONE);
    let c = h.db.signin_challenge(&id).await.unwrap().unwrap();
    assert!(!c.sms_code_hash.as_deref().unwrap().contains(&first));

    let early = h
        .call(
            "POST",
            "/v1/signin/sms",
            Some(json!({"challenge_id": id})),
            &Jar::default(),
        )
        .await;
    assert_eq!((early.status, early.code()), (429, "RATE_LIMITED"));
    assert!(early.retry_after().unwrap() <= 30);
    h.advance(31_000);
    let resent = h
        .call(
            "POST",
            "/v1/signin/sms",
            Some(json!({"challenge_id": id})),
            &Jar::default(),
        )
        .await;
    assert_eq!(resent.status, 202);
    let mut second = h.last_sms_code();
    while second == first {
        h.advance(31_000);
        let again = h
            .call(
                "POST",
                "/v1/signin/sms",
                Some(json!({"challenge_id": id})),
                &Jar::default(),
            )
            .await;
        assert_eq!(again.status, 202, "{}", again.body);
        second = h.last_sms_code();
    }
    h.verify(&id, "sms", &first, false, &Jar::default())
        .await
        .assert_signin_failed();
    let ok = h.verify(&id, "sms", &second, false, &Jar::default()).await;
    assert_eq!(ok.status, 200, "{}", ok.body);
    assert_eq!(ok.body, json!({"next": "done"}));
}

#[tokio::test]
async fn five_mixed_failures_pause_sign_in_for_fifteen_minutes() {
    let h = Harness::new().await;
    let e = h.enrol().await;
    for _ in 0..2 {
        h.signin(PHONE, "Wrong-Password-99", &Jar::default())
            .await
            .assert_signin_failed();
    }
    let r = h.signin(PHONE, PASSWORD, &Jar::default()).await;
    let id = r.challenge();
    h.verify(&id, "totp", "000000", false, &Jar::default())
        .await
        .assert_signin_failed();
    h.verify(&id, "backup_code", "AAAAA-AAAAA", false, &Jar::default())
        .await
        .assert_signin_failed();
    assert_eq!(h.owner().await.failed_signins, 4);
    h.signin(EMAIL, "Wrong-Password-99", &Jar::default())
        .await
        .assert_signin_failed();
    let o = h.owner().await;
    assert_eq!(
        (o.failed_signins, o.locked_until),
        (0, Some(h.now() + 15 * MINUTE_MS))
    );

    let locked = h.signin(PHONE, PASSWORD, &Jar::default()).await;
    assert_eq!(
        (locked.status, locked.code()),
        (429, "AUTH_LOCKED"),
        "{}",
        locked.body
    );
    assert_eq!(locked.retry_after(), Some(900));
    assert!(locked.set_cookies().is_empty());
    let v = h
        .verify(&id, "totp", &h.totp(&e.secret), false, &Jar::default())
        .await;
    assert_eq!((v.status, v.code()), (429, "AUTH_LOCKED"));
    h.signin(PHONE, "Wrong-Password-99", &Jar::default())
        .await
        .assert_signin_failed();
    assert_eq!(h.owner().await.locked_until, Some(h.now() + 15 * MINUTE_MS));

    h.advance(10 * MINUTE_MS);
    let later = h.signin(PHONE, PASSWORD, &Jar::default()).await;
    assert_eq!(later.retry_after(), Some(300));

    h.advance(5 * MINUTE_MS);
    let mut jar = Jar::default();
    h.sign_in_with_totp(&e, false, &mut jar).await;
    let o = h.owner().await;
    assert_eq!((o.failed_signins, o.locked_until), (0, None));
}

#[tokio::test]
async fn five_wrong_codes_void_the_challenge() {
    let h = Harness::new().await;
    let e = h.enrol().await;
    let r = h.signin(PHONE, PASSWORD, &Jar::default()).await;
    let id = r.challenge();
    for _ in 0..5 {
        h.verify(&id, "totp", "000000", false, &Jar::default())
            .await
            .assert_signin_failed();
    }
    h.verify(&id, "totp", &h.totp(&e.secret), false, &Jar::default())
        .await
        .assert_signin_failed();
    let c = h.db.signin_challenge(&id).await.unwrap().unwrap();
    assert_eq!((c.attempts, c.consumed_at), (5, None));
}

#[tokio::test]
async fn an_expired_challenge_fails() {
    let h = Harness::new().await;
    let e = h.enrol().await;
    let r = h.signin(PHONE, PASSWORD, &Jar::default()).await;
    h.advance(5 * MINUTE_MS + 1);
    h.verify(
        &r.challenge(),
        "totp",
        &h.totp(&e.secret),
        false,
        &Jar::default(),
    )
    .await
    .assert_signin_failed();
    assert_eq!(h.owner().await.failed_signins, 0);
}

#[tokio::test]
async fn sign_out_everywhere_revokes_every_session_and_trusted_device() {
    let h = Harness::new().await;
    let e = h.enrol().await;
    let mut a = Jar::default();
    h.sign_in_with_totp(&e, true, &mut a).await;
    let mut b = Jar::default();
    h.sign_in_with_totp(&e, false, &mut b).await;
    assert_eq!(h.session(&a).await.status, 200);
    assert_eq!(h.session(&b).await.status, 200);

    let out = h
        .call("POST", "/v1/account/sign-out-everywhere", None, &b)
        .await;
    assert_eq!(out.status, 204, "{}", out.body);
    for name in ["arena_access", "arena_refresh", "arena_trusted"] {
        let c = out
            .set_cookie(name)
            .unwrap_or_else(|| panic!("{name} not cleared"));
        assert!(c.contains("Max-Age=0"), "{c}");
    }
    assert!(
        out.set_cookie("arena_trusted")
            .unwrap()
            .contains("Path=/v1/signin;")
    );
    assert_eq!(h.session(&a).await.code(), "AUTH_FAILED");
    assert_eq!(h.session(&b).await.status, 401);
    let refresh = h
        .call(
            "POST",
            "/v1/session/refresh",
            None,
            &Jar {
                refresh: a.refresh.clone(),
                ..Jar::default()
            },
        )
        .await;
    assert_eq!(refresh.status, 401);
    let r = h
        .signin(
            PHONE,
            PASSWORD,
            &Jar {
                trusted: a.trusted.clone(),
                ..Jar::default()
            },
        )
        .await;
    assert!(r.body.get("challenge_id").is_some(), "{}", r.body);
}

#[tokio::test]
async fn refresh_rotation_and_reuse_revocation_hold_after_a_sign_in() {
    let h = Harness::new().await;
    let e = h.enrol().await;
    let mut jar = Jar::default();
    h.sign_in_with_totp(&e, true, &mut jar).await;
    let old = jar.refresh.clone().unwrap();
    let r = h
        .call(
            "POST",
            "/v1/session/refresh",
            None,
            &Jar {
                refresh: Some(old.clone()),
                ..Jar::default()
            },
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let mut rotated = Jar::default();
    rotated.absorb(&r);
    assert_ne!(rotated.refresh.as_deref(), Some(old.as_str()));
    assert_eq!(h.session(&rotated).await.status, 200);
    assert_eq!(h.session(&jar).await.status, 401);
    let devices = h.call("GET", "/v1/account/devices", None, &rotated).await;
    assert_eq!(devices.body[0]["current"], true);

    h.advance(11_000);
    let reuse = h
        .call(
            "POST",
            "/v1/session/refresh",
            None,
            &Jar {
                refresh: Some(old),
                ..Jar::default()
            },
        )
        .await;
    assert_eq!((reuse.status, reuse.code()), (401, "AUTH_FAILED"));
    assert_eq!(h.session(&rotated).await.status, 401);
}

#[tokio::test]
async fn account_security_needs_an_enrolled_session_and_a_sane_label() {
    let h = Harness::new().await;
    for (method, path) in [
        ("PUT", "/v1/account/security/sms-fallback"),
        ("GET", "/v1/account/devices"),
        ("DELETE", "/v1/account/devices/x"),
        ("POST", "/v1/account/sign-out-everywhere"),
    ] {
        let r = h
            .call(
                method,
                path,
                Some(json!({"enabled": true})),
                &Jar::default(),
            )
            .await;
        assert_eq!(
            (r.status, r.code()),
            (401, "AUTH_FAILED"),
            "{method} {path}"
        );
    }
    let pending = h.sign_up().await;
    let r = h
        .call(
            "PUT",
            "/v1/account/security/sms-fallback",
            Some(json!({"enabled": true})),
            &pending,
        )
        .await;
    assert_eq!((r.status, r.code()), (403, "SETUP_INCOMPLETE"));
    assert!(!h.owner().await.sms_fallback);

    let h = Harness::new().await;
    let e = h.enrol().await;
    let r = h.signin(PHONE, PASSWORD, &Jar::default()).await;
    let long = h
        .call(
            "POST",
            "/v1/signin/verify",
            Some(json!({
                "challenge_id": r.challenge(),
                "method": "totp",
                "code": h.totp(&e.secret),
                "trust_device": true,
                "device_label": "x".repeat(61),
            })),
            &Jar::default(),
        )
        .await;
    assert_eq!((long.status, long.code()), (422, "VALIDATION_FAILED"));
    assert_eq!(long.body["field"], "device_label");
    let named = h
        .call(
            "POST",
            "/v1/signin/verify",
            Some(json!({
                "challenge_id": r.challenge(),
                "method": "totp",
                "code": h.totp(&e.secret),
                "trust_device": true,
                "device_label": "  Front desk PC  ",
            })),
            &Jar::default(),
        )
        .await;
    assert_eq!(named.status, 200, "{}", named.body);
    let devices =
        h.db.owner_devices(&h.owner().await.id, h.now())
            .await
            .unwrap();
    assert_eq!(devices[0].label, "Front desk PC");
    assert_eq!(h.owner().await.mfa_state, MfaState::Enrolled);
}

#[tokio::test]
async fn cors_preflight_allows_put_and_delete() {
    let h = Harness::new().await;
    let req = Request::builder()
        .method("OPTIONS")
        .uri("/v1/account/devices/x")
        .header("origin", ORIGIN)
        .header("access-control-request-method", "DELETE")
        .body(Body::empty())
        .unwrap();
    let res = h.app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status().as_u16(), 204);
    let methods = res.headers()[header::ACCESS_CONTROL_ALLOW_METHODS]
        .to_str()
        .unwrap();
    assert!(
        methods.contains("PUT") && methods.contains("DELETE"),
        "{methods}"
    );
}

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
async fn logs_never_carry_logins_passwords_codes_or_tokens() {
    let logs = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(logs.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("the only global subscriber");

    let h = Harness::new().await;
    let e = h.enrol().await;
    h.signin("+919999999999", PASSWORD, &Jar::default()).await;
    h.signin(
        "someone.else@example.in",
        "Other-Password-77",
        &Jar::default(),
    )
    .await;
    h.signin(EMAIL, "Wrong-Password-99", &Jar::default()).await;
    let mut jar = Jar::default();
    h.sign_in_with_totp(&e, true, &mut jar).await;
    let totp_code = totp::code_at(&e.secret, totp::step_of(h.now()) - 1);
    h.set_sms_fallback(&jar, true).await;
    let r = h.signin(PHONE, PASSWORD, &Jar::default()).await;
    h.verify(
        &r.challenge(),
        "backup_code",
        "ZZZZZ-ZZZZZ",
        false,
        &Jar::default(),
    )
    .await;
    h.call(
        "POST",
        "/v1/signin/sms",
        Some(json!({"challenge_id": r.challenge()})),
        &Jar::default(),
    )
    .await;
    let sms_code = h.last_sms_code();
    let v = h
        .verify(&r.challenge(), "sms", &sms_code, false, &Jar::default())
        .await;
    assert_eq!(v.status, 200, "{}", v.body);
    let r = h.signin(PHONE, PASSWORD, &Jar::default()).await;
    let b = h
        .verify(
            &r.challenge(),
            "backup_code",
            &e.codes[0],
            true,
            &Jar::default(),
        )
        .await;
    assert_eq!(b.status, 200, "{}", b.body);
    let mut jar2 = Jar::default();
    jar2.absorb(&b);
    h.call("POST", "/v1/account/sign-out-everywhere", None, &jar2)
        .await;

    let text: String = String::from_utf8(logs.0.lock().unwrap().clone())
        .unwrap()
        .lines()
        .map(|l| {
            let mut v: Value = serde_json::from_str(l).unwrap();
            v.as_object_mut().unwrap().remove("timestamp");
            format!("{v}\n")
        })
        .collect();
    assert!(
        text.contains("owner signed in with a second factor"),
        "{text}"
    );
    assert!(text.contains("sign-in refused: unknown login"), "{text}");
    assert!(text.contains("login_tag"), "{text}");
    let mut secrets = vec![
        PASSWORD.to_owned(),
        "Other-Password-77".to_owned(),
        "Wrong-Password-99".to_owned(),
        PHONE.to_owned(),
        "9876543210".to_owned(),
        "9999999999".to_owned(),
        EMAIL.to_owned(),
        "someone.else".to_owned(),
        "asha.rao".to_owned(),
        totp_code,
        sms_code,
        "ZZZZZ".to_owned(),
        e.codes[0].clone(),
        e.codes[0].replace('-', ""),
    ];
    for j in [&jar, &jar2] {
        secrets.extend(j.access.clone());
        secrets.extend(j.refresh.clone());
        secrets.extend(j.trusted.clone());
    }
    for s in &secrets {
        assert!(!text.contains(s.as_str()), "logs contain {s:?}:\n{text}");
    }
}

#[tokio::test]
async fn sign_in_totp_shares_the_per_owner_totp_failure_limit() {
    let h = Harness::new().await;
    let e = h.enrol().await;
    let mut jar = Jar::default();
    h.sign_in_with_totp(&e, false, &mut jar).await;
    for _ in 0..4 {
        let r = h
            .call(
                "POST",
                "/v1/account/mfa/backup-codes/regenerate",
                Some(json!({ "code": "000000" })),
                &jar,
            )
            .await;
        assert_eq!((r.status, r.code()), (400, "TOTP_INVALID"), "{}", r.body);
    }
    let r = h.signin(PHONE, PASSWORD, &Jar::default()).await;
    let id = r.challenge();
    h.verify(&id, "totp", "000000", false, &Jar::default())
        .await
        .assert_signin_failed();
    let limited = h
        .verify(&id, "totp", &h.totp(&e.secret), false, &Jar::default())
        .await;
    assert_eq!(
        (limited.status, limited.code()),
        (429, "AUTH_LOCKED"),
        "{}",
        limited.body
    );
    assert!(limited.retry_after().unwrap() > 0);
    assert!(limited.set_cookies().is_empty());
    assert_eq!(h.owner().await.failed_signins, 1);
    let regen = h
        .call(
            "POST",
            "/v1/account/mfa/backup-codes/regenerate",
            Some(json!({ "code": h.totp(&e.secret) })),
            &jar,
        )
        .await;
    assert_eq!((regen.status, regen.code()), (429, "RATE_LIMITED"));
    let backup = h
        .verify(&id, "backup_code", &e.codes[0], false, &Jar::default())
        .await;
    assert_eq!(backup.status, 200, "{}", backup.body);
}

async fn send_sms(h: &Harness, id: &str) -> Reply {
    h.call(
        "POST",
        "/v1/signin/sms",
        Some(json!({ "challenge_id": id })),
        &Jar::default(),
    )
    .await
}

async fn sms_owner(h: &Harness) -> Enrolled {
    let e = h.enrol().await;
    let mut jar = Jar::default();
    h.sign_in_with_totp(&e, false, &mut jar).await;
    h.set_sms_fallback(&jar, true).await;
    e
}

#[tokio::test]
async fn a_failed_sms_send_restores_the_challenge_and_releases_the_slot() {
    let h = Harness::new().await;
    sms_owner(&h).await;
    let id = h.signin(PHONE, PASSWORD, &Jar::default()).await.challenge();
    let before = h.db.signin_challenge(&id).await.unwrap().unwrap();
    let key = format!("otp_send:{PHONE}");
    let counted = |b: Vec<(i64, u64)>| b.iter().map(|(_, n)| n).sum::<u64>();
    let slots = counted(h.db.rate_buckets(&key).await.unwrap());
    h.st.sms.fake().unwrap().set_failing(true);
    let r = send_sms(&h, &id).await;
    assert_eq!((r.status, r.code()), (503, "INTERNAL"), "{}", r.body);
    let after = h.db.signin_challenge(&id).await.unwrap().unwrap();
    assert_eq!(
        (
            after.sms_sent_at,
            after.sms_code_hash.as_deref(),
            after.expires_at
        ),
        (
            before.sms_sent_at,
            before.sms_code_hash.as_deref(),
            before.expires_at
        )
    );
    assert_eq!(counted(h.db.rate_buckets(&key).await.unwrap()), slots);
    h.st.sms.fake().unwrap().set_failing(false);
    let ok = send_sms(&h, &id).await;
    assert_eq!(ok.status, 202, "{}", ok.body);
}

#[tokio::test]
async fn wrong_sms_codes_count_and_an_expired_sms_code_fails() {
    let h = Harness::new().await;
    sms_owner(&h).await;
    let id = h.signin(PHONE, PASSWORD, &Jar::default()).await.challenge();
    assert_eq!(send_sms(&h, &id).await.status, 202);
    let code = h.last_sms_code();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for n in 1..=2 {
        h.verify(&id, "sms", wrong, false, &Jar::default())
            .await
            .assert_signin_failed();
        let c = h.db.signin_challenge(&id).await.unwrap().unwrap();
        assert_eq!(c.attempts, n);
        assert_eq!(h.owner().await.failed_signins, n);
    }
    h.advance(5 * MINUTE_MS);
    h.verify(&id, "sms", &code, false, &Jar::default())
        .await
        .assert_signin_failed();
    assert_eq!(h.owner().await.failed_signins, 2);
}

#[tokio::test]
async fn removing_a_trusted_device_ends_the_sessions_signed_in_through_it() {
    let h = Harness::new().await;
    let e = h.enrol().await;
    let mut trusted = Jar::default();
    h.sign_in_with_totp(&e, true, &mut trusted).await;
    let mut other = Jar::default();
    h.sign_in_with_totp(&e, false, &mut other).await;
    let id =
        h.db.owner_devices(&h.owner().await.id, h.now())
            .await
            .unwrap()[0]
            .id
            .clone();
    let del = h
        .call("DELETE", &format!("/v1/account/devices/{id}"), None, &other)
        .await;
    assert_eq!(del.status, 204);
    assert_eq!(h.session(&trusted).await.status, 401);
    let refresh = h
        .call(
            "POST",
            "/v1/session/refresh",
            None,
            &Jar {
                refresh: trusted.refresh.clone(),
                ..Jar::default()
            },
        )
        .await;
    assert_eq!(refresh.status, 401);
    assert_eq!(h.session(&other).await.status, 200);
}

#[tokio::test]
async fn a_challenge_from_before_sign_out_everywhere_is_dead() {
    let h = Harness::new().await;
    let e = h.enrol().await;
    let mut jar = Jar::default();
    h.sign_in_with_totp(&e, false, &mut jar).await;
    let id = h.signin(PHONE, PASSWORD, &Jar::default()).await.challenge();
    let out = h
        .call("POST", "/v1/account/sign-out-everywhere", None, &jar)
        .await;
    assert_eq!(out.status, 204);
    h.verify(&id, "totp", &h.totp(&e.secret), false, &Jar::default())
        .await
        .assert_signin_failed();
    assert_eq!(h.owner().await.failed_signins, 0);
}

#[tokio::test]
async fn a_new_challenge_retires_the_previous_one() {
    let h = Harness::new().await;
    let e = h.enrol().await;
    let first = h.signin(PHONE, PASSWORD, &Jar::default()).await.challenge();
    let second = h.signin(PHONE, PASSWORD, &Jar::default()).await.challenge();
    h.verify(&first, "totp", &h.totp(&e.secret), false, &Jar::default())
        .await
        .assert_signin_failed();
    let ok = h
        .verify(&second, "totp", &h.totp(&e.secret), false, &Jar::default())
        .await;
    assert_eq!(ok.status, 200, "{}", ok.body);
    assert_eq!(h.owner().await.failed_signins, 0);
}

#[tokio::test]
async fn a_double_submitted_code_signs_in_once_and_counts_no_failure() {
    let h = Harness::new().await;
    let e = h.enrol().await;
    let id = h.signin(PHONE, PASSWORD, &Jar::default()).await.challenge();
    let code = h.totp(&e.secret);
    let empty = Jar::default();
    let (a, b) = tokio::join!(
        h.verify(&id, "totp", &code, false, &empty),
        h.verify(&id, "totp", &code, false, &empty)
    );
    let mut statuses = [a.status, b.status];
    statuses.sort_unstable();
    assert_eq!(statuses, [200, 401], "{} {}", a.body, b.body);
    let loser = if a.status == 401 { &a } else { &b };
    loser.assert_signin_failed();
    assert!(loser.set_cookies().is_empty());
    assert_eq!(h.owner().await.failed_signins, 0);
    let mut jar = Jar::default();
    jar.absorb(if a.status == 200 { &a } else { &b });
    assert_eq!(h.session(&jar).await.status, 200);
    let sessions: i64 = rusqlite::Connection::open(h._dir.path().join("gz_control.db"))
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM owner_sessions WHERE owner_id = ? AND created_at = ?",
            rusqlite::params![h.owner().await.id, h.now()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sessions, 1);
}

#[tokio::test]
async fn email_login_needs_exactly_one_owner_with_that_email() {
    let h = Harness::new().await;
    h.enrol().await;
    assert_eq!(h.signin(EMAIL, PASSWORD, &Jar::default()).await.status, 200);
    let s = h
        .call(
            "POST",
            "/v1/signup/start",
            Some(json!({
                "name": "Ravi",
                "phone": "+919811111111",
                "email": "ASHA.RAO@example.in",
                "cafe_name": "Other",
                "city": "Pune",
            })),
            &Jar::default(),
        )
        .await;
    let code = h.last_sms_code();
    let v = h
        .call(
            "POST",
            "/v1/signup/verify",
            Some(json!({"challenge_id": s.challenge(), "code": code})),
            &Jar::default(),
        )
        .await;
    let mut jar = Jar::default();
    jar.absorb(&v);
    assert_eq!(h.signin(EMAIL, PASSWORD, &Jar::default()).await.status, 200);
    let p = h
        .call(
            "POST",
            "/v1/account/password",
            Some(json!({ "password": "Another-Password-55" })),
            &jar,
        )
        .await;
    assert_eq!(p.status, 204);
    let before = h.dummies();
    let failures = h.owner().await.failed_signins;
    h.signin(EMAIL, PASSWORD, &Jar::default())
        .await
        .assert_signin_failed();
    assert_eq!(h.dummies(), before + 1);
    assert_eq!(h.owner().await.failed_signins, failures);
    assert_eq!(h.signin(PHONE, PASSWORD, &Jar::default()).await.status, 200);
}

#[tokio::test]
async fn trusting_again_refreshes_the_same_device() {
    let h = Harness::new().await;
    let e = h.enrol().await;
    let mut jar = Jar::default();
    h.sign_in_with_totp(&e, true, &mut jar).await;
    let old = jar.trusted.clone().unwrap();
    let owner_id = h.owner().await.id;
    let first = h.db.owner_devices(&owner_id, h.now()).await.unwrap();
    h.advance(DAY_MS);
    let r = h
        .signin(
            PHONE,
            PASSWORD,
            &Jar {
                trusted: Some("stale".into()),
                ..Jar::default()
            },
        )
        .await;
    let v = h
        .verify(
            &r.challenge(),
            "totp",
            &h.totp(&e.secret),
            true,
            &Jar {
                trusted: Some(old.clone()),
                ..Jar::default()
            },
        )
        .await;
    assert_eq!(v.status, 200, "{}", v.body);
    let mut next = Jar::default();
    next.absorb(&v);
    let new = next.trusted.clone().unwrap();
    assert_ne!(new, old);
    let devices = h.db.owner_devices(&owner_id, h.now()).await.unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].id, first[0].id);
    assert_eq!(devices[0].expires_at, h.now() + 30 * DAY_MS);
    let stale = h
        .signin(
            PHONE,
            PASSWORD,
            &Jar {
                trusted: Some(old),
                ..Jar::default()
            },
        )
        .await;
    assert!(stale.body.get("challenge_id").is_some(), "{}", stale.body);
    let fresh = h
        .signin(
            PHONE,
            PASSWORD,
            &Jar {
                trusted: Some(new),
                ..Jar::default()
            },
        )
        .await;
    assert_eq!(fresh.body, json!({"next": "done"}));
}

#[tokio::test]
async fn oversized_fields_are_refused_before_any_lookup() {
    let h = Harness::new().await;
    let e = h.enrol().await;
    let before = h.dummies();
    for (body, field) in [
        (
            json!({"login": "9".repeat(255), "password": PASSWORD}),
            "login",
        ),
        (
            json!({"login": PHONE, "password": "x".repeat(129)}),
            "password",
        ),
    ] {
        let r = h
            .call("POST", "/v1/signin", Some(body), &Jar::default())
            .await;
        assert_eq!(
            (r.status, r.code()),
            (422, "VALIDATION_FAILED"),
            "{}",
            r.body
        );
        assert_eq!(r.body["field"], field);
    }
    assert_eq!(h.dummies(), before);
    assert_eq!(h.owner().await.failed_signins, 0);
    let id = h.signin(PHONE, PASSWORD, &Jar::default()).await.challenge();
    let r = h
        .verify(&id, "backup_code", &"A".repeat(33), false, &Jar::default())
        .await;
    assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));
    assert_eq!(r.body["field"], "code");
    assert_eq!(
        h.db.signin_challenge(&id).await.unwrap().unwrap().attempts,
        0
    );
    let ok = h
        .verify(&id, "totp", &h.totp(&e.secret), false, &Jar::default())
        .await;
    assert_eq!(ok.status, 200);
}
