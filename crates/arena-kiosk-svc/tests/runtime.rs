//! The Kiosk service against a real, in-process Hub LAN server (`arena-api` on a seeded branch),
//! with the app side played by a test client on the Unix socket. One test per non-installer row
//! of the Story 1.16 matrix (the backoff bounds and the pipe ACL rows are unit tests in
//! `arena-kiosk-core::backoff` and `ipc::acl`).

#![cfg(unix)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use arena_api::lan::{HubCert, LanHub, Presence, bind_lan, serve_lan};
use arena_core::devices::{format_token, hash_token_secret, new_token_secret};
use arena_core::{
    DeviceDirectory, HubStatus, LeaseInfo, Role, SystemClock, Writer, WriterHandle, status_channel,
};
use arena_kiosk_core::ipc::{IpcError, IpcMsg, read_msg, write_msg};
use arena_kiosk_svc::config::KioskConfig;
use arena_kiosk_svc::ipc::UnixIpcServer;
use arena_kiosk_svc::launcher::AppLauncher;
use arena_kiosk_svc::paths::DataDir;
use arena_kiosk_svc::{Runtime, Timings};
use arena_proto::{
    Actor, ActorKind, BranchBootstrap, ClientClass, DeviceRegister, DeviceRevoke, Intent,
    IntentPayload, KioskHubState, KioskLogLevel,
};
use arena_store::{LeaseRow, Store, Table, write_lease};
use tokio::io::BufReader;
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;

const HUB_ID: &str = "01JHUBAAAAAAAAAAAAAAAAAAAA";

// ---- log capture (for the pin-mismatch error line) ----

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

fn logs() -> &'static Captured {
    static LOGS: OnceLock<Captured> = OnceLock::new();
    LOGS.get_or_init(|| {
        let captured = Captured::default();
        let writer = captured.clone();
        tracing_subscriber::fmt()
            .json()
            .with_writer(move || writer.clone())
            .with_max_level(tracing::Level::INFO)
            .init();
        captured
    })
}

fn log_text() -> String {
    String::from_utf8_lossy(&logs().0.lock().unwrap()).into_owned()
}

// ---- a seeded hub ----

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
        reason: payload.kind().reasons().map(|r| r[0]),
        payload,
    }
}

fn owner() -> Actor {
    Actor {
        kind: ActorKind::Owner,
        id: "test-owner".into(),
    }
}

struct Hub {
    _dir: tempfile::TempDir,
    path: PathBuf,
    writer: WriterHandle,
    status: watch::Receiver<HubStatus>,
    _status_tx: watch::Sender<HubStatus>,
    cert: HubCert,
    device_id: String,
    token: String,
    lan_port: u16,
    /// Serve as a follower (no writer: every intent is NOT_PRIMARY).
    follower: bool,
    presence: Option<Presence>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl Hub {
    /// A seeded branch with one paired device. Not listening yet.
    async fn seeded() -> Self {
        logs();
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
        let (status_tx, status) = status_channel(initial);
        let writer = Writer::spawn(store, Arc::new(SystemClock), status.clone()).unwrap();
        let system = Actor {
            kind: ActorKind::System,
            id: "test".into(),
        };
        writer
            .submit_intent(intent(
                system,
                IntentPayload::BranchBootstrap(BranchBootstrap {
                    name: "Dev Branch".into(),
                    branch_code: "DEV01".into(),
                    currency: "INR".into(),
                    tz: "Asia/Kolkata".into(),
                    day_start_min: 360,
                    fy_start_month: 4,
                }),
            ))
            .await
            .unwrap();
        let secret = new_token_secret().unwrap();
        let applied = writer
            .submit_intent(intent(
                owner(),
                IntentPayload::DeviceRegister(DeviceRegister {
                    class: ClientClass::Paired,
                    label: "Dev Kiosk".into(),
                    token_hash: hash_token_secret(&secret).unwrap(),
                }),
            ))
            .await
            .unwrap();
        let device_id = applied.changes[0].row_id.clone();
        let token = format_token(&device_id, &secret);
        Self {
            _dir: dir,
            path,
            writer,
            status,
            _status_tx: status_tx,
            cert: HubCert::generate(None).unwrap(),
            device_id,
            token,
            lan_port: free_port(),
            follower: false,
            presence: None,
            stop: None,
            task: None,
        }
    }

    /// Starts (or restarts) the LAN server on the same port.
    async fn up(&mut self) {
        let lan = Arc::new(LanHub::new(
            (!self.follower).then(|| self.writer.clone()),
            self.status.clone(),
            Arc::new(SystemClock),
            Arc::new(DeviceDirectory::open(&self.path).unwrap()),
            self.cert.server_config().unwrap(),
            env!("CARGO_PKG_VERSION"),
        ));
        self.presence = Some(lan.presence());
        let listeners = loop {
            match bind_lan(IpAddr::V4(Ipv4Addr::LOCALHOST), self.lan_port, 0).await {
                Ok(l) => break l,
                Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        };
        let (tx, rx) = oneshot::channel::<()>();
        self.stop = Some(tx);
        self.task = Some(tokio::spawn(serve_lan(listeners, lan, async {
            let _ = rx.await;
        })));
    }

    /// Stops the LAN server (every session is closed).
    async fn down(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
        if let Some(t) = self.task.take() {
            let _ = tokio::time::timeout(Duration::from_secs(5), t).await;
        }
    }

    fn online(&self) -> bool {
        self.presence
            .as_ref()
            .is_some_and(|p| p.online(&self.device_id))
    }

    fn addr(&self) -> String {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), self.lan_port).to_string()
    }

    /// The stored codes among `wanted`, in change-log order (the watchdog may log its own
    /// `app_restarted` / `no_session` in tests where no app connects).
    fn logged_of(&self, wanted: &[&str]) -> Vec<String> {
        self.logged_codes()
            .into_iter()
            .filter(|c| wanted.contains(&c.as_str()))
            .collect()
    }

    /// The `kiosk.log` codes the Hub stored, in change-log order.
    fn logged_codes(&self) -> Vec<String> {
        let store = Store::open_reader(&self.path).unwrap();
        let rows = store.rows(Table::DeviceEvents).unwrap();
        let mut by_id = std::collections::HashMap::new();
        for r in rows {
            let (Some(arena_store::RowValue::Text(id)), Some(arena_store::RowValue::Text(code))) =
                (r.get("event_id"), r.get("code"))
            else {
                panic!("{r:?}")
            };
            by_id.insert(id.clone(), code.clone());
        }
        store
            .changes()
            .unwrap()
            .into_iter()
            .filter(|c| c.tbl == "device_events")
            .map(|c| by_id[&c.row_id].clone())
            .collect()
    }

    /// Registers another paired device; returns its id and token.
    async fn register(&self) -> (String, String) {
        let secret = new_token_secret().unwrap();
        let applied = self
            .writer
            .submit_intent(intent(
                owner(),
                IntentPayload::DeviceRegister(DeviceRegister {
                    class: ClientClass::Paired,
                    label: "Replacement Kiosk".into(),
                    token_hash: hash_token_secret(&secret).unwrap(),
                }),
            ))
            .await
            .unwrap();
        let id = applied.changes[0].row_id.clone();
        let token = format_token(&id, &secret);
        (id, token)
    }

    async fn revoke(&self) {
        self.writer
            .submit_intent(intent(
                owner(),
                IntentPayload::DeviceRevoke(DeviceRevoke {
                    device_id: self.device_id.clone(),
                }),
            ))
            .await
            .unwrap();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

// ---- the kiosk service ----

#[derive(Default)]
struct MockLauncher {
    restarts: AtomicUsize,
}

impl AppLauncher for MockLauncher {
    fn restart(&self) -> Result<(), String> {
        self.restarts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn fast() -> Timings {
    Timings {
        backoff_min: Duration::from_millis(50),
        backoff_max: Duration::from_millis(200),
        rejected_retry: Duration::from_millis(400),
        replay_retry: Duration::from_millis(200),
        ping_interval: Duration::from_millis(100),
        restart_grace: Duration::from_millis(400),
    }
}

struct Kiosk {
    _dir: tempfile::TempDir,
    data: DataDir,
    runtime: Option<Runtime>,
    launcher: Arc<MockLauncher>,
}

fn provision(data: &DataDir, hub: &Hub, fingerprint: &str) {
    std::fs::create_dir_all(data.root()).unwrap();
    KioskConfig {
        hub_addr: hub.addr(),
        hub_fingerprint: fingerprint.into(),
        device_id: hub.device_id.clone(),
        player_user: "player".into(),
        app_path: None,
    }
    .save(&data.config())
    .unwrap();
    arena_kiosk_svc::secret::platform_store(data.token())
        .save(hub.token.as_bytes())
        .unwrap();
}

impl Kiosk {
    fn new(hub: &Hub) -> Self {
        Self::with_fingerprint(hub, &hub.cert.fingerprint)
    }

    fn with_fingerprint(hub: &Hub, fingerprint: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let data = DataDir(dir.path().join("kiosk"));
        provision(&data, hub, fingerprint);
        Self {
            _dir: dir,
            data,
            runtime: None,
            launcher: Arc::default(),
        }
    }

    fn start(&mut self) {
        let ipc = UnixIpcServer::bind(&self.data.socket()).unwrap();
        let runtime = Runtime::start(
            self.data.clone(),
            arena_kiosk_svc::secret::platform_store(self.data.token()),
            Arc::new(arena_kiosk_svc::clock::SystemClock),
            fast(),
            ipc,
            Arc::clone(&self.launcher) as Arc<dyn AppLauncher>,
        )
        .unwrap();
        self.runtime = Some(runtime);
    }

    fn rt(&self) -> &Runtime {
        self.runtime.as_ref().unwrap()
    }

    async fn wait_state(&self, want: KioskHubState) {
        let mut rx = self.rt().hub_state();
        tokio::time::timeout(Duration::from_secs(10), rx.wait_for(|s| *s == want))
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
        }
    }
}

/// The app side of the socket.
struct App {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
}

impl App {
    async fn recv(&mut self) -> Result<IpcMsg, IpcError> {
        tokio::time::timeout(Duration::from_secs(10), read_msg(&mut self.reader))
            .await
            .expect("an ipc message in time")
    }

    async fn send(&mut self, msg: &IpcMsg) {
        write_msg(&mut self.writer, msg).await.unwrap();
    }

    /// Reads until `pred` matches, answering pings on the way.
    async fn until(&mut self, mut pred: impl FnMut(&IpcMsg) -> bool) -> IpcMsg {
        loop {
            let msg = self.recv().await.unwrap();
            if pred(&msg) {
                return msg;
            }
            if let IpcMsg::Ping { n } = msg {
                self.send(&IpcMsg::Pong { n }).await;
            }
        }
    }
}

async fn eventually(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// ---- the matrix ----

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connect_shows_online_then_hub_down_and_back_reconnects() {
    let mut hub = Hub::seeded().await;
    hub.up().await;
    let mut kiosk = Kiosk::new(&hub);
    kiosk.start();
    kiosk.wait_state(KioskHubState::Connected).await;
    eventually("the device online", || hub.online()).await;

    let mut app = kiosk.app().await;
    assert_eq!(
        app.recv().await.unwrap(),
        IpcMsg::HubStatus {
            state: KioskHubState::Connected
        }
    );

    hub.down().await;
    kiosk.wait_state(KioskHubState::Reconnecting).await;
    app.until(|m| {
        *m == IpcMsg::HubStatus {
            state: KioskHubState::Reconnecting,
        }
    })
    .await;

    hub.up().await;
    kiosk.wait_state(KioskHubState::Connected).await;
    app.until(|m| {
        *m == IpcMsg::HubStatus {
            state: KioskHubState::Connected,
        }
    })
    .await;
    eventually("the device online again", || hub.online()).await;
    kiosk.runtime.take().unwrap().shutdown().await;
    eventually("the device offline", || !hub.online()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wrong_pin_is_refused_with_both_fingerprints_and_no_fallback() {
    let mut hub = Hub::seeded().await;
    hub.up().await;
    let wrong = HubCert::generate(None).unwrap().fingerprint;
    let mut kiosk = Kiosk::with_fingerprint(&hub, &wrong);
    kiosk.start();
    let real = hub.cert.fingerprint.clone();
    eventually("the pin error line", || {
        let text = log_text();
        text.lines()
            .any(|l| l.contains("\"level\":\"ERROR\"") && l.contains(&wrong) && l.contains(&real))
    })
    .await;
    // It keeps retrying with back-off, and never gets in.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        *kiosk.rt().hub_state().borrow(),
        KioskHubState::Reconnecting
    );
    assert!(!hub.online());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn events_queued_offline_replay_in_creation_order_once_each() {
    let mut hub = Hub::seeded().await;
    let mut kiosk = Kiosk::new(&hub);
    kiosk.start();
    let ctx = Arc::clone(kiosk.rt().ctx());
    for code in ["first", "second", "third"] {
        ctx.log_event(KioskLogLevel::Info, code, "").unwrap();
    }
    assert_eq!(ctx.queue_len(), 3);
    // Persisted: the file holds them while the Hub is down.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(ctx.queue_len() >= 3);
    assert_eq!(
        *kiosk.rt().hub_state().borrow(),
        KioskHubState::Reconnecting
    );

    hub.up().await;
    kiosk.wait_state(KioskHubState::Connected).await;
    eventually("the queue drained", || ctx.queue_len() == 0).await;
    let all = ["first", "second", "third", "fourth"];
    assert_eq!(hub.logged_of(&all), ["first", "second", "third"]);

    // An event while connected goes straight through.
    ctx.log_event(KioskLogLevel::Warn, "fourth", "").unwrap();
    eventually("the live event", || hub.logged_of(&all).len() == 4).await;
    assert_eq!(hub.logged_of(&all)[3], "fourth");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_replay_after_a_crash_before_the_local_delete_adds_no_duplicate() {
    let mut hub = Hub::seeded().await;
    hub.up().await;
    let kiosk_dir = tempfile::tempdir().unwrap();
    let data = DataDir(kiosk_dir.path().join("kiosk"));
    provision(&data, &hub, &hub.cert.fingerprint);
    // The intent reached the Hub and was applied, but the kiosk crashed before deleting it.
    let queued = arena_kiosk_svc::intents::kiosk_log(
        &hub.device_id,
        now_ms(),
        KioskLogLevel::Error,
        "crashed",
        "",
    );
    hub.writer.submit_intent(queued.clone()).await.unwrap();
    arena_kiosk_svc::queue::QueueDb::open(&data.queue())
        .unwrap()
        .push(&queued)
        .unwrap();
    assert_eq!(hub.logged_of(&["crashed"]), ["crashed"]);

    let mut kiosk = Kiosk {
        _dir: kiosk_dir,
        data,
        runtime: None,
        launcher: Arc::default(),
    };
    kiosk.start();
    kiosk.wait_state(KioskHubState::Connected).await;
    let ctx = Arc::clone(kiosk.rt().ctx());
    eventually("the queue drained", || ctx.queue_len() == 0).await;
    assert_eq!(
        hub.logged_of(&["crashed"]),
        ["crashed"],
        "the stored outcome, no new row"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_revoked_device_reports_token_rejected_and_stops_fast_reconnects() {
    let mut hub = Hub::seeded().await;
    hub.up().await;
    let mut kiosk = Kiosk::new(&hub);
    kiosk.start();
    kiosk.wait_state(KioskHubState::Connected).await;
    let mut app = kiosk.app().await;
    app.until(|m| matches!(m, IpcMsg::HubStatus { .. })).await;

    hub.revoke().await;
    kiosk.wait_state(KioskHubState::TokenRejected).await;
    app.until(|m| {
        *m == IpcMsg::HubStatus {
            state: KioskHubState::TokenRejected,
        }
    })
    .await;
    // Slow retries only: still rejected after several of them, and never online.
    for _ in 0..4 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert_eq!(
            *kiosk.rt().hub_state().borrow(),
            KioskHubState::TokenRejected
        );
        assert!(!hub.online());
    }
    let refusals = log_text()
        .lines()
        .filter(|l| l.contains(&hub.device_id) && l.contains("provision this kiosk again"))
        .count();
    // ~1 s at one try per 400 ms: a handful, not the dozens fast back-off would make.
    assert!((1..=5).contains(&refusals), "{refusals} refusals logged");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_watchdog_restarts_a_silent_app_and_restores_its_state() {
    let hub = Hub::seeded().await;
    let mut kiosk = Kiosk::new(&hub);
    kiosk.start();
    let mut app = kiosk.app().await;
    app.until(|m| matches!(m, IpcMsg::HubStatus { .. })).await;
    let state = serde_json::json!({"route": "/play/console-3"});
    app.send(&IpcMsg::AppState {
        json: state.clone(),
    })
    .await;
    // Answering: several pings (more than 3 intervals), no restart.
    for _ in 0..6 {
        let IpcMsg::Ping { n } = app.until(|m| matches!(m, IpcMsg::Ping { .. })).await else {
            unreachable!()
        };
        app.send(&IpcMsg::Pong { n }).await;
    }
    assert_eq!(kiosk.launcher.restarts.load(Ordering::SeqCst), 0);

    // Hung: it stops answering, and after 3 missed pings it is restarted and its link closed.
    loop {
        match app.recv().await {
            Ok(IpcMsg::Ping { .. }) => {}
            Ok(other) => panic!("{other:?}"),
            Err(IpcError::Closed) => break,
            Err(e) => panic!("{e}"),
        }
    }
    eventually("the restart", || {
        kiosk.launcher.restarts.load(Ordering::SeqCst) == 1
    })
    .await;
    let ctx = Arc::clone(kiosk.rt().ctx());
    assert_eq!(ctx.queue_len(), 1, "the restart is queued as a kiosk.log");

    // The relaunched app gets the link state, then its last state back.
    let mut again = kiosk.app().await;
    assert!(matches!(
        again.recv().await.unwrap(),
        IpcMsg::HubStatus { .. }
    ));
    assert_eq!(
        again.recv().await.unwrap(),
        IpcMsg::RestoreState { json: state }
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_newest_app_connection_wins_and_requests_are_answered() {
    let hub = Hub::seeded().await;
    let mut kiosk = Kiosk::new(&hub);
    kiosk.start();
    let mut first = kiosk.app().await;
    first.until(|m| matches!(m, IpcMsg::HubStatus { .. })).await;
    let mut second = kiosk.app().await;
    second
        .until(|m| matches!(m, IpcMsg::HubStatus { .. }))
        .await;
    // The older connection is closed.
    loop {
        match first.recv().await {
            Err(IpcError::Closed) => break,
            Ok(IpcMsg::Ping { .. }) => {}
            other => panic!("{other:?}"),
        }
    }
    second
        .send(&IpcMsg::Request {
            rid: "r1".into(),
            kind: "log".into(),
            body: serde_json::json!({"level": "info", "code": "app_started"}),
        })
        .await;
    let reply = second.until(|m| matches!(m, IpcMsg::Reply { .. })).await;
    assert_eq!(
        reply,
        IpcMsg::Reply {
            rid: "r1".into(),
            body: serde_json::json!({}),
            error: None
        }
    );
    assert_eq!(kiosk.rt().ctx().queue_len(), 1);
    second
        .send(&IpcMsg::Request {
            rid: "r2".into(),
            kind: "shell".into(),
            body: serde_json::Value::Null,
        })
        .await;
    let IpcMsg::Reply { rid, error, .. } =
        second.until(|m| matches!(m, IpcMsg::Reply { .. })).await
    else {
        unreachable!()
    };
    assert_eq!(rid, "r2");
    assert_eq!(error.unwrap().code, arena_proto::ErrorCode::Unsupported);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_follower_hub_keeps_the_queue_until_a_primary_applies_it_in_order() {
    let mut hub = Hub::seeded().await;
    hub.follower = true;
    hub.up().await;
    let mut kiosk = Kiosk::new(&hub);
    kiosk.start();
    kiosk.wait_state(KioskHubState::Connected).await;
    let ctx = Arc::clone(kiosk.rt().ctx());
    for code in ["one", "two", "three"] {
        ctx.log_event(KioskLogLevel::Info, code, "").unwrap();
    }
    // NOT_PRIMARY is transient: nothing is dropped, however often it is retried.
    tokio::time::sleep(Duration::from_millis(800)).await;
    // (No app connects in this test, so the watchdog may have queued `app_restarted` too.)
    assert!(ctx.queue_len() >= 3);
    assert!(hub.logged_codes().is_empty());

    hub.down().await;
    hub.follower = false;
    hub.up().await;
    eventually("the queue drained", || ctx.queue_len() == 0).await;
    assert_eq!(
        hub.logged_of(&["one", "two", "three"]),
        ["one", "two", "three"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_re_provision_after_revoke_reconnects_without_a_restart() {
    let mut hub = Hub::seeded().await;
    hub.up().await;
    let mut kiosk = Kiosk::new(&hub);
    kiosk.start();
    kiosk.wait_state(KioskHubState::Connected).await;
    hub.revoke().await;
    kiosk.wait_state(KioskHubState::TokenRejected).await;

    let (id, token) = hub.register().await;
    hub.device_id = id;
    hub.token = token;
    provision(&kiosk.data, &hub, &hub.cert.fingerprint);
    kiosk.wait_state(KioskHubState::Connected).await;
    eventually("the new device online", || hub.online()).await;
}
