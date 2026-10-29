#![cfg(unix)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arena_api::lan::{HubCert, HubInfoSource, LanHub, Presence, bind_lan, serve_lan};
use arena_api::pairing::Pairing;
use arena_core::rbac::{BUILTIN_ROLES, ROLE_COUNTER, ROLE_MANAGER};
use arena_core::staff::hash_secret;
use arena_core::{
    DeviceDirectory, HubStatus, LeaseInfo, Role, SnapshotReader, StaffDirectory, SystemClock,
    Writer, status_channel,
};
use arena_kiosk_core::ipc::{IpcMsg, read_msg, write_msg};
use arena_kiosk_svc::config::KioskConfig;
use arena_kiosk_svc::discovery::{Browse, Browser, Discovery, NetProber};
use arena_kiosk_svc::ipc::UnixIpcServer;
use arena_kiosk_svc::launcher::AppLauncher;
use arena_kiosk_svc::paths::DataDir;
use arena_kiosk_svc::{Runtime, Timings};
use arena_proto::{
    Actor, ActorKind, BranchBootstrap, ErrorCode, Intent, IntentPayload, KioskHubState, StaffUpsert,
};
use arena_store::{LeaseRow, Store, write_lease};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{mpsc, oneshot, watch};

const HUB_ID: &str = "01JHUBAAAAAAAAAAAAAAAAAAAA";

fn now_ms() -> i64 {
    arena_core::Clock::now_ms(&SystemClock)
}

fn intent(actor: Actor, payload: IntentPayload) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: payload.kind(),
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: now_ms(),
        base_rev: None,
        actor,
        elevated_by: None,
        reason: None,
        payload,
    }
}

struct Hub {
    _dir: tempfile::TempDir,
    path: PathBuf,
    web: SocketAddr,
    lan: SocketAddr,
    health: SocketAddr,
    fingerprint: String,
    presence: Presence,
    _status: watch::Sender<HubStatus>,
    _stops: Vec<oneshot::Sender<()>>,
}

async fn hub() -> Hub {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("branch.db");
    let mut store = Store::open(&path, now_ms()).unwrap();
    let lease = LeaseRow {
        counter: 1,
        hub_instance_id: HUB_ID.into(),
        provisional: true,
        renewed_at: now_ms(),
        expires_at: i64::MAX,
    };
    store.write_tx(|tx| write_lease(tx, &lease)).unwrap();
    let mut initial = HubStatus::opened(store.schema_version().unwrap(), HUB_ID);
    initial.role = Role::Primary;
    initial.lease = Some(LeaseInfo::from(&lease));
    let cert = HubCert::generate(None).unwrap();
    initial.tls_fingerprint = Some(cert.fingerprint.clone());
    let (status_tx, status) = status_channel(initial);
    let clock: Arc<dyn arena_core::Clock> = Arc::new(SystemClock);
    let writer = Writer::spawn(store, Arc::clone(&clock), status.clone()).unwrap();
    let boot = writer
        .submit_intent(intent(
            Actor {
                kind: ActorKind::System,
                id: "test".into(),
            },
            IntentPayload::BranchBootstrap(BranchBootstrap {
                name: "Koramangala".into(),
                branch_code: "KOR01".into(),
                currency: "INR".into(),
                tz: "Asia/Kolkata".into(),
                day_start_min: 360,
                fy_start_month: 4,
                onboarding: None,
            }),
        ))
        .await
        .unwrap();
    let roles: Vec<String> = boot
        .changes
        .iter()
        .filter(|c| c.tbl == "roles")
        .map(|c| c.row_id.clone())
        .collect();
    let role = |name: &str| roles[BUILTIN_ROLES.iter().position(|r| *r == name).unwrap()].clone();
    for (login, role_name, pin) in [
        ("manager", ROLE_MANAGER, "2222"),
        ("counter", ROLE_COUNTER, "3333"),
    ] {
        writer
            .submit_intent(intent(
                Actor {
                    kind: ActorKind::Owner,
                    id: "owner".into(),
                },
                IntentPayload::StaffUpsert(StaffUpsert {
                    staff_id: None,
                    login: login.into(),
                    display_name: login.into(),
                    role_id: role(role_name),
                    password_hash: Some(hash_secret(&format!("{login}-pass")).unwrap()),
                    pin_hash: Some(hash_secret(pin).unwrap()),
                    active: true,
                    setup_code_hash: None,
                    setup_expires_at: None,
                }),
            ))
            .await
            .unwrap();
    }
    let listeners = bind_lan(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, 0)
        .await
        .unwrap();
    let (lan, web) = (listeners.lan_addr().unwrap(), listeners.web_addr().unwrap());
    let staff = Arc::new(StaffDirectory::open(&path).unwrap());
    let snapshots = Arc::new(SnapshotReader::open(&path).unwrap());
    let pairing = Arc::new(Pairing::new(Arc::clone(&clock)).unwrap());
    let lan_hub = Arc::new(
        LanHub::new(
            Some(writer.clone()),
            status.clone(),
            Arc::clone(&clock),
            Arc::new(DeviceDirectory::open(&path).unwrap()),
            cert.server_config().unwrap(),
            env!("CARGO_PKG_VERSION"),
        )
        .with_staff(Arc::clone(&staff))
        .with_pairing(Arc::clone(&pairing))
        .with_info(HubInfoSource {
            reader: Arc::clone(&snapshots),
            hub_name: "front-desk".into(),
            lan_port: lan.port(),
            web_port: web.port(),
        }),
    );
    let presence = lan_hub.presence();
    let ui_auth = Arc::new(
        arena_api::UiAuth::new(
            staff,
            Arc::clone(&snapshots),
            Some(writer),
            Arc::clone(&clock),
        )
        .with_status(status.clone())
        .with_presence(presence.clone())
        .with_pairing(pairing),
    );
    let router = arena_api::ui_router(snapshots, clock, presence.clone(), Some(status), None)
        .merge(arena_api::ui_auth_router(ui_auth));
    let health_listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let health = health_listener.local_addr().unwrap();
    let (lan_stop, lan_rx) = oneshot::channel::<()>();
    tokio::spawn(serve_lan(listeners, lan_hub, async {
        let _ = lan_rx.await;
    }));
    let (ui_stop, ui_rx) = oneshot::channel::<()>();
    tokio::spawn(arena_api::serve_router(health_listener, router, async {
        let _ = ui_rx.await;
    }));
    Hub {
        _dir: dir,
        path,
        web,
        lan,
        health,
        fingerprint: cert.fingerprint,
        presence,
        _status: status_tx,
        _stops: vec![lan_stop, ui_stop],
    }
}

impl Hub {
    async fn http(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut tcp = tokio::net::TcpStream::connect(self.health).await.unwrap();
        let body = body.map(|b| b.to_string()).unwrap_or_default();
        let auth = token
            .map(|t| format!("Authorization: Bearer {t}\r\n"))
            .unwrap_or_default();
        let req = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        tcp.write_all(req.as_bytes()).await.unwrap();
        let mut out = String::new();
        tcp.read_to_string(&mut out).await.unwrap();
        let (head, rest) = out.split_once("\r\n\r\n").unwrap();
        let status = head.split(' ').nth(1).unwrap().parse().unwrap();
        let json = serde_json::from_str(rest.trim()).unwrap_or(Value::Null);
        (status, json)
    }

    async fn sign_in(&self, login: &str) -> String {
        let (status, body) = self
            .http(
                "POST",
                "/ui/auth/sign-in",
                None,
                Some(json!({"login": login, "password": format!("{login}-pass")})),
            )
            .await;
        assert_eq!(status, 200, "{body}");
        body["token"].as_str().unwrap().to_owned()
    }

    async fn pairing_code(&self, token: &str) -> String {
        let (status, body) = self
            .http("POST", "/ui/pairing-code", Some(token), None)
            .await;
        assert_eq!(status, 200, "{body}");
        body["code"].as_str().unwrap().to_owned()
    }

    fn device_count(&self) -> usize {
        Store::open_reader(&self.path)
            .unwrap()
            .devices()
            .unwrap()
            .len()
    }
}

struct NoBrowser;

impl Browser for NoBrowser {
    fn browse(&self) -> Result<Browse, String> {
        let (tx, rx) = mpsc::channel(1);
        Ok(Browse::new(rx, move || drop(tx)))
    }
}

struct Quiet;

impl AppLauncher for Quiet {
    fn restart(&self) -> Result<(), String> {
        Ok(())
    }

    fn start_desktop(&self) -> Result<(), String> {
        Ok(())
    }

    fn stop_desktop(&self) {}
}

fn fast() -> Timings {
    Timings {
        backoff_min: Duration::from_millis(50),
        backoff_max: Duration::from_millis(200),
        rejected_retry: Duration::from_millis(400),
        replay_retry: Duration::from_millis(200),
        ping_interval: Duration::from_millis(100),
        restart_grace: Duration::from_millis(400),
        admin_tick: Duration::from_millis(20),
        scan_tick: Duration::from_millis(20),
        cleanup_retry: Duration::from_millis(20),
    }
}

struct Kiosk {
    _dir: tempfile::TempDir,
    data: DataDir,
    runtime: Runtime,
}

impl Kiosk {
    fn start(hub: &Hub) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let data = DataDir(dir.path().join("kiosk"));
        std::fs::create_dir_all(data.root()).unwrap();
        let ipc = UnixIpcServer::bind(&data.socket()).unwrap();
        let discovery = Discovery::new(Arc::new(NoBrowser), Arc::new(NetProber))
            .with_window(Duration::from_millis(100))
            .with_web_port(hub.web.port());
        let runtime = Runtime::start_with_discovery(
            data.clone(),
            arena_kiosk_svc::secret::platform_store(data.token()),
            Arc::new(arena_kiosk_svc::clock::SystemClock),
            fast(),
            ipc,
            Arc::new(Quiet),
            discovery,
        )
        .unwrap();
        Self {
            _dir: dir,
            data,
            runtime,
        }
    }

    async fn wait_state(&self, want: KioskHubState) {
        let mut rx = self.runtime.hub_state();
        tokio::time::timeout(Duration::from_secs(20), rx.wait_for(|s| *s == want))
            .await
            .unwrap_or_else(|_| panic!("hub state never became {want:?}"))
            .unwrap();
    }

    async fn app(&self) -> App {
        let stream = UnixStream::connect(self.data.socket()).await.unwrap();
        let (r, w) = stream.into_split();
        App {
            reader: BufReader::new(r),
            writer: w,
            n: 0,
        }
    }
}

struct App {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
    n: u32,
}

impl App {
    async fn until(&mut self, mut pred: impl FnMut(&IpcMsg) -> bool) -> IpcMsg {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            assert!(
                Instant::now() < deadline,
                "the awaited ipc message never came"
            );
            let msg = tokio::time::timeout(Duration::from_secs(20), read_msg(&mut self.reader))
                .await
                .unwrap()
                .unwrap();
            if pred(&msg) {
                return msg;
            }
            if let IpcMsg::Ping { n } = msg {
                write_msg(&mut self.writer, &IpcMsg::Pong { n })
                    .await
                    .unwrap();
            }
        }
    }

    async fn request(&mut self, kind: &str, body: Value) -> (Value, Option<arena_proto::AppError>) {
        self.n += 1;
        let rid = format!("r{}", self.n);
        write_msg(
            &mut self.writer,
            &IpcMsg::Request {
                rid: rid.clone(),
                kind: kind.into(),
                body,
            },
        )
        .await
        .unwrap();
        match self
            .until(|m| matches!(m, IpcMsg::Reply { rid: r, .. } if *r == rid))
            .await
        {
            IpcMsg::Reply { body, error, .. } => (body, error),
            _ => unreachable!(),
        }
    }

    async fn choose(&mut self, hub: &Hub) {
        let (body, error) = self.request("probe_ip", json!({"ip": "127.0.0.1"})).await;
        assert_eq!(error, None);
        let (_, error) = self
            .request(
                "select_hub",
                json!({"addr": body["hub"]["addr"], "fingerprint": hub.fingerprint}),
            )
            .await;
        assert_eq!(error, None);
    }
}

async fn eventually<F: std::future::Future<Output = bool>>(
    what: &str,
    mut cond: impl FnMut() -> F,
) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        if cond().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("{what} never happened");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_code_from_the_hub_pairs_the_kiosk_which_shows_as_unassigned_and_remote_unlocks() {
    let hub = hub().await;
    let manager = hub.sign_in("manager").await;
    let code = hub.pairing_code(&manager).await;
    let kiosk = Kiosk::start(&hub);
    kiosk.wait_state(KioskHubState::Unpaired).await;
    let mut app = kiosk.app().await;
    let (info, error) = app.request("pair_info", Value::Null).await;
    assert_eq!(error, None);
    assert!(arena_proto::is_pc_name(info["pc_name"].as_str().unwrap()));
    assert_eq!(info["hub"], Value::Null);

    let (_, error) = app
        .request("pair", json!({"code": code, "pc_name": "PC-07"}))
        .await;
    assert_eq!(error.unwrap().code, ErrorCode::NoHub, "no hub chosen yet");
    app.choose(&hub).await;
    let (info, _) = app.request("pair_info", Value::Null).await;
    assert_eq!(info["hub"]["branch_name"], "Koramangala");

    let (_, error) = app
        .request("pair", json!({"code": "000000", "pc_name": "PC-07"}))
        .await;
    assert_eq!(error.unwrap().code, ErrorCode::PairCodeInvalid);
    let (_, error) = app
        .request("pair", json!({"pin": "3333", "pc_name": "PC-07"}))
        .await;
    assert_eq!(error.unwrap().code, ErrorCode::PinRejected);
    assert_eq!(hub.device_count(), 0);
    assert!(!kiosk.data.config().exists());

    let paired_at = Instant::now();
    let (body, error) = app
        .request("pair", json!({"code": code, "pc_name": "PC-07"}))
        .await;
    assert_eq!(error, None, "{body}");
    assert_eq!(body["branch_name"], "Koramangala");
    let device_id = body["device_id"].as_str().unwrap().to_owned();

    let config = KioskConfig::load(&kiosk.data.config()).unwrap().unwrap();
    assert_eq!(config.hub_addr, hub.lan.to_string());
    assert_eq!(config.hub_fingerprint, hub.fingerprint);
    assert_eq!(config.device_id, device_id);
    let stored = arena_kiosk_svc::secret::platform_store(kiosk.data.token())
        .load()
        .unwrap()
        .unwrap();
    assert!(
        String::from_utf8(stored)
            .unwrap()
            .starts_with(&format!("{device_id}."))
    );
    assert!(!kiosk.data.selected_hub().exists());
    let config_text = std::fs::read_to_string(kiosk.data.config()).unwrap();
    assert!(!config_text.contains("token"));

    kiosk.wait_state(KioskHubState::Connected).await;
    eventually("the paired kiosk on the floor as unassigned", || {
        let hub = &hub;
        let device_id = device_id.clone();
        async move {
            let (_, snap) = hub.http("GET", "/ui/snapshot", None, None).await;
            snap["devices"].as_array().is_some_and(|devices| {
                devices.iter().any(|d| {
                    d["device_id"] == device_id
                        && d["label"] == "PC-07"
                        && d["group_id"].is_null()
                        && d["kind"] == "pc"
                        && d["online"] == true
                })
            })
        }
    })
    .await;
    assert!(paired_at.elapsed() < Duration::from_secs(60));

    let mut acks = hub.presence.admin_unlock_acks().unwrap();
    let unlock = json!({ "intent": intent(
        Actor { kind: ActorKind::Staff, id: "ignored".into() },
        IntentPayload::DeviceAdminUnlock(arena_proto::DeviceAdminUnlock { device_id: device_id.clone() }),
    ) });
    let (status, applied) = hub
        .http("POST", "/ui/intent", Some(&manager), Some(unlock))
        .await;
    assert_eq!(status, 200, "{applied}");
    let exit_id = applied["applied"]["changes"][0]["row_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let ack = tokio::time::timeout(Duration::from_secs(5), acks.recv())
        .await
        .expect("an ack within 5 s")
        .unwrap();
    assert_eq!(
        (ack.exit_id.as_str(), ack.device_id.as_str()),
        (exit_id.as_str(), device_id.as_str())
    );
    app.until(|m| matches!(m, IpcMsg::AdminExitStarted { .. }))
        .await;
    kiosk.runtime.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_manager_pin_pairs_and_a_tampered_fingerprint_is_refused_before_sending() {
    let hub = hub().await;
    let kiosk = Kiosk::start(&hub);
    kiosk.wait_state(KioskHubState::Unpaired).await;
    let mut app = kiosk.app().await;
    app.choose(&hub).await;
    let path = kiosk.data.selected_hub();
    let original = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, original.replace(&hub.fingerprint, &"ab".repeat(32))).unwrap();
    let (_, error) = app
        .request("pair", json!({"pin": "2222", "pc_name": "PC-PIN"}))
        .await;
    assert_eq!(error.unwrap().code, ErrorCode::NoHub);
    assert_eq!(hub.device_count(), 0);
    assert!(!kiosk.data.config().exists());

    std::fs::write(&path, original).unwrap();
    let (body, error) = app
        .request("pair", json!({"pin": "2222", "pc_name": "PC-PIN"}))
        .await;
    assert_eq!(error, None, "{body}");
    kiosk.wait_state(KioskHubState::Connected).await;
    assert_eq!(hub.device_count(), 1);
    let token_before = std::fs::read(kiosk.data.token()).unwrap();
    let (_, error) = app
        .request("pair", json!({"pin": "2222", "pc_name": "PC-PIN"}))
        .await;
    assert_eq!(error.unwrap().code, ErrorCode::AlreadySet);
    assert_eq!(std::fs::read(kiosk.data.token()).unwrap(), token_before);
    assert_eq!(hub.device_count(), 1);
    kiosk.runtime.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn five_wrong_pins_pause_the_pin_option_with_a_countdown() {
    let hub = hub().await;
    let kiosk = Kiosk::start(&hub);
    kiosk.wait_state(KioskHubState::Unpaired).await;
    let mut app = kiosk.app().await;
    app.choose(&hub).await;
    for _ in 0..4 {
        let (_, error) = app
            .request("pair", json!({"pin": "9999", "pc_name": "PC-1"}))
            .await;
        assert_eq!(error.unwrap().code, ErrorCode::PinRejected);
    }
    let (body, error) = app
        .request("pair", json!({"pin": "9999", "pc_name": "PC-1"}))
        .await;
    assert_eq!(error.unwrap().code, ErrorCode::PairPinPaused);
    assert_eq!(body["retry_after_s"], 60);
    kiosk.runtime.shutdown().await;
}
