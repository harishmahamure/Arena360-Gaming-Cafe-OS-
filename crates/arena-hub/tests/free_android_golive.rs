//! Story 4.8: a Free café goes from sign-up to go-live on an Android Hub with no comp
//! entitlement. A real cloud (in-process, over TCP), the real `arena-hub` binary started with
//! `--platform android` and a kiosk speaking the real LAN protocol walk the whole path through
//! public APIs only: the owner's cookie on the cloud, loopback `/ui/*` on the Hub, the command
//! mailbox and the heartbeat. The emulator and Windows kiosk VM run is a CI job (`android-e2e`).
//! Every step is announced with its name so a failure says which step broke.
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
use arena_cloud::{Clock, app_with_auth, auth_state};
use arena_kiosk_core::HubClient;
use arena_proto::{
    ADS_POLICY_VERSION, Actor, ActorKind, ClientClass, Hello, HubEvent, Intent, IntentKind,
    IntentPayload, Versions,
};
use arena_store::control::{ControlDb, LATEST_CONTROL_VERSION, NewBackupCode};
use serde_json::{Value, json};
use tokio::sync::watch;

const PEPPER: &str = "test-pepper-0123456789abcdef0123456789";
const PHONE: &str = "+919876543210";
const OWNER_PASSWORD: &str = "Owner-pass-2026!";
const OWNER_PIN: &str = "4826";
const WAIT: Duration = Duration::from_secs(90);

fn step(name: &str) {
    println!("android-e2e step: {name}");
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct Reply {
    status: u16,
    head: String,
    body: Value,
}

impl Reply {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or("")
    }

    fn cookie(&self, name: &str) -> Option<String> {
        self.head.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            if !k.eq_ignore_ascii_case("set-cookie") {
                return None;
            }
            let v = v.trim().strip_prefix(&format!("{name}="))?;
            Some(v.split(';').next()?.to_owned())
        })
    }
}

fn http(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: Option<&Value>,
) -> Option<Reply> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(30))).ok()?;
    let body = body.map(Value::to_string).unwrap_or_default();
    let extra: String = headers
        .iter()
        .map(|(k, v)| format!("{k}: {v}\r\n"))
        .collect();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    let (head, payload) = out.split_once("\r\n\r\n")?;
    let status = head.split_whitespace().nth(1)?.parse().ok()?;
    Some(Reply {
        status,
        head: head.to_owned(),
        body: serde_json::from_str(payload).unwrap_or(Value::Null),
    })
}

async fn call(
    port: u16,
    method: &'static str,
    path: &str,
    headers: Vec<(&'static str, String)>,
    body: Option<Value>,
) -> Reply {
    let p = path.to_owned();
    tokio::task::spawn_blocking(move || http(port, method, &p, &headers, body.as_ref()))
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("{method} {path} got no answer"))
}

/// Polls `cond` every 250 ms until it returns `Some` or `WAIT` passes.
async fn eventually<T, F, Fut>(what: &str, mut cond: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(v) = cond().await {
            return v;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

struct Cloud {
    db: Arc<ControlDb>,
    st: Arc<AuthState>,
    port: u16,
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
    let addr = listener.local_addr().unwrap();
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
        port: addr.port(),
        url: format!("http://{addr}"),
        _dir: dir,
    }
}

/// The owner's calls on the cloud, with the `arena_access` cookie.
struct Owner<'a> {
    cloud: &'a Cloud,
    access: String,
}

impl Owner<'_> {
    async fn call(&self, method: &'static str, path: &str, body: Option<Value>) -> Reply {
        call(
            self.cloud.port,
            method,
            path,
            vec![("Cookie", format!("arena_access={}", self.access))],
            body,
        )
        .await
    }

    async fn wizard(&self) -> Value {
        let r = self.call("GET", "/v1/wizard", None).await;
        assert_eq!(r.status, 200, "{}", r.body);
        r.body
    }

    fn step_state(wizard: &Value, key: &str) -> String {
        wizard["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["key"] == key)
            .unwrap_or_else(|| panic!("no step {key}"))["state"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    async fn wait_step(&self, key: &str) {
        eventually(&format!("wizard step {key} done"), || async {
            (Self::step_state(&self.wizard().await, key) == "done").then_some(())
        })
        .await;
    }

    async fn command(&self, kind: &str, payload: Value) -> String {
        let r = self
            .call(
                "POST",
                "/v1/wizard/devices",
                Some(json!({"kind": kind, "payload": payload})),
            )
            .await;
        assert_eq!(r.status, 200, "{kind}: {}", r.body);
        r.body["key"].as_str().unwrap().to_owned()
    }

    async fn wait_command_applied(&self, key: &str) {
        eventually(&format!("command {key} applied"), || async {
            let r = self.call("GET", "/v1/wizard/devices", None).await;
            assert_eq!(r.status, 200, "{}", r.body);
            let cmds = r.body["commands"].as_array().cloned().unwrap_or_default();
            let mine = cmds.iter().find(|c| c["key"] == key)?;
            assert_ne!(mine["status"], "rejected", "{mine}");
            (mine["status"] == "applied").then_some(())
        })
        .await;
    }
}

/// Signs up with a phone OTP, then finishes the owner's account setup (password and TOTP) in
/// the control DB the way the account screens would.
async fn sign_up(cloud: &Cloud) -> Owner<'_> {
    let r = call(
        cloud.port,
        "POST",
        "/v1/signup/start",
        vec![],
        Some(json!({
            "name": "Asha Rao",
            "phone": PHONE,
            "email": "asha@example.in",
            "cafe_name": "Pixel Arena Gaming",
            "city": "Pune",
        })),
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.body);
    let id = r.body["challenge_id"].as_str().unwrap().to_owned();
    let text = cloud
        .st
        .sms
        .fake()
        .unwrap()
        .sent()
        .last()
        .unwrap()
        .text
        .clone();
    let r = call(
        cloud.port,
        "POST",
        "/v1/signup/verify",
        vec![],
        Some(json!({"challenge_id": id, "code": &text[..6]})),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let access = r.cookie("arena_access").expect("the sign-up sets a cookie");

    let now = now_ms();
    let owner = cloud.db.owner_by_phone(PHONE).await.unwrap().unwrap().id;
    assert!(
        cloud
            .db
            .set_owner_password(&owner, "$argon2id$x", now)
            .await
            .unwrap()
    );
    assert!(
        cloud
            .db
            .start_owner_mfa(&owner, b"ct", b"n", now)
            .await
            .unwrap()
    );
    let codes: Vec<NewBackupCode> = (0..10)
        .map(|i| NewBackupCode {
            id: format!("{owner}-b{i}"),
            code_hash: format!("h{i}"),
        })
        .collect();
    assert!(
        cloud
            .db
            .confirm_owner_mfa(&owner, b"n", 1, &codes, now)
            .await
            .unwrap()
    );
    assert!(cloud.db.ack_backup_codes(&owner, now).await.unwrap());
    Owner { cloud, access }
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

struct Hub {
    _proc: Running,
    health: u16,
    lan: u16,
    web: u16,
    fingerprint: String,
}

async fn start_hub(data: &Path, cloud: &str, platform: &str) -> Hub {
    let (health, lan, web) = (free_port(), free_port(), free_port());
    let child = Command::new(env!("CARGO_BIN_EXE_arena-hub"))
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
    let proc = Running(Some(child));
    let body = eventually("the hub as primary", || async {
        let r = call(health, "GET", "/healthz", vec![], None).await;
        (r.body["role"] == "primary").then_some(r.body)
    })
    .await;
    assert_eq!(body["platform"], platform);
    Hub {
        _proc: proc,
        health,
        lan,
        web,
        fingerprint: body["tls_fingerprint"].as_str().unwrap().to_owned(),
    }
}

/// The signed-in staff's calls on the Hub's loopback `/ui/*`.
struct Staff<'a> {
    hub: &'a Hub,
    token: String,
}

impl Staff<'_> {
    async fn call(&self, method: &'static str, path: &str, body: Option<Value>) -> Reply {
        call(
            self.hub.health,
            method,
            path,
            vec![("Authorization", format!("Bearer {}", self.token))],
            body,
        )
        .await
    }

    async fn intent(&self, kind: IntentKind, base_rev: Option<i64>, payload: IntentPayload) {
        let intent = Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind,
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: now_ms(),
            base_rev,
            actor: Actor {
                kind: ActorKind::Staff,
                id: "e2e".into(),
            },
            elevated_by: None,
            reason: None,
            payload,
        };
        let r = self
            .call("POST", "/ui/intent", Some(json!({ "intent": intent })))
            .await;
        assert_eq!(r.status, 200, "{}: {}", kind.as_str(), r.body);
    }

    async fn rev(&self, key: &str) -> i64 {
        let r = self.call("GET", "/ui/settings", None).await;
        assert_eq!(r.status, 200, "{}", r.body);
        r.body
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["key"] == key)
            .unwrap_or_else(|| panic!("no setting {key}"))["rev"]
            .as_i64()
            .unwrap()
    }

    async fn set(&self, key: &str, value: &str) {
        let rev = self.rev(key).await;
        self.intent(
            IntentKind::SettingsSet,
            Some(rev),
            IntentPayload::SettingsSet(arena_proto::SettingsSet {
                key: key.into(),
                value: value.into(),
            }),
        )
        .await;
    }
}

fn pair_body(code: &str) -> Value {
    json!({ "code": code })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_free_cafe_goes_from_sign_up_to_go_live_on_android_with_no_comp_entitlement() {
    let cloud = start_cloud().await;

    step("sign-up with phone OTP");
    let owner = sign_up(&cloud).await;

    step("step 1: café and first branch");
    let r = owner
        .call(
            "PUT",
            "/v1/wizard/cafe",
            Some(json!({
                "cafe_name": "Pixel Arena Gaming",
                "city": "Pune",
                "country": "IN",
                "currency": "INR",
                "timezone": "Asia/Kolkata",
                "gst_registered": false,
            })),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let branch = r.body["branch"]["id"].as_str().unwrap().to_owned();

    step("step 2: Free tier, no comp entitlement");
    let r = owner
        .call("PUT", "/v1/wizard/tier", Some(json!({"tier": "free"})))
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let entitlement = cloud.db.entitlement(&branch).await.unwrap().unwrap();
    assert_eq!(entitlement.tier, arena_proto::Tier::Free);
    assert_eq!(
        entitlement.source,
        arena_proto::EntitlementSource::Free,
        "the branch must be Free, not a comp grant"
    );

    step("step 3: the single-use pairing code");
    let r = owner
        .call("POST", "/v1/wizard/install-hub/code", Some(json!({})))
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let code = r.body["code"].as_str().unwrap().to_owned();

    step("a Windows Hub on Free is refused and the code is not consumed");
    let windows_data = tempfile::tempdir().unwrap();
    {
        let windows = start_hub(windows_data.path(), &cloud.url, "windows").await;
        let r = call(
            windows.health,
            "POST",
            "/ui/pair-hub",
            vec![],
            Some(pair_body(&code)),
        )
        .await;
        assert_eq!(
            (r.status, r.code(), r.body["field"].as_str()),
            (403, "FEATURE_NOT_ENTITLED", Some("platform")),
            "{}",
            r.body
        );
        assert!(cloud.db.hubs_of_branch(&branch).await.unwrap().is_empty());
    }

    step("pair the Android Hub with the same code");
    let data = tempfile::tempdir().unwrap();
    let hub = start_hub(data.path(), &cloud.url, "android").await;
    let r = call(
        hub.health,
        "POST",
        "/ui/pair-hub",
        vec![],
        Some(pair_body(&code)),
    )
    .await;
    assert_eq!((r.status, r.body.clone()), (200, json!({"paired": true})));
    let hubs = cloud.db.hubs_of_branch(&branch).await.unwrap();
    assert_eq!(hubs.len(), 1);
    assert_eq!(hubs[0].platform, arena_proto::HubPlatform::Android);

    step("the wizard sees the Hub's first heartbeat");
    owner.wait_step("install_hub").await;

    step("owner first sign-in at the Hub with the setup code");
    let r = owner
        .call("POST", "/v1/wizard/setup-code/reveal", Some(json!({})))
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let setup_code = r.body["code"].as_str().unwrap().to_owned();
    let r = call(
        hub.health,
        "POST",
        "/ui/auth/setup-code",
        vec![],
        Some(json!({
            "login": PHONE,
            "code": setup_code,
            "password": OWNER_PASSWORD,
            "pin": OWNER_PIN,
        })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let staff = Staff {
        hub: &hub,
        token: r.body["token"].as_str().unwrap().to_owned(),
    };

    step("pair a kiosk with a 6-digit code from the Hub");
    let r = staff.call("POST", "/ui/pairing-code", None).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let pc_code = r.body["code"].as_str().unwrap().to_owned();
    let paired = arena_kiosk_core::pair(
        "127.0.0.1",
        hub.web,
        &hub.fingerprint,
        &arena_proto::PairRequest {
            code: Some(pc_code),
            pin: None,
            pc_name: "PC-01".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
        Duration::from_secs(20),
    )
    .await
    .unwrap();
    let kiosk = HubClient::connect(
        &format!("wss://127.0.0.1:{}/", hub.lan),
        &paired.hub_fingerprint,
        Hello {
            credential: paired.token.clone(),
            class: ClientClass::Paired,
            versions: Versions {
                app: env!("CARGO_PKG_VERSION").to_owned(),
            },
            capabilities: vec![],
        },
    )
    .await
    .unwrap();
    let mut events = kiosk.events().expect("the kiosk's event stream");
    let device_id = paired.device_id.clone();
    eventually("the kiosk online on the floor", || async {
        let r = call(hub.health, "GET", "/ui/snapshot", vec![], None).await;
        r.body["devices"]
            .as_array()?
            .iter()
            .any(|d| d["device_id"] == device_id.as_str() && d["online"] == true)
            .then_some(())
    })
    .await;

    step("step 4: group the kiosk from the web wizard");
    let key = owner
        .command(
            "group.upsert",
            json!({"name": "Gaming PCs", "group_kind": "pc", "colour": "teal", "sort_order": 1}),
        )
        .await;
    owner.wait_command_applied(&key).await;
    let group_id = eventually("the group on the Hub", || async {
        let r = call(hub.health, "GET", "/ui/snapshot", vec![], None).await;
        r.body["groups"]
            .as_array()?
            .first()?
            .get("id")?
            .as_str()
            .map(str::to_owned)
    })
    .await;
    let key = owner
        .command(
            "device.assign",
            json!({"device_id": device_id, "group_id": group_id}),
        )
        .await;
    owner.wait_command_applied(&key).await;
    owner.wait_step("devices").await;

    step("step 5: set the hourly rate");
    staff
        .intent(
            IntentKind::RateUpsert,
            None,
            IntentPayload::RateUpsert(arena_proto::RateUpsert {
                group_id: group_id.clone(),
                price_per_hour_minor: 6_000,
            }),
        )
        .await;
    owner.wait_step("rates").await;

    step("skip products and staff invites");
    for path in ["/v1/wizard/products/skip", "/v1/wizard/staff/skip"] {
        let r = owner.call("POST", path, Some(json!({}))).await;
        assert_eq!(r.status, 200, "{path}: {}", r.body);
    }

    step("skip the printer at the Hub");
    let r = staff.call("POST", "/ui/printer/skip", None).await;
    assert_eq!(r.status, 200, "{}", r.body);

    step("opening hours");
    let week: Vec<Value> = (0..7)
        .map(|_| json!({"open_min": 600, "close_min": 1380}))
        .collect();
    staff
        .set("branch.opening_hours", &Value::Array(week).to_string())
        .await;

    step("choose a backup folder");
    let backup = tempfile::tempdir().unwrap();
    staff
        .set("system.backup_dir", &backup.path().to_string_lossy())
        .await;
    assert_eq!(
        std::fs::read_dir(backup.path()).unwrap().count(),
        0,
        "the probe file is removed"
    );

    step("tick ad consent");
    let rev = staff.rev("branch.ads_consent").await;
    staff
        .intent(
            IntentKind::AdsConsent,
            Some(rev),
            IntentPayload::AdsConsent(arena_proto::AdsConsentSet {
                given: true,
                policy_version: ADS_POLICY_VERSION,
            }),
        )
        .await;

    step("step 9: the 2-minute test session locks the kiosk at zero");
    let key = owner
        .command("setup.test_session", json!({"device_id": device_id}))
        .await;
    let deadline = Instant::now() + WAIT;
    let lock_after = loop {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the test_session event at the kiosk"
        );
        match tokio::time::timeout(Duration::from_millis(500), events.next_event()).await {
            Ok(Some(HubEvent::TestSession { lock_after_ms })) => break lock_after_ms,
            Ok(Some(_)) | Err(_) => {}
            Ok(None) => panic!("the kiosk connection closed"),
        }
    };
    assert_eq!(lock_after, arena_proto::TEST_SESSION_MS);
    kiosk
        .submit(Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: IntentKind::DeviceTestSessionCompleted,
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: now_ms(),
            base_rev: None,
            actor: Actor {
                kind: ActorKind::Device,
                id: device_id.clone(),
            },
            elevated_by: None,
            reason: None,
            payload: IntentPayload::DeviceTestSessionCompleted(
                arena_proto::DeviceTestSessionCompleted {
                    device_id: device_id.clone(),
                    outcome: arena_proto::TestSessionOutcome::Locked,
                },
            ),
        })
        .await
        .unwrap();
    owner.wait_command_applied(&key).await;
    owner.wait_step("test_session").await;

    step("step 10: the go-live checklist from the Android Hub's setup status");
    let go = eventually("go-live with nothing blocking", || async {
        let r = owner.call("GET", "/v1/wizard/go-live", None).await;
        assert_eq!(r.status, 200, "{}", r.body);
        r.body["blocker"].is_null().then_some(r.body)
    })
    .await;
    assert_eq!(go["free"], true);
    let item = |k: &str| {
        go["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["key"] == k)
            .unwrap_or_else(|| panic!("no go-live item {k}: {go}"))["done"]
            .clone()
    };
    assert_eq!(item("opening_hours"), true);
    assert_eq!(item("backup_location"), true);
    assert_eq!(item("ad_consent"), true);
    assert_eq!(
        item("standby"),
        false,
        "standby stays unset and does not block"
    );

    step("go live");
    let r = owner
        .call("POST", "/v1/wizard/go-live", Some(json!({})))
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let b = cloud.db.branch(&branch).await.unwrap().unwrap();
    assert!(b.live_at.is_some(), "the branch shows Live");

    step("Settings → Staff on the Android Hub: the Owner is 1 of the 3 Free users");
    let r = staff.call("GET", "/ui/staff", None).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        (r.body["used"].as_i64(), r.body["limit"].as_i64()),
        (Some(1), Some(3)),
        "{}",
        r.body
    );
    assert_eq!(r.body["staff"][0]["role"], "Owner");
    assert_eq!(r.body["staff"][0]["status"], "active");

    step("DEVICE_LIMIT applies unchanged on the Android Hub");
    {
        // The Free cap is 20; lower it to the one kiosk already paired, as a licence would.
        let db = rusqlite::Connection::open(data.path().join("branch.db")).unwrap();
        db.busy_timeout(Duration::from_secs(10)).unwrap();
        let n = db
            .execute(
                "UPDATE settings SET value = '1' WHERE key = 'licence.device_cap'",
                [],
            )
            .unwrap();
        assert_eq!(n, 1, "the licence wrote the device cap");
    }
    let r = staff.call("POST", "/ui/pairing-code", None).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let full = arena_kiosk_core::pair(
        "127.0.0.1",
        hub.web,
        &hub.fingerprint,
        &arena_proto::PairRequest {
            code: Some(r.body["code"].as_str().unwrap().to_owned()),
            pin: None,
            pc_name: "PC-02".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
        Duration::from_secs(20),
    )
    .await;
    match full {
        Err(arena_kiosk_core::PairError::Hub { error, .. }) => {
            assert_eq!(error.code, arena_proto::ErrorCode::DeviceLimit);
        }
        other => panic!("expected DEVICE_LIMIT, got {other:?}"),
    }

    step("onboarding_progress holds every step time and no personal data");
    let progress = cloud.db.onboarding_progress(&branch).await.unwrap();
    for s in arena_proto::WizardStep::ALL {
        let row = progress
            .iter()
            .find(|p| p.step == s)
            .unwrap_or_else(|| panic!("no onboarding row for {s:?}"));
        let skipped = matches!(
            s,
            arena_proto::WizardStep::Products | arena_proto::WizardStep::Staff
        );
        let want = if skipped {
            arena_proto::StepState::Skipped
        } else {
            arena_proto::StepState::Done
        };
        assert_eq!(row.state, want, "{s:?}");
        assert_eq!(
            row.completed_at.is_some(),
            !skipped,
            "{s:?}: only a finished step has a time"
        );
    }
    let db = rusqlite::Connection::open(cloud._dir.path().join("gz_control.db")).unwrap();
    let mut cols = db
        .prepare("PRAGMA table_info(onboarding_progress)")
        .unwrap();
    let cols: Vec<String> = cols
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(cols, ["branch_id", "step", "state", "completed_at"]);
    let dump: String = db
        .prepare("SELECT branch_id || step || state || COALESCE(completed_at, '') FROM onboarding_progress")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for pii in [PHONE, "asha@example.in", "Asha", &setup_code] {
        assert!(!dump.contains(pii), "onboarding_progress holds {pii}");
    }
}
