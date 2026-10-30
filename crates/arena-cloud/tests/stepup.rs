//! Step-up MFA for sensitive owner commands (Story 8.25): the gate on `POST /v1/commands`, the
//! step-up endpoints, their limits and single-use rules, and the `elevated_by` the Hub fetches.
//! The gate is tested with a fixture kind (`AuthState::extra_kinds`) and with the real sensitive
//! kinds of Story 8.26 (staff and roles).

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use arena_cloud::auth::AuthState;
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
use arena_cloud::{Clock, app_with_auth, auth_state, totp};
use arena_proto::{COMMAND_SCHEMA_VERSION, OwnerCommandKind, STEPUP_TTL_MS, Tier};
use arena_store::control::{CompGrant, ControlDb, LATEST_CONTROL_VERSION};
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
const STEP_MS: i64 = 30_000;
const PHONE: &str = "+919876543210";
const PASSWORD: &str = "Tandoori-Nights-42";
const SENSITIVE: &str = "test.sensitive";
const K1: &str = "01JBQ6W3Y8Z9X0A1B2C3D4E5F6";
const K2: &str = "01JBQ6W3Y8Z9X0A1B2C3D4E5F7";
const WRONG_COPY: &str = "That code didn't work. Check it and try again.";

struct Reply {
    status: u16,
    body: Value,
    retry_after: Option<u64>,
}

impl Reply {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or("")
    }
}

struct Cafe {
    branch: String,
    token: String,
    access: String,
    refresh: String,
    secret: Vec<u8>,
    backup: Vec<String>,
}

struct Harness {
    app: axum::Router,
    db: Arc<ControlDb>,
    st: Arc<AuthState>,
    now: Arc<AtomicI64>,
    _dir: tempfile::TempDir,
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

fn cookie_of(res: &axum::response::Response, name: &str) -> Option<String> {
    res.headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .find_map(|c| {
            c.to_str()
                .unwrap()
                .strip_prefix(&format!("{name}="))
                .map(|v| v.split(';').next().unwrap().to_owned())
        })
}

async fn reply(res: axum::response::Response) -> Reply {
    let status = res.status().as_u16();
    let retry_after = res
        .headers()
        .get(header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());
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
        st.extra_kinds.lock().unwrap().push(OwnerCommandKind {
            kind: SENSITIVE,
            schema_version: COMMAND_SCHEMA_VERSION,
            min_tier: Tier::Basic,
            flag: None,
            sensitive: true,
            handled: true,
            cloud: false,
        });
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

    async fn raw(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        cookies: &str,
    ) -> axum::response::Response {
        let mut req = Request::builder().method(method).uri(path);
        if !cookies.is_empty() {
            req = req.header("cookie", cookies);
        }
        let req = match body {
            Some(b) => req
                .header("content-type", "application/json")
                .body(Body::from(b.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        self.app.clone().oneshot(req).await.unwrap()
    }

    async fn call(&self, method: &str, path: &str, body: Option<Value>, access: &str) -> Reply {
        let cookies = if access.is_empty() {
            String::new()
        } else {
            format!("arena_access={access}")
        };
        reply(self.raw(method, path, body, &cookies).await).await
    }

    /// A signed-up, fully enrolled owner (real TOTP and backup codes) whose Basic branch has a
    /// Hub paired to it.
    async fn cafe(&self) -> Cafe {
        let s = self
            .call(
                "POST",
                "/v1/signup/start",
                Some(json!({
                    "name": "Asha Rao",
                    "phone": PHONE,
                    "email": "asha.rao@example.in",
                    "cafe_name": "Pixel Arena",
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
        let res = self
            .raw(
                "POST",
                "/v1/signup/verify",
                Some(json!({"challenge_id": id, "code": &text[..6]})),
                "",
            )
            .await;
        assert_eq!(res.status().as_u16(), 200);
        let access = cookie_of(&res, "arena_access").unwrap();
        let refresh = cookie_of(&res, "arena_refresh").unwrap();

        let p = self
            .call(
                "POST",
                "/v1/account/password",
                Some(json!({ "password": PASSWORD })),
                &access,
            )
            .await;
        assert_eq!(p.status, 204, "{}", p.body);
        let s = self
            .call("POST", "/v1/account/mfa/totp/start", None, &access)
            .await;
        assert_eq!(s.status, 200, "{}", s.body);
        let secret = unbase32(s.body["secret"].as_str().unwrap());
        let c = self
            .call(
                "POST",
                "/v1/account/mfa/totp/confirm",
                Some(json!({ "code": totp::code_at(&secret, totp::step_of(self.now())) })),
                &access,
            )
            .await;
        assert_eq!(c.status, 200, "{}", c.body);
        let backup = c.body["backup_codes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_str().unwrap().to_owned())
            .collect();
        let a = self
            .call("POST", "/v1/account/mfa/backup-codes/ack", None, &access)
            .await;
        assert_eq!(a.status, 204);
        // The enrolment used this step; the next code is the first a step-up may use.
        self.advance(STEP_MS);

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
        req.extensions_mut().insert(ConnectInfo(SocketAddr::new(
            "198.51.100.4".parse().unwrap(),
            40000,
        )));
        let r = reply(self.app.clone().oneshot(req).await.unwrap()).await;
        assert_eq!(r.status, 200, "{}", r.body);
        let token = r.body["token"].as_str().unwrap().to_owned();
        self.upgrade(&branch).await;
        Cafe {
            branch,
            token,
            access,
            refresh,
            secret,
            backup,
        }
    }

    async fn upgrade(&self, branch: &str) {
        self.db
            .grant_comp(&CompGrant {
                branch_id: branch,
                tier: Tier::Basic,
                until: T0 + 30 * DAY,
                audit_id: &format!("audit-{branch}"),
                operator: "ops",
                reason: "beta cafe",
                details_json: "{}",
                now: T0,
            })
            .await
            .unwrap();
    }

    async fn extra_branch(&self, c: &Cafe, name: &str) -> String {
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
        self.upgrade(&id).await;
        id
    }

    fn totp(&self, c: &Cafe) -> String {
        totp::code_at(&c.secret, totp::step_of(self.now()))
    }

    async fn send(&self, c: &Cafe, branch: &str, key: &str, kind: &str) -> Reply {
        let payload = if kind == "rate.upsert" {
            json!({"group_id": "g1", "price_per_hour_minor": 6000})
        } else {
            json!({"anything": true})
        };
        self.call(
            "POST",
            "/v1/commands",
            Some(json!({
                "branch_id": branch,
                "idempotency_key": key,
                "kind": kind,
                "payload": payload,
            })),
            &c.access,
        )
        .await
    }

    async fn stepup(&self, c: &Cafe, method: &str, code: &str) -> Reply {
        self.call(
            "POST",
            "/v1/session/stepup",
            Some(json!({"method": method, "code": code})),
            &c.access,
        )
        .await
    }

    async fn owner(&self) -> arena_store::control::Owner {
        self.db.owner_by_phone(PHONE).await.unwrap().unwrap()
    }

    fn rows(&self) -> i64 {
        rusqlite::Connection::open(self._dir.path().join("gz_control.db"))
            .unwrap()
            .query_row("SELECT COUNT(*) FROM commands", [], |r| r.get(0))
            .unwrap()
    }

    async fn session_alive(&self, c: &Cafe) -> bool {
        self.call("GET", "/v1/session", None, &c.access)
            .await
            .status
            == 200
    }
}

#[tokio::test]
async fn a_sensitive_post_needs_a_stepup_and_the_row_records_who_approved_it() {
    let h = Harness::new().await;
    let c = h.cafe().await;
    let owner = h.owner().await;

    let none = h.send(&c, &c.branch, K1, SENSITIVE).await;
    assert_eq!(
        (none.status, none.code()),
        (403, "MFA_REQUIRED"),
        "{}",
        none.body
    );
    assert_eq!(h.rows(), 0, "nothing is queued without a proof");

    let ok = h.stepup(&c, "totp", &h.totp(&c)).await;
    assert_eq!(ok.status, 204, "{}", ok.body);
    let stored =
        h.db.owner_session_by_access(&arena_cloud::auth::token_hash(&c.access), h.now())
            .await
            .unwrap()
            .unwrap();
    assert_eq!(stored.session.stepup_at, Some(h.now()));

    let sent = h.send(&c, &c.branch, K1, SENSITIVE).await;
    assert_eq!(sent.status, 200, "{}", sent.body);
    let id = sent.body["commands"][0]["id"].as_i64().unwrap();
    let row = h.db.command(id).await.unwrap().unwrap();
    assert_eq!(row.elevated_by.as_deref(), Some(owner.id.as_str()));

    // The same key again finds the same row.
    let again = h.send(&c, &c.branch, K1, SENSITIVE).await;
    assert_eq!(again.body["commands"][0]["id"], id);
    assert_eq!(h.rows(), 1);

    // The Hub's mailbox carries it on the wire.
    let mut req = Request::builder()
        .method("GET")
        .uri("/v1/commands?after=0&epoch=1:01HUBINSTANCE")
        .header("authorization", format!("Bearer {}", c.token))
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(SocketAddr::new(
        "198.51.100.4".parse().unwrap(),
        40000,
    )));
    let page = reply(h.app.clone().oneshot(req).await.unwrap()).await;
    assert_eq!(page.status, 200, "{}", page.body);
    assert_eq!(page.body["commands"][0]["elevated_by"], owner.id.as_str());
    assert_eq!(page.body["commands"][0]["issued_by"], owner.id.as_str());
}

#[tokio::test]
async fn a_non_sensitive_kind_never_asks_and_is_not_elevated_even_after_a_stepup() {
    let h = Harness::new().await;
    let c = h.cafe().await;

    let plain = h.send(&c, &c.branch, K1, "rate.upsert").await;
    assert_eq!(plain.status, 200, "{}", plain.body);
    let row =
        h.db.command(plain.body["commands"][0]["id"].as_i64().unwrap())
            .await
            .unwrap()
            .unwrap();
    assert_eq!(row.elevated_by, None);

    assert_eq!(h.stepup(&c, "totp", &h.totp(&c)).await.status, 204);
    let after = h.send(&c, &c.branch, K2, "rate.upsert").await;
    assert_eq!(after.status, 200, "{}", after.body);
    let row =
        h.db.command(after.body["commands"][0]["id"].as_i64().unwrap())
            .await
            .unwrap()
            .unwrap();
    assert_eq!(row.elevated_by, None);
}

#[tokio::test]
async fn the_proof_lasts_five_minutes_and_is_checked_after_validation() {
    let h = Harness::new().await;
    let c = h.cafe().await;
    assert_eq!(h.stepup(&c, "totp", &h.totp(&c)).await.status, 204);

    h.advance(STEPUP_TTL_MS - 1);
    // Keep the access token alive (15 minutes) and the proof just inside its age.
    let ok = h.send(&c, &c.branch, K1, SENSITIVE).await;
    assert_eq!(ok.status, 200, "{}", ok.body);

    h.advance(1);
    let old = h.send(&c, &c.branch, K2, SENSITIVE).await;
    assert_eq!(
        (old.status, old.code()),
        (403, "MFA_REQUIRED"),
        "{}",
        old.body
    );

    // A bad payload is a validation error, not a prompt.
    let bad = h
        .call(
            "POST",
            "/v1/commands",
            Some(json!({
                "branch_id": c.branch,
                "idempotency_key": K2,
                "kind": SENSITIVE,
                "payload": [1],
            })),
            &c.access,
        )
        .await;
    assert_eq!((bad.status, bad.code()), (422, "VALIDATION_FAILED"));
    assert_eq!(h.rows(), 1);
}

#[tokio::test]
async fn an_all_branches_post_is_checked_once_and_every_row_is_elevated() {
    let h = Harness::new().await;
    let c = h.cafe().await;
    let second = h.extra_branch(&c, "Second").await;

    let none = h.send(&c, "all", K1, SENSITIVE).await;
    assert_eq!((none.status, none.code()), (403, "MFA_REQUIRED"));
    assert_eq!(h.rows(), 0);

    assert_eq!(h.stepup(&c, "totp", &h.totp(&c)).await.status, 204);
    let sent = h.send(&c, "all", K1, SENSITIVE).await;
    assert_eq!(sent.status, 200, "{}", sent.body);
    let commands = sent.body["commands"].as_array().unwrap();
    assert_eq!(commands.len(), 2);
    let owner = h.owner().await;
    for cmd in commands {
        let row =
            h.db.command(cmd["id"].as_i64().unwrap())
                .await
                .unwrap()
                .unwrap();
        assert_eq!(row.elevated_by.as_deref(), Some(owner.id.as_str()));
        assert!(row.branch_id == c.branch || row.branch_id == second);
    }
}

#[tokio::test]
async fn five_wrong_codes_pause_the_stepup_for_fifteen_minutes_and_leave_the_session_alone() {
    let h = Harness::new().await;
    let c = h.cafe().await;

    let mut first = None;
    for _ in 0..5 {
        let r = h.stepup(&c, "totp", "000000").await;
        assert_eq!((r.status, r.code()), (400, "TOTP_INVALID"), "{}", r.body);
        assert_eq!(r.body["field"], "code");
        assert_eq!(r.body["message"], WRONG_COPY);
        first.get_or_insert(r.body.clone());
    }
    // The sixth, even a correct one, is paused.
    let paused = h.stepup(&c, "totp", &h.totp(&c)).await;
    assert_eq!(
        (paused.status, paused.code()),
        (429, "RATE_LIMITED"),
        "{}",
        paused.body
    );
    let wait = paused.retry_after.expect("Retry-After");
    assert!((900..=960).contains(&wait), "{wait}");

    assert!(h.session_alive(&c).await, "the session is untouched");
    let owner = h.owner().await;
    assert_eq!((owner.failed_signins, owner.locked_until), (0, None));
    let blocked = h.send(&c, &c.branch, K1, SENSITIVE).await;
    assert_eq!(blocked.code(), "MFA_REQUIRED");

    // After the window the step-up works again (the access token has expired by then, so refresh).
    h.advance(16 * 60_000);
    let res = h
        .raw(
            "POST",
            "/v1/session/refresh",
            None,
            &format!("arena_refresh={}", c.refresh),
        )
        .await;
    assert_eq!(res.status().as_u16(), 200);
    let c = Cafe {
        access: cookie_of(&res, "arena_access").unwrap(),
        ..c
    };
    let ok = h.stepup(&c, "totp", &h.totp(&c)).await;
    assert_eq!(ok.status, 204, "{}", ok.body);
}

#[tokio::test]
async fn a_correct_code_does_not_count_and_every_factor_fails_with_the_same_copy() {
    let h = Harness::new().await;
    let c = h.cafe().await;
    for _ in 0..2 {
        assert_eq!(h.stepup(&c, "totp", "000000").await.status, 400);
    }
    // A correct one in between is not a failure.
    assert_eq!(h.stepup(&c, "totp", &h.totp(&c)).await.status, 204);

    h.advance(STEP_MS);
    let totp_wrong = h.stepup(&c, "totp", "111111").await;
    let backup_wrong = h.stepup(&c, "backup_code", "zzzz-zzzz").await;
    assert_eq!(totp_wrong.body, backup_wrong.body);
    assert_eq!(totp_wrong.body["message"], WRONG_COPY);
    let owner = h.owner().await;
    assert!(h.db.set_sms_fallback(&owner.id, true).await.unwrap());
    let sms_wrong = h.stepup(&c, "sms", "222222").await;
    assert_eq!(sms_wrong.body, totp_wrong.body);
}

#[tokio::test]
async fn a_totp_step_or_backup_code_works_once() {
    let h = Harness::new().await;
    let c = h.cafe().await;

    let code = h.totp(&c);
    assert_eq!(h.stepup(&c, "totp", &code).await.status, 204);
    let replay = h.stepup(&c, "totp", &code).await;
    assert_eq!((replay.status, replay.code()), (400, "TOTP_INVALID"));
    assert_eq!(replay.body["message"], WRONG_COPY);

    // The step also cannot be used to sign in again later; the next one can step up.
    h.advance(STEP_MS);
    assert_eq!(h.stepup(&c, "totp", &h.totp(&c)).await.status, 204);

    let backup = c.backup[0].clone();
    assert_eq!(h.stepup(&c, "backup_code", &backup).await.status, 204);
    let again = h.stepup(&c, "backup_code", &backup).await;
    assert_eq!((again.status, again.code()), (400, "TOTP_INVALID"));
    assert_eq!(again.body["message"], WRONG_COPY);
    let left =
        h.db.backup_codes_remaining(&h.owner().await.id)
            .await
            .unwrap();
    assert_eq!(left, 9);
}

#[tokio::test]
async fn text_message_codes_step_up_only_when_the_owner_allowed_them() {
    let h = Harness::new().await;
    let c = h.cafe().await;

    let off = h
        .call("POST", "/v1/session/stepup/sms", Some(json!({})), &c.access)
        .await;
    assert_eq!((off.status, off.code()), (403, "FORBIDDEN"), "{}", off.body);
    let off = h.stepup(&c, "sms", "123456").await;
    assert_eq!((off.status, off.code()), (403, "FORBIDDEN"));

    let owner = h.owner().await;
    assert!(h.db.set_sms_fallback(&owner.id, true).await.unwrap());
    let sent = h
        .call("POST", "/v1/session/stepup/sms", Some(json!({})), &c.access)
        .await;
    assert_eq!(sent.status, 202, "{}", sent.body);
    assert!(sent.body["resend_after_s"].as_u64().unwrap() > 0);
    let text = h.st.sms.fake().unwrap().sent().last().unwrap().text.clone();
    let code = text[..6].to_owned();

    // Not twice in a row.
    let soon = h
        .call("POST", "/v1/session/stepup/sms", Some(json!({})), &c.access)
        .await;
    assert_eq!((soon.status, soon.code()), (429, "RATE_LIMITED"));

    assert_eq!(h.stepup(&c, "sms", &code).await.status, 204);
    let replay = h.stepup(&c, "sms", &code).await;
    assert_eq!((replay.status, replay.code()), (400, "TOTP_INVALID"));
    assert_eq!(h.send(&c, &c.branch, K1, SENSITIVE).await.status, 200);

    // A code older than 5 minutes is dead.
    h.advance(61_000);
    let again = h
        .call("POST", "/v1/session/stepup/sms", Some(json!({})), &c.access)
        .await;
    assert_eq!(again.status, 202, "{}", again.body);
    let old = h.st.sms.fake().unwrap().sent().last().unwrap().text[..6].to_owned();
    h.advance(5 * 60_000 + 1_000);
    let late = h.stepup(&c, "sms", &old).await;
    assert_eq!((late.status, late.code()), (400, "TOTP_INVALID"));
}

#[tokio::test]
async fn a_refresh_carries_the_proof_to_the_new_session_row() {
    let h = Harness::new().await;
    let c = h.cafe().await;
    assert_eq!(h.stepup(&c, "totp", &h.totp(&c)).await.status, 204);
    let proof_at = h.now();

    h.advance(60_000);
    let res = h
        .raw(
            "POST",
            "/v1/session/refresh",
            None,
            &format!("arena_refresh={}", c.refresh),
        )
        .await;
    assert_eq!(res.status().as_u16(), 200);
    let access = cookie_of(&res, "arena_access").unwrap();
    assert_ne!(access, c.access);

    let live =
        h.db.owner_session_by_access(&arena_cloud::auth::token_hash(&access), h.now())
            .await
            .unwrap()
            .unwrap();
    assert_eq!(live.session.stepup_at, Some(proof_at));
    let sent = h
        .call(
            "POST",
            "/v1/commands",
            Some(json!({
                "branch_id": c.branch,
                "idempotency_key": K1,
                "kind": SENSITIVE,
                "payload": {"anything": true},
            })),
            &access,
        )
        .await;
    assert_eq!(sent.status, 200, "{}", sent.body);
}

#[tokio::test]
async fn the_fixture_kind_is_unknown_in_production_and_the_step_up_needs_a_session() {
    let h = Harness::new().await;
    let c = h.cafe().await;
    h.st.extra_kinds.lock().unwrap().clear();
    let r = h.send(&c, &c.branch, K1, SENSITIVE).await;
    assert_eq!((r.status, r.code()), (400, "UNSUPPORTED"), "{}", r.body);

    let anon = h
        .call(
            "POST",
            "/v1/session/stepup",
            Some(json!({"method": "totp", "code": "123456"})),
            "",
        )
        .await;
    assert_eq!((anon.status, anon.code()), (401, "AUTH_FAILED"));
}

// ---- Staff users and roles (Story 8.26) ------------------------------------------------------

const K3: &str = "01JBQ6W3Y8Z9X0A1B2C3D4E5F8";
const STAFF_ID: &str = "01JSTAFFAAAAAAAAAAAAAAAAAA";

impl Harness {
    async fn post(&self, c: &Cafe, key: &str, kind: &str, payload: Value) -> Reply {
        self.call(
            "POST",
            "/v1/commands",
            Some(json!({
                "branch_id": c.branch,
                "idempotency_key": key,
                "kind": kind,
                "payload": payload,
            })),
            &c.access,
        )
        .await
    }

    /// The Hub's mailbox as the paired Hub fetches it.
    async fn mailbox(&self, c: &Cafe) -> Vec<Value> {
        let mut req = Request::builder()
            .method("GET")
            .uri("/v1/commands?after=0&epoch=1:01HUBINSTANCE")
            .header("authorization", format!("Bearer {}", c.token))
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(ConnectInfo(SocketAddr::new(
            "198.51.100.4".parse().unwrap(),
            40000,
        )));
        let page = reply(self.app.clone().oneshot(req).await.unwrap()).await;
        assert_eq!(page.status, 200, "{}", page.body);
        page.body["commands"].as_array().unwrap().clone()
    }
}

fn new_member() -> Value {
    json!({
        "login": "+919876500001",
        "display_name": "Neha Rao",
        "role_id": "Counter",
        "active": true,
    })
}

#[tokio::test]
async fn a_new_staff_member_needs_a_stepup_and_gets_one_setup_code_whose_hash_alone_goes_to_the_hub()
 {
    let h = Harness::new().await;
    let c = h.cafe().await;

    let none = h.post(&c, K1, "staff.upsert", new_member()).await;
    assert_eq!((none.status, none.code()), (403, "MFA_REQUIRED"));
    assert_eq!(h.rows(), 0);

    assert_eq!(h.stepup(&c, "totp", &h.totp(&c)).await.status, 204);
    let res = h
        .raw(
            "POST",
            "/v1/commands",
            Some(json!({
                "branch_id": c.branch,
                "idempotency_key": K1,
                "kind": "staff.upsert",
                "payload": new_member(),
            })),
            &format!("arena_access={}", c.access),
        )
        .await;
    assert_eq!(res.status().as_u16(), 200);
    assert_eq!(
        res.headers()
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok()),
        Some("no-store"),
        "a setup code is never cached"
    );
    let sent = reply(res).await;
    let codes = sent.body["setup_codes"].as_array().unwrap();
    assert_eq!(codes.len(), 1, "{}", sent.body);
    let code = codes[0]["code"].as_str().unwrap();
    assert_eq!(code.len(), 8);
    assert_eq!(codes[0]["branch_id"], c.branch.as_str());

    // The Hub gets the member id, the hash and the expiry, never the code.
    let mail = h.mailbox(&c).await;
    assert_eq!(mail.len(), 1);
    let payload = &mail[0]["payload"];
    assert_eq!(payload["staff_id"], codes[0]["staff_id"]);
    assert_eq!(payload["login"], "+919876500001");
    assert_eq!(payload["role_id"], "Counter");
    assert!(
        payload["setup_code_hash"]
            .as_str()
            .unwrap()
            .starts_with("$argon2id$")
    );
    assert_eq!(payload["setup_expires_at"], h.now() + 72 * 60 * 60 * 1000);
    assert!(!mail[0].to_string().contains(code));
    assert!(mail[0]["elevated_by"].is_string());

    // The owner list hides the hash and says a code was issued.
    let list = h.call("GET", "/v1/commands", None, &c.access).await;
    let shown = &list.body["commands"][0]["payload"];
    assert!(shown.get("setup_code_hash").is_none(), "{shown}");
    assert_eq!(shown["setup_code"], true);
    assert!(!list.body.to_string().contains(code));

    // Sent again with the same key: the first row, and no second code that would not work.
    let again = h.post(&c, K1, "staff.upsert", new_member()).await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert!(again.body.get("setup_codes").is_none(), "{}", again.body);
    assert_eq!(h.rows(), 1);
}

#[tokio::test]
async fn staff_details_are_checked_before_any_stepup_and_never_carry_a_credential() {
    let h = Harness::new().await;
    let c = h.cafe().await;
    let mut bad_phone = new_member();
    bad_phone["login"] = "98765".into();
    let mut no_name = new_member();
    no_name["display_name"] = "  ".into();
    let mut password = new_member();
    password["password_hash"] = "$argon2id$x".into();
    let mut pin = new_member();
    pin["pin_hash"] = "$argon2id$x".into();
    for (field, payload) in [
        ("login", bad_phone),
        ("display_name", no_name),
        ("payload", password),
        ("payload", pin),
    ] {
        let r = h.post(&c, K1, "staff.upsert", payload).await;
        assert_eq!(
            (r.status, r.code(), r.body["field"].as_str()),
            (422, "VALIDATION_FAILED", Some(field)),
            "{}",
            r.body
        );
    }
    assert_eq!(h.rows(), 0);
}

#[tokio::test]
async fn editing_a_member_sends_no_code_unless_a_new_one_is_asked_for() {
    let h = Harness::new().await;
    let c = h.cafe().await;
    assert_eq!(h.stepup(&c, "totp", &h.totp(&c)).await.status, 204);
    let edit = json!({
        "staff_id": STAFF_ID,
        "login": "+919876500001",
        "display_name": "Neha R.",
        "role_id": "Manager",
        "active": true,
    });
    let r = h.post(&c, K1, "staff.upsert", edit.clone()).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(r.body.get("setup_codes").is_none());
    let mail = h.mailbox(&c).await;
    assert!(mail[0]["payload"].get("setup_code_hash").is_none());
    assert_eq!(mail[0]["payload"]["staff_id"], STAFF_ID);

    let mut renew = edit;
    renew["setup_code"] = true.into();
    let r = h.post(&c, K2, "staff.upsert", renew).await;
    assert_eq!(r.body["setup_codes"].as_array().unwrap().len(), 1);
    assert_eq!(r.body["setup_codes"][0]["staff_id"], STAFF_ID);
}

#[tokio::test]
async fn deactivating_and_editing_roles_are_sensitive_and_roles_take_catalogue_permissions_only() {
    let h = Harness::new().await;
    let c = h.cafe().await;
    let deactivate = json!({"staff_id": STAFF_ID});
    let role = json!({
        "name": "Floor lead",
        "perms": [
            {"permission": "session.start", "effect": "allow"},
            {"permission": "session.end", "effect": "allow"},
        ],
    });
    for (key, kind, payload) in [
        (K1, "staff.deactivate", deactivate.clone()),
        (K2, "role.upsert", role.clone()),
    ] {
        let r = h.post(&c, key, kind, payload).await;
        assert_eq!((r.status, r.code()), (403, "MFA_REQUIRED"), "{kind}");
    }
    assert_eq!(h.rows(), 0);

    assert_eq!(h.stepup(&c, "totp", &h.totp(&c)).await.status, 204);
    let mut bad = role.clone();
    bad["perms"][1]["permission"] = "session.destroy".into();
    let r = h.post(&c, K3, "role.upsert", bad).await;
    assert_eq!(
        (r.status, r.code(), r.body["field"].as_str()),
        (422, "VALIDATION_FAILED", Some("perms[1]")),
        "{}",
        r.body
    );
    assert_eq!(
        h.rows(),
        0,
        "an unknown permission is refused before sending"
    );

    for (key, kind, payload) in [
        (K1, "staff.deactivate", deactivate),
        (K2, "role.upsert", role),
    ] {
        let r = h.post(&c, key, kind, payload).await;
        assert_eq!(r.status, 200, "{kind}: {}", r.body);
    }
    let mail = h.mailbox(&c).await;
    assert_eq!(mail.len(), 2);
    assert!(mail.iter().all(|m| m["elevated_by"].is_string()));
}

#[tokio::test]
async fn a_device_revoke_needs_a_stepup_and_a_reason_but_groups_and_assignment_do_not() {
    let h = Harness::new().await;
    let c = h.cafe().await;
    let group = json!({"name": "Row A", "group_kind": "pc", "colour": "teal", "sort_order": 1});
    let assign = json!({"device_id": "D1", "group_id": null});
    let revoke = json!({"device_id": "D1", "reason": "lost"});

    // Groups and assignment go through with no step-up and no elevation.
    for (key, kind, payload) in [
        (K1, "group.upsert", group.clone()),
        (K2, "device.assign", assign),
    ] {
        let r = h.post(&c, key, kind, payload).await;
        assert_eq!(r.status, 200, "{kind}: {}", r.body);
    }
    assert_eq!(h.rows(), 2);
    assert!(
        h.mailbox(&c)
            .await
            .iter()
            .all(|m| m["elevated_by"].is_null())
    );

    let r = h.post(&c, K3, "device.revoke", revoke.clone()).await;
    assert_eq!((r.status, r.code()), (403, "MFA_REQUIRED"));
    assert_eq!(h.rows(), 2);

    assert_eq!(h.stepup(&c, "totp", &h.totp(&c)).await.status, 204);
    // The reason is one of the four the Hub lists; a missing or unlisted one is refused.
    for bad in [
        json!({"device_id": "D1"}),
        json!({"device_id": "D1", "reason": "closing"}),
    ] {
        let r = h.post(&c, K3, "device.revoke", bad).await;
        assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));
    }
    assert_eq!(h.rows(), 2);

    let r = h.post(&c, K3, "device.revoke", revoke).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let mail = h.mailbox(&c).await;
    let sent = mail.iter().find(|m| m["kind"] == "device.revoke").unwrap();
    assert!(sent["elevated_by"].is_string());
}

impl Harness {
    async fn hub_fetch(&self, token: &str) -> Reply {
        let mut req = Request::builder()
            .method("GET")
            .uri("/v1/commands?after=0&epoch=1:01HUBINSTANCE")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(ConnectInfo(SocketAddr::new(
            "198.51.100.4".parse().unwrap(),
            40000,
        )));
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    async fn issue_code(&self, c: &Cafe) -> Reply {
        self.call(
            "POST",
            "/v1/wizard/install-hub/code",
            Some(json!({"branch_id": c.branch})),
            &c.access,
        )
        .await
    }

    async fn pair_hub(&self, code: &str, instance: &str) -> Reply {
        let mut req = Request::builder()
            .method("POST")
            .uri("/v1/pair")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "code": code,
                    "platform": "windows",
                    "hub_instance_id": instance,
                    "version": "0.1.0",
                })
                .to_string(),
            ))
            .unwrap();
        req.extensions_mut().insert(ConnectInfo(SocketAddr::new(
            "198.51.100.4".parse().unwrap(),
            40000,
        )));
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }
}

#[tokio::test]
async fn replacing_the_hub_needs_a_stepup_then_revokes_the_token_at_once_and_records_the_approval()
{
    let h = Harness::new().await;
    let c = h.cafe().await;
    let owner = h.owner().await;
    let live = h.db.active_hub(&c.branch).await.unwrap().unwrap();

    let none = h.post(&c, K1, "hub.revoke", json!({})).await;
    assert_eq!((none.status, none.code()), (403, "MFA_REQUIRED"));
    assert_eq!(h.rows(), 0);
    assert!(h.db.active_hub(&c.branch).await.unwrap().is_some());
    assert_eq!(h.hub_fetch(&c.token).await.status, 200);

    assert_eq!(h.stepup(&c, "totp", &h.totp(&c)).await.status, 204);
    let bad = h.post(&c, K1, "hub.revoke", json!({"x": 1})).await;
    assert_eq!((bad.status, bad.code()), (422, "VALIDATION_FAILED"));
    assert!(h.db.active_hub(&c.branch).await.unwrap().is_some());

    let done = h.post(&c, K1, "hub.revoke", json!({})).await;
    assert_eq!(done.status, 200, "{}", done.body);
    let view = &done.body["commands"][0];
    assert_eq!(
        (view["kind"].as_str(), view["status"].as_str()),
        (Some("hub.revoke"), Some("applied"))
    );
    assert!(view["done_at"].is_i64());
    let row =
        h.db.command(view["id"].as_i64().unwrap())
            .await
            .unwrap()
            .unwrap();
    assert_eq!(row.elevated_by.as_deref(), Some(owner.id.as_str()));
    assert_eq!(row.issued_by, owner.id);

    assert!(h.db.active_hub(&c.branch).await.unwrap().is_none());
    assert!(
        h.db.hub(&live.id)
            .await
            .unwrap()
            .unwrap()
            .revoked_at
            .is_some()
    );
    let refused = h.hub_fetch(&c.token).await;
    assert_eq!((refused.status, refused.code()), (401, "TOKEN_REVOKED"));

    // The same post again finds its row and changes nothing.
    let again = h.post(&c, K1, "hub.revoke", json!({})).await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(again.body["commands"][0]["id"], view["id"]);
    assert_eq!(h.rows(), 1);
}

#[tokio::test]
async fn after_a_revoke_a_new_code_pairs_a_replacement_and_a_repeated_post_never_revokes_it() {
    let h = Harness::new().await;
    let c = h.cafe().await;
    assert_eq!(h.stepup(&c, "totp", &h.totp(&c)).await.status, 204);
    assert_eq!(h.post(&c, K1, "hub.revoke", json!({})).await.status, 200);

    let code = h.issue_code(&c).await;
    assert_eq!(code.status, 200, "{}", code.body);
    let paired = h
        .pair_hub(code.body["code"].as_str().unwrap(), "01REPLACEMENT")
        .await;
    assert_eq!(paired.status, 200, "{}", paired.body);
    let live = h.db.active_hub(&c.branch).await.unwrap().unwrap();

    // The first post again: its row is returned and the replacement keeps its token.
    let again = h.post(&c, K1, "hub.revoke", json!({})).await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(again.body["commands"][0]["status"], "applied");
    assert_eq!(
        h.db.active_hub(&c.branch).await.unwrap().unwrap().id,
        live.id
    );
    assert_eq!(h.rows(), 1);

    // A new key revokes the replacement; with no live Hub left a further key has nothing to revoke.
    assert_eq!(h.post(&c, K2, "hub.revoke", json!({})).await.status, 200);
    let none = h.post(&c, K3, "hub.revoke", json!({})).await;
    assert_eq!((none.status, none.code()), (409, "STEP_NOT_READY"));
    assert_eq!(h.rows(), 2);
}

#[tokio::test]
async fn replacing_a_hub_is_for_one_branch_of_the_owners_own_cafe() {
    let h = Harness::new().await;
    let c = h.cafe().await;
    assert_eq!(h.stepup(&c, "totp", &h.totp(&c)).await.status, 204);
    let all = h
        .call(
            "POST",
            "/v1/commands",
            Some(json!({
                "branch_id": "all", "idempotency_key": K1, "kind": "hub.revoke", "payload": {},
            })),
            &c.access,
        )
        .await;
    assert_eq!((all.status, all.code()), (422, "VALIDATION_FAILED"));
    let other = h
        .call(
            "POST",
            "/v1/commands",
            Some(json!({
                "branch_id": "01JBQ6W3Y8Z9X0A1B2C3D4E5FZ", "idempotency_key": K1,
                "kind": "hub.revoke", "payload": {},
            })),
            &c.access,
        )
        .await;
    assert_eq!((other.status, other.code()), (404, "NOT_FOUND"));
    assert!(h.db.active_hub(&c.branch).await.unwrap().is_some());
    assert_eq!(h.rows(), 0);
    let signed_out = h
        .call(
            "POST",
            "/v1/commands",
            Some(json!({
                "branch_id": c.branch, "idempotency_key": K1, "kind": "hub.revoke", "payload": {},
            })),
            "",
        )
        .await;
    assert_eq!(signed_out.status, 401);
}
