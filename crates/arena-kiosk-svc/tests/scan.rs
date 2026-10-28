//! Story 6.4 against a real, in-process Hub LAN server: the kiosk service scans its (fake) PC on
//! the first Hub connection and daily at 05:00 branch time, never while a session runs, queues
//! the report while the Hub is down and the Hub stores the latest scan per device.
//!
//! The scan host is a fixed fake and the app channel is never used. Unix only, like the other
//! service tests: on Windows the service refuses a `config.json` that SYSTEM or Administrators do
//! not own, which a test run by an ordinary account cannot provide.

#![cfg(unix)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use arena_api::lan::{HubCert, LanHub, bind_lan, serve_lan};
use arena_core::devices::{format_token, hash_token_secret, new_token_secret};
use arena_core::{
    DeviceDirectory, HubStatus, LeaseInfo, Role, SystemClock, Writer, WriterHandle, status_channel,
};
use arena_kiosk_svc::clock::{Clock, ManualClock};
use arena_kiosk_svc::config::KioskConfig;
use arena_kiosk_svc::ipc::{IpcServer, IpcStream};
use arena_kiosk_svc::launcher::AppLauncher;
use arena_kiosk_svc::paths::DataDir;
use arena_kiosk_svc::pc::ScreenSession;
use arena_kiosk_svc::scan::{Locations, ScanFs, ScanHost, StdFs};
use arena_kiosk_svc::{Runtime, Timings};
use arena_proto::{
    Actor, ActorKind, BranchBootstrap, ClientClass, DeviceRegister, Intent, IntentPayload,
    KioskHubState, ScanEntry,
};
use arena_store::{LeaseRow, RowValue, Store, Table, write_lease};
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;

const HUB_ID: &str = "01JHUBAAAAAAAAAAAAAAAAAAAA";
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

fn real_now() -> i64 {
    arena_core::Clock::now_ms(&SystemClock)
}

fn intent(actor: Actor, payload: IntentPayload) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: payload.kind(),
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: real_now(),
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

// ---- a seeded hub with one paired device ----

struct Hub {
    _dir: tempfile::TempDir,
    path: PathBuf,
    writer: WriterHandle,
    status: watch::Receiver<arena_core::HubStatus>,
    _status_tx: watch::Sender<HubStatus>,
    cert: HubCert,
    device_id: String,
    token: String,
    lan_port: u16,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl Hub {
    async fn seeded() -> Self {
        let _ = tracing_subscriber::fmt()
            .with_test_writer()
            .with_max_level(tracing::Level::INFO)
            .try_init();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let mut store = Store::open(&path, real_now()).unwrap();
        let lease = LeaseRow {
            counter: 1,
            hub_instance_id: HUB_ID.into(),
            provisional: true,
            renewed_at: real_now(),
            expires_at: i64::MAX,
        };
        store.write_tx(|tx| write_lease(tx, &lease)).unwrap();
        let mut initial = HubStatus::opened(store.schema_version().unwrap(), HUB_ID);
        initial.role = Role::Primary;
        initial.lease = Some(LeaseInfo::from(&lease));
        let (status_tx, status) = status_channel(initial);
        let writer = Writer::spawn(store, Arc::new(SystemClock), status.clone()).unwrap();
        let system = || Actor {
            kind: ActorKind::System,
            id: "test".into(),
        };
        writer
            .submit_intent(intent(
                system(),
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
                system(),
                IntentPayload::LicenceApply(arena_proto::LicenceApply {
                    standing: arena_proto::LicenceStanding::Verified,
                    clock_wrong: false,
                    tier: arena_proto::Tier::Free,
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
        let lan_port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        Self {
            _dir: dir,
            path,
            writer,
            status,
            _status_tx: status_tx,
            cert: HubCert::generate(None).unwrap(),
            device_id,
            token,
            lan_port,
            stop: None,
            task: None,
        }
    }

    async fn up(&mut self) {
        let lan = Arc::new(
            LanHub::new(
                Some(self.writer.clone()),
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

    async fn down(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
        if let Some(t) = self.task.take() {
            let _ = tokio::time::timeout(Duration::from_secs(5), t).await;
        }
    }

    /// The stored scan of this hub's device: `(scanned_at, rev, entries, skipped)`.
    fn scan(&self) -> Option<(i64, i64, Vec<ScanEntry>, i64)> {
        let store = Store::open_reader(&self.path).unwrap();
        let row = store
            .rows(Table::KioskScans)
            .unwrap()
            .into_iter()
            .find(|r| r.get("device_id") == Some(&RowValue::Text(self.device_id.clone())))?;
        let int = |c: &str| match &row[c] {
            RowValue::Integer(n) => *n,
            other => panic!("{c}: {other:?}"),
        };
        let RowValue::Text(entries) = &row["entries"] else {
            panic!("entries")
        };
        Some((
            int("scanned_at"),
            int("rev"),
            serde_json::from_str(entries).unwrap(),
            int("skipped"),
        ))
    }

    fn scan_changes(&self) -> usize {
        Store::open_reader(&self.path)
            .unwrap()
            .changes()
            .unwrap()
            .iter()
            .filter(|c| c.tbl == "kiosk_scans")
            .count()
    }
}

// ---- the kiosk service, with a fake PC ----

struct NoIpc;

impl IpcServer for NoIpc {
    async fn accept(&mut self) -> std::io::Result<IpcStream> {
        std::future::pending().await
    }
}

#[derive(Default)]
struct MockLauncher;

impl AppLauncher for MockLauncher {
    fn restart(&self) -> Result<(), String> {
        Ok(())
    }
}

/// A fake PC: the real file system under a temp dir, with `Games` as its one source.
struct FakePc {
    dir: tempfile::TempDir,
}

impl FakePc {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            dir: tempfile::tempdir().unwrap(),
        })
    }

    fn game(&self, name: &str) {
        let dir = self.dir.path().join("Games").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{name}.exe")), "").unwrap();
    }
}

impl ScanHost for FakePc {
    fn fs(&self) -> &dyn ScanFs {
        &StdFs
    }

    fn locations(&self) -> Locations {
        let root: &Path = self.dir.path();
        Locations {
            steam_roots: vec![root.join("Steam")],
            epic_manifests: root.join("Epic").join("Manifests"),
            epic_launcher_dirs: Vec::new(),
            riot_installs: root.join("Riot").join("RiotClientInstalls.json"),
            game_folders: vec![root.join("Games")],
            program_folders: Vec::new(),
        }
    }
}

fn fast() -> Timings {
    Timings {
        backoff_min: Duration::from_millis(50),
        backoff_max: Duration::from_millis(200),
        rejected_retry: Duration::from_millis(400),
        replay_retry: Duration::from_millis(200),
        ping_interval: Duration::from_secs(60),
        restart_grace: Duration::from_secs(60),
        admin_tick: Duration::from_millis(20),
        scan_tick: Duration::from_millis(20),
    }
}

struct Kiosk {
    _dir: tempfile::TempDir,
    data: DataDir,
    clock: Arc<ManualClock>,
    runtime: Option<Runtime>,
}

impl Kiosk {
    /// A provisioned kiosk whose clock reads `now_ms`.
    fn new(hub: &Hub, now_ms: i64) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let data = DataDir(dir.path().join("kiosk"));
        std::fs::create_dir_all(data.root()).unwrap();
        KioskConfig {
            hub_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), hub.lan_port).to_string(),
            hub_fingerprint: hub.cert.fingerprint.clone(),
            device_id: hub.device_id.clone(),
            player_user: "player".into(),
            app_path: None,
        }
        .save(&data.config())
        .unwrap();
        arena_kiosk_svc::secret::platform_store(data.token())
            .save(hub.token.as_bytes())
            .unwrap();
        Self {
            _dir: dir,
            data,
            clock: Arc::new(ManualClock::new(now_ms)),
            runtime: None,
        }
    }

    fn start(&mut self, pc: &Arc<FakePc>) {
        let mut runtime = Runtime::start(
            self.data.clone(),
            arena_kiosk_svc::secret::platform_store(self.data.token()),
            Arc::clone(&self.clock) as Arc<dyn Clock>,
            fast(),
            NoIpc,
            Arc::new(MockLauncher),
        )
        .unwrap();
        runtime.scan(Arc::clone(pc) as Arc<dyn ScanHost>);
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

    /// The scan queued in `queue.db` and still waiting for the Hub, if any.
    fn queued_scans(&self) -> usize {
        self.rt()
            .ctx()
            .queue
            .lock()
            .unwrap()
            .entries()
            .unwrap()
            .iter()
            .filter(|q| matches!(q.intent.payload, IntentPayload::KioskScanReported(_)))
            .count()
    }

    fn last_scan(&self) -> Option<i64> {
        arena_kiosk_svc::scan::schedule::last_scan(&self.rt().ctx().queue.lock().unwrap())
    }
}

async fn eventually(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A session on the PC, as the Hub's session screen would set it.
fn start_session(kiosk: &Kiosk) {
    let now = tokio::time::Instant::now();
    kiosk.rt().ctx().pc.screen.send_modify(|s| {
        s.session = Some(ScreenSession {
            device_name: "PC-01".into(),
            started: now,
            warning_min: None,
            lock_at: now + Duration::from_secs(3600),
            grace_ms: 0,
            ordered_items: 0,
            hold: false,
        });
    });
}

fn end_session(kiosk: &Kiosk) {
    kiosk.rt().ctx().pc.screen.send_modify(|s| s.session = None);
}

// ---- the matrix ----

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_first_connection_scans_and_the_hub_stores_it_then_a_daily_one_replaces_it() {
    let mut hub = Hub::seeded().await;
    hub.up().await;
    let pc = FakePc::new();
    pc.game("Hades");
    pc.game("Quake");
    // The kiosk's clock reads three days ago, so a day later is still in the Hub's past.
    let start = real_now() - 3 * DAY_MS;
    let mut kiosk = Kiosk::new(&hub, start);
    kiosk.start(&pc);
    kiosk.wait_state(KioskHubState::Connected).await;

    // First scan: the Hub holds the scan, rev 1, and the kiosk recorded it.
    eventually("the first scan stored", || hub.scan().is_some()).await;
    let (at, rev, entries, skipped) = hub.scan().unwrap();
    assert_eq!((at, rev, skipped), (start, 1, 0));
    assert_eq!(
        entries
            .iter()
            .map(|e| e.file_name.as_str())
            .collect::<Vec<_>>(),
        ["Hades.exe", "Quake.exe"]
    );
    assert!(entries.iter().all(|e| e.launcher == "none"));
    assert_eq!(kiosk.last_scan(), Some(start));
    eventually("the queue drained", || kiosk.rt().ctx().queue_len() == 0).await;

    // The branch zone was read from the Hub and cached.
    let tz = arena_kiosk_svc::scan::schedule::load_tz(&kiosk.rt().ctx().queue.lock().unwrap());
    assert_eq!(tz.name(), "Asia/Kolkata");

    // Nothing more happens while the next 05:00 is ahead: a game added now is not seen.
    pc.game("Doom");
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(hub.scan().unwrap().1, 1);
    assert_eq!(hub.scan_changes(), 1);

    // A day and a bit later the 05:00 has passed: the scan runs again and replaces the row.
    kiosk.clock.advance(DAY_MS + 60_000);
    eventually("the daily scan replaced the first", || {
        hub.scan().is_some_and(|s| s.1 == 2)
    })
    .await;
    let (at, rev, entries, _) = hub.scan().unwrap();
    assert_eq!((at, rev), (start + DAY_MS + 60_000, 2));
    assert_eq!(
        entries.len(),
        3,
        "the new game is in, nothing of the old row is kept twice"
    );
    assert_eq!(hub.scan_changes(), 2);
    kiosk.runtime.take().unwrap().shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_scan_that_completes_while_the_hub_is_down_is_queued_and_applied_once_on_reconnect() {
    let mut hub = Hub::seeded().await;
    let pc = FakePc::new();
    pc.game("Hades");
    let start = real_now() - 3 * DAY_MS;
    let mut kiosk = Kiosk::new(&hub, start);
    // A scan is recorded from two days before, so the daily one is due without any connection.
    {
        let q = arena_kiosk_svc::queue::QueueDb::open(&kiosk.data.queue()).unwrap();
        q.kv_set(
            arena_kiosk_svc::scan::schedule::LAST_SCAN_KEY,
            &(start - 2 * DAY_MS).to_string(),
            0,
        )
        .unwrap();
    }
    kiosk.start(&pc);
    eventually("the scan queued offline", || kiosk.queued_scans() == 1).await;
    assert_eq!(
        *kiosk.rt().hub_state().borrow(),
        KioskHubState::Reconnecting
    );
    assert!(hub.scan().is_none());
    assert_eq!(kiosk.last_scan(), Some(start));
    // It is queued once, not once per tick.
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(kiosk.queued_scans(), 1);

    hub.up().await;
    kiosk.wait_state(KioskHubState::Connected).await;
    eventually("the queued scan applied", || hub.scan().is_some()).await;
    eventually("the queue drained", || kiosk.queued_scans() == 0).await;
    let (at, rev, entries, _) = hub.scan().unwrap();
    assert_eq!((at, rev, entries.len()), (start, 1, 1));
    // The hub applied it once: a later reconnect replays nothing.
    hub.down().await;
    hub.up().await;
    kiosk.wait_state(KioskHubState::Connected).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(hub.scan_changes(), 1);
    kiosk.runtime.take().unwrap().shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_scan_that_is_due_during_a_session_waits_for_the_next_moment_with_none() {
    let mut hub = Hub::seeded().await;
    hub.up().await;
    let pc = FakePc::new();
    pc.game("Hades");
    let mut kiosk = Kiosk::new(&hub, real_now() - 3 * DAY_MS);
    kiosk.start(&pc);
    // A session is on the PC from before the first connection.
    start_session(&kiosk);
    kiosk.wait_state(KioskHubState::Connected).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(hub.scan().is_none(), "no scan while a session runs");
    assert_eq!(kiosk.last_scan(), None);
    end_session(&kiosk);
    eventually("the scan after the session", || hub.scan().is_some()).await;
    kiosk.runtime.take().unwrap().shutdown().await;
}

/// The fake PC's file system with a hook on the first listing, a count of listings and
/// optionally one folder that refuses to be listed.
struct Hooked {
    pc: Arc<FakePc>,
    listings: std::sync::atomic::AtomicUsize,
    on_first: Option<Box<dyn Fn() + Send + Sync>>,
    denied: Option<PathBuf>,
}

impl ScanFs for Hooked {
    fn list(&self, dir: &Path) -> std::io::Result<arena_kiosk_svc::scan::DirListing> {
        if self
            .listings
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            == 0
            && let Some(hook) = &self.on_first
        {
            hook();
        }
        if self.denied.as_deref() == Some(dir) {
            return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        }
        StdFs.list(dir)
    }

    fn read_text(&self, file: &Path) -> std::io::Result<String> {
        StdFs.read_text(file)
    }

    fn is_dir(&self, path: &Path) -> bool {
        StdFs.is_dir(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        StdFs.is_file(path)
    }

    fn version_info(&self, file: &Path) -> arena_kiosk_svc::scan::VersionInfo {
        StdFs.version_info(file)
    }
}

impl ScanHost for Hooked {
    fn fs(&self) -> &dyn ScanFs {
        self
    }

    fn locations(&self) -> Locations {
        self.pc.locations()
    }
}

fn hooked(pc: &Arc<FakePc>) -> Hooked {
    Hooked {
        pc: Arc::clone(pc),
        listings: Default::default(),
        on_first: None,
        denied: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_session_that_starts_mid_scan_holds_it_until_the_session_ends() {
    use std::sync::atomic::Ordering::SeqCst;
    let mut hub = Hub::seeded().await;
    hub.up().await;
    let pc = FakePc::new();
    for g in ["a", "b", "c"] {
        pc.game(g);
    }
    // The scan has listed its first folder when a session starts: the hook tells the test, which
    // starts the session, and only then lets the listing return.
    let (started_tx, started_rx) = std::sync::mpsc::channel::<()>();
    let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
    let (started_tx, go_rx) = (
        std::sync::Mutex::new(started_tx),
        std::sync::Mutex::new(go_rx),
    );
    let mut host = hooked(&pc);
    host.on_first = Some(Box::new(move || {
        started_tx.lock().unwrap().send(()).unwrap();
        let _ = go_rx.lock().unwrap().recv_timeout(Duration::from_secs(10));
    }));
    let host = Arc::new(host);
    let mut kiosk = Kiosk::new(&hub, real_now() - 3 * DAY_MS);
    let mut runtime = Runtime::start(
        kiosk.data.clone(),
        arena_kiosk_svc::secret::platform_store(kiosk.data.token()),
        Arc::clone(&kiosk.clock) as Arc<dyn Clock>,
        fast(),
        NoIpc,
        Arc::new(MockLauncher),
    )
    .unwrap();
    runtime.scan(Arc::clone(&host) as Arc<dyn ScanHost>);
    kiosk.runtime = Some(runtime);

    tokio::task::spawn_blocking(move || started_rx.recv_timeout(Duration::from_secs(10)))
        .await
        .unwrap()
        .expect("the scan started");
    start_session(&kiosk);
    go_tx.send(()).unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        host.listings.load(SeqCst),
        1,
        "the scan holds at its next folder while the session runs"
    );
    assert!(hub.scan().is_none());
    end_session(&kiosk);
    eventually("the scan resumed and reported", || hub.scan().is_some()).await;
    assert_eq!(hub.scan().unwrap().2.len(), 3);
    assert!(host.listings.load(SeqCst) > 1);
    kiosk.runtime.take().unwrap().shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unreadable_folder_is_counted_and_the_report_still_goes_out() {
    let mut hub = Hub::seeded().await;
    hub.up().await;
    let pc = FakePc::new();
    pc.game("a");
    pc.game("locked");
    pc.game("z");
    let mut host = hooked(&pc);
    host.denied = Some(pc.dir.path().join("Games").join("locked"));
    let host = Arc::new(host);
    let mut kiosk = Kiosk::new(&hub, real_now() - 3 * DAY_MS);
    let mut runtime = Runtime::start(
        kiosk.data.clone(),
        arena_kiosk_svc::secret::platform_store(kiosk.data.token()),
        Arc::clone(&kiosk.clock) as Arc<dyn Clock>,
        fast(),
        NoIpc,
        Arc::new(MockLauncher),
    )
    .unwrap();
    runtime.scan(host);
    kiosk.runtime = Some(runtime);
    eventually("the report stored", || hub.scan().is_some()).await;
    let (_, _, entries, skipped) = hub.scan().unwrap();
    assert_eq!(
        entries
            .iter()
            .map(|e| e.file_name.as_str())
            .collect::<Vec<_>>(),
        ["a.exe", "z.exe"]
    );
    assert_eq!(skipped, 1);
    kiosk.runtime.take().unwrap().shutdown().await;
}
