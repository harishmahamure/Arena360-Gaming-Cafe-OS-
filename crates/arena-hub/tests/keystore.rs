#![cfg(feature = "dev-fixtures")]

//! Story 4.3: the Android Hub keeps its token and branch key wrapped by a platform key, notices
//! when that key is lost and pairs again without touching the branch data.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use arena_cloud::auth::AuthState;
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
use arena_cloud::pairing_code::new_pairing_code;
use arena_cloud::{Clock, app_with_auth, auth_state};
use arena_proto::PAIRING_CODE_TTL_MS;
use arena_store::control::{
    BranchProfile, ControlDb, LATEST_CONTROL_VERSION, NewOtpChallenge, NewOwner, NewOwnerSession,
    NewPairingCode, NewTenantBranch, SignupCompletion,
};
use arena_tasks::{SecretWrapper, WrapError};
use serde_json::{Value, json};
use tokio::sync::watch;

const PEPPER: &str = "test-pepper-0123456789abcdef0123456789";

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

/// A wrapper standing in for the Kotlin Keystore bridge: the "key" is one byte; a different key
/// cannot open what another wrapped (the check byte fails), like a reset Android Keystore.
struct FakeKeystore(AtomicU8);

impl SecretWrapper for FakeKeystore {
    fn wrap(&self, plain: &[u8]) -> Result<Vec<u8>, WrapError> {
        let k = self.0.load(Ordering::SeqCst);
        let mut out = vec![k ^ 0x5a];
        out.extend(plain.iter().map(|b| b ^ k));
        Ok(out)
    }

    fn unwrap(&self, wrapped: &[u8]) -> Result<Vec<u8>, WrapError> {
        let k = self.0.load(Ordering::SeqCst);
        match wrapped.split_first() {
            Some((check, rest)) if *check == k ^ 0x5a => Ok(rest.iter().map(|b| b ^ k).collect()),
            _ => Err(WrapError::Lost),
        }
    }
}

struct RunningHub {
    task: std::thread::JoinHandle<std::process::ExitCode>,
    port: u16,
}

impl RunningHub {
    async fn stop(self) {
        arena_hub::stop();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !self.task.is_finished() {
            assert!(Instant::now() < deadline, "the hub did not stop");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(
            format!("{:?}", self.task.join().unwrap()),
            format!("{:?}", std::process::ExitCode::SUCCESS)
        );
    }
}

async fn start_hub(data: &Path, cloud: &str, wrapper: &Arc<FakeKeystore>) -> RunningHub {
    let (health, lan, web) = (free_port(), free_port(), free_port());
    let args: Vec<std::ffi::OsString> = vec![
        "--data-dir".into(),
        data.into(),
        "--health-port".into(),
        health.to_string().into(),
        "--lan-port".into(),
        lan.to_string().into(),
        "--web-port".into(),
        web.to_string().into(),
        "--cloud-host".into(),
        cloud.into(),
        "--platform".into(),
        "android".into(),
        "--probe-internet".into(),
        "127.0.0.1:9".into(),
    ];
    let options = arena_hub::StartOptions {
        secret_wrapper: Some(Arc::clone(wrapper) as Arc<dyn SecretWrapper>),
        ..Default::default()
    };
    let task = std::thread::spawn(move || {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(arena_hub::start_with(args.into_iter(), options))
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let up = tokio::task::spawn_blocking(move || call(health, "GET", "/healthz", None))
            .await
            .unwrap();
        if up.as_ref().is_some_and(|(_, b)| b["role"] == "primary") {
            break;
        }
        assert!(!task.is_finished(), "the hub exited while starting");
        if Instant::now() >= deadline {
            let logs: String = std::fs::read_dir(data.join("logs"))
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
                .collect();
            panic!(
                "the hub never became primary: {up:?}
{logs}"
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    RunningHub { task, port: health }
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
        escrow_key: None,
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
}

/// The branch's data, without the identity, the licence and the lease bookkeeping.
fn branch_data(data: &Path) -> String {
    let store = arena_store::Store::open_reader(data.join("branch.db")).unwrap();
    format!(
        "{:?}|{:?}|{}",
        store.rows(arena_store::Table::Staff).unwrap(),
        store.settings().unwrap(),
        store.changes().unwrap().len()
    )
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lost_keystore_key_asks_for_a_new_pairing_and_the_branch_data_survives() {
    let cloud = start_cloud().await;
    let branch = cloud.branch_at_step_3().await;
    let data = tempfile::tempdir().unwrap();
    let wrapper = Arc::new(FakeKeystore(AtomicU8::new(0x21)));
    let token_file = data.path().join("hub-token.bin");
    let kek_file = data.path().join("branch-kek.bin");

    // Pair: the token and the branch key are stored wrapped, never in clear.
    let hub = start_hub(data.path(), &cloud.url, &wrapper).await;
    let port = hub.port;
    let (_, snap) = request(port, "GET", "/ui/snapshot", None).await;
    assert_eq!(snap["paired"], false);
    assert!(snap.get("needs_repair").is_none(), "{snap}");
    let code = cloud.code(&branch).await;
    let (s, body) = request(port, "POST", "/ui/pair-hub", Some(json!({"code": code}))).await;
    assert_eq!((s, body.clone()), (200, json!({"paired": true})), "{body}");
    let first = cloud.db.hubs_of_branch(&branch).await.unwrap();
    assert_eq!(first.len(), 1);
    let token_bytes = {
        let w = Arc::clone(&wrapper);
        w.unwrap(&std::fs::read(&token_file).unwrap()).unwrap()
    };
    let token = String::from_utf8(token_bytes).unwrap();
    let secret = token
        .strip_prefix(&format!("hub_{}.", first[0].id))
        .unwrap();
    let kek_before = wrapper.unwrap(&std::fs::read(&kek_file).unwrap()).unwrap();
    assert_eq!(kek_before.len(), arena_core::players::KEK_LEN);
    let on_disk = std::fs::read(&token_file).unwrap();
    assert!(!contains(&on_disk, secret.as_bytes()));
    assert!(!contains(&std::fs::read(&kek_file).unwrap(), &kek_before));
    let mut all_files = Vec::new();
    for entry in std::fs::read_dir(data.path()).unwrap() {
        let path = entry.unwrap().path();
        // A file the Hub holds open (or a temp file mid-rename) may refuse to be read.
        if let Ok(bytes) = std::fs::read(&path) {
            all_files.push(bytes);
        }
    }
    assert!(
        all_files.iter().all(|f| !contains(f, secret.as_bytes())),
        "the token appears in clear in the data dir"
    );
    assert!(
        all_files.iter().all(|f| !contains(f, &kek_before)),
        "the branch key appears in clear in the data dir"
    );
    hub.stop().await;
    let data_before = branch_data(data.path());

    // A restart with the same key skips the pairing screen.
    let hub = start_hub(data.path(), &cloud.url, &wrapper).await;
    let (_, snap) = request(hub.port, "GET", "/ui/snapshot", None).await;
    assert_eq!(snap["paired"], true);
    hub.stop().await;

    // The key is lost: the Hub starts, asks to pair again and mints nothing.
    wrapper.0.store(0x22, Ordering::SeqCst);
    let (token_before, kek_wrapped) = (
        std::fs::read(&token_file).unwrap(),
        std::fs::read(&kek_file).unwrap(),
    );
    let hub = start_hub(data.path(), &cloud.url, &wrapper).await;
    let port = hub.port;
    let (s, snap) = request(port, "GET", "/ui/snapshot", None).await;
    assert_eq!(s, 200);
    assert_eq!(snap["paired"], false, "{snap}");
    assert_eq!(snap["needs_repair"], true, "{snap}");
    assert_eq!(snap["branch"], Value::Null);
    let (s, body) = request(port, "GET", "/ui/me", None).await;
    assert_eq!((s, body["code"].as_str()), (409, Some("HUB_NOT_PAIRED")));
    assert_eq!(std::fs::read(&token_file).unwrap(), token_before);
    assert_eq!(std::fs::read(&kek_file).unwrap(), kek_wrapped);
    assert_eq!(branch_data(data.path()), data_before);

    // A wrong code leaves it needing a new pairing.
    let (s, body) = request(
        port,
        "POST",
        "/ui/pair-hub",
        Some(json!({"code": "ZZZZ ZZZZ"})),
    )
    .await;
    assert_eq!((s, body["code"].as_str()), (401, Some("PAIR_CODE_INVALID")));
    let (_, snap) = request(port, "GET", "/ui/snapshot", None).await;
    assert_eq!(snap["needs_repair"], true);
    hub.stop().await;

    // Files gone is lost too (identity present); still nothing is minted.
    wrapper.0.store(0x21, Ordering::SeqCst);
    std::fs::remove_file(&token_file).unwrap();
    std::fs::remove_file(&kek_file).unwrap();
    let hub = start_hub(data.path(), &cloud.url, &wrapper).await;
    let port = hub.port;
    let (_, snap) = request(port, "GET", "/ui/snapshot", None).await;
    assert_eq!(snap["needs_repair"], true, "{snap}");
    assert!(!kek_file.exists() && !token_file.exists());
    assert_eq!(branch_data(data.path()), data_before);

    // Re-pair with a new code: the token, hub id, tier and licence are replaced.
    let again = cloud.code(&branch).await;
    let (s, body) = request(port, "POST", "/ui/pair-hub", Some(json!({"code": again}))).await;
    assert_eq!((s, body.clone()), (200, json!({"paired": true})), "{body}");
    let (_, snap) = request(port, "GET", "/ui/snapshot", None).await;
    assert_eq!(snap["paired"], true);
    assert!(snap.get("needs_repair").is_none(), "{snap}");
    let hubs = cloud.db.hubs_of_branch(&branch).await.unwrap();
    let active: Vec<_> = hubs.iter().filter(|h| h.revoked_at.is_none()).collect();
    assert_eq!((hubs.len(), active.len()), (2, 1));
    let new_token = String::from_utf8(
        wrapper
            .unwrap(&std::fs::read(&token_file).unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(new_token.starts_with(&format!("hub_{}.", active[0].id)));
    let kek_after = wrapper.unwrap(&std::fs::read(&kek_file).unwrap()).unwrap();
    assert_eq!(kek_after.len(), arena_core::players::KEK_LEN);
    assert_ne!(kek_after, kek_before, "a fresh branch key");
    assert_eq!(branch_data(data.path()), data_before, "old data intact");
    {
        let store = arena_store::Store::open_reader(data.path().join("branch.db")).unwrap();
        let identity = store.hub_identity().unwrap().unwrap();
        assert_eq!(identity.hub_id, active[0].id);
        assert_eq!(identity.branch_id, "b1");
        assert!(store.cached_licence().unwrap().is_some());
    }
    // Pairing a healthy Hub again stays refused.
    let more = cloud.code(&branch).await;
    let (s, body) = request(port, "POST", "/ui/pair-hub", Some(json!({"code": more}))).await;
    assert_eq!((s, body["code"].as_str()), (409, Some("ALREADY_SET")));
    hub.stop().await;

    let hub = start_hub(data.path(), &cloud.url, &wrapper).await;
    let (_, snap) = request(hub.port, "GET", "/ui/snapshot", None).await;
    assert_eq!(snap["paired"], true, "{snap}");
    hub.stop().await;
}
