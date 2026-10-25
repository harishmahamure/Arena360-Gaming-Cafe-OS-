use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use arena_cloud::auth::AuthState;
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
use arena_cloud::pairing_code::redeem_pairing_code;
use arena_cloud::{Clock, app_with_auth, auth_state};
use arena_proto::{PAIRING_CODE_ALPHABET, Tier};
use arena_store::control::{ControlDb, LATEST_CONTROL_VERSION, NewBackupCode, RedeemInvalid};
use axum::body::{Body, to_bytes};
use axum::http::{Request, header};
use serde_json::{Value, json};
use tokio::sync::watch;
use tower::ServiceExt;

const ORIGIN: &str = "https://start.arena360.cloud";
const PEPPER: &str = "test-pepper-0123456789abcdef0123456789";
const T0: i64 = 1_800_000_000_000;
const MIN15: i64 = 15 * 60 * 1000;
const PHONE_A: &str = "+919876543210";
const PHONE_B: &str = "+919876543211";

struct Harness {
    app: axum::Router,
    db: Arc<ControlDb>,
    st: Arc<AuthState>,
    now: Arc<AtomicI64>,
    dir: tempfile::TempDir,
    _serial: Option<tokio::sync::RwLockReadGuard<'static, ()>>,
}

static LOG_CAPTURE: tokio::sync::RwLock<()> = tokio::sync::RwLock::const_new(());

struct Reply {
    status: u16,
    body: Value,
    headers: axum::http::HeaderMap,
}

impl Reply {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or("")
    }
}

fn cafe() -> Value {
    json!({
        "cafe_name": "Pixel Arena Gaming",
        "city": "Pune",
        "country": "IN",
        "currency": "INR",
        "timezone": "Asia/Kolkata",
        "gst_registered": false,
    })
}

impl Harness {
    async fn new() -> Self {
        let serial = LOG_CAPTURE.read().await;
        Self {
            _serial: Some(serial),
            ..Self::unlocked().await
        }
    }

    async fn unlocked() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = ControlDb::open_local(&dir.path().join("gz_control.db"))
            .await
            .unwrap();
        db.migrate(T0).await.unwrap();
        let db = Arc::new(db);
        let now = Arc::new(AtomicI64::new(T0));
        let tick = Arc::clone(&now);
        let clock: Clock = Arc::new(move || tick.load(Ordering::SeqCst));
        let cfg = AuthConfig {
            pepper: PEPPER.as_bytes().to_vec(),
            web_origins: vec![ORIGIN.to_owned()],
            sms: SmsProviderConfig::Fake,
            sms_global_per_min: 10_000,
            mfa_key: MfaKey::from_bytes([0xa5; 32]),
            licence: arena_cloud::licence::LicenceSigner::dev(),
            billing_enabled: false,
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
            dir,
            _serial: None,
        }
    }

    fn advance(&self, ms: i64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
        rusqlite::Connection::open(self.dir.path().join("gz_control.db"))
            .unwrap()
            .execute(
                "UPDATE owner_sessions SET access_expires_at = access_expires_at + ?1, \
                 refresh_expires_at = refresh_expires_at + ?1",
                [ms],
            )
            .unwrap();
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
            serde_json::from_slice(&bytes).unwrap()
        };
        Reply {
            status,
            body,
            headers,
        }
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

    async fn owner(&self, phone: &str, cafe_name: &str) -> String {
        let access = self.sign_up(phone, cafe_name).await;
        self.complete_setup(phone).await;
        access
    }

    async fn cafe_done(&self, access: &str) -> String {
        let r = self
            .call("PUT", "/v1/wizard/cafe", Some(cafe()), access)
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
        r.body["branch"]["id"].as_str().unwrap().to_owned()
    }

    async fn tier_done(&self, access: &str) -> String {
        let branch = self.cafe_done(access).await;
        let r = self
            .call(
                "PUT",
                "/v1/wizard/tier",
                Some(json!({"tier": "free"})),
                access,
            )
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
        branch
    }

    async fn issue(&self, access: &str) -> (String, i64) {
        let r = self
            .call(
                "POST",
                "/v1/wizard/install-hub/code",
                Some(json!({})),
                access,
            )
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
        (
            r.body["code"].as_str().unwrap().to_owned(),
            r.body["expires_at"].as_i64().unwrap(),
        )
    }

    fn rows(&self) -> Vec<Vec<String>> {
        let c = rusqlite::Connection::open(self.dir.path().join("gz_control.db")).unwrap();
        let mut st = c
            .prepare(
                "SELECT id, branch_id, code_hash, tier_at_issue, CAST(created_at AS TEXT), \
                 CAST(expires_at AS TEXT), COALESCE(CAST(used_at AS TEXT), ''), \
                 COALESCE(CAST(invalidated_at AS TEXT), '') FROM pairing_codes \
                 ORDER BY created_at, id",
            )
            .unwrap();
        st.query_map([], |r| (0..8).map(|i| r.get::<_, String>(i)).collect())
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn install_step(&self, branch: &str) -> String {
        rusqlite::Connection::open(self.dir.path().join("gz_control.db"))
            .unwrap()
            .query_row(
                "SELECT state FROM onboarding_progress WHERE branch_id = ? AND step = 'install_hub'",
                [branch],
                |r| r.get(0),
            )
            .unwrap()
    }
}

fn assert_shape(code: &str) {
    assert_eq!(code.len(), 8, "{code}");
    assert!(
        code.bytes().all(|c| PAIRING_CODE_ALPHABET.contains(&c)),
        "{code}"
    );
}

#[tokio::test]
async fn issuing_answers_an_eight_character_code_for_fifteen_minutes_stored_only_as_a_hash() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let branch = h.tier_done(&a).await;
    let r = h
        .call("POST", "/v1/wizard/install-hub/code", Some(json!({})), &a)
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let obj = r.body.as_object().unwrap();
    assert_eq!(
        obj.keys().map(String::as_str).collect::<Vec<_>>(),
        ["code", "expires_at", "server_now"]
    );
    let code = r.body["code"].as_str().unwrap();
    assert_shape(code);
    assert_eq!(r.body["expires_at"], T0 + MIN15);
    assert_eq!(r.body["server_now"], T0);
    assert_eq!(
        r.headers
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok()),
        Some("no-store")
    );

    let rows = h.rows();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row[1], branch);
    assert_eq!(row[2], h.st.pairing_code_hash(code));
    assert_eq!(row[2].len(), 64);
    assert_eq!(row[3], "free");
    assert_eq!(row[4], T0.to_string());
    assert_eq!(row[5], (T0 + MIN15).to_string());
    assert_eq!((row[6].as_str(), row[7].as_str()), ("", ""));
    for cell in row {
        assert!(!cell.contains(code), "{cell}");
        assert!(!cell.contains(&code.to_lowercase()), "{cell}");
    }
    let file = std::fs::read(h.dir.path().join("gz_control.db")).unwrap();
    assert!(
        !file.windows(code.len()).any(|w| w == code.as_bytes()),
        "the plain code is in the database file"
    );

    let lower = format!("{}-{}", &code[..4], &code[4..]).to_lowercase();
    assert_eq!(h.st.pairing_code_hash(&lower), row[2]);
    assert_eq!(h.st.pairing_code_hash(&format!(" {lower} ")), row[2]);
    assert_eq!(h.install_step(&branch), "pending");
    let w = h.call("GET", "/v1/wizard", None, &a).await;
    assert_eq!(w.body["current"], "install_hub");
}

#[tokio::test]
async fn regenerating_invalidates_the_previous_code() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    h.tier_done(&a).await;
    let (first, _) = h.issue(&a).await;
    h.advance(30_000);
    let (second, expires) = h.issue(&a).await;
    assert_ne!(first, second);
    assert_eq!(expires, T0 + 30_000 + MIN15);
    let rows = h.rows();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][7], (T0 + 30_000).to_string());
    assert_eq!(rows[1][7], "");
    assert_eq!(
        redeem_pairing_code(&h.st, &first).await.unwrap(),
        Err(RedeemInvalid)
    );
    let ok = redeem_pairing_code(&h.st, &second).await.unwrap().unwrap();
    assert_eq!(ok.tier, Tier::Free);
}

#[tokio::test]
async fn a_reload_shows_the_live_code_without_its_value_then_issues_a_new_one() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let branch = h.tier_done(&a).await;
    let r = h.call("GET", "/v1/wizard/install-hub", None, &a).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        r.body,
        json!({"active": null, "hub_paired": false, "server_now": T0})
    );

    let (first, expires) = h.issue(&a).await;
    h.advance(60_000);
    let r = h.call("GET", "/v1/wizard/install-hub", None, &a).await;
    assert_eq!(
        r.body,
        json!({"active": {"expires_at": expires}, "hub_paired": false, "server_now": T0 + 60_000})
    );
    assert!(!r.body.to_string().contains(&first));
    let r = h
        .call(
            "GET",
            &format!("/v1/wizard/install-hub?branch_id={branch}"),
            None,
            &a,
        )
        .await;
    assert_eq!(r.body["active"]["expires_at"], expires);

    let (second, second_expires) = h.issue(&a).await;
    assert_ne!(first, second);
    let r = h.call("GET", "/v1/wizard/install-hub", None, &a).await;
    assert_eq!(r.body["active"]["expires_at"], second_expires);
    assert_eq!(
        redeem_pairing_code(&h.st, &first).await.unwrap(),
        Err(RedeemInvalid)
    );
    let ok = redeem_pairing_code(&h.st, &second).await.unwrap().unwrap();
    assert_eq!(ok.branch_id, branch);
}

#[tokio::test]
async fn a_code_expires_after_fifteen_minutes() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    h.tier_done(&a).await;
    let (code, _) = h.issue(&a).await;
    h.advance(MIN15);
    let r = h.call("GET", "/v1/wizard/install-hub", None, &a).await;
    assert_eq!(r.body["active"], Value::Null);
    assert_eq!(
        redeem_pairing_code(&h.st, &code).await.unwrap(),
        Err(RedeemInvalid)
    );
    assert_eq!(h.rows()[0][6], "");
    let (fresh, expires) = h.issue(&a).await;
    assert_eq!(expires, T0 + 2 * MIN15);
    assert_eq!(h.rows()[0][7], (T0 + MIN15).to_string());
    h.advance(MIN15 - 1);
    assert!(redeem_pairing_code(&h.st, &fresh).await.unwrap().is_ok());
}

#[tokio::test]
async fn redeeming_answers_the_branch_tenant_and_tier_once() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let branch = h.tier_done(&a).await;
    let tenant = h.db.branch(&branch).await.unwrap().unwrap().tenant_id;
    let (code, _) = h.issue(&a).await;
    assert_eq!(
        redeem_pairing_code(&h.st, "ZZZZZZZZ").await.unwrap(),
        Err(RedeemInvalid)
    );
    let typed = format!("{} {}", &code[..4], &code[4..]).to_lowercase();
    let ok = redeem_pairing_code(&h.st, &typed).await.unwrap().unwrap();
    assert_eq!(
        (ok.branch_id.as_str(), ok.tenant_id.as_str(), ok.tier),
        (branch.as_str(), tenant.as_str(), Tier::Free)
    );
    assert_eq!(h.rows()[0][6], T0.to_string());
    assert_eq!(
        redeem_pairing_code(&h.st, &code).await.unwrap(),
        Err(RedeemInvalid)
    );
    let r = h.call("GET", "/v1/wizard/install-hub", None, &a).await;
    assert_eq!(r.body["active"], Value::Null);

    let (next, _) = h.issue(&a).await;
    assert_eq!(h.rows()[0][7], "", "a used code is not invalidated later");
    let (x, y) = tokio::join!(
        redeem_pairing_code(&h.st, &next),
        redeem_pairing_code(&h.st, &next)
    );
    let outcomes = [x.unwrap(), y.unwrap()];
    assert_eq!(outcomes.iter().filter(|o| o.is_ok()).count(), 1);
}

#[tokio::test]
async fn the_code_records_the_tier_at_issue() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let branch = h.cafe_done(&a).await;
    let c = rusqlite::Connection::open(h.dir.path().join("gz_control.db")).unwrap();
    c.execute_batch(&format!(
        "INSERT INTO entitlements (branch_id, tenant_id, tier, source, issued_at, until, \
         updated_at) SELECT id, tenant_id, 'premium', 'comp', 1, {}, 1 FROM branches \
         WHERE id = '{branch}'; \
         UPDATE onboarding_progress SET state = 'done', completed_at = 1 \
         WHERE branch_id = '{branch}' AND step = 'tier';",
        T0 + MIN15 * 4
    ))
    .unwrap();
    let (code, _) = h.issue(&a).await;
    assert_eq!(h.rows()[0][3], "premium");
    let ok = redeem_pairing_code(&h.st, &code).await.unwrap().unwrap();
    assert_eq!(ok.tier, Tier::Premium);
}

#[tokio::test]
async fn before_the_plan_step_the_code_is_not_ready() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let r = h.call("GET", "/v1/wizard/install-hub", None, &a).await;
    assert_eq!((r.status, r.code()), (403, "FORBIDDEN"));
    let r = h
        .call("POST", "/v1/wizard/install-hub/code", Some(json!({})), &a)
        .await;
    assert_eq!((r.status, r.code()), (403, "FORBIDDEN"));

    h.cafe_done(&a).await;
    let r = h.call("GET", "/v1/wizard/install-hub", None, &a).await;
    assert_eq!((r.status, r.code()), (409, "STEP_NOT_READY"));
    let r = h
        .call("POST", "/v1/wizard/install-hub/code", Some(json!({})), &a)
        .await;
    assert_eq!((r.status, r.code()), (409, "STEP_NOT_READY"));
    assert!(h.rows().is_empty());
}

#[tokio::test]
async fn the_step_needs_a_signed_in_owner_with_setup_done() {
    let h = Harness::new().await;
    let a = h.sign_up(PHONE_A, "Pixel Arena Gaming").await;
    let r = h.call("GET", "/v1/wizard/install-hub", None, &a).await;
    assert_eq!((r.status, r.code()), (403, "SETUP_INCOMPLETE"));
    let r = h
        .call("POST", "/v1/wizard/install-hub/code", Some(json!({})), "")
        .await;
    assert_eq!((r.status, r.code()), (401, "AUTH_FAILED"));
    assert!(h.rows().is_empty());
}

#[tokio::test]
async fn another_tenants_branch_is_forbidden_and_untouched() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let b = h.owner(PHONE_B, "Other Den").await;
    let a_branch = h.tier_done(&a).await;
    h.tier_done(&b).await;
    let (a_code, _) = h.issue(&a).await;
    for target in [a_branch.as_str(), "01JNOSUCHBRANCH0000000000000"] {
        let r = h
            .call(
                "GET",
                &format!("/v1/wizard/install-hub?branch_id={target}"),
                None,
                &b,
            )
            .await;
        assert_eq!((r.status, r.code()), (403, "FORBIDDEN"), "{target}");
        let r = h
            .call(
                "POST",
                "/v1/wizard/install-hub/code",
                Some(json!({"branch_id": target})),
                &b,
            )
            .await;
        assert_eq!((r.status, r.code()), (403, "FORBIDDEN"), "{target}");
    }
    let rows = h.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][7], "");
    assert!(redeem_pairing_code(&h.st, &a_code).await.unwrap().is_ok());
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
async fn logs_never_carry_the_plain_code() {
    let _exclusive = LOG_CAPTURE.write().await;
    let logs = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(logs.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    tracing::callsite::rebuild_interest_cache();

    let h = Harness::unlocked().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    h.tier_done(&a).await;
    let mut codes = Vec::new();
    for _ in 0..3 {
        codes.push(h.issue(&a).await.0);
    }
    for c in &codes {
        let _ = redeem_pairing_code(&h.st, c).await.unwrap();
    }
    let _ = redeem_pairing_code(&h.st, "not a code").await.unwrap();
    let text = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(text.contains("hub pairing code issued"), "{text}");
    assert!(text.contains("hub pairing code redeemed"), "{text}");
    assert_eq!(
        text.matches("hub pairing code refused").count(),
        3,
        "{text}"
    );
    assert!(text.contains("\"reason\":\"malformed\""), "{text}");
    for c in &codes {
        assert!(!text.contains(c.as_str()), "{c} in {text}");
        assert!(!text.contains(&h.st.pairing_code_hash(c)));
    }
}

#[tokio::test]
async fn malformed_codes_are_refused_before_lookup() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    h.tier_done(&a).await;
    let (code, _) = h.issue(&a).await;
    for bad in [
        "",
        &code[..7],
        &format!("{code}2"),
        &format!("{}0", &code[..7]),
        &format!("{}!", &code[..7]),
    ] {
        assert_eq!(
            redeem_pairing_code(&h.st, bad).await.unwrap(),
            Err(RedeemInvalid),
            "{bad:?}"
        );
    }
    assert_eq!(h.rows()[0][6], "");
    let typed = format!(
        "\t{}_{}\u{a0}",
        code[..4].to_lowercase(),
        code[4..].to_lowercase()
    );
    assert!(redeem_pairing_code(&h.st, &typed).await.unwrap().is_ok());
}

#[tokio::test]
async fn issuing_is_limited_to_thirty_codes_an_hour_per_branch() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    h.tier_done(&a).await;
    for _ in 0..30 {
        h.issue(&a).await;
        h.advance(1_000);
    }
    let r = h
        .call("POST", "/v1/wizard/install-hub/code", Some(json!({})), &a)
        .await;
    assert_eq!((r.status, r.code()), (429, "RATE_LIMITED"));
    let retry: i64 = r
        .headers
        .get(header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap();
    assert!((3_500..=3_660).contains(&retry), "{retry}");
    let rows = h.rows();
    assert_eq!(rows.len(), 30);
    assert_eq!(rows[29][7], "", "the live code survives a refused issue");
    h.advance(retry * 1_000);
    h.issue(&a).await;
}
