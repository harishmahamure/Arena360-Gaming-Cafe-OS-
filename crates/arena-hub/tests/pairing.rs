#![cfg(feature = "dev-fixtures")]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command};
use std::sync::Arc;
use std::time::{Duration, Instant};

use arena_cloud::auth::AuthState;
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
use arena_cloud::pairing_code::new_pairing_code;
use arena_cloud::{Clock, app_with_auth, auth_state};
use arena_proto::licence::{DEV_LICENCE_VERIFY_KEYS, verify};
use arena_proto::{PAIRING_CODE_TTL_MS, Tier};
use arena_store::control::{
    BranchProfile, ControlDb, LATEST_CONTROL_VERSION, NewOtpChallenge, NewOwner, NewOwnerSession,
    NewPairingCode, NewTenantBranch, SignupCompletion,
};
use serde_json::{Value, json};
use tokio::sync::watch;

const PEPPER: &str = "test-pepper-0123456789abcdef0123456789";

fn hub() -> Command {
    Command::new(env!("CARGO_BIN_EXE_arena-hub"))
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn call(port: u16, method: &str, path: &str, body: Option<Value>) -> Option<(u16, Value)> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(30))).ok()?;
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    let (head, payload) = out.split_once("\r\n\r\n")?;
    let status = head.split_whitespace().nth(1)?.parse().ok()?;
    Some((status, serde_json::from_str(payload).unwrap_or(Value::Null)))
}

async fn request(
    port: u16,
    method: &'static str,
    path: &'static str,
    body: Option<Value>,
) -> (u16, Value) {
    tokio::task::spawn_blocking(move || call(port, method, path, body))
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("{method} {path} got no answer"))
}

struct Running(Option<Child>);

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Running {
    fn stop(mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

async fn start_hub(data: &Path, cloud: &str, platform: &str) -> (Running, u16) {
    let (health, lan, web) = (free_port(), free_port(), free_port());
    let child = hub()
        .arg("--data-dir")
        .arg(data)
        .args(["--health-port", &health.to_string()])
        .args(["--lan-port", &lan.to_string()])
        .args(["--web-port", &web.to_string()])
        .args(["--cloud-host", cloud])
        .args(["--platform", platform])
        .args(["--probe-internet", "127.0.0.1:9"])
        .spawn()
        .unwrap();
    let running = Running(Some(child));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let up = tokio::task::spawn_blocking(move || call(health, "GET", "/healthz", None))
            .await
            .unwrap();
        if let Some((_, b)) = up.filter(|(_, b)| b["role"] == "primary") {
            assert_eq!(b["platform"], platform);
            break;
        }
        assert!(Instant::now() < deadline, "the hub never became primary");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    (running, health)
}

struct Cloud {
    db: Arc<ControlDb>,
    st: Arc<AuthState>,
    url: String,
    _dir: tempfile::TempDir,
}

async fn start_cloud() -> Cloud {
    let dir = tempfile::tempdir().unwrap();
    let db = ControlDb::open_local(&dir.path().join("gz_control.db"))
        .await
        .unwrap();
    db.migrate(now_ms()).await.unwrap();
    let db = Arc::new(db);
    let clock: Clock = Arc::new(now_ms);
    let cfg = AuthConfig {
        pepper: PEPPER.as_bytes().to_vec(),
        web_origins: vec![],
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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    Cloud {
        db,
        st,
        url,
        _dir: dir,
    }
}

impl Cloud {
    async fn branch_at_step_3(&self) -> String {
        let owner = NewOwner {
            name: "Asha Rao".into(),
            phone_e164: "+919876543210".into(),
            email: "asha@example.in".into(),
            cafe_name: "Pixel Arena Gaming".into(),
            city: "Pune".into(),
        };
        let now = now_ms();
        self.db
            .insert_otp_challenge(&NewOtpChallenge {
                id: "c1".into(),
                phone_e164: owner.phone_e164.clone(),
                code_hash: "h".into(),
                payload: owner.clone(),
                created_at: now,
                expires_at: now + 300_000,
            })
            .await
            .unwrap();
        self.db
            .complete_signup(&SignupCompletion {
                challenge_id: "c1",
                code_hash: "h",
                max_attempts: 5,
                now,
                new_owner_id: "o1",
                owner: &owner,
                session: &NewOwnerSession {
                    id: "s1".into(),
                    owner_id: String::new(),
                    family_id: "f1".into(),
                    access_hash: "a1".into(),
                    access_expires_at: now + 900_000,
                    refresh_hash: "r1".into(),
                    refresh_expires_at: now + 9_000_000,
                    created_at: now,
                    device_id: None,
                },
            })
            .await
            .unwrap();
        let profile = BranchProfile {
            name: "Pixel Arena Gaming".into(),
            city: "Pune".into(),
            country: "IN".into(),
            currency: "INR".into(),
            timezone: "Asia/Kolkata".into(),
            gst_registered: false,
            gstin: None,
        };
        assert!(
            self.db
                .create_tenant_with_branch(&NewTenantBranch {
                    tenant_id: "t1",
                    owner_id: "o1",
                    tenant_name: "Pixel Arena Gaming",
                    branch_id: "b1",
                    branch_code: "PAG",
                    profile: &profile,
                    now,
                })
                .await
                .unwrap()
        );
        self.db.choose_free_tier("t1", "b1", now).await.unwrap();
        "b1".to_owned()
    }

    async fn code(&self, branch_id: &str) -> String {
        let code = new_pairing_code().unwrap();
        let hash = self.st.pairing_code_hash(&code);
        let now = now_ms();
        let id = ulid::Ulid::generate().to_string();
        self.db
            .issue_pairing_code(&NewPairingCode {
                id: &id,
                branch_id,
                code_hash: &hash,
                now,
                expires_at: now + PAIRING_CODE_TTL_MS,
            })
            .await
            .unwrap();
        code
    }

    async fn pair_attempts(&self) -> u64 {
        self.db
            .rate_buckets("pair_ip:127.0.0.1")
            .await
            .unwrap()
            .iter()
            .map(|(_, n)| n)
            .sum()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_fresh_hub_pairs_with_the_step_3_code_and_a_restart_skips_pairing() {
    let cloud = start_cloud().await;
    let branch = cloud.branch_at_step_3().await;
    let code = cloud.code(&branch).await;
    let data = tempfile::tempdir().unwrap();
    let (hub, port) = start_hub(data.path(), &cloud.url, "android").await;

    let (s, snap) = request(port, "GET", "/ui/snapshot", None).await;
    assert_eq!(s, 200);
    assert_eq!(snap["paired"], false, "{snap}");
    assert_eq!(snap["branch"], Value::Null);
    assert_eq!(snap["lan"], Value::Null);
    {
        let store = arena_store::Store::open_reader(data.path().join("branch.db")).unwrap();
        assert!(store.branch_summary().unwrap().is_none());
        assert!(store.rows(arena_store::Table::Staff).unwrap().is_empty());
    }
    for path in ["/ui/me", "/ui/settings", "/ui/audit"] {
        let (s, body) = tokio::task::spawn_blocking(move || call(port, "GET", path, None))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (s, body["code"].as_str()),
            (409, Some("HUB_NOT_PAIRED")),
            "{path}"
        );
    }
    let (s, body) = request(
        port,
        "POST",
        "/ui/auth/sign-in",
        Some(json!({"login": "owner", "password": "x"})),
    )
    .await;
    assert_eq!((s, body["code"].as_str()), (409, Some("HUB_NOT_PAIRED")));

    let (s, body) = request(
        port,
        "POST",
        "/ui/pair-hub",
        Some(json!({"code": "ZZZZ ZZZZ"})),
    )
    .await;
    assert_eq!(
        (s, body["code"].as_str()),
        (401, Some("PAIR_CODE_INVALID")),
        "{body}"
    );

    let typed = format!("{} {}", &code[..4], &code[4..]).to_lowercase();
    let (s, body) = request(port, "POST", "/ui/pair-hub", Some(json!({"code": typed}))).await;
    assert_eq!((s, body.clone()), (200, json!({"paired": true})), "{body}");

    let (_, snap) = request(port, "GET", "/ui/snapshot", None).await;
    assert_eq!(snap["paired"], true);
    let (s, body) = request(port, "GET", "/ui/me", None).await;
    assert_eq!((s, body["code"].as_str()), (401, Some("AUTH_FAILED")));
    let (s, body) = request(
        port,
        "POST",
        "/ui/pair-hub",
        Some(json!({"code": code.clone()})),
    )
    .await;
    assert_eq!((s, body["code"].as_str()), (409, Some("ALREADY_SET")));

    let hubs = cloud.db.hubs_of_branch(&branch).await.unwrap();
    assert_eq!(hubs.len(), 1);
    let cloud_hub = &hubs[0];
    assert_eq!(cloud_hub.platform, arena_proto::HubPlatform::Android);
    assert_eq!(cloud_hub.revoked_at, None);

    let store = arena_store::Store::open_reader(data.path().join("branch.db")).unwrap();
    let identity = store.hub_identity().unwrap().unwrap();
    assert_eq!(
        (
            identity.tenant_id.as_str(),
            identity.branch_id.as_str(),
            identity.hub_id.as_str(),
            identity.tier
        ),
        ("t1", "b1", cloud_hub.id.as_str(), Tier::Free)
    );
    assert_eq!(cloud_hub.hub_instance_id.len(), 26);
    let summary = store.branch_summary().unwrap().unwrap();
    assert_eq!(
        (
            summary.name.as_str(),
            summary.branch_code.as_str(),
            summary.code_locked
        ),
        ("Pixel Arena Gaming", "PAG", false)
    );
    let staff = store.rows(arena_store::Table::Staff).unwrap();
    assert_eq!(staff.len(), 1);
    use arena_store::RowValue::{Null, Text};
    assert_eq!(staff[0]["login"], Text("+919876543210".into()));
    assert_eq!(staff[0]["password_hash"], Null);
    assert_eq!(staff[0]["pin_hash"], Null);
    let Text(setup_hash) = &staff[0]["setup_code_hash"] else {
        panic!("the owner has a setup code hash")
    };
    let sealed = cloud
        .db
        .peek_setup_code("b1", now_ms())
        .await
        .unwrap()
        .unwrap();
    let plain = MfaKey::from_bytes([0xa5; 32])
        .open("b1", &sealed.ciphertext, &sealed.nonce)
        .unwrap();
    assert!(arena_cloud::passwords::verify_secret(
        std::str::from_utf8(&plain).unwrap(),
        setup_hash
    ));
    let tax = store
        .settings()
        .unwrap()
        .into_iter()
        .find(|s| s.key == "branch.tax")
        .unwrap();
    assert!(tax.value.contains(r#""country":"IN""#) && tax.value.contains(r#""gst":false"#));
    let changes = store.changes().unwrap();
    let boot: Vec<_> = changes
        .iter()
        .filter(|c| c.intent_kind == "branch.bootstrap")
        .collect();
    assert!(
        boot.iter()
            .all(|c| c.actor_kind == "owner" && c.actor_id == "t1")
    );
    assert!(boot.iter().any(|c| c.tbl == "staff"));
    let licence = store.cached_licence().unwrap().unwrap();
    let claims = verify(&licence.jwt, DEV_LICENCE_VERIFY_KEYS, now_ms()).unwrap();
    assert_eq!(
        (
            claims.tenant_id.as_str(),
            claims.branch_id.as_str(),
            claims.tier
        ),
        ("t1", "b1", Tier::Free)
    );
    drop(store);

    let token = String::from_utf8(
        arena_tasks::platform_keystore(data.path())
            .get()
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let secret = token
        .strip_prefix(&format!("hub_{}.", cloud_hub.id))
        .expect("the stored token is the cloud's");
    assert!(arena_cloud::passwords::verify_secret(
        secret,
        &cloud_hub.token_hash
    ));
    let db_file = std::fs::read(data.path().join("branch.db")).unwrap();
    assert!(
        !db_file
            .windows(secret.len())
            .any(|w| w == secret.as_bytes())
    );

    let attempts = cloud.pair_attempts().await;
    assert_eq!(attempts, 2);
    hub.stop();
    let (_hub, port) = start_hub(data.path(), &cloud.url, "android").await;
    let (_, snap) = request(port, "GET", "/ui/snapshot", None).await;
    assert_eq!(snap["paired"], true, "a restart skips the pairing screen");
    let (s, _) = request(port, "GET", "/ui/me", None).await;
    assert_eq!(s, 401);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        cloud.pair_attempts().await,
        attempts,
        "no /v1/pair after a restart"
    );
    assert_eq!(cloud.db.hubs_of_branch(&branch).await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unreachable_cloud_and_a_windows_hub_on_free_retry_in_place() {
    let cloud = start_cloud().await;
    let branch = cloud.branch_at_step_3().await;
    let code = cloud.code(&branch).await;

    let offline = tempfile::tempdir().unwrap();
    let closed = format!("http://127.0.0.1:{}", free_port());
    let (_a, port) = start_hub(offline.path(), &closed, "android").await;
    let (s, body) = request(
        port,
        "POST",
        "/ui/pair-hub",
        Some(json!({"code": code.clone()})),
    )
    .await;
    assert_eq!(
        (s, body["code"].as_str()),
        (503, Some("CLOUD_UNREACHABLE")),
        "{body}"
    );
    let (_, snap) = request(port, "GET", "/ui/snapshot", None).await;
    assert_eq!(snap["paired"], false);

    let windows = tempfile::tempdir().unwrap();
    let (_b, port) = start_hub(windows.path(), &cloud.url, "windows").await;
    let (s, body) = request(
        port,
        "POST",
        "/ui/pair-hub",
        Some(json!({"code": code.clone()})),
    )
    .await;
    assert_eq!(
        (s, body["code"].as_str(), body["field"].as_str()),
        (403, Some("FEATURE_NOT_ENTITLED"), Some("platform")),
        "{body}"
    );
    assert!(cloud.db.hubs_of_branch(&branch).await.unwrap().is_empty());
    assert!(
        arena_tasks::platform_keystore(windows.path())
            .get()
            .unwrap()
            .is_none()
    );

    let android = tempfile::tempdir().unwrap();
    let (_c, port) = start_hub(android.path(), &cloud.url, "android").await;
    let (s, _) = request(port, "POST", "/ui/pair-hub", Some(json!({"code": code}))).await;
    assert_eq!(s, 200, "the refused code was left unconsumed");
}
