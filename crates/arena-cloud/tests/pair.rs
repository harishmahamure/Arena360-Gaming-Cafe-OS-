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
    assert_eq!(state.body["hub_paired"], true);
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
