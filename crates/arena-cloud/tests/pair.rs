use std::net::SocketAddr;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use arena_cloud::auth::AuthState;
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
use arena_cloud::{Clock, app_with_auth, auth_state};
use arena_proto::Tier;
use arena_proto::licence::{DEV_LICENCE_VERIFY_KEYS, verify};
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
const MIN15: i64 = 15 * 60 * 1000;
const PHONE_A: &str = "+919876543210";
const PEER: &str = "198.51.100.4";
const CF_PEER: &str = "173.245.48.10";

struct Harness {
    app: axum::Router,
    db: Arc<ControlDb>,
    st: Arc<AuthState>,
    now: Arc<AtomicI64>,
    dir: tempfile::TempDir,
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
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    async fn pair_from(&self, code: &str, platform: &str, peer: &str, cf: Option<&str>) -> Reply {
        let mut req = Request::builder()
            .method("POST")
            .uri("/v1/pair")
            .header("content-type", "application/json");
        if let Some(ip) = cf {
            req = req.header("cf-connecting-ip", ip);
        }
        let mut req = req
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
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::new(peer.parse().unwrap(), 40000)));
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    async fn pair(&self, code: &str, platform: &str) -> Reply {
        self.pair_from(code, platform, PEER, None).await
    }

    async fn beat(&self, token: &str, epoch: &str) -> Reply {
        let req = Request::builder()
            .method("POST")
            .uri("/v1/heartbeat")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(
                json!({"epoch": epoch, "version": "0.2.0", "schema_version": 12, "last_seq_uploaded": 0})
                    .to_string(),
            ))
            .unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    async fn paired_hub(&self, access: &str) -> (String, String) {
        let (code, _) = self.issue(access).await;
        let r = self.pair(&code, "android").await;
        assert_eq!(r.status, 200, "{}", r.body);
        (
            r.body["token"].as_str().unwrap().to_owned(),
            r.body["hub_id"].as_str().unwrap().to_owned(),
        )
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
    fn install_step_opt(&self, branch: &str) -> Option<String> {
        rusqlite::Connection::open(self.dir.path().join("gz_control.db"))
            .unwrap()
            .query_row(
                "SELECT state FROM onboarding_progress WHERE branch_id = ? AND step = 'install_hub' AND state = 'done'",
                [branch],
                |r| r.get(0),
            )
            .ok()
    }
    fn hubs(&self) -> Vec<Vec<String>> {
        let c = rusqlite::Connection::open(self.dir.path().join("gz_control.db")).unwrap();
        let mut st = c
            .prepare(
                "SELECT id, branch_id, platform, hub_instance_id, version, token_hash, \
                 CAST(paired_at AS TEXT), COALESCE(CAST(last_seen_at AS TEXT), ''), \
                 COALESCE(CAST(revoked_at AS TEXT), '') FROM hubs ORDER BY paired_at, id",
            )
            .unwrap();
        st.query_map([], |r| (0..9).map(|i| r.get::<_, String>(i)).collect())
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    async fn ready(&self) -> (String, String) {
        let a = self.owner(PHONE_A, "Pixel Arena Gaming").await;
        let branch = self.tier_done(&a).await;
        (a, branch)
    }
}

async fn reply(res: axum::response::Response) -> Reply {
    let status = res.status().as_u16();
    let retry_after = res
        .headers()
        .get(header::RETRY_AFTER)
        .map(|v| v.to_str().unwrap().to_owned());
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

#[tokio::test]
async fn an_android_hub_on_free_pairs_once_and_gets_a_hashed_token_and_a_free_licence() {
    let h = Harness::new().await;
    let (a, branch) = h.ready().await;
    let b = h.db.branch(&branch).await.unwrap().unwrap();
    let (code, _) = h.issue(&a).await;
    let typed = format!("{}-{}", &code[..4], &code[4..]).to_lowercase();
    let r = h.pair(&typed, "android").await;
    assert_eq!(r.status, 200, "{}", r.body);
    let obj = r.body.as_object().unwrap();
    let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "bootstrap",
            "branch_code",
            "branch_id",
            "hub_id",
            "licence",
            "tenant_id",
            "tier",
            "token"
        ]
    );
    let hub_id = r.body["hub_id"].as_str().unwrap();
    let token = r.body["token"].as_str().unwrap();
    let secret = token
        .strip_prefix(&format!("hub_{hub_id}."))
        .expect("hub_<id>.<secret>");
    assert_eq!(secret.len(), 43, "256 bits in base64url");
    assert!(
        secret
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    );
    assert_eq!(r.body["tenant_id"], b.tenant_id.as_str());
    assert_eq!(r.body["branch_id"], branch.as_str());
    assert_eq!(r.body["tier"], "free");
    assert_eq!(r.body["branch_code"], b.branch_code.as_str());

    let claims = verify(
        r.body["licence"].as_str().unwrap(),
        DEV_LICENCE_VERIFY_KEYS,
        T0,
    )
    .unwrap();
    assert_eq!(claims.tier, Tier::Free);
    assert_eq!(claims.limits.devices, 20);
    assert_eq!(
        (claims.tenant_id.as_str(), claims.branch_id.as_str()),
        (b.tenant_id.as_str(), branch.as_str())
    );

    let hubs = h.hubs();
    assert_eq!(hubs.len(), 1);
    let row = &hubs[0];
    assert_eq!(
        (
            &row[0],
            &row[1],
            row[2].as_str(),
            row[3].as_str(),
            row[4].as_str()
        ),
        (
            &hub_id.to_owned(),
            &branch,
            "android",
            "01HUBINSTANCE",
            "0.1.0"
        )
    );
    assert!(row[5].starts_with("$argon2id$"), "{}", row[5]);
    assert!(arena_cloud::passwords::verify_secret(secret, &row[5]));
    assert!(!row[5].contains(secret));
    assert_eq!(
        (row[6].as_str(), row[7].as_str(), row[8].as_str()),
        (T0.to_string().as_str(), "", "")
    );
    let file = std::fs::read(h.dir.path().join("gz_control.db")).unwrap();
    assert!(
        !file.windows(secret.len()).any(|w| w == secret.as_bytes()),
        "the token is in the database file"
    );

    let locked = h.db.branch(&branch).await.unwrap().unwrap();
    assert_eq!(locked.code_locked_at, Some(T0));
    let used =
        h.db.pairing_code_by_hash(&h.st.pairing_code_hash(&code))
            .await
            .unwrap()
            .unwrap();
    assert_eq!(used.used_at, Some(T0));
    let state = h.call("GET", "/v1/wizard/install-hub", None, &a).await;
    assert_eq!(state.body["hub_paired"], false);
    assert_eq!(h.install_step(&branch), "pending");

    let again = h.pair(&code, "android").await;
    assert_eq!((again.status, again.code()), (401, "PAIR_CODE_INVALID"));
    assert!(again.body.get("token").is_none());
    assert_eq!(h.hubs().len(), 1);
}

#[tokio::test]
async fn a_windows_hub_on_free_is_refused_and_the_code_stays_valid() {
    let h = Harness::new().await;
    let (a, branch) = h.ready().await;
    let (code, _) = h.issue(&a).await;
    let r = h.pair(&code, "windows").await;
    assert_eq!(
        (r.status, r.code()),
        (403, "FEATURE_NOT_ENTITLED"),
        "{}",
        r.body
    );
    assert_eq!(r.body["field"], "platform");
    assert!(h.hubs().is_empty());
    let row =
        h.db.pairing_code_by_hash(&h.st.pairing_code_hash(&code))
            .await
            .unwrap()
            .unwrap();
    assert!(row.is_valid_at(T0), "{row:?}");
    assert_eq!(
        h.db.branch(&branch).await.unwrap().unwrap().code_locked_at,
        None
    );
    let ok = h.pair(&code, "android").await;
    assert_eq!(ok.status, 200, "{}", ok.body);
}

#[tokio::test]
async fn a_windows_hub_on_comp_basic_pairs_with_a_basic_licence() {
    let h = Harness::new().await;
    let (a, branch) = h.ready().await;
    h.db.grant_comp(&CompGrant {
        branch_id: &branch,
        tier: Tier::Basic,
        until: T0 + 30 * 24 * 60 * 60 * 1000,
        audit_id: "audit-1",
        operator: "ops",
        reason: "beta cafe",
        details_json: "{}",
        now: T0,
    })
    .await
    .unwrap();
    let (code, _) = h.issue(&a).await;
    let r = h.pair(&code, "windows").await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["tier"], "basic");
    let claims = verify(
        r.body["licence"].as_str().unwrap(),
        DEV_LICENCE_VERIFY_KEYS,
        T0,
    )
    .unwrap();
    assert_eq!(claims.tier, Tier::Basic);
    assert_eq!(claims.limits.devices, 60);
    assert_eq!(h.hubs()[0][2], "windows");
}

#[tokio::test]
async fn wrong_malformed_and_expired_codes_are_refused() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let (code, _) = h.issue(&a).await;
    for wrong in ["ZZZZZZZZ", "abc", "", "7K2M9PQ0", "ZZZZ ZZZZ ZZZZ"] {
        let r = h.pair(wrong, "android").await;
        assert_eq!((r.status, r.code()), (401, "PAIR_CODE_INVALID"), "{wrong}");
    }
    h.advance(MIN15);
    let r = h.pair(&code, "android").await;
    assert_eq!(
        (r.status, r.code()),
        (401, "PAIR_CODE_EXPIRED"),
        "{}",
        r.body
    );
    assert!(h.hubs().is_empty());
}

#[tokio::test]
async fn the_request_body_is_checked() {
    let h = Harness::new().await;
    let r = h.pair("ZZZZZZZZ", "linux").await;
    assert_eq!((r.status, r.code()), (400, "BAD_REQUEST"), "{}", r.body);
    let req = Request::builder()
        .method("POST")
        .uri("/v1/pair")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"code": "ZZZZZZZZ", "platform": "android", "hub_instance_id": "", "version": "1"})
                .to_string(),
        ))
        .unwrap();
    let r = reply(h.app.clone().oneshot(req).await.unwrap()).await;
    assert_eq!(
        (r.status, r.code()),
        (422, "VALIDATION_FAILED"),
        "{}",
        r.body
    );
    assert_eq!(r.body["field"], "hub_instance_id");
}

#[tokio::test]
async fn five_failures_invalidate_the_code() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let (code, _) = h.issue(&a).await;
    for _ in 0..5 {
        let r = h.pair(&code, "windows").await;
        assert_eq!(r.code(), "FEATURE_NOT_ENTITLED");
    }
    let row =
        h.db.pairing_code_by_hash(&h.st.pairing_code_hash(&code))
            .await
            .unwrap()
            .unwrap();
    assert_eq!(row.invalidated_at, Some(T0));
    let r = h.pair(&code, "android").await;
    assert_eq!((r.status, r.code()), (401, "PAIR_CODE_INVALID"));
    assert!(h.hubs().is_empty());
}

#[tokio::test]
async fn four_failures_leave_the_code_usable() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let (code, _) = h.issue(&a).await;
    for _ in 0..4 {
        assert_eq!(
            h.pair(&code, "windows").await.code(),
            "FEATURE_NOT_ENTITLED"
        );
    }
    assert_eq!(h.pair(&code, "android").await.status, 200);
}

#[tokio::test]
async fn more_than_twenty_attempts_an_hour_from_one_ip_are_rate_limited() {
    let h = Harness::new().await;
    for _ in 0..20 {
        let r = h.pair("ZZZZZZZZ", "android").await;
        assert_eq!(r.code(), "PAIR_CODE_INVALID");
    }
    let r = h.pair("ZZZZZZZZ", "android").await;
    assert_eq!((r.status, r.code()), (429, "RATE_LIMITED"), "{}", r.body);
    let retry: u64 = r.retry_after.as_deref().unwrap().parse().unwrap();
    assert!((3500..=3660).contains(&retry), "{retry}");
    let other = h
        .pair_from("ZZZZZZZZ", "android", "198.51.100.5", None)
        .await;
    assert_eq!(other.code(), "PAIR_CODE_INVALID");
    h.advance(61 * 60 * 1000);
    assert_eq!(
        h.pair("ZZZZZZZZ", "android").await.code(),
        "PAIR_CODE_INVALID"
    );
}

#[tokio::test]
async fn the_client_ip_comes_from_cloudflare_only_when_the_peer_is_cloudflare() {
    let h = Harness::new().await;
    for _ in 0..20 {
        let r = h
            .pair_from("ZZZZZZZZ", "android", CF_PEER, Some("203.0.113.7"))
            .await;
        assert_eq!(r.code(), "PAIR_CODE_INVALID");
    }
    let limited = h
        .pair_from("ZZZZZZZZ", "android", CF_PEER, Some("203.0.113.7"))
        .await;
    assert_eq!(limited.code(), "RATE_LIMITED");
    let other_client = h
        .pair_from("ZZZZZZZZ", "android", CF_PEER, Some("203.0.113.8"))
        .await;
    assert_eq!(other_client.code(), "PAIR_CODE_INVALID");

    for i in 0..20 {
        let spoof = format!("203.0.113.{}", 100 + i);
        let r = h.pair_from("ZZZZZZZZ", "android", PEER, Some(&spoof)).await;
        assert_eq!(r.code(), "PAIR_CODE_INVALID");
    }
    let r = h
        .pair_from("ZZZZZZZZ", "android", PEER, Some("203.0.113.200"))
        .await;
    assert_eq!(
        r.code(),
        "RATE_LIMITED",
        "a header from a non-Cloudflare peer is ignored"
    );
}

#[tokio::test]
async fn re_pairing_revokes_the_previous_hub() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let (first, _) = h.issue(&a).await;
    let one = h.pair(&first, "android").await;
    assert_eq!(one.status, 200);
    h.advance(1_000);
    let (second, _) = h.issue(&a).await;
    let two = h.pair(&second, "android").await;
    assert_eq!(two.status, 200, "{}", two.body);
    let hubs = h.hubs();
    assert_eq!(hubs.len(), 2);
    assert_eq!(hubs[0][0], one.body["hub_id"].as_str().unwrap());
    assert_eq!(hubs[0][8], (T0 + 1_000).to_string());
    assert_eq!(hubs[1][0], two.body["hub_id"].as_str().unwrap());
    assert_eq!(hubs[1][8], "");
    let new_secret = two.body["token"]
        .as_str()
        .unwrap()
        .split_once('.')
        .unwrap()
        .1
        .to_owned();
    assert!(arena_cloud::passwords::verify_secret(
        &new_secret,
        &hubs[1][5]
    ));
    assert_ne!(one.body["token"], two.body["token"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_pairs_with_one_code_succeed_exactly_once() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let (code, _) = h.issue(&a).await;
    let (x, y) = tokio::join!(h.pair(&code, "android"), h.pair(&code, "android"));
    let mut statuses = [x.status, y.status];
    statuses.sort_unstable();
    assert_eq!(statuses, [200, 401], "{} / {}", x.body, y.body);
    let loser = if x.status == 401 { &x } else { &y };
    assert_eq!(loser.code(), "PAIR_CODE_INVALID");
    assert_eq!(h.hubs().len(), 1);
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
async fn logs_never_carry_the_code_or_the_token() {
    let logs = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(logs.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("the only global subscriber");

    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let (refused, _) = h.issue(&a).await;
    assert_eq!(
        h.pair(&refused, "windows").await.code(),
        "FEATURE_NOT_ENTITLED"
    );
    let r = h.pair(&refused, "android").await;
    assert_eq!(r.status, 200);
    let token = r.body["token"].as_str().unwrap().to_owned();
    let secret = token.split_once('.').unwrap().1.to_owned();
    let text = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(text.contains("hub paired"), "{text}");
    assert!(text.contains("hub pairing refused"), "{text}");
    for needle in [
        refused.as_str(),
        token.as_str(),
        secret.as_str(),
        h.st.pairing_code_hash(&refused).as_str(),
    ] {
        assert!(!text.contains(needle), "{needle} in {text}");
    }
}

#[tokio::test]
async fn the_first_beat_creates_the_fence_marks_step_three_and_answers_a_licence() {
    let h = Harness::new().await;
    let (a, branch) = h.ready().await;
    let (token, hub_id) = h.paired_hub(&a).await;
    assert_eq!(h.install_step_opt(&branch), None);
    let before = h.call("GET", "/v1/wizard/install-hub", None, &a).await;
    assert_eq!(before.body["hub_paired"], false);
    h.advance(1_000);
    let r = h.beat(&token, "1:HUBA").await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        r.body["fence"],
        json!({"epoch": "1:HUBA", "confirmed": true})
    );
    assert_eq!(r.body["cloud_time"], T0 + 1_000);
    let claims = verify(
        r.body["licence"].as_str().unwrap(),
        DEV_LICENCE_VERIFY_KEYS,
        T0 + 1_000,
    )
    .unwrap();
    assert_eq!(
        (claims.tier, claims.branch_id.as_str()),
        (Tier::Free, branch.as_str())
    );
    assert_eq!(h.install_step(&branch), "done");
    let hub = &h.hubs()[0];
    assert_eq!(
        (hub[0].as_str(), hub[4].as_str()),
        (hub_id.as_str(), "0.2.0")
    );
    assert_eq!(hub[7], (T0 + 1_000).to_string());
    let after = h.call("GET", "/v1/wizard/install-hub", None, &a).await;
    assert_eq!(after.body["hub_paired"], true);
    let wizard = h.call("GET", "/v1/wizard", None, &a).await;
    assert_eq!(wizard.body["current"], "devices");
    h.advance(5_000);
    assert_eq!(h.beat(&token, "1:HUBA").await.status, 200);
    let c = rusqlite::Connection::open(h.dir.path().join("gz_control.db")).unwrap();
    let (fences, schema): (i64, i64) = c
        .query_row(
            "SELECT (SELECT COUNT(*) FROM branch_fence), schema_version FROM hubs",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((fences, schema), (1, 12));
    let done_at: i64 = c
        .query_row(
            "SELECT completed_at FROM onboarding_progress WHERE branch_id = ? AND step = 'install_hub'",
            [&branch],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(done_at, T0 + 1_000);
}

#[tokio::test]
async fn a_later_beat_with_another_epoch_is_not_confirmed() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let (token, _) = h.paired_hub(&a).await;
    assert_eq!(
        h.beat(&token, "1:HUBA").await.body["fence"]["confirmed"],
        true
    );
    h.advance(5_000);
    let same = h.beat(&token, "1:HUBA").await;
    assert_eq!(same.body["fence"]["confirmed"], true);
    h.advance(5_000);
    let other = h.beat(&token, "2:HUBB").await;
    assert_eq!(
        other.body["fence"],
        json!({"epoch": "1:HUBA", "confirmed": false})
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_first_beats_with_different_epochs_leave_one_fence() {
    let h = Harness::new().await;
    let (a, branch) = h.ready().await;
    let (_, hub_id) = h.paired_hub(&a).await;
    let beat = |epoch: &'static str| {
        let db = Arc::clone(&h.db);
        let (branch, hub_id) = (branch.clone(), hub_id.clone());
        async move {
            db.heartbeat(&arena_store::control::Beat {
                hub_id: &hub_id,
                branch_id: &branch,
                epoch,
                version: "0.2.0",
                schema_version: 12,
                now: T0,
            })
            .await
            .unwrap()
            .unwrap()
        }
    };
    let (x, y) = tokio::join!(beat("1:HUBA"), beat("1:HUBB"));
    assert_eq!(x, y);
    assert!(x == "1:HUBA" || x == "1:HUBB");
}

#[tokio::test]
async fn a_comp_grant_shows_up_on_the_next_beat() {
    let h = Harness::new().await;
    let (a, branch) = h.ready().await;
    let (token, _) = h.paired_hub(&a).await;
    let free = h.beat(&token, "1:HUBA").await;
    let tier = |r: &Reply| {
        verify(
            r.body["licence"].as_str().unwrap(),
            DEV_LICENCE_VERIFY_KEYS,
            T0,
        )
        .unwrap()
        .tier
    };
    assert_eq!(tier(&free), Tier::Free);
    h.db.grant_comp(&CompGrant {
        branch_id: &branch,
        tier: Tier::Basic,
        until: T0 + 30 * 24 * 60 * 60 * 1000,
        audit_id: "audit-1",
        operator: "ops",
        reason: "beta cafe",
        details_json: "{}",
        now: T0,
    })
    .await
    .unwrap();
    h.advance(5_000);
    assert_eq!(tier(&h.beat(&token, "1:HUBA").await), Tier::Basic);
}

#[tokio::test]
async fn bad_missing_and_revoked_tokens_are_refused() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let (token, hub_id) = h.paired_hub(&a).await;
    for bad in [
        "nope",
        "hub_x.y",
        &format!("hub_{hub_id}.wrong"),
        "hub_01NOSUCH.secret",
    ] {
        let r = h.beat(bad, "1:HUBA").await;
        assert_eq!((r.status, r.code()), (401, "TOKEN_INVALID"), "{bad}");
    }
    let bare = Request::builder()
        .method("POST")
        .uri("/v1/heartbeat")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"epoch": "1:HUBA", "version": "0.2.0", "schema_version": 12, "last_seq_uploaded": 0})
                .to_string(),
        ))
        .unwrap();
    let r = reply(h.app.clone().oneshot(bare).await.unwrap()).await;
    assert_eq!(r.status, 401);
    assert_eq!(h.beat(&token, "1:HUBA").await.status, 200);
    h.advance(1_000);
    let (second, _) = h.issue(&a).await;
    assert_eq!(h.pair(&second, "android").await.status, 200);
    h.advance(5_000);
    let r = h.beat(&token, "1:HUBA").await;
    assert_eq!((r.status, r.code()), (401, "TOKEN_REVOKED"));
}

#[tokio::test]
async fn beats_closer_than_five_seconds_are_rate_limited() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let (token, _) = h.paired_hub(&a).await;
    assert_eq!(h.beat(&token, "1:HUBA").await.status, 200);
    h.advance(4_000);
    let r = h.beat(&token, "1:HUBA").await;
    assert_eq!((r.status, r.code()), (429, "RATE_LIMITED"));
    assert!(r.retry_after.is_some());
    h.advance(1_000);
    assert_eq!(h.beat(&token, "1:HUBA").await.status, 200);
}

#[tokio::test]
async fn a_verified_token_is_cached_for_five_minutes() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let (token, _) = h.paired_hub(&a).await;
    assert_eq!(h.beat(&token, "1:HUBA").await.status, 200);
    assert_eq!(h.st.hub_tokens.lock().unwrap().len(), 1);
    let (id, _) = token.split_once('.').unwrap();
    let forged = h.beat(&format!("{id}.wrong"), "1:HUBA").await;
    assert_eq!((forged.status, forged.code()), (401, "TOKEN_INVALID"));
    assert_eq!(h.st.hub_tokens.lock().unwrap().len(), 1);
    rusqlite::Connection::open(h.dir.path().join("gz_control.db"))
        .unwrap()
        .execute("UPDATE hubs SET token_hash = 'garbage'", [])
        .unwrap();
    h.advance(5_000);
    assert_eq!(h.beat(&token, "1:HUBA").await.status, 200);
    h.advance(5 * 60 * 1000);
    assert_eq!(h.beat(&token, "1:HUBA").await.status, 401);
}

#[tokio::test]
async fn the_wizard_event_stream_delivers_the_first_beat() {
    use futures_util::StreamExt;
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let (token, _) = h.paired_hub(&a).await;
    let req = Request::builder()
        .uri("/v1/wizard/events")
        .header("cookie", format!("arena_access={a}"))
        .body(Body::empty())
        .unwrap();
    let res = h.app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status().as_u16(), 200);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "text/event-stream");
    let mut stream = res.into_body().into_data_stream();
    assert_eq!(h.beat(&token, "1:HUBA").await.status, 200);
    let frame = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&frame).contains("event: wizard.changed"));
}

#[tokio::test]
async fn the_wizard_event_stream_needs_a_signed_in_owner() {
    let h = Harness::new().await;
    let r = h.call("GET", "/v1/wizard/events", None, "").await;
    assert_eq!(r.status, 401);
}

#[tokio::test]
async fn re_pairing_resets_the_fence_so_the_new_hub_confirms() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let (old, _) = h.paired_hub(&a).await;
    assert_eq!(
        h.beat(&old, "1:HUBA").await.body["fence"]["confirmed"],
        true
    );
    h.advance(5_000);
    let (new, _) = h.paired_hub(&a).await;
    let r = h.beat(&new, "1:HUBB").await;
    assert_eq!(
        r.body["fence"],
        json!({"epoch": "1:HUBB", "confirmed": true})
    );
}

#[tokio::test]
async fn a_beat_for_a_revoked_hub_writes_nothing() {
    let h = Harness::new().await;
    let (a, branch) = h.ready().await;
    let (_, hub_id) = h.paired_hub(&a).await;
    let (_, _) = h.paired_hub(&a).await;
    let out =
        h.db.heartbeat(&arena_store::control::Beat {
            hub_id: &hub_id,
            branch_id: &branch,
            epoch: "1:HUBA",
            version: "0.2.0",
            schema_version: 12,
            now: T0,
        })
        .await
        .unwrap();
    assert_eq!(out, None);
    let c = rusqlite::Connection::open(h.dir.path().join("gz_control.db")).unwrap();
    let fences: i64 = c
        .query_row("SELECT COUNT(*) FROM branch_fence", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fences, 0);
    assert_eq!(h.install_step_opt(&branch), None);
}

#[tokio::test]
async fn the_pair_response_carries_the_bootstrap_command_without_the_setup_code() {
    let h = Harness::new().await;
    let (a, branch) = h.ready().await;
    let (code, _) = h.issue(&a).await;
    let r = h.pair(&code, "android").await;
    assert_eq!(r.status, 200, "{}", r.body);
    let b = &r.body["bootstrap"];
    let key = b["idempotency_key"].as_str().unwrap();
    assert!(ulid::Ulid::from_string(key).is_ok(), "{key}");
    let live = h.db.branch(&branch).await.unwrap().unwrap();
    assert_eq!(
        b["profile"],
        json!({
            "name": "Pixel Arena Gaming",
            "branch_code": live.branch_code,
            "currency": "INR",
            "tz": "Asia/Kolkata",
            "day_start_min": 0,
            "country": "IN",
            "gst_registered": false,
            "gstin": "",
            "tax_mode": "simple",
        })
    );
    let o = &b["owner_staff"];
    assert_eq!(
        (o["name"].as_str(), o["login"].as_str(), o["role"].as_str()),
        (Some("Asha Rao"), Some(PHONE_A), Some("Owner"))
    );
    assert_eq!(o["setup_expires_at"], T0 + 72 * 3_600_000);
    let hash = o["setup_code_hash"].as_str().unwrap();
    assert!(hash.starts_with("$argon2id$"));
    let row = h.db.setup_code(&branch).await.unwrap().unwrap();
    assert_eq!((row.code_hash.as_str(), row.has_plaintext), (hash, true));
}

#[tokio::test]
async fn a_gst_registered_indian_branch_bootstraps_as_gst_in_and_others_as_simple() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let c = h
        .call(
            "PUT",
            "/v1/wizard/cafe",
            Some(json!({
                "cafe_name": "Pixel Arena Gaming", "city": "Pune", "country": "IN",
                "currency": "INR", "timezone": "Asia/Kolkata",
                "gst_registered": true, "gstin": "27AAPFU0939F1ZV",
            })),
            &a,
        )
        .await;
    assert_eq!(c.status, 200, "{}", c.body);
    let t = h
        .call("PUT", "/v1/wizard/tier", Some(json!({"tier": "free"})), &a)
        .await;
    assert_eq!(t.status, 200, "{}", t.body);
    let (code, _) = h.issue(&a).await;
    let r = h.pair(&code, "android").await;
    let p = &r.body["bootstrap"]["profile"];
    assert_eq!(
        (
            p["gst_registered"].clone(),
            p["gstin"].as_str(),
            p["tax_mode"].as_str()
        ),
        (json!(true), Some("27AAPFU0939F1ZV"), Some("gst_in"))
    );

    let h2 = Harness::new().await;
    let a2 = h2.owner(PHONE_A, "Gulf Gaming").await;
    let c = h2
        .call(
            "PUT",
            "/v1/wizard/cafe",
            Some(json!({
                "cafe_name": "Gulf Gaming", "city": "Dubai", "country": "AE",
                "currency": "AED", "timezone": "Asia/Dubai", "gst_registered": false,
            })),
            &a2,
        )
        .await;
    assert_eq!(c.status, 200, "{}", c.body);
    let t = h2
        .call("PUT", "/v1/wizard/tier", Some(json!({"tier": "free"})), &a2)
        .await;
    assert_eq!(t.status, 200, "{}", t.body);
    let (code, _) = h2.issue(&a2).await;
    let r = h2.pair(&code, "android").await;
    let p = &r.body["bootstrap"]["profile"];
    assert_eq!(
        (
            p["country"].as_str(),
            p["tax_mode"].as_str(),
            p["currency"].as_str()
        ),
        (Some("AE"), Some("simple"), Some("AED"))
    );
}

#[tokio::test]
async fn re_pairing_sends_the_same_bootstrap_key_and_the_same_owner_code_hash() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let (first, _) = h.issue(&a).await;
    let one = h.pair(&first, "android").await;
    h.advance(1_000);
    let (second, _) = h.issue(&a).await;
    let two = h.pair(&second, "android").await;
    assert_eq!(two.status, 200, "{}", two.body);
    assert_eq!(
        one.body["bootstrap"]["idempotency_key"],
        two.body["bootstrap"]["idempotency_key"]
    );
    assert_eq!(
        one.body["bootstrap"]["owner_staff"]["setup_code_hash"],
        two.body["bootstrap"]["owner_staff"]["setup_code_hash"]
    );
}

#[tokio::test]
async fn the_owner_sees_the_setup_code_once_after_step_three_and_it_opens_to_the_stored_hash() {
    let h = Harness::new().await;
    let (a, branch) = h.ready().await;
    let reveal = |a: String| {
        let h = &h;
        async move {
            h.call("POST", "/v1/wizard/setup-code/reveal", Some(json!({})), &a)
                .await
        }
    };
    let (code, _) = h.issue(&a).await;
    let paired = h.pair(&code, "android").await;
    let hash = paired.body["bootstrap"]["owner_staff"]["setup_code_hash"]
        .as_str()
        .unwrap()
        .to_owned();
    let token = paired.body["token"].as_str().unwrap().to_owned();
    assert_eq!(reveal(a.clone()).await.code(), "STEP_NOT_READY");
    assert_eq!(
        h.call("POST", "/v1/wizard/setup-code/reveal", Some(json!({})), "")
            .await
            .status,
        401
    );
    h.advance(1_000);
    assert_eq!(h.beat(&token, "1:HUBA").await.status, 200);
    let shown = reveal(a.clone()).await;
    assert_eq!(shown.status, 200, "{}", shown.body);
    let plain = shown.body["code"].as_str().unwrap();
    assert!(arena_proto::valid_pairing_code(plain), "{plain}");
    assert!(arena_cloud::passwords::verify_secret(plain, &hash));
    let leaked = paired.body.to_string();
    assert!(!leaked.contains(plain) && !leaked.contains(&plain[..4]));
    let again = reveal(a.clone()).await;
    assert_eq!((again.status, again.code()), (404, "NOT_FOUND"));
    let row = h.db.setup_code(&branch).await.unwrap().unwrap();
    assert_eq!((row.has_plaintext, row.shown_at), (false, Some(T0 + 1_000)));
    assert_eq!(row.code_hash, hash);
}

#[tokio::test]
async fn the_setup_code_plaintext_is_gone_after_twenty_four_hours() {
    let h = Harness::new().await;
    let (a, branch) = h.ready().await;
    let (token, _) = h.paired_hub(&a).await;
    h.advance(1_000);
    assert_eq!(h.beat(&token, "1:HUBA").await.status, 200);
    h.advance(24 * 3_600_000);
    let r = h
        .call("POST", "/v1/wizard/setup-code/reveal", Some(json!({})), &a)
        .await;
    assert_eq!((r.status, r.code()), (404, "NOT_FOUND"));
    let row = h.db.setup_code(&branch).await.unwrap().unwrap();
    assert!(!row.has_plaintext);
    assert_eq!(row.shown_at, None);
}

#[tokio::test]
async fn a_heartbeat_wipes_an_expired_setup_code_plaintext() {
    let h = Harness::new().await;
    let (a, branch) = h.ready().await;
    let (token, _) = h.paired_hub(&a).await;
    h.advance(1_000);
    assert_eq!(h.beat(&token, "1:HUBA").await.status, 200);
    assert!(
        h.db.setup_code(&branch)
            .await
            .unwrap()
            .unwrap()
            .has_plaintext
    );
    h.advance(24 * 3_600_000);
    assert_eq!(h.beat(&token, "1:HUBA").await.status, 200);
    assert!(
        !h.db
            .setup_code(&branch)
            .await
            .unwrap()
            .unwrap()
            .has_plaintext
    );
}

#[tokio::test]
async fn another_owner_cannot_reveal_this_branchs_setup_code() {
    let h = Harness::new().await;
    let (a, branch) = h.ready().await;
    let (token, _) = h.paired_hub(&a).await;
    h.advance(1_000);
    assert_eq!(h.beat(&token, "1:HUBA").await.status, 200);
    let b = h.owner("+919812345678", "Other Cafe").await;
    let other = h.tier_done(&b).await;
    let r = h
        .call(
            "POST",
            "/v1/wizard/setup-code/reveal",
            Some(json!({"branch_id": branch})),
            &b,
        )
        .await;
    assert_eq!(r.status, 403, "{}", r.body);
    assert!(
        h.db.setup_code(&branch)
            .await
            .unwrap()
            .unwrap()
            .has_plaintext
    );
    let own = h
        .call(
            "POST",
            "/v1/wizard/setup-code/reveal",
            Some(json!({"branch_id": other})),
            &b,
        )
        .await;
    assert_eq!(own.code(), "STEP_NOT_READY");
}

#[tokio::test]
async fn the_setup_code_plaintext_lasts_until_the_last_millisecond_of_the_twenty_four_hours() {
    for (offset, shown) in [(24 * 3_600_000 - 1, true), (24 * 3_600_000, false)] {
        let h = Harness::new().await;
        let (a, _) = h.ready().await;
        let (token, _) = h.paired_hub(&a).await;
        h.advance(1_000);
        assert_eq!(h.beat(&token, "1:HUBA").await.status, 200);
        h.advance(offset - 1_000);
        let r = h
            .call("POST", "/v1/wizard/setup-code/reveal", Some(json!({})), &a)
            .await;
        assert_eq!(r.status == 200, shown, "{offset}: {}", r.body);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_simultaneous_reveals_show_the_code_to_exactly_one() {
    let h = Arc::new(Harness::new().await);
    let (a, _) = h.ready().await;
    let (token, _) = h.paired_hub(&a).await;
    h.advance(1_000);
    assert_eq!(h.beat(&token, "1:HUBA").await.status, 200);
    let go = |h: Arc<Harness>, a: String| async move {
        h.call("POST", "/v1/wizard/setup-code/reveal", Some(json!({})), &a)
            .await
            .status
    };
    let (x, y) = tokio::join!(go(Arc::clone(&h), a.clone()), go(Arc::clone(&h), a.clone()));
    let mut got = [x, y];
    got.sort_unstable();
    assert_eq!(got, [200, 404]);
}

#[tokio::test]
async fn a_re_pair_after_the_hub_expiry_hands_over_a_fresh_seventy_two_hours() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let (first, _) = h.issue(&a).await;
    let one = h.pair(&first, "android").await;
    h.advance(5 * 24 * 3_600_000);
    let (second, _) = h.issue(&a).await;
    let two = h.pair(&second, "android").await;
    assert_eq!(two.status, 200, "{}", two.body);
    assert_eq!(
        one.body["bootstrap"]["owner_staff"]["setup_code_hash"],
        two.body["bootstrap"]["owner_staff"]["setup_code_hash"]
    );
    assert_eq!(
        two.body["bootstrap"]["owner_staff"]["setup_expires_at"],
        T0 + 5 * 24 * 3_600_000 + 72 * 3_600_000
    );
}

#[tokio::test]
async fn a_profile_the_hub_would_refuse_fails_before_the_pairing_code_is_spent() {
    let h = Harness::new().await;
    let (a, branch) = h.ready().await;
    let (code, _) = h.issue(&a).await;
    let c = rusqlite::Connection::open(h.dir.path().join("gz_control.db")).unwrap();
    c.execute("UPDATE owners SET name = ?1", ["x".repeat(120)])
        .unwrap();
    let r = h.pair(&code, "android").await;
    assert_eq!(r.status, 500, "{}", r.body);
    assert!(h.hubs().is_empty());
    assert!(h.db.setup_code(&branch).await.unwrap().is_none());
    let unused =
        h.db.pairing_code_by_hash(&h.st.pairing_code_hash(&code))
            .await
            .unwrap()
            .unwrap();
    assert_eq!(unused.used_at, None);
    c.execute("UPDATE owners SET name = 'Asha Rao'", [])
        .unwrap();
    assert_eq!(h.pair(&code, "android").await.status, 200);
}

impl Harness {
    async fn submit(&self, branch: &str, key: &str, kind: &str, payload: Value) -> Reply {
        use arena_cloud::api::commands::submit_command;
        match submit_command(&self.st, branch, key, kind, &payload, "owner-1").await {
            Ok(row) => Reply {
                status: 200,
                body: json!({"id": row.id, "status": row.status.as_str(), "kind": row.kind}),
                retry_after: None,
            },
            Err(e) => Reply {
                status: e.status().as_u16(),
                body: serde_json::to_value(e.body()).unwrap(),
                retry_after: None,
            },
        }
    }

    async fn fetch_commands(&self, token: &str, after: i64) -> Reply {
        let req = Request::builder()
            .method("GET")
            .uri(format!("/v1/commands?after={after}&epoch=1:HUBA"))
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    async fn command_result(&self, token: &str, id: i64, body: Value) -> Reply {
        let req = Request::builder()
            .method("POST")
            .uri(format!("/v1/commands/{id}/result"))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(body.to_string()))
            .unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }
}

fn key() -> String {
    ulid::Ulid::generate().to_string()
}

fn group(name: &str) -> Value {
    json!({"name": name, "kind": "pc", "colour": "teal"})
}

#[tokio::test]
async fn a_setup_command_is_stored_pending_and_a_repeat_key_returns_the_same_row() {
    let h = Harness::new().await;
    let (a, branch) = h.ready().await;
    let _ = h.paired_hub(&a).await;
    let k = key();
    let first = h.submit(&branch, &k, "group.upsert", group("Row A")).await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(first.body["status"], "pending");
    let again = h.submit(&branch, &k, "group.upsert", group("Row B")).await;
    assert_eq!(again.body["id"], first.body["id"]);
    let next = h
        .submit(&branch, &key(), "group.upsert", group("Row C"))
        .await;
    assert!(next.body["id"].as_i64() > first.body["id"].as_i64());
    let c = rusqlite::Connection::open(h.dir.path().join("gz_control.db")).unwrap();
    let (rows, payload): (i64, String) = c
        .query_row(
            "SELECT COUNT(*), (SELECT payload FROM commands WHERE id = ?) FROM commands",
            [first.body["id"].as_i64().unwrap()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(rows, 2);
    assert!(payload.contains("Row A"), "{payload}");
}

#[tokio::test]
async fn only_setup_kinds_with_a_valid_key_and_payload_are_accepted() {
    let h = Harness::new().await;
    let (_a, branch) = h.ready().await;
    for kind in [
        "branch.bootstrap",
        "pricing.set",
        "product.upsert",
        "device.revoke",
    ] {
        let r = h.submit(&branch, &key(), kind, json!({})).await;
        assert_eq!((r.status, r.code()), (400, "UNSUPPORTED"), "{kind}");
    }
    for kind in [
        "group.upsert",
        "device.assign",
        "device.register",
        "staff.upsert",
        "setup.test_session",
        "branch.go_live",
    ] {
        let r = h.submit(&branch, &key(), kind, json!({})).await;
        assert_eq!(r.status, 200, "{kind}: {}", r.body);
    }
    let bad_key = h
        .submit(&branch, "not-a-ulid", "group.upsert", json!({}))
        .await;
    assert_eq!((bad_key.status, bad_key.code()), (422, "VALIDATION_FAILED"));
    let not_object = h.submit(&branch, &key(), "group.upsert", json!([1])).await;
    assert_eq!(not_object.body["field"], "payload");
    let big = h
        .submit(
            &branch,
            &key(),
            "group.upsert",
            json!({"name": "x".repeat(20_000)}),
        )
        .await;
    assert_eq!(big.body["field"], "payload");
    for (key_name, want) in [
        ("branch.tz", 200),
        ("branch.name", 200),
        ("branch.currency", 422),
        ("kiosk.volume_default", 422),
    ] {
        let r = h
            .submit(
                &branch,
                &key(),
                "settings.set",
                json!({"key": key_name, "value": "x"}),
            )
            .await;
        assert_eq!(r.status, want, "{key_name}: {}", r.body);
    }
    let no_key = h
        .submit(&branch, &key(), "settings.set", json!({"value": "x"}))
        .await;
    assert_eq!(no_key.status, 422);
}

#[tokio::test]
async fn the_hub_fetches_open_commands_in_id_order_and_a_heartbeat_pokes_when_some_are_pending() {
    let h = Harness::new().await;
    let (a, branch) = h.ready().await;
    let (token, _) = h.paired_hub(&a).await;
    h.advance(1_000);
    let quiet = h.beat(&token, "1:HUBA").await;
    assert!(
        quiet.body.get("pending_commands").is_none(),
        "{}",
        quiet.body
    );
    let mut ids = Vec::new();
    for n in 0..3 {
        let r = h
            .submit(&branch, &key(), "group.upsert", group(&format!("G{n}")))
            .await;
        ids.push(r.body["id"].as_i64().unwrap());
    }
    h.advance(5_000);
    let poke = h.beat(&token, "1:HUBA").await;
    assert_eq!(poke.body["pending_commands"], 3);
    let page = h.fetch_commands(&token, 0).await;
    assert_eq!(page.status, 200, "{}", page.body);
    let got: Vec<i64> = page.body["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_i64().unwrap())
        .collect();
    assert_eq!(got, ids);
    let first = &page.body["commands"][0];
    assert_eq!(first["kind"], "group.upsert");
    assert_eq!(first["schema_version"], 1);
    assert_eq!(first["issued_by"], "owner-1");
    assert_eq!(first["payload"]["name"], "G0");
    assert_eq!(first["idempotency_key"].as_str().unwrap().len(), 26);
    h.advance(5_000);
    let after_fetch = h.beat(&token, "1:HUBA").await;
    assert!(after_fetch.body.get("pending_commands").is_none());
    let again = h.fetch_commands(&token, ids[0]).await;
    assert_eq!(again.body["commands"].as_array().unwrap().len(), 2);
    let none = h.fetch_commands(&token, ids[2]).await;
    assert!(none.body["commands"].as_array().unwrap().is_empty());
    let c = rusqlite::Connection::open(h.dir.path().join("gz_control.db")).unwrap();
    let fetched: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM commands WHERE status = 'fetched'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(fetched, 3);
}

#[tokio::test]
async fn a_result_resolves_the_command_once_and_a_repeat_changes_nothing() {
    let h = Harness::new().await;
    let (a, branch) = h.ready().await;
    let (token, _) = h.paired_hub(&a).await;
    let ok = h
        .submit(&branch, &key(), "group.upsert", group("G"))
        .await
        .body["id"]
        .as_i64()
        .unwrap();
    let bad = h
        .submit(&branch, &key(), "device.assign", json!({}))
        .await
        .body["id"]
        .as_i64()
        .unwrap();
    h.fetch_commands(&token, 0).await;
    let applied = h
        .command_result(&token, ok, json!({"epoch": "1:HUBA", "applied_seq": 41}))
        .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(applied.body["status"], "applied");
    let repeat = h
        .command_result(&token, ok, json!({"epoch": "1:HUBA", "applied_seq": 41}))
        .await;
    assert_eq!(repeat.body["status"], "applied");
    let conflicting = h
        .command_result(
            &token,
            ok,
            json!({"epoch": "1:HUBA", "error": {"code": "STALE", "message": "no"}}),
        )
        .await;
    assert_eq!(conflicting.body["status"], "applied");
    let rejected = h
        .command_result(
            &token,
            bad,
            json!({"epoch": "1:HUBA", "error": {"code": "VALIDATION_FAILED", "message": "no group", "field": "group_id"}}),
        )
        .await;
    assert_eq!(rejected.body["status"], "rejected");
    let c = rusqlite::Connection::open(h.dir.path().join("gz_control.db")).unwrap();
    let rows: Vec<(i64, String, Option<i64>, Option<String>)> = c
        .prepare("SELECT id, status, applied_seq, error FROM commands ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(rows[0], (ok, "applied".into(), Some(41), None));
    assert_eq!(rows[1].1, "rejected");
    assert!(rows[1].3.as_deref().unwrap().contains("VALIDATION_FAILED"));
    assert!(
        h.fetch_commands(&token, 0).await.body["commands"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_hub_reaches_only_its_own_branchs_commands() {
    let h = Harness::new().await;
    let (a, branch) = h.ready().await;
    let (token, _) = h.paired_hub(&a).await;
    let other_owner = h.owner("+919876543299", "Other Arena").await;
    let other = h.tier_done(&other_owner).await;
    let theirs = h
        .submit(&other, &key(), "group.upsert", group("Theirs"))
        .await
        .body["id"]
        .as_i64()
        .unwrap();
    let mine = h
        .submit(&branch, &key(), "group.upsert", group("Mine"))
        .await
        .body["id"]
        .as_i64()
        .unwrap();
    let page = h.fetch_commands(&token, 0).await;
    let ids: Vec<i64> = page.body["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, vec![mine]);
    let r = h
        .command_result(&token, theirs, json!({"epoch": "1:HUBA", "applied_seq": 1}))
        .await;
    assert_eq!((r.status, r.code()), (404, "NOT_FOUND"));
    let c = rusqlite::Connection::open(h.dir.path().join("gz_control.db")).unwrap();
    let status: String = c
        .query_row("SELECT status FROM commands WHERE id = ?", [theirs], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(status, "pending");
}

#[tokio::test]
async fn the_mailbox_needs_a_valid_hub_token_and_an_epoch() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let (token, _) = h.paired_hub(&a).await;
    let none = Request::builder()
        .method("GET")
        .uri("/v1/commands?epoch=1:HUBA")
        .body(Body::empty())
        .unwrap();
    let r = reply(h.app.clone().oneshot(none).await.unwrap()).await;
    assert_eq!((r.status, r.code()), (401, "TOKEN_INVALID"));
    let forged = h.fetch_commands("hub_nope.nope", 0).await;
    assert_eq!((forged.status, forged.code()), (401, "TOKEN_INVALID"));
    let no_epoch = Request::builder()
        .method("GET")
        .uri("/v1/commands")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let r = reply(h.app.clone().oneshot(no_epoch).await.unwrap()).await;
    assert_eq!(r.status, 400);
    let blank = h
        .command_result(&token, 1, json!({"epoch": "", "applied_seq": 1}))
        .await;
    assert_eq!((blank.status, blank.code()), (422, "VALIDATION_FAILED"));
}

#[tokio::test]
async fn the_command_role_serves_the_mailbox_and_nothing_else() {
    let h = Harness::new().await;
    let (a, branch) = h.ready().await;
    let (token, _) = h.paired_hub(&a).await;
    h.submit(&branch, &key(), "group.upsert", group("G")).await;
    let (_tx, rx) = watch::channel(Reachability {
        reachable: true,
        checked_at: None,
    });
    let app = app_with_auth(
        Role::Command,
        false,
        rx,
        LATEST_CONTROL_VERSION,
        Some(Arc::clone(&h.st)),
    );
    let get = |uri: &str| {
        Request::builder()
            .method("GET")
            .uri(uri.to_owned())
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap()
    };
    let mail = reply(
        app.clone()
            .oneshot(get("/v1/commands?epoch=1:HUBA"))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(mail.status, 200, "{}", mail.body);
    assert_eq!(mail.body["commands"].as_array().unwrap().len(), 1);
    let health = reply(app.clone().oneshot(get("/healthz")).await.unwrap()).await;
    assert_eq!(health.body["role"], "command");
    let beat = Request::builder()
        .method("POST")
        .uri("/v1/heartbeat")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let r = reply(app.oneshot(beat).await.unwrap()).await;
    assert_eq!((r.status, r.code()), (404, "NOT_FOUND"));
}

impl Harness {
    async fn invite(&self, a: &str, name: &str, phone: &str, role: &str) -> Reply {
        let body = json!({"name": name, "phone": phone, "role": role});
        self.call("POST", "/v1/wizard/staff", Some(body), a).await
    }

    async fn step(&self, a: &str, step: &str) -> String {
        let wizard = self.call("GET", "/v1/wizard", None, a).await;
        wizard.body["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["key"] == step)
            .unwrap()["state"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    async fn paired_and_seen(&self, a: &str) -> String {
        let (token, _) = self.paired_hub(a).await;
        self.advance(1_000);
        assert_eq!(self.beat(&token, "1:HUBA").await.status, 200);
        token
    }
}

#[tokio::test]
async fn an_invite_queues_a_hash_only_staff_command_and_shows_the_code_once() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let early = h.invite(&a, "Neha", "+919876543211", "manager").await;
    assert_eq!((early.status, early.code()), (409, "STEP_NOT_READY"));
    let token = h.paired_and_seen(&a).await;

    let made = h.invite(&a, "Neha", "+919876543211", "manager").await;
    assert_eq!(made.status, 200, "{}", made.body);
    let code = made.body["code"].as_str().unwrap().to_owned();
    assert_eq!(code.len(), 8);
    assert_eq!(made.body["invite"]["status"], "waiting");
    assert_eq!(made.body["invite"]["phone"], "+919876543211");
    assert_eq!(
        made.body["invite"]["setup_expires_at"],
        h.now.load(Ordering::SeqCst) + 72 * 3_600_000
    );

    let page = h.fetch_commands(&token, 0).await;
    let cmds = page.body["commands"].as_array().unwrap();
    assert_eq!(cmds.len(), 1);
    let (kind, payload) = (&cmds[0]["kind"], &cmds[0]["payload"]);
    assert_eq!(kind, "staff.upsert");
    assert_eq!(payload["staff_id"], made.body["invite"]["id"]);
    assert_eq!(payload["login"], "+919876543211");
    assert_eq!(payload["role_id"], "Manager");
    assert!(
        payload["setup_code_hash"]
            .as_str()
            .unwrap()
            .starts_with("$argon2id$")
    );
    assert!(!payload.to_string().contains(&code));
    assert!(
        !h.call("GET", "/v1/wizard/staff", None, &a)
            .await
            .body
            .to_string()
            .contains(&code)
    );
}

#[tokio::test]
async fn invite_details_are_checked() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    h.paired_and_seen(&a).await;
    for (name, phone, field) in [
        ("", "+919876543211", "name"),
        ("Neha", "9876543211", "phone"),
        ("Neha", "+911234567890", "phone"),
    ] {
        let r = h.invite(&a, name, phone, "counter").await;
        assert_eq!((r.status, r.body["field"].as_str()), (422, Some(field)));
    }
    let bad_role = h.invite(&a, "Neha", "+919876543211", "owner").await;
    assert!(bad_role.status >= 400);
}

#[tokio::test]
async fn an_applied_manager_invite_finishes_step_seven_and_a_counter_one_does_not() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let token = h.paired_and_seen(&a).await;
    let counter = h.invite(&a, "Ravi", "+919876543212", "counter").await;
    let manager = h.invite(&a, "Neha", "+919876543211", "manager").await;
    let page = h.fetch_commands(&token, 0).await;
    let ids: Vec<i64> = page.body["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_i64().unwrap())
        .collect();
    let applied = json!({"epoch": "1:HUBA", "applied_seq": 5});
    h.command_result(&token, ids[0], applied.clone()).await;
    assert_eq!(h.step(&a, "staff").await, "pending");
    h.command_result(&token, ids[1], applied).await;
    assert_eq!(h.step(&a, "staff").await, "done");
    let list = h.call("GET", "/v1/wizard/staff", None, &a).await;
    let statuses: Vec<&str> = list.body["invites"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, ["applied", "applied"]);
    assert_ne!(counter.body["invite"]["id"], manager.body["invite"]["id"]);
}

#[tokio::test]
async fn a_refused_invite_shows_the_hub_s_reason_and_a_new_code_goes_out_by_a_new_command() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let token = h.paired_and_seen(&a).await;
    let made = h.invite(&a, "Neha", "+919876543211", "manager").await;
    let id = made.body["invite"]["id"].as_str().unwrap().to_owned();
    let first = h.fetch_commands(&token, 0).await.body["commands"][0].clone();
    let refusal = json!({"epoch": "1:HUBA", "error": {"code": "LOGIN_TAKEN", "message": "that login is already in use", "field": "login"}});
    h.command_result(&token, first["id"].as_i64().unwrap(), refusal)
        .await;
    let list = h.call("GET", "/v1/wizard/staff", None, &a).await;
    assert_eq!(list.body["invites"][0]["status"], "failed");
    assert_eq!(list.body["invites"][0]["error"]["code"], "LOGIN_TAKEN");
    assert_eq!(h.step(&a, "staff").await, "pending");

    let fresh = h
        .call(
            "POST",
            &format!("/v1/wizard/staff/{id}/code"),
            Some(json!({})),
            &a,
        )
        .await;
    assert_eq!(fresh.status, 200, "{}", fresh.body);
    assert_ne!(fresh.body["code"], made.body["code"]);
    assert_eq!(fresh.body["invite"]["status"], "waiting");
    let page = h.fetch_commands(&token, 0).await;
    let next = &page.body["commands"][0];
    assert_ne!(next["id"], first["id"]);
    assert_eq!(next["payload"]["staff_id"], id.as_str());
    assert_ne!(
        next["payload"]["setup_code_hash"],
        first["payload"]["setup_code_hash"]
    );
    let missing = h
        .call(
            "POST",
            "/v1/wizard/staff/01JMISSING000000000000000/code",
            Some(json!({})),
            &a,
        )
        .await;
    assert_eq!(missing.status, 404);
}

#[tokio::test]
async fn skipping_marks_step_seven_skipped_and_a_manager_invite_can_still_finish_it() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let token = h.paired_and_seen(&a).await;
    let skipped = h
        .call("POST", "/v1/wizard/staff/skip", Some(json!({})), &a)
        .await;
    assert_eq!(skipped.status, 200, "{}", skipped.body);
    assert_eq!(h.step(&a, "staff").await, "skipped");
    h.invite(&a, "Neha", "+919876543211", "manager").await;
    let id = h.fetch_commands(&token, 0).await.body["commands"][0]["id"]
        .as_i64()
        .unwrap();
    h.command_result(&token, id, json!({"epoch": "1:HUBA", "applied_seq": 5}))
        .await;
    assert_eq!(h.step(&a, "staff").await, "done");
    h.call("POST", "/v1/wizard/staff/skip", Some(json!({})), &a)
        .await;
    assert_eq!(h.step(&a, "staff").await, "done");
}

impl Harness {
    async fn beat_with(&self, token: &str, setup: Value) -> Reply {
        let req = Request::builder()
            .method("POST")
            .uri("/v1/heartbeat")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(
                json!({"epoch": "1:HUBA", "version": "0.2.0", "schema_version": 13, "last_seq_uploaded": 0, "setup": setup})
                    .to_string(),
            ))
            .unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }
}

fn pc(group_id: Value) -> Value {
    json!({"id": "D1", "label": "PC-1", "kind": "pc", "group_id": group_id, "status": "online"})
}

#[tokio::test]
async fn a_setup_block_is_stored_and_a_grouped_device_finishes_step_four() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let early = h.call("GET", "/v1/wizard/devices", None, &a).await;
    assert_eq!((early.status, early.code()), (409, "STEP_NOT_READY"));
    let token = h.paired_and_seen(&a).await;
    let none = h.call("GET", "/v1/wizard/devices", None, &a).await;
    assert_eq!(none.body["setup"], Value::Null);
    assert!(none.body["hub_online"].as_bool().unwrap());

    h.advance(6_000);
    let setup = json!({"devices": [pc(Value::Null)], "groups": []});
    assert_eq!(h.beat_with(&token, setup.clone()).await.status, 200);
    let seen = h.call("GET", "/v1/wizard/devices", None, &a).await;
    assert_eq!(seen.body["setup"], setup);
    assert_eq!(h.step(&a, "devices").await, "pending");

    h.advance(6_000);
    let grouped = json!({
        "devices": [pc(json!("G1"))],
        "groups": [{"id": "G1", "name": "Gaming", "kind": "pc", "colour": "teal", "sort": 0, "has_rate": false}],
    });
    assert_eq!(h.beat_with(&token, grouped.clone()).await.status, 200);
    assert_eq!(h.step(&a, "devices").await, "done");
    let again = h.call("GET", "/v1/wizard/devices", None, &a).await;
    assert_eq!(again.body["setup"], grouped);

    h.advance(4 * 60 * 1000);
    let stale = h.call("GET", "/v1/wizard/devices", None, &a).await;
    assert!(!stale.body["hub_online"].as_bool().unwrap());
}

fn rated_group(rate: bool) -> Value {
    json!({"id": "G1", "name": "Gaming", "kind": "pc", "colour": "teal", "sort": 0, "has_rate": rate})
}

fn setup_status(printer: &str) -> Value {
    json!({"opening_hours_set": false, "backup_location_set": false, "ad_consent": "missing", "printer": printer, "products_count": 0, "groups_without_rate": 0, "manager_exists": false, "test_session": "unset"})
}

#[tokio::test]
async fn rates_and_printer_steps_complete_from_the_setup_block() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let token = h.paired_and_seen(&a).await;
    let beat = |groups: Vec<Value>, printer: Option<&str>| {
        h.advance(6_000);
        let mut b = json!({"devices": [], "groups": groups});
        if let Some(p) = printer {
            b["status"] = setup_status(p);
        }
        h.beat_with(&token, b)
    };
    beat(vec![], Some("unset")).await;
    assert_eq!(h.step(&a, "rates").await, "pending");
    beat(vec![rated_group(false), rated_group(true)], Some("failed")).await;
    assert_eq!(h.step(&a, "rates").await, "pending");
    assert_eq!(h.step(&a, "printer_payments").await, "pending");
    beat(vec![rated_group(true)], Some("ok")).await;
    assert_eq!(h.step(&a, "rates").await, "done");
    assert_eq!(h.step(&a, "printer_payments").await, "done");
    beat(vec![rated_group(false)], Some("failed")).await;
    assert_eq!(h.step(&a, "rates").await, "done");
    assert_eq!(h.step(&a, "printer_payments").await, "done");
}

#[tokio::test]
async fn the_test_session_step_completes_only_on_passed_and_never_reverts() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let token = h.paired_and_seen(&a).await;
    let beat = |ts: Option<&str>| {
        h.advance(6_000);
        let mut b = json!({"devices": [], "groups": []});
        if let Some(t) = ts {
            let mut status = setup_status("unset");
            status["test_session"] = t.into();
            b["status"] = status;
        }
        h.beat_with(&token, b)
    };
    for ts in [None, Some("unset"), Some("running"), Some("failed")] {
        assert_eq!(beat(ts).await.status, 200);
        assert_eq!(h.step(&a, "test_session").await, "pending", "{ts:?}");
    }
    let old = json!({"devices": [], "groups": [], "status": {"opening_hours_set": false, "backup_location_set": false, "ad_consent": "missing", "printer": "unset"}});
    h.advance(6_000);
    assert_eq!(h.beat_with(&token, old).await.status, 200);
    assert_eq!(h.step(&a, "test_session").await, "pending");
    beat(Some("passed")).await;
    assert_eq!(h.step(&a, "test_session").await, "done");
    for ts in [Some("failed"), Some("running"), Some("unset"), None] {
        beat(ts).await;
        assert_eq!(h.step(&a, "test_session").await, "done", "{ts:?}");
    }
}

#[tokio::test]
async fn a_test_session_command_is_queued_and_shows_its_rejection() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let token = h.paired_and_seen(&a).await;
    let body = json!({"kind": "setup.test_session", "payload": {"device_id": "D1"}});
    let made = h.call("POST", "/v1/wizard/devices", Some(body), &a).await;
    assert_eq!(made.status, 200, "{}", made.body);
    assert_eq!(made.body["status"], "pending");
    let page = h.fetch_commands(&token, 0).await;
    let id = page.body["commands"][0]["id"].as_i64().unwrap();
    let error = json!({"code": "VALIDATION_FAILED", "message": "the device is offline", "field": "device_id"});
    h.command_result(&token, id, json!({"epoch": "1:HUBA", "error": error}))
        .await;
    let view = h.call("GET", "/v1/wizard/devices", None, &a).await;
    let cmd = &view.body["commands"][0];
    assert_eq!(cmd["kind"], "setup.test_session");
    assert_eq!(cmd["status"], "rejected");
    assert_eq!(cmd["error"]["message"], "the device is offline");
}

#[tokio::test]
async fn an_old_hub_block_completes_nothing_new_and_a_skipped_printer_step_stays() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let token = h.paired_and_seen(&a).await;
    h.advance(6_000);
    let old = json!({"devices": [], "groups": [{"id": "G1", "name": "Gaming", "kind": "pc", "colour": "teal", "sort": 0}]});
    assert_eq!(h.beat_with(&token, old).await.status, 200);
    assert_eq!(h.step(&a, "rates").await, "pending");
    assert_eq!(h.step(&a, "printer_payments").await, "pending");
    let r = h
        .call(
            "POST",
            "/v1/wizard/printer-payments/skip",
            Some(json!({})),
            &a,
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(h.step(&a, "printer_payments").await, "skipped");
    h.advance(6_000);
    let b = json!({"devices": [], "groups": [], "status": setup_status("skipped")});
    h.beat_with(&token, b).await;
    assert_eq!(h.step(&a, "printer_payments").await, "skipped");
}

#[tokio::test]
async fn skipping_products_marks_it_skipped_and_refuses_early_or_anonymous_callers() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let early = h
        .call("POST", "/v1/wizard/products/skip", Some(json!({})), &a)
        .await;
    assert_eq!((early.status, early.code()), (409, "STEP_NOT_READY"));
    h.paired_and_seen(&a).await;
    let r = h
        .call("POST", "/v1/wizard/products/skip", Some(json!({})), &a)
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(h.step(&a, "products").await, "skipped");
    let anon = h
        .call("POST", "/v1/wizard/products/skip", Some(json!({})), "")
        .await;
    assert_eq!(anon.status, 401);
}

#[tokio::test]
async fn a_malformed_or_oversized_setup_block_is_ignored_or_refused() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let token = h.paired_and_seen(&a).await;
    h.advance(6_000);
    let many: Vec<Value> = (0..501).map(|_| pc(Value::Null)).collect();
    let big = h
        .beat_with(&token, json!({"devices": many, "groups": []}))
        .await;
    assert_eq!(big.status, 200);
    let view = h.call("GET", "/v1/wizard/devices", None, &a).await;
    assert_eq!(view.body["setup"], Value::Null);
    h.advance(6_000);
    let bad = h.beat_with(&token, json!({"devices": "x"})).await;
    assert_eq!(bad.status, 400);
}

#[tokio::test]
async fn a_device_command_waits_until_the_hub_reports_and_a_refusal_shows_its_reason() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let token = h.paired_and_seen(&a).await;
    let post = |kind: &str, payload: Value| {
        let body = json!({"kind": kind, "payload": payload});
        h.call("POST", "/v1/wizard/devices", Some(body), &a)
    };
    let made = post(
        "group.upsert",
        json!({"name": "Gaming", "group_kind": "pc", "colour": "teal", "sort_order": 0}),
    )
    .await;
    assert_eq!(made.status, 200, "{}", made.body);
    assert_eq!(made.body["status"], "pending");
    let assign = post(
        "device.assign",
        json!({"device_id": "D1", "group_id": "G1"}),
    )
    .await;
    let refused = post("staff.upsert", json!({})).await;
    assert_eq!((refused.status, refused.code()), (422, "VALIDATION_FAILED"));

    let view = h.call("GET", "/v1/wizard/devices", None, &a).await;
    let kinds: Vec<&str> = view.body["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["group.upsert", "device.assign"]);

    let page = h.fetch_commands(&token, 0).await;
    let ids: Vec<i64> = page.body["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_i64().unwrap())
        .collect();
    h.command_result(&token, ids[0], json!({"epoch": "1:HUBA", "applied_seq": 3}))
        .await;
    let error = json!({"code": "GROUP_KIND_MISMATCH", "message": "That group is for another kind of device."});
    h.command_result(&token, ids[1], json!({"epoch": "1:HUBA", "error": error}))
        .await;
    let view = h.call("GET", "/v1/wizard/devices", None, &a).await;
    let cmds = view.body["commands"].as_array().unwrap();
    assert_eq!(cmds.len(), 2);
    assert_eq!(cmds[0]["status"], "applied");
    assert_eq!(cmds[1]["status"], "rejected");
    assert_eq!(cmds[1]["error"]["code"], "GROUP_KIND_MISMATCH");
    assert_eq!(cmds[1]["key"], assign.body["key"]);

    h.advance(6_000);
    h.beat_with(&token, json!({"devices": [], "groups": []}))
        .await;
    let view = h.call("GET", "/v1/wizard/devices", None, &a).await;
    let cmds = view.body["commands"].as_array().unwrap();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0]["status"], "rejected");

    h.advance(6 * 60 * 1000);
    let view = h.call("GET", "/v1/wizard/devices", None, &a).await;
    assert!(view.body["commands"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn go_live_needs_every_required_item_then_queues_one_command() {
    let h = Harness::new().await;
    let (a, _) = h.ready().await;
    let token = h.paired_and_seen(&a).await;
    let status = |hours: bool, backup: bool, ads: &str| {
        let mut s = setup_status("ok");
        s["opening_hours_set"] = hours.into();
        s["backup_location_set"] = backup.into();
        s["ad_consent"] = ads.into();
        s["test_session"] = "passed".into();
        s
    };
    let beat = |s: Value| {
        h.advance(6_000);
        h.beat_with(
            &token,
            json!({"devices": [pc(json!("G1"))], "groups": [rated_group(true)], "status": s}),
        )
    };
    beat(status(false, false, "missing")).await;
    let early = h
        .call("POST", "/v1/wizard/go-live", Some(json!({})), &a)
        .await;
    assert_eq!((early.status, early.code()), (422, "VALIDATION_FAILED"));
    for kind in ["products", "staff"] {
        let path = format!("/v1/wizard/{kind}/skip");
        let r = h.call("POST", &path, Some(json!({})), &a).await;
        assert_eq!(r.status, 200, "{kind}: {}", r.body);
    }
    for (s, first) in [
        (status(false, true, "given"), "opening_hours"),
        (status(true, false, "given"), "backup_location"),
        (status(true, true, "missing"), "ad_consent"),
    ] {
        beat(s).await;
        let view = h.call("GET", "/v1/wizard/go-live", None, &a).await;
        assert_eq!(view.body["blocker"], json!({"item": first}), "{first}");
        let r = h
            .call("POST", "/v1/wizard/go-live", Some(json!({})), &a)
            .await;
        assert_eq!(r.status, 422, "{first}");
    }
    beat(status(true, true, "given")).await;
    let view = h.call("GET", "/v1/wizard/go-live", None, &a).await;
    assert_eq!(
        (view.body["blocker"].clone(), view.body["live"].clone()),
        (Value::Null, json!(false))
    );
    let standby = view.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["key"] == "standby")
        .unwrap();
    assert_eq!(standby["done"], false);
    for _ in 0..2 {
        let r = h
            .call("POST", "/v1/wizard/go-live", Some(json!({})), &a)
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
    }
    assert_eq!(h.step(&a, "go_live").await, "done");
    let page = h.fetch_commands(&token, 0).await;
    let kinds: Vec<_> = page.body["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["kind"].as_str().unwrap().to_owned())
        .filter(|k| k == "branch.go_live")
        .collect();
    assert_eq!(kinds.len(), 1);
    let view = h.call("GET", "/v1/wizard/go-live", None, &a).await;
    assert_eq!(view.body["live"], true);
}

#[tokio::test]
async fn a_paid_branch_goes_live_without_ad_consent_and_a_live_one_stays_live() {
    let h = Harness::new().await;
    let (a, branch) = h.ready().await;
    h.db.grant_comp(&CompGrant {
        branch_id: &branch,
        tier: Tier::Basic,
        until: T0 + 30 * 24 * 60 * 60 * 1000,
        audit_id: "audit-1",
        operator: "ops",
        reason: "beta cafe",
        details_json: "{}",
        now: T0,
    })
    .await
    .unwrap();
    let token = h.paired_and_seen(&a).await;
    let mut s = setup_status("ok");
    s["opening_hours_set"] = true.into();
    s["backup_location_set"] = true.into();
    s["test_session"] = "passed".into();
    h.advance(6_000);
    h.beat_with(
        &token,
        json!({"devices": [pc(json!("G1"))], "groups": [rated_group(true)], "status": s}),
    )
    .await;
    for kind in ["products", "staff"] {
        let path = format!("/v1/wizard/{kind}/skip");
        h.call("POST", &path, Some(json!({})), &a).await;
    }
    let view = h.call("GET", "/v1/wizard/go-live", None, &a).await;
    assert_eq!(view.body["free"], false);
    assert_eq!(view.body["blocker"], Value::Null);
    assert!(
        view.body["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["key"] != "ad_consent")
    );
    let r = h
        .call("POST", "/v1/wizard/go-live", Some(json!({})), &a)
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    h.advance(6_000);
    h.beat_with(
        &token,
        json!({"devices": [pc(json!("G1"))], "groups": [rated_group(true)], "status": setup_status("ok")}),
    )
    .await;
    let r = h
        .call("POST", "/v1/wizard/go-live", Some(json!({})), &a)
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
}
