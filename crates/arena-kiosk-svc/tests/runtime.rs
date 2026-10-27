//! The Kiosk service against a real, in-process Hub LAN server (`arena-api` on a seeded branch),
//! with the app side played by a test client on the Unix socket. One test per non-installer row
//! of the Story 1.16 matrix (the backoff bounds and the pipe ACL rows are unit tests in
//! `arena-kiosk-core::backoff` and `ipc::acl`). The Story 1.18 process enforcement runs here
//! too, on the fake process table (`procs::fake`), against the same Hub.

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
                    onboarding: None,
                }),
            ))
            .await
            .unwrap();
        writer
            .submit_intent(intent(
                Actor {
                    kind: ActorKind::System,
                    id: "test".into(),
                },
                IntentPayload::LicenceApply(arena_proto::LicenceApply {
                    standing: arena_proto::LicenceStanding::Verified,
                    clock_wrong: false,
                    flags: String::new(),
                    device_cap: 20,
                    user_cap: 10,
                }),
            ))
            .await
            .unwrap();
        let secret = new_token_secret().unwrap();
        let applied = writer
            .submit_intent(intent(
                owner(),
                IntentPayload::DeviceRegister(DeviceRegister {
                    device_id: None,
                    device_kind: Default::default(),
                    class: ClientClass::Paired,
                    label: "Dev Kiosk".into(),
                    token_hash: hash_token_secret(&secret).unwrap(),
                    tv_number: None,
                    group_id: None,
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
        let lan = Arc::new(
            LanHub::new(
                (!self.follower).then(|| self.writer.clone()),
                self.status.clone(),
                Arc::new(SystemClock),
                Arc::new(DeviceDirectory::open(&self.path).unwrap()),
                self.cert.server_config().unwrap(),
                env!("CARGO_PKG_VERSION"),
            )
            .with_staff(Arc::new(
                arena_core::StaffDirectory::open(&self.path).unwrap(),
            )),
        );
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
        let changes = store.changes().unwrap();
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
        changes
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
                    device_id: None,
                    device_kind: Default::default(),
                    class: ClientClass::Paired,
                    label: "Replacement Kiosk".into(),
                    token_hash: hash_token_secret(&secret).unwrap(),
                    tv_number: None,
                    group_id: None,
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
    desktops: AtomicUsize,
    desktop_stops: AtomicUsize,
}

impl AppLauncher for MockLauncher {
    fn restart(&self) -> Result<(), String> {
        self.restarts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn start_desktop(&self) -> Result<(), String> {
        self.desktops.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn stop_desktop(&self) {
        self.desktop_stops.fetch_add(1, Ordering::SeqCst);
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
        admin_tick: Duration::from_millis(20),
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
        self.start_with_clock(Arc::new(arena_kiosk_svc::clock::SystemClock));
    }

    fn start_with_clock(&mut self, clock: Arc<dyn arena_kiosk_svc::clock::Clock>) {
        let ipc = UnixIpcServer::bind(&self.data.socket()).unwrap();
        let runtime = Runtime::start(
            self.data.clone(),
            arena_kiosk_svc::secret::platform_store(self.data.token()),
            clock,
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
            skipped: Vec::new(),
        }
    }
}

/// The app side of the socket.
struct App {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
    /// Messages `until` passed over, for a later `until` to find.
    skipped: Vec<IpcMsg>,
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
        if let Some(i) = self.skipped.iter().position(&mut pred) {
            return self.skipped.remove(i);
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            assert!(
                Instant::now() < deadline,
                "the awaited ipc message never came"
            );
            let msg = self.recv().await.unwrap();
            if pred(&msg) {
                return msg;
            }
            match msg {
                IpcMsg::Ping { n } => self.send(&IpcMsg::Pong { n }).await,
                other => self.skipped.push(other),
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

    // The relaunched app gets the link state and its theme, then its last state back.
    let mut again = kiosk.app().await;
    assert!(matches!(
        again.recv().await.unwrap(),
        IpcMsg::HubStatus { .. }
    ));
    assert_eq!(again.recv().await.unwrap(), IpcMsg::Theme { hc: false });
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
            Ok(
                IpcMsg::Ping { .. }
                | IpcMsg::Theme { .. }
                | IpcMsg::AdminExitEnded
                | IpcMsg::TestSessionEnded,
            ) => {}
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

// ---- Story 1.18: process allow/block enforcement (fake process table) ----

use arena_kiosk_core::ipc::TOAST_APP_BLOCKED;
use arena_kiosk_svc::procs::fake::{FakeProc, FakeSystem};
use arena_kiosk_svc::procs::{EnforceOptions, ProcessApi};
use arena_proto::{IntentKind, KEY_PROCESS_ALLOW, KEY_PROCESS_MODE, ProcessRule, SettingsSet};

const PLAYER_SID: &str = "S-1-5-21-1000-2000-3000-1001";
const OTHER_SID: &str = "S-1-5-21-1000-2000-3000-1002";
const SYSTEM_SID: &str = "S-1-5-18";
const INSTALL: &str = r"C:\Program Files\Arena 360 Kiosk";

/// The player's shell-launched process (pid 2, an allowed Steam client, plays the launcher).
const LAUNCHER: u32 = 2;

fn player_proc(path: &str) -> FakeProc {
    FakeProc {
        session_id: 1,
        user_sid: PLAYER_SID.into(),
        path: path.into(),
        parent: Some(LAUNCHER),
    }
}

fn fake_system() -> Arc<FakeSystem> {
    let sys = Arc::new(FakeSystem::default());
    sys.set_player("player", PLAYER_SID, 1);
    sys.add_running(
        LAUNCHER,
        FakeProc {
            parent: None,
            ..player_proc(r"C:\Windows\System32\userinit.exe")
        },
    );
    sys
}

fn enforce_opts() -> EnforceOptions {
    EnforceOptions {
        install_dir: Some(PathBuf::from(INSTALL)),
        app_image: Some(PathBuf::from(format!(r"{INSTALL}\arena-kiosk-ui.exe"))),
        app_config: None,
        own_images: vec![PathBuf::from(format!(r"{INSTALL}\arena-kiosk-svc.exe"))],
        system_root: r"C:\Windows".into(),
    }
}

impl Kiosk {
    /// Caches `kiosk.process_mode` as if the Hub had sent it before (rev 1), so the kiosk
    /// starts in that mode while the Hub is down.
    fn cache_mode(&self, mode: &str) {
        std::fs::create_dir_all(self.data.root()).unwrap();
        arena_kiosk_svc::queue::QueueDb::open(&self.data.queue())
            .unwrap()
            .kv_set(KEY_PROCESS_MODE, mode, 1)
            .unwrap();
    }

    fn enforce(&mut self, sys: &Arc<FakeSystem>) {
        self.runtime
            .as_mut()
            .unwrap()
            .enforce(ProcessApi::of(Arc::clone(sys)), enforce_opts());
    }

    /// The queued `kiosk.process_blocked` intents.
    fn blocked_queued(&self) -> usize {
        self.rt()
            .ctx()
            .queue
            .lock()
            .unwrap()
            .entries()
            .unwrap()
            .iter()
            .filter(|q| q.intent.kind == IntentKind::KioskProcessBlocked)
            .count()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_blocked_launch_is_killed_toasted_and_reported_once_after_reconnect() {
    let mut hub = Hub::seeded().await;
    let mut kiosk = Kiosk::new(&hub);
    kiosk.cache_mode("enforce");
    kiosk.start();
    let sys = fake_system();
    kiosk.enforce(&sys);
    eventually("the creation watch", || sys.subscribed()).await;
    let mut app = kiosk.app().await;
    app.until(|m| matches!(m, IpcMsg::HubStatus { .. })).await;

    // Offline: the Hub is down.
    sys.spawn(100, player_proc(r"C:\Windows\System32\cmd.exe"));
    app.until(|m| {
        *m == IpcMsg::Toast {
            key: TOAST_APP_BLOCKED.into(),
        }
    })
    .await;
    assert_eq!(sys.killed(), [100]);
    assert!(!sys.alive(100));
    eventually("the intent queued", || kiosk.blocked_queued() == 1).await;
    // The same program again at once: killed, but not reported a second time.
    sys.spawn(101, player_proc(r"C:\Windows\System32\cmd.exe"));
    eventually("the second kill", || sys.killed() == [100, 101]).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(kiosk.blocked_queued(), 1);

    hub.up().await;
    kiosk.wait_state(KioskHubState::Connected).await;
    eventually("the intent delivered", || kiosk.blocked_queued() == 0).await;
    assert_eq!(hub.logged_of(&["process_blocked"]), ["process_blocked"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn running_offenders_are_swept_at_start_and_a_list_change_applies_live() {
    let mut hub = Hub::seeded().await;
    hub.up().await;
    let mut kiosk = Kiosk::new(&hub);
    kiosk.cache_mode("enforce");
    let sys = fake_system();
    sys.add_running(200, player_proc(r"C:\Games\old.exe"));
    kiosk.start();
    let started = Instant::now();
    kiosk.enforce(&sys);
    eventually("the startup sweep", || sys.killed() == [200]).await;
    assert!(started.elapsed() < Duration::from_secs(2));

    // The service read the lists after hello and cached them.
    kiosk.wait_state(KioskHubState::Connected).await;
    let ctx = Arc::clone(kiosk.rt().ctx());
    let cached = || ctx.queue.lock().unwrap().kv_get(KEY_PROCESS_ALLOW).unwrap();
    eventually("the lists cached", || cached().is_some()).await;
    let (_, rev) = cached().unwrap();

    let games = ProcessRule {
        path_glob: r"C:\Games\*.exe".into(),
        signer: None,
    };
    let value = serde_json::to_string(std::slice::from_ref(&games)).unwrap();
    hub.writer
        .submit_intent(Intent {
            base_rev: Some(rev),
            ..intent(
                owner(),
                IntentPayload::SettingsSet(SettingsSet {
                    key: KEY_PROCESS_ALLOW.into(),
                    value,
                }),
            )
        })
        .await
        .unwrap();
    eventually("the pushed list applied", || {
        ctx.lists.borrow().allow == [games.clone()]
    })
    .await;
    sys.spawn(201, player_proc(r"C:\Games\new.exe"));
    sys.spawn(202, player_proc(r"C:\Temp\new.exe"));
    eventually("the disallowed one killed", || sys.killed().contains(&202)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(sys.alive(201), "allowed by the new rule, with no restart");

    // The cache holds across a restart with the Hub down.
    kiosk.runtime.take().unwrap().shutdown().await;
    hub.down().await;
    kiosk.start();
    assert_eq!(kiosk.rt().ctx().lists.borrow().allow, [games]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_the_players_own_processes_are_ever_killed() {
    let hub = Hub::seeded().await;
    let mut kiosk = Kiosk::new(&hub);
    kiosk.cache_mode("enforce");
    let sys = fake_system();
    let cmd = r"C:\Windows\System32\cmd.exe";
    sys.add_running(
        300,
        FakeProc {
            user_sid: SYSTEM_SID.into(),
            ..player_proc(cmd)
        },
    );
    sys.add_running(
        301,
        FakeProc {
            session_id: 2,
            ..player_proc(cmd)
        },
    );
    sys.add_running(
        302,
        FakeProc {
            user_sid: OTHER_SID.into(),
            ..player_proc(cmd)
        },
    );
    // The kiosk app itself, unsigned (a debug build): never killed.
    sys.add_running(303, player_proc(&format!(r"{INSTALL}\arena-kiosk-ui.exe")));
    // A critical Windows image somehow running as the player: never killed.
    sys.add_running(304, player_proc(r"C:\Windows\System32\svchost.exe"));
    sys.add_running(305, player_proc(cmd));
    kiosk.start();
    kiosk.enforce(&sys);
    eventually("the player's cmd killed", || sys.killed() == [305]).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(sys.killed(), [305]);
    // With no player logged on, nothing is considered at all.
    sys.set_player("someone-else", OTHER_SID, 1);
    sys.spawn(306, player_proc(cmd));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(sys.alive(306));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn signatures_decide_allowed_paths_and_are_cached_by_path_size_and_mtime() {
    let hub = Hub::seeded().await;
    let mut kiosk = Kiosk::new(&hub);
    kiosk.cache_mode("enforce");
    let sys = fake_system();
    let conhost = r"C:\Windows\System32\conhost.exe";
    let dllhost = r"C:\Windows\System32\dllhost.exe";
    let broker = r"C:\Windows\System32\RuntimeBroker.exe";
    sys.sign(conhost, "Microsoft Windows", true);
    sys.sign(dllhost, "Microsoft Windows", false);
    sys.sign(broker, "Not Microsoft", true);
    kiosk.start();
    kiosk.enforce(&sys);
    eventually("the creation watch", || sys.subscribed()).await;

    sys.spawn(400, player_proc(conhost));
    sys.spawn(401, player_proc(conhost));
    sys.spawn(402, player_proc(dllhost));
    sys.spawn(403, player_proc(broker));
    sys.spawn(404, player_proc(r"C:\Temp\arena-kiosk-ui.exe"));
    eventually("the three bad ones", || {
        let mut k = sys.killed();
        k.sort_unstable();
        k == [402, 403, 404]
    })
    .await;
    assert!(sys.alive(400) && sys.alive(401));
    // conhost verified once for two launches; a changed file is verified again.
    let before = sys.verifies();
    sys.stamp_file(conhost, 2, 2);
    sys.spawn(405, player_proc(conhost));
    eventually("the re-verification", || sys.verifies() == before + 1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(sys.alive(405));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn without_a_creation_watch_the_enforcer_polls() {
    let hub = Hub::seeded().await;
    let mut kiosk = Kiosk::new(&hub);
    kiosk.cache_mode("enforce");
    let sys = fake_system();
    sys.fail_subscribe();
    kiosk.start();
    kiosk.enforce(&sys);
    tokio::time::sleep(Duration::from_millis(100)).await;
    // No event is ever sent; the fallback poll still finds it.
    sys.add_running(500, player_proc(r"C:\Windows\System32\regedit.exe"));
    eventually("the polled kill", || sys.killed() == [500]).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn report_mode_from_the_hub_kills_nothing_and_reports_once() {
    let mut hub = Hub::seeded().await;
    hub.up().await;
    let mut kiosk = Kiosk::new(&hub);
    kiosk.cache_mode("enforce");
    kiosk.start();
    let sys = fake_system();
    kiosk.enforce(&sys);
    kiosk.wait_state(KioskHubState::Connected).await;
    let ctx = Arc::clone(kiosk.rt().ctx());
    let mode_rev = || ctx.queue.lock().unwrap().kv_get(KEY_PROCESS_MODE).unwrap();
    // The seeded branch enforces (bootstrap); its rev replaces the cached one.
    eventually("the hub's mode", || {
        mode_rev().is_some_and(|(m, _)| m == "enforce")
    })
    .await;
    let (_, rev) = mode_rev().unwrap();
    hub.writer
        .submit_intent(Intent {
            base_rev: Some(rev),
            ..intent(
                owner(),
                IntentPayload::SettingsSet(SettingsSet {
                    key: KEY_PROCESS_MODE.into(),
                    value: "report".into(),
                }),
            )
        })
        .await
        .unwrap();
    eventually("report mode applied", || {
        ctx.lists.borrow().mode == arena_proto::ProcessMode::Report
    })
    .await;
    let mut app = kiosk.app().await;
    app.until(|m| matches!(m, IpcMsg::HubStatus { .. })).await;
    sys.spawn(600, player_proc(r"C:\Windows\System32\cmd.exe"));
    eventually("the report delivered", || {
        hub.logged_of(&["process_blocked"]) == ["process_blocked"]
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(sys.alive(600));
    assert_eq!(sys.kill_attempts(), 0);
    assert_eq!(hub.logged_of(&["process_blocked"]).len(), 1);
    // No toast reached the app.
    while let Ok(Ok(m)) =
        tokio::time::timeout(Duration::from_millis(200), read_msg(&mut app.reader)).await
    {
        assert!(!matches!(m, IpcMsg::Toast { .. }), "{m:?}");
    }
}

// ---- Story 1.20: admin exit ----

impl Hub {
    /// Adds a staff member with a PIN in the built-in role `role`.
    async fn add_staff(&self, login: &str, role: &str, pin: &str) -> String {
        let role_id = Store::open_reader(&self.path)
            .unwrap()
            .rows(Table::Roles)
            .unwrap()
            .into_iter()
            .find(|r| r["name"] == arena_store::RowValue::Text(role.into()))
            .map(|r| match &r["role_id"] {
                arena_store::RowValue::Text(t) => t.clone(),
                other => panic!("{other:?}"),
            })
            .unwrap();
        let applied = self
            .writer
            .submit_intent(intent(
                owner(),
                IntentPayload::StaffUpsert(arena_proto::StaffUpsert {
                    staff_id: None,
                    login: login.into(),
                    display_name: login.into(),
                    role_id,
                    password_hash: Some(hash_token_secret("pw").unwrap()),
                    pin_hash: Some(hash_token_secret(pin).unwrap()),
                    active: true,
                    setup_code_hash: None,
                    setup_expires_at: None,
                }),
            ))
            .await
            .unwrap();
        applied.changes[0].row_id.clone()
    }

    fn exits(&self) -> Vec<arena_store::Row> {
        Store::open_reader(&self.path)
            .unwrap()
            .rows(Table::DeviceAdminExits)
            .unwrap()
    }
}

fn text(row: &arena_store::Row, col: &str) -> String {
    match &row[col] {
        arena_store::RowValue::Text(t) => t.clone(),
        other => panic!("{col} is {other:?}"),
    }
}

fn int(row: &arena_store::Row, col: &str) -> i64 {
    match &row[col] {
        arena_store::RowValue::Integer(n) => *n,
        other => panic!("{col} is {other:?}"),
    }
}

impl App {
    /// Sends `admin_exit{pin}` and returns the reply.
    async fn admin_exit(&mut self, pin: &str) -> IpcMsg {
        self.send(&IpcMsg::AdminExit {
            rid: "x".into(),
            pin: arena_kiosk_core::ipc::SecretPin::new(pin),
        })
        .await;
        self.until(|m| matches!(m, IpcMsg::Reply { rid, .. } if rid == "x"))
            .await
    }
}

fn reply_code(reply: &IpcMsg) -> Option<arena_proto::ErrorCode> {
    match reply {
        IpcMsg::Reply { error, .. } => error.as_ref().map(|e| e.code),
        other => panic!("{other:?}"),
    }
}

/// A kiosk connected to a hub with a Manager (2222) and a Counter (3333), holding the verifier.
async fn admin_setup() -> (Hub, Kiosk, String) {
    let mut hub = Hub::seeded().await;
    let manager = hub.add_staff("manager", "Manager", "2222").await;
    hub.add_staff("counter", "Counter", "3333").await;
    hub.up().await;
    let mut kiosk = Kiosk::new(&hub);
    kiosk.start();
    kiosk.wait_state(KioskHubState::Connected).await;
    let ctx = Arc::clone(kiosk.rt().ctx());
    eventually("the verifier", || ctx.admin.verifier_version().is_some()).await;
    (hub, kiosk, manager)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_online_exit_is_granted_recorded_with_its_approver_and_shown() {
    let (hub, kiosk, _) = admin_setup().await;
    // A PIN distinctive enough that no timestamp or id in the logs can contain it.
    let manager = hub.add_staff("manager9", "Manager", "731946").await;
    let ctx = Arc::clone(kiosk.rt().ctx());
    let before = ctx.admin.verifier_version();
    eventually("the newer verifier", || {
        ctx.admin.verifier_version() > before
    })
    .await;
    let mut app = kiosk.app().await;
    let reply = app.admin_exit("731946").await;
    assert_eq!(reply_code(&reply), None, "{reply:?}");
    let IpcMsg::Reply { body, .. } = &reply else {
        unreachable!()
    };
    let until = body["until"].as_i64().unwrap();
    assert!(until > now_ms() + 14 * 60_000);
    app.until(|m| matches!(m, IpcMsg::AdminExitStarted { .. }))
        .await;
    assert_eq!(kiosk.launcher.desktops.load(Ordering::SeqCst), 1);
    eventually("the exit at the hub", || !hub.exits().is_empty()).await;
    let row = &hub.exits()[0];
    assert_eq!(text(row, "mode"), "hub");
    assert_eq!(text(row, "staff_id"), manager);
    assert_eq!(int(row, "flagged"), 0);
    let change = Store::open_reader(&hub.path)
        .unwrap()
        .changes()
        .unwrap()
        .into_iter()
        .rfind(|c| c.tbl == "device_admin_exits")
        .unwrap();
    assert_eq!(change.elevated_by_mode.as_deref(), Some("hub"));
    assert!(!log_text().contains("731946"), "the PIN is never logged");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wrong_and_counter_pins_are_refused_and_five_pause_across_an_app_restart() {
    let (hub, kiosk, _) = admin_setup().await;
    let mut app = kiosk.app().await;
    for pin in ["9999", "3333"] {
        let reply = app.admin_exit(pin).await;
        assert_eq!(
            reply_code(&reply),
            Some(arena_proto::ErrorCode::PinRejected),
            "{pin}"
        );
    }
    eventually("the failures logged at the hub", || {
        hub.logged_of(&["admin_exit_failed"]).len() == 2
    })
    .await;
    assert_eq!(kiosk.launcher.desktops.load(Ordering::SeqCst), 0);
    for _ in 0..2 {
        app.admin_exit("9999").await;
    }
    let reply = app.admin_exit("9999").await;
    assert_eq!(reply_code(&reply), Some(arena_proto::ErrorCode::AuthLocked));
    let IpcMsg::Reply { body, .. } = &reply else {
        unreachable!()
    };
    assert_eq!(body["retry_after_s"], 60);
    // The app restarts: still paused, even for the right PIN.
    drop(app);
    let mut app = kiosk.app().await;
    app.send(&IpcMsg::Request {
        rid: "s".into(),
        kind: "admin_exit_status".into(),
        body: serde_json::Value::Null,
    })
    .await;
    let status = app
        .until(|m| matches!(m, IpcMsg::Reply { rid, .. } if rid == "s"))
        .await;
    let IpcMsg::Reply { body, .. } = status else {
        unreachable!()
    };
    assert!(body["retry_after_s"].as_u64().unwrap() > 50);
    let reply = app.admin_exit("2222").await;
    assert_eq!(reply_code(&reply), Some(arena_proto::ErrorCode::AuthLocked));
    assert!(hub.exits().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn offline_exits_use_the_cached_verifier_and_a_stale_one_is_flagged() {
    let (mut hub, kiosk, manager) = admin_setup().await;
    let ctx = Arc::clone(kiosk.rt().ctx());
    let v2 = ctx.admin.verifier_version().unwrap();
    assert!(kiosk.data.verifier().exists(), "stored in verifier.bin");
    // A verifier update while connected replaces the cached one.
    hub.add_staff("manager2", "Manager", "5555").await;
    eventually("the newer verifier", || {
        ctx.admin.verifier_version().is_some_and(|v| v > v2)
    })
    .await;
    let v3 = ctx.admin.verifier_version().unwrap();
    let mut app = kiosk.app().await;
    // Hub down: the cached verifier v3 grants it.
    hub.down().await;
    kiosk.wait_state(KioskHubState::Reconnecting).await;
    let reply = app.admin_exit("2222").await;
    assert_eq!(reply_code(&reply), None, "{reply:?}");
    // One exit at a time: the first one ends before the next.
    arena_kiosk_svc::admin_exit::relock(&ctx).await;
    // The Hub moves on to v4 while this kiosk is offline; the next offline exit used v3.
    hub.add_staff("manager3", "Manager", "6666").await;
    let reply = app.admin_exit("5555").await;
    assert_eq!(reply_code(&reply), None, "{reply:?}");
    assert!(hub.exits().is_empty(), "nothing reaches the hub offline");
    hub.up().await;
    eventually("both exits at the hub", || hub.exits().len() == 2).await;
    let mut rows = hub.exits();
    rows.sort_by_key(|r| int(r, "started_at"));
    assert_eq!(text(&rows[0], "mode"), "cached_verifier");
    assert_eq!(text(&rows[0], "staff_id"), manager);
    assert_eq!(int(&rows[0], "flagged"), 0);
    assert_eq!(int(&rows[1], "flagged"), 1);
    assert_eq!(text(&rows[1], "flag_reason"), "stale_verifier");
    let versions: Vec<Option<i64>> = Store::open_reader(&hub.path)
        .unwrap()
        .changes()
        .unwrap()
        .into_iter()
        .filter(|c| c.tbl == "device_admin_exits" && c.op == arena_store::ChangeOp::Insert)
        .map(|c| c.elevated_by_verifier_version)
        .collect();
    assert_eq!(versions, vec![Some(v3), Some(v3)]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_remote_unlock_is_granted_and_acknowledged() {
    let (hub, kiosk, _) = admin_setup().await;
    let mut app = kiosk.app().await;
    let mut acks = hub.presence.as_ref().unwrap().admin_unlock_acks().unwrap();
    let applied = hub
        .writer
        .submit_intent(intent(
            owner(),
            IntentPayload::DeviceAdminUnlock(arena_proto::DeviceAdminUnlock {
                device_id: hub.device_id.clone(),
            }),
        ))
        .await
        .unwrap();
    let exit_id = applied.changes[0].row_id.clone();
    let ack = tokio::time::timeout(Duration::from_secs(5), acks.recv())
        .await
        .expect("an ack within 5 s")
        .unwrap();
    assert_eq!(ack.exit_id, exit_id);
    app.until(|m| matches!(m, IpcMsg::AdminExitStarted { .. }))
        .await;
    assert_eq!(kiosk.launcher.desktops.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_exit_relocks_after_fifteen_minutes_and_a_restart_relocks_at_once() {
    let mut hub = Hub::seeded().await;
    hub.add_staff("manager", "Manager", "2222").await;
    hub.up().await;
    let mut kiosk = Kiosk::new(&hub);
    let clock = Arc::new(arena_kiosk_svc::clock::ManualClock::new(now_ms()));
    kiosk.start_with_clock(clock.clone());
    kiosk.wait_state(KioskHubState::Connected).await;
    let mut app = kiosk.app().await;
    assert_eq!(reply_code(&app.admin_exit("2222").await), None);
    app.until(|m| matches!(m, IpcMsg::AdminExitStarted { .. }))
        .await;
    eventually("the exit at the hub", || hub.exits().len() == 1).await;
    // Fake clock: 15 minutes later it re-locks.
    hub.down().await;
    clock.advance(arena_proto::ADMIN_EXIT_MS);
    app.until(|m| matches!(m, IpcMsg::AdminExitEnded)).await;
    let launcher = Arc::clone(&kiosk.launcher);
    eventually("the desktop stopped", || {
        launcher.desktop_stops.load(Ordering::SeqCst) == 1
    })
    .await;
    let ctx = Arc::clone(kiosk.rt().ctx());
    eventually("the relock queued", || {
        ctx.queue
            .lock()
            .unwrap()
            .entries()
            .unwrap()
            .iter()
            .any(|q| q.intent.kind == arena_proto::IntentKind::DeviceAdminRelock)
    })
    .await;
    let relocks = || {
        ctx.queue
            .lock()
            .unwrap()
            .entries()
            .unwrap()
            .into_iter()
            .filter(|q| q.intent.kind == arena_proto::IntentKind::DeviceAdminRelock)
            .count()
    };
    assert_eq!(relocks(), 1);

    // Another exit, then the service stops mid-exit: the restart re-locks at once.
    drop(app);
    clock.advance(1);
    let mut app = kiosk.app().await;
    assert_eq!(reply_code(&app.admin_exit("2222").await), None);
    let runtime = kiosk.runtime.take().unwrap();
    runtime.shutdown().await;
    drop(app);
    let _ = std::fs::remove_file(kiosk.data.socket());
    kiosk.start_with_clock(clock.clone());
    let ctx = Arc::clone(kiosk.rt().ctx());
    eventually("the restart re-lock", || {
        ctx.queue
            .lock()
            .unwrap()
            .entries()
            .unwrap()
            .into_iter()
            .filter(|q| q.intent.kind == arena_proto::IntentKind::DeviceAdminRelock)
            .count()
            == 2
    })
    .await;
    assert!(!ctx.admin.suspended());
    assert_eq!(kiosk.launcher.desktop_stops.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reconnecting_app_learns_whether_an_exit_runs() {
    let (_hub, kiosk, _) = admin_setup().await;
    let mut app = kiosk.app().await;
    assert_eq!(reply_code(&app.admin_exit("2222").await), None);
    app.until(|m| matches!(m, IpcMsg::AdminExitStarted { .. }))
        .await;
    // The app goes away and comes back during the exit: it is told the exit runs.
    drop(app);
    let mut app = kiosk.app().await;
    app.until(|m| matches!(m, IpcMsg::AdminExitStarted { .. }))
        .await;
    // The exit ends while no app is connected: the next connection is told it is over.
    drop(app);
    let ctx = Arc::clone(kiosk.rt().ctx());
    arena_kiosk_svc::admin_exit::relock(&ctx).await;
    let mut app = kiosk.app().await;
    app.until(|m| matches!(m, IpcMsg::AdminExitEnded)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pc_commands_cover_and_message_the_screen_once_per_command_and_are_acknowledged() {
    use arena_proto::PcAction;
    let (hub, kiosk, _) = admin_setup().await;
    let presence = hub.presence.as_ref().unwrap();
    let mut acks = presence.pc_acks().unwrap();
    let mut app = kiosk.app().await;
    let send = |cmd: &str, action, text: Option<&str>| {
        assert_eq!(
            presence.push_pc_command(
                &hub.device_id,
                cmd,
                action,
                text.map(Into::into),
                (action == PcAction::Lock).then_some(arena_proto::LockReason::LockedByStaff)
            ),
            1
        );
    };
    async fn acked(
        cmd: &str,
        acks: &mut tokio::sync::broadcast::Receiver<arena_proto::PcAckEvent>,
    ) {
        let ack = tokio::time::timeout(Duration::from_secs(5), acks.recv())
            .await
            .expect("an ack within 5 s")
            .unwrap();
        assert_eq!(ack.cmd_id, cmd);
    }
    send("L1", PcAction::Lock, None);
    acked("L1", &mut acks).await;
    let locked = |m: &IpcMsg| matches!(m, IpcMsg::PcScreen { locked: true, .. });
    app.until(locked).await;
    send("M1", PcAction::Message, Some("  Closing in 10  "));
    acked("M1", &mut acks).await;
    let shown = app
        .until(|m| {
            matches!(
                m,
                IpcMsg::PcScreen {
                    message: Some(_),
                    ..
                }
            )
        })
        .await;
    assert_eq!(
        shown,
        IpcMsg::PcScreen {
            locked: true,
            reason: Some(arena_proto::LockReason::LockedByStaff),
            message: Some("Closing in 10".into()),
            session: None
        }
    );
    app.send(&IpcMsg::Request {
        rid: "d".into(),
        kind: "pc_dismiss".into(),
        body: serde_json::Value::Null,
    })
    .await;
    app.until(|m| {
        matches!(
            m,
            IpcMsg::PcScreen {
                message: None,
                locked: true,
                ..
            }
        )
    })
    .await;
    send("M1", PcAction::Message, Some("Closing in 10"));
    acked("M1", &mut acks).await;
    let mut again = kiosk.app().await;
    let first = again.until(|m| matches!(m, IpcMsg::PcScreen { .. })).await;
    assert_eq!(
        first,
        IpcMsg::PcScreen {
            locked: true,
            reason: Some(arena_proto::LockReason::LockedByStaff),
            message: None,
            session: None
        },
        "a repeated command is acknowledged but not shown again; the lock survives a new app"
    );
    send("U1", PcAction::Unlock, None);
    acked("U1", &mut acks).await;
    again
        .until(|m| matches!(m, IpcMsg::PcScreen { locked: false, .. }))
        .await;
    send("R1", PcAction::Restart, None);
    assert!(
        tokio::time::timeout(Duration::from_millis(500), acks.recv())
            .await
            .is_err(),
        "a power command this OS cannot run is refused, not acknowledged"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_started_session_unlocks_ticks_and_locks_at_its_lock_time() {
    let (hub, kiosk, _) = admin_setup().await;
    let presence = hub.presence.as_ref().unwrap();
    let mut acks = presence.pc_acks().unwrap();
    let mut app = kiosk.app().await;
    let screen = |elapsed_ms, lock_after_ms| arena_proto::SessionScreen {
        session_id: "S1".into(),
        device_name: "PC 1".into(),
        elapsed_ms,
        lock_after_ms,
        time_up_mode: "clean_up".into(),
    };
    assert_eq!(
        presence.push_pc_command(
            &hub.device_id,
            "L1",
            arena_proto::PcAction::Lock,
            None,
            Some(arena_proto::LockReason::LockedByStaff)
        ),
        1
    );
    app.until(|m| matches!(m, IpcMsg::PcScreen { locked: true, .. }))
        .await;
    assert_eq!(
        presence.push_session_started(&hub.device_id, "S1", screen(60_000, 1_500)),
        1
    );
    let mut got = false;
    while !got {
        let ack = tokio::time::timeout(Duration::from_secs(5), acks.recv())
            .await
            .expect("an ack within 5 s")
            .unwrap();
        got = ack.cmd_id == "S1";
    }
    let shown = app
        .until(|m| {
            matches!(
                m,
                IpcMsg::PcScreen {
                    session: Some(_),
                    ..
                }
            )
        })
        .await;
    let IpcMsg::PcScreen {
        locked,
        reason,
        session: Some(session),
        ..
    } = shown
    else {
        panic!("a session screen")
    };
    assert!(!locked && reason.is_none());
    assert_eq!(session.device_name, "PC 1");
    assert!((60_000..62_000).contains(&session.elapsed_ms));
    let ended = app
        .until(|m| matches!(m, IpcMsg::PcScreen { locked: true, .. }))
        .await;
    assert_eq!(
        ended,
        IpcMsg::PcScreen {
            locked: true,
            reason: Some(arena_proto::LockReason::TimesUp),
            message: None,
            session: None
        }
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_started_test_session_reaches_the_app_with_two_minutes_on_the_clock() {
    let mut hub = Hub::seeded().await;
    hub.up().await;
    let mut kiosk = Kiosk::new(&hub);
    kiosk.start();
    kiosk.wait_state(KioskHubState::Connected).await;
    let mut app = kiosk.app().await;
    let group = hub
        .writer
        .submit_intent(intent(
            owner(),
            IntentPayload::GroupUpsert(arena_proto::GroupUpsert {
                id: None,
                name: "PCs".into(),
                group_kind: "pc".into(),
                colour: "teal".into(),
                sort_order: 1,
            }),
        ))
        .await
        .unwrap()
        .changes[0]
        .row_id
        .clone();
    hub.writer
        .submit_intent(intent(
            owner(),
            IntentPayload::DeviceAssign(arena_proto::DeviceAssign {
                device_id: hub.device_id.clone(),
                group_id: Some(group),
            }),
        ))
        .await
        .unwrap();
    hub.writer
        .submit_intent(intent(
            owner(),
            IntentPayload::DeviceTestSessionStart(arena_proto::DeviceTestSessionStart {
                device_id: hub.device_id.clone(),
            }),
        ))
        .await
        .unwrap();
    let IpcMsg::TestSessionStarted { remaining_ms } = app
        .until(|m| matches!(m, IpcMsg::TestSessionStarted { .. }))
        .await
    else {
        unreachable!()
    };
    assert!(
        (115_000..=120_000).contains(&remaining_ms),
        "{remaining_ms}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_test_session_locks_at_zero_on_the_monotonic_clock_without_the_hub() {
    let hub = Hub::seeded().await;
    let mut kiosk = Kiosk::new(&hub);
    kiosk.start();
    let ctx = Arc::clone(kiosk.rt().ctx());
    let mut app = kiosk.app().await;
    app.until(|m| matches!(m, IpcMsg::TestSessionEnded)).await;
    let completed = |ctx: &arena_kiosk_svc::Ctx| {
        ctx.queue
            .lock()
            .unwrap()
            .entries()
            .unwrap()
            .into_iter()
            .filter(|q| q.intent.kind == arena_proto::IntentKind::DeviceTestSessionCompleted)
            .inspect(|q| {
                let arena_proto::IntentPayload::DeviceTestSessionCompleted(c) = &q.intent.payload
                else {
                    panic!("wrong payload")
                };
                assert_eq!(c.outcome, arena_proto::TestSessionOutcome::Locked);
                assert_eq!(c.device_id, q.intent.actor.id);
                assert_eq!(q.intent.actor.kind, arena_proto::ActorKind::Device);
                assert_ne!(c.device_id, "unprovisioned");
            })
            .count()
    };
    arena_kiosk_svc::test_session::begin(&ctx, 120_000);
    arena_kiosk_svc::test_session::begin(&ctx, 1);
    let IpcMsg::TestSessionStarted { remaining_ms } = app
        .until(|m| matches!(m, IpcMsg::TestSessionStarted { .. }))
        .await
    else {
        unreachable!()
    };
    assert!(
        (110_000..=120_000).contains(&remaining_ms),
        "{remaining_ms}"
    );
    tokio::time::advance(Duration::from_secs(60)).await;
    assert!(ctx.test_session.borrow().is_some());
    assert_eq!(completed(&ctx), 0);
    drop(app);
    let mut app = kiosk.app().await;
    let IpcMsg::TestSessionStarted { remaining_ms } = app
        .until(|m| matches!(m, IpcMsg::TestSessionStarted { .. }))
        .await
    else {
        unreachable!()
    };
    assert!(remaining_ms <= 60_000, "{remaining_ms}");
    tokio::time::advance(Duration::from_secs(61)).await;
    app.until(|m| matches!(m, IpcMsg::TestSessionEnded)).await;
    assert!(ctx.test_session.borrow().is_none());
    assert_eq!(completed(&ctx), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_wrong_pins_at_the_kiosk_cannot_get_past_the_limit() {
    let (hub, kiosk, _) = admin_setup().await;
    let mut app = kiosk.app().await;
    for i in 0..8 {
        app.send(&IpcMsg::AdminExit {
            rid: format!("p{i}"),
            pin: arena_kiosk_core::ipc::SecretPin::new("9999"),
        })
        .await;
    }
    let mut codes = Vec::new();
    for i in 0..8 {
        let rid = format!("p{i}");
        let reply = app
            .until(|m| matches!(m, IpcMsg::Reply { rid: r, .. } if *r == rid))
            .await;
        codes.push(reply_code(&reply).unwrap());
    }
    let rejected = codes
        .iter()
        .filter(|c| **c == arena_proto::ErrorCode::PinRejected)
        .count();
    assert!(rejected <= 4, "{codes:?}");
    assert!(
        codes.contains(&arena_proto::ErrorCode::AuthLocked),
        "{codes:?}"
    );
    // Paused even for the right PIN, and no exit was granted.
    let reply = app.admin_exit("2222").await;
    assert_eq!(reply_code(&reply), Some(arena_proto::ErrorCode::AuthLocked));
    assert!(hub.exits().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_end_of_an_exit_closes_explorer_and_resumes_enforcement() {
    let mut hub = Hub::seeded().await;
    hub.add_staff("manager", "Manager", "2222").await;
    hub.up().await;
    let mut kiosk = Kiosk::new(&hub);
    kiosk.cache_mode("enforce");
    let clock = Arc::new(arena_kiosk_svc::clock::ManualClock::new(now_ms()));
    kiosk.start_with_clock(clock.clone());
    let sys = fake_system();
    kiosk.enforce(&sys);
    eventually("the creation watch", || sys.subscribed()).await;
    kiosk.wait_state(KioskHubState::Connected).await;
    let mut app = kiosk.app().await;
    assert_eq!(reply_code(&app.admin_exit("2222").await), None);
    // The exit's desktop, and something the manager started from it.
    sys.spawn(300, player_proc(r"C:\Windows\explorer.exe"));
    sys.spawn(301, player_proc(r"C:\Windows\System32\cmd.exe"));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(sys.killed().is_empty(), "nothing is killed during the exit");
    clock.advance(arena_proto::ADMIN_EXIT_MS);
    app.until(|m| matches!(m, IpcMsg::AdminExitEnded)).await;
    eventually("Explorer closed and the sweep done", || {
        let k = sys.killed();
        k.contains(&300) && k.contains(&301)
    })
    .await;
    assert!(!sys.alive(300));
}

// ---- Story 1.21: this kiosk's high contrast ----

impl Hub {
    /// Sets the setting `key` to `value` as the owner, at its current rev.
    async fn set_setting(&self, key: &str, value: &str) {
        let rev = Store::open_reader(&self.path)
            .unwrap()
            .settings()
            .unwrap()
            .into_iter()
            .find(|s| s.key == key)
            .unwrap_or_else(|| panic!("no {key}"))
            .rev;
        self.writer
            .submit_intent(Intent {
                base_rev: Some(rev),
                ..intent(
                    owner(),
                    IntentPayload::SettingsSet(arena_proto::SettingsSet {
                        key: key.into(),
                        value: value.into(),
                    }),
                )
            })
            .await
            .unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_manager_turns_this_kiosks_high_contrast_on_live_and_the_hubs_key_is_not_ours() {
    let mut hub = Hub::seeded().await;
    hub.up().await;
    let mut kiosk = Kiosk::new(&hub);
    kiosk.start();
    let mut app = kiosk.app().await;
    app.until(|m| *m == IpcMsg::Theme { hc: false }).await;
    kiosk.wait_state(KioskHubState::Connected).await;
    let key = arena_proto::hc_key(&hub.device_id);
    let ctx = Arc::clone(kiosk.rt().ctx());
    let cached = || ctx.queue.lock().unwrap().kv_get(&key).unwrap();
    eventually("the theme read after hello", || cached().is_some()).await;
    assert_eq!(cached().unwrap(), ("false".to_owned(), 1));

    // The Hub's own key changes nothing here.
    hub.set_setting("display.hc.hub", "true").await;
    let started = Instant::now();
    hub.set_setting(&key, "true").await;
    app.until(|m| *m == IpcMsg::Theme { hc: true }).await;
    assert!(started.elapsed() < Duration::from_secs(2), "within 2 s");
    assert!(
        !app.skipped.contains(&IpcMsg::Theme { hc: true }),
        "exactly one theme change"
    );
    assert_eq!(cached().unwrap(), ("true".to_owned(), 2));
    // A `theme` request answers the same value.
    app.send(&IpcMsg::Request {
        rid: "t1".into(),
        kind: "theme".into(),
        body: serde_json::Value::Null,
    })
    .await;
    let reply = app.until(|m| matches!(m, IpcMsg::Reply { .. })).await;
    assert_eq!(
        reply,
        IpcMsg::Reply {
            rid: "t1".into(),
            body: serde_json::json!({"hc": true}),
            error: None,
        }
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_kiosk_stores_the_pushed_fulfilment_and_default_volume() {
    let mut hub = Hub::seeded().await;
    hub.up().await;
    let mut kiosk = Kiosk::new(&hub);
    kiosk.start();
    kiosk.wait_state(KioskHubState::Connected).await;
    let ctx = Arc::clone(kiosk.rt().ctx());
    let cached = |key: &str| ctx.queue.lock().unwrap().kv_get(key).unwrap();
    eventually("the operating settings read after hello", || {
        cached("kiosk.volume_default").is_some()
            && cached("branch.fulfilment").is_some()
            && cached("billing.grace_min").is_some()
            && cached("billing.time_up_mode").is_some()
    })
    .await;
    assert_eq!(
        cached("kiosk.volume_default").unwrap(),
        ("50".to_owned(), 1)
    );
    assert_eq!(
        cached("branch.fulfilment").unwrap(),
        ("counter".to_owned(), 1)
    );
    assert_eq!(cached("billing.grace_min").unwrap(), ("2".to_owned(), 1));
    assert_eq!(
        cached("billing.time_up_mode").unwrap(),
        ("clean_up".to_owned(), 1)
    );
    assert_eq!(cached("billing.rounding_min"), None);

    hub.set_setting("kiosk.volume_default", "35").await;
    hub.set_setting("branch.fulfilment", "seat").await;
    hub.set_setting("billing.grace_min", "5").await;
    hub.set_setting("billing.time_up_mode", "staff_handles")
        .await;
    hub.set_setting("billing.rounding_min", "10").await;
    eventually("the pushed values", || {
        cached("kiosk.volume_default") == Some(("35".to_owned(), 2))
            && cached("branch.fulfilment") == Some(("seat".to_owned(), 2))
            && cached("billing.grace_min") == Some(("5".to_owned(), 2))
            && cached("billing.time_up_mode") == Some(("staff_handles".to_owned(), 2))
    })
    .await;
    let stored = arena_kiosk_svc::operating::load(&ctx.queue.lock().unwrap());
    assert_eq!(
        stored,
        arena_kiosk_svc::operating::KioskOperating {
            volume_default: Some(35),
            fulfilment: Some(arena_kiosk_svc::operating::Fulfilment::Seat),
            grace_min: Some(5),
            time_up_mode: Some(arena_proto::TimeUpMode::StaffHandles),
        }
    );
    assert_eq!(cached("billing.rounding_min"), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cold_start_offline_sends_the_cached_high_contrast_first() {
    let hub = Hub::seeded().await;
    let kiosk_cache = |kiosk: &Kiosk, value: &str, rev: i64| {
        std::fs::create_dir_all(kiosk.data.root()).unwrap();
        arena_kiosk_svc::queue::QueueDb::open(&kiosk.data.queue())
            .unwrap()
            .kv_set(&arena_proto::hc_key(&hub.device_id), value, rev)
            .unwrap();
    };
    // The Hub is never up: only the cache can say.
    let mut kiosk = Kiosk::new(&hub);
    kiosk_cache(&kiosk, "true", 3);
    kiosk.start();
    let mut app = kiosk.app().await;
    assert!(matches!(
        app.recv().await.unwrap(),
        IpcMsg::HubStatus {
            state: KioskHubState::Reconnecting
        }
    ));
    assert_eq!(app.recv().await.unwrap(), IpcMsg::Theme { hc: true });
    kiosk.runtime.take().unwrap().shutdown().await;
    // A damaged cache is not high contrast.
    let mut kiosk = Kiosk::new(&hub);
    kiosk_cache(&kiosk, "maybe", 3);
    kiosk.start();
    let mut app = kiosk.app().await;
    app.until(|m| *m == IpcMsg::Theme { hc: false }).await;
}

// ---- Story 1.22: the dev hooks a WAN-down driver uses (feature `dev-fixtures`) ----

#[cfg(feature = "dev-fixtures")]
impl App {
    /// Sends the request `kind` and returns its reply.
    async fn request(&mut self, kind: &str, body: serde_json::Value) -> IpcMsg {
        let rid = format!("dev-{kind}");
        self.send(&IpcMsg::Request {
            rid: rid.clone(),
            kind: kind.into(),
            body,
        })
        .await;
        self.until(|m| matches!(m, IpcMsg::Reply { rid: r, .. } if *r == rid))
            .await
    }
}

#[cfg(feature = "dev-fixtures")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dev_hooks_hold_the_link_spawn_a_blocked_process_relock_and_report_status() {
    let (hub, mut kiosk, manager) = admin_setup().await;
    let (api, opts) = arena_kiosk_svc::dev::fake_enforcement();
    kiosk.runtime.as_mut().unwrap().enforce(api, opts);
    let sys = arena_kiosk_svc::dev::fake_system();
    eventually("the creation watch", || sys.subscribed()).await;
    let mut app = kiosk.app().await;

    // Status.
    let IpcMsg::Reply { body, error, .. } = app.request("dev.status", serde_json::json!({})).await
    else {
        unreachable!()
    };
    assert_eq!(error, None);
    assert_eq!(body["hub_state"], "connected");
    assert!(body["verifier_version"].as_i64().is_some(), "{body}");
    assert_eq!(body["admin_exit_until"], serde_json::Value::Null);

    // A fake player process the lists do not allow is blocked and reported to the Hub.
    let IpcMsg::Reply { body, error, .. } = app
        .request(
            "dev.spawn_process",
            serde_json::json!({"path": r"C:\Users\player\Downloads\e2e-1.exe"}),
        )
        .await
    else {
        unreachable!()
    };
    assert_eq!(error, None);
    let pid = u32::try_from(body["pid"].as_u64().unwrap()).unwrap();
    eventually("the kill", || sys.killed().contains(&pid)).await;
    eventually("the report at the hub", || {
        hub.logged_of(&["process_blocked"]).len() == 1
    })
    .await;

    // Held down: the kiosk goes offline and an admin exit uses the cached verifier.
    let reply = app
        .request("dev.hub_link", serde_json::json!({"online": false}))
        .await;
    assert_eq!(reply_code(&reply), None);
    kiosk.wait_state(KioskHubState::Reconnecting).await;
    eventually("the hub sees it offline", || !hub.online()).await;
    let reply = app.admin_exit("2222").await;
    assert_eq!(reply_code(&reply), None, "{reply:?}");
    let reply = app.request("dev.relock", serde_json::json!({})).await;
    assert_eq!(reply_code(&reply), None);
    let IpcMsg::Reply { body, .. } = app.request("dev.status", serde_json::json!({})).await else {
        unreachable!()
    };
    assert_eq!(body["admin_exit_until"], serde_json::Value::Null);
    assert!(body["queued"].as_u64().unwrap() >= 2, "{body}");
    assert!(hub.exits().is_empty(), "nothing reaches the hub while held");

    // Released: it reconnects and replays the exit, recorded as a cached-verifier one.
    let reply = app
        .request("dev.hub_link", serde_json::json!({"online": true}))
        .await;
    assert_eq!(reply_code(&reply), None);
    kiosk.wait_state(KioskHubState::Connected).await;
    eventually("the exit at the hub", || {
        hub.exits()
            .iter()
            .any(|r| matches!(&r["relocked_at"], arena_store::RowValue::Integer(_)))
    })
    .await;
    let row = &hub.exits()[0];
    assert_eq!(text(row, "mode"), "cached_verifier");
    assert_eq!(text(row, "staff_id"), manager);

    // Unknown and malformed dev requests are refused.
    for (kind, body) in [
        ("dev.nope", serde_json::json!({})),
        ("dev.hub_link", serde_json::json!({})),
        ("dev.spawn_process", serde_json::json!({"path": ""})),
    ] {
        assert!(
            reply_code(&app.request(kind, body).await).is_some(),
            "{kind}"
        );
    }
}

#[cfg(not(feature = "dev-fixtures"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dev_requests_are_unsupported_without_the_feature() {
    let mut hub = Hub::seeded().await;
    hub.up().await;
    let mut kiosk = Kiosk::new(&hub);
    kiosk.start();
    let mut app = kiosk.app().await;
    app.send(&IpcMsg::Request {
        rid: "d".into(),
        kind: "dev.hub_link".into(),
        body: serde_json::json!({"online": false}),
    })
    .await;
    let reply = app
        .until(|m| matches!(m, IpcMsg::Reply { rid, .. } if rid == "d"))
        .await;
    assert_eq!(
        reply_code(&reply),
        Some(arena_proto::ErrorCode::Unsupported)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unpaired_link_connects_as_soon_as_it_is_provisioned() {
    let mut hub = Hub::seeded().await;
    hub.up().await;
    let dir = tempfile::tempdir().unwrap();
    let data = DataDir(dir.path().join("kiosk"));
    std::fs::create_dir_all(data.root()).unwrap();
    let ipc = UnixIpcServer::bind(&data.socket()).unwrap();
    let runtime = Runtime::start(
        data.clone(),
        arena_kiosk_svc::secret::platform_store(data.token()),
        Arc::new(arena_kiosk_svc::clock::SystemClock),
        Timings {
            rejected_retry: Duration::from_secs(600),
            ..fast()
        },
        ipc,
        Arc::new(MockLauncher::default()) as Arc<dyn AppLauncher>,
    )
    .unwrap();
    let mut state = runtime.hub_state();
    tokio::time::timeout(
        Duration::from_secs(5),
        state.wait_for(|s| *s == KioskHubState::Unpaired),
    )
    .await
    .unwrap()
    .unwrap();
    let cfg = KioskConfig {
        hub_addr: hub.addr(),
        hub_fingerprint: hub.cert.fingerprint.clone(),
        device_id: hub.device_id.clone(),
        player_user: "player".into(),
        app_path: None,
    };
    runtime.ctx().provision(&cfg, hub.token.as_bytes()).unwrap();
    tokio::time::timeout(
        Duration::from_secs(5),
        state.wait_for(|s| *s == KioskHubState::Connected),
    )
    .await
    .expect("connected without waiting for the slow retry")
    .unwrap();
    runtime.shutdown().await;
}
