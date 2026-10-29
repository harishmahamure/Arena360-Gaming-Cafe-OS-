//! The running service: the Hub link, the app supervisor and (where the host provides one) the
//! process enforcer, sharing one [`Ctx`]. The console mode, the Windows service host and the
//! integration tests all start it the same way.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_kiosk_core::backoff::{MAX_DELAY, MIN_DELAY};
use arena_kiosk_core::watchdog::{PING_INTERVAL, RESTART_GRACE};
use arena_proto::{KioskHubState, KioskLogLevel};
use serde_json::Value;
use tokio::sync::{Notify, broadcast, watch};
use tokio::task::JoinHandle;

use crate::clock::Clock;
use crate::config::KioskConfig;
use crate::ipc::IpcServer;
use crate::launcher::AppLauncher;
use crate::paths::DataDir;
use crate::procs::{EnforceOptions, ProcessApi, ProcessLists};
use crate::queue::QueueDb;
use crate::secret::SecretStore;

/// How long a rejected token waits before the next try.
pub const REJECTED_RETRY: Duration = Duration::from_secs(30);
/// How long a replay stopped by a transient error waits before trying again on the same
/// connection (a reconnect retries at once).
pub const REPLAY_RETRY: Duration = Duration::from_secs(30);

/// Timings (tests shorten them).
#[derive(Debug, Clone)]
pub struct Timings {
    pub backoff_min: Duration,
    pub backoff_max: Duration,
    pub rejected_retry: Duration,
    pub replay_retry: Duration,
    pub ping_interval: Duration,
    pub restart_grace: Duration,
    /// How often the admin-exit timer re-reads the clock (at most).
    pub admin_tick: Duration,
    /// How often the scan schedule checks whether a scan is due (and whether a session still
    /// blocks it).
    pub scan_tick: Duration,
    /// How long a failed cleanup step waits before its one retry (Story 6.11).
    pub cleanup_retry: Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            backoff_min: MIN_DELAY,
            backoff_max: MAX_DELAY,
            rejected_retry: REJECTED_RETRY,
            replay_retry: REPLAY_RETRY,
            ping_interval: PING_INTERVAL,
            restart_grace: RESTART_GRACE,
            admin_tick: Duration::from_secs(1),
            scan_tick: Duration::from_secs(60),
            cleanup_retry: crate::cleanup::DEFAULT_RETRY,
        }
    }
}

/// What both halves of the service share.
pub struct Ctx {
    pub data: DataDir,
    pub secrets: Box<dyn SecretStore>,
    pub clock: Arc<dyn Clock>,
    pub timings: Timings,
    pub queue: Mutex<QueueDb>,
    /// Woken whenever an intent is queued.
    pub queued: Notify,
    pub hub_state: watch::Sender<KioskHubState>,
    /// The last `app_state` the app reported (sent back as `restore_state`).
    pub app_state: Mutex<Option<Value>>,
    /// The process allow-list and block-list in effect (cached in `queue.db`, updated by the
    /// Hub link).
    pub lists: watch::Sender<Arc<ProcessLists>>,
    /// Whether this kiosk is high contrast (`display.hc.<device_id>`, cached in `queue.db`,
    /// updated by the Hub link; the supervisor sends it to the app as IPC `theme`).
    pub theme: watch::Sender<bool>,
    /// The attract view the Hub last pushed and the branch logo (cached in `queue.db`; the
    /// supervisor sends them to the app as IPC `attract`, Story 6.7).
    pub attract: watch::Sender<crate::attract::Shown>,
    /// Toast catalogue keys for the app (the supervisor forwards them as IPC `toast`).
    pub toasts: broadcast::Sender<String>,
    /// Restarts the app; starts and stops the admin exit's desktop.
    pub launcher: Arc<dyn AppLauncher>,
    /// The admin exit: throttle, running exit, cached verifier, live Hub link.
    pub admin: crate::admin_exit::AdminState,
    /// While `true` the Hub link stays down (closing a live connection) as if the Hub were
    /// unreachable. Only the dev-fixtures `dev.hub_link` request sets it; release builds
    /// never do.
    pub link_hold: watch::Sender<bool>,
    pub discovery: crate::discovery::Discovery,
    pub provisioned: Notify,
    pub test_session: watch::Sender<Option<tokio::time::Instant>>,
    pub pc: crate::pc::PcState,
    /// Ends the player's programs for the session-end cleanup; set by the process enforcer
    /// while it runs (Story 6.11).
    pub closer: Mutex<Option<crate::procs::Closer>>,
    /// Freezes and thaws the player's programs for the Staff-handles lock; set by the process
    /// enforcer while it runs (Story 6.22).
    pub freezer: Mutex<Option<crate::procs::Freezer>>,
    /// The session-end cleanup: dirty flag, cancel epoch, worker wake-up.
    pub cleanup: crate::cleanup::CleanupState,
    /// The games the Games tab started and the host that starts them (Story 6.23).
    pub games: crate::games::GameState,
}

impl Ctx {
    /// The provisioned configuration, if any (read fresh: a re-provision needs no restart).
    pub fn config(&self) -> Option<KioskConfig> {
        match KioskConfig::load(&self.data.config()) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = %e, "config.json is unreadable");
                None
            }
        }
    }

    pub fn provision(&self, cfg: &KioskConfig, token: &[u8]) -> Result<(), String> {
        crate::config::write_provisioning(&self.data, self.secrets.as_ref(), cfg, token)?;
        self.provisioned.notify_one();
        Ok(())
    }

    /// Queues a `kiosk.log` event, stamped now, and wakes the Hub link.
    pub fn log_event(&self, level: KioskLogLevel, code: &str, detail: &str) -> std::io::Result<()> {
        let device_id = self
            .config()
            .map(|c| c.device_id)
            .unwrap_or_else(|| "unprovisioned".into());
        let intent =
            crate::intents::kiosk_log(&device_id, self.clock.now_ms(), level, code, detail);
        self.queue
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(&intent)?;
        self.queued.notify_one();
        Ok(())
    }

    /// How many intents wait.
    pub fn queue_len(&self) -> usize {
        self.queue
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .len()
            .unwrap_or(0)
    }

    pub(crate) fn set_hub_state(&self, state: KioskHubState) {
        self.hub_state.send_if_modified(|s| {
            if *s == state {
                false
            } else {
                tracing::info!(state = ?state, "hub link");
                *s = state;
                true
            }
        });
    }
}

/// A started service.
pub struct Runtime {
    ctx: Arc<Ctx>,
    stop: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
}

impl Runtime {
    /// Opens the queue and starts the Hub link and the app supervisor on `ipc`.
    pub fn start<S: IpcServer>(
        data: DataDir,
        secrets: Box<dyn SecretStore>,
        clock: Arc<dyn Clock>,
        timings: Timings,
        ipc: S,
        launcher: Arc<dyn AppLauncher>,
    ) -> std::io::Result<Self> {
        Self::start_with_discovery(
            data,
            secrets,
            clock,
            timings,
            ipc,
            launcher,
            crate::discovery::Discovery::system(),
        )
    }

    pub fn start_with_discovery<S: IpcServer>(
        data: DataDir,
        secrets: Box<dyn SecretStore>,
        clock: Arc<dyn Clock>,
        timings: Timings,
        ipc: S,
        launcher: Arc<dyn AppLauncher>,
        discovery: crate::discovery::Discovery,
    ) -> std::io::Result<Self> {
        std::fs::create_dir_all(data.root())?;
        let queue = QueueDb::open(&data.queue())?;
        let data_verifier = data.verifier();
        let (lists, _) = watch::channel(Arc::new(ProcessLists::load(&queue)));
        let device_id = KioskConfig::load(&data.config())
            .ok()
            .flatten()
            .map(|c| c.device_id);
        let (theme, _) = watch::channel(crate::theme::load(&queue, device_id.as_deref()));
        let (attract, _) = watch::channel(crate::attract::load(&queue));
        let pc = crate::pc::PcState::load(&queue);
        let cleanup = crate::cleanup::CleanupState::load(&queue);
        let (toasts, _) = broadcast::channel(16);
        let (hub_state, _) = watch::channel(KioskHubState::Reconnecting);
        let ctx = Arc::new(Ctx {
            data,
            secrets,
            clock,
            timings,
            queue: Mutex::new(queue),
            queued: Notify::new(),
            hub_state,
            app_state: Mutex::new(None),
            lists,
            theme,
            attract,
            toasts,
            admin: crate::admin_exit::AdminState::load(crate::secret::platform_store(
                data_verifier,
            )),
            launcher: Arc::clone(&launcher),
            link_hold: watch::channel(false).0,
            discovery,
            provisioned: Notify::new(),
            test_session: watch::channel(None).0,
            pc,
            closer: Mutex::new(None),
            freezer: Mutex::new(None),
            cleanup,
            games: crate::games::GameState::default(),
        });
        let (stop, stop_rx) = watch::channel(false);
        let tasks = vec![
            tokio::spawn(crate::hub_link::run(Arc::clone(&ctx), stop_rx.clone())),
            tokio::spawn(crate::admin_exit::run(Arc::clone(&ctx), stop_rx.clone())),
            tokio::spawn(crate::test_session::run(Arc::clone(&ctx), stop_rx.clone())),
            tokio::spawn(crate::session::run(Arc::clone(&ctx), stop_rx.clone())),
            tokio::spawn(crate::app::run(Arc::clone(&ctx), ipc, launcher, stop_rx)),
        ];
        Ok(Self { ctx, stop, tasks })
    }

    /// Starts enforcing the process lists in the player's session through `api` (Windows: the
    /// real process control; tests: fakes). Without this call nothing is ever killed.
    pub fn enforce(&mut self, api: ProcessApi, opts: EnforceOptions) {
        self.tasks.push(tokio::spawn(crate::procs::run(
            Arc::clone(&self.ctx),
            api,
            opts,
            self.stop.subscribe(),
        )));
    }

    /// Starts the installed-software scan schedule (Story 6.4) on `host` (Windows: the real
    /// PC; tests: a fixed one). Without this call nothing is ever scanned.
    pub fn scan(&mut self, host: Arc<dyn crate::scan::ScanHost>) {
        self.tasks.push(tokio::spawn(crate::scan::schedule::run(
            Arc::clone(&self.ctx),
            host,
            self.stop.subscribe(),
        )));
    }

    /// Starts the session-end cleanup (Story 6.11) on `host` (Windows: the real PC; tests:
    /// fakes) with the standard steps. Without this call a session end only locks the screen.
    pub fn cleanup(&mut self, host: Arc<dyn crate::cleanup::CleanupHost>) {
        self.cleanup_with(host, crate::cleanup::Pipeline::standard());
    }

    /// [`cleanup`](Self::cleanup) with a chosen pipeline.
    pub fn cleanup_with(
        &mut self,
        host: Arc<dyn crate::cleanup::CleanupHost>,
        pipeline: crate::cleanup::Pipeline,
    ) {
        crate::cleanup::enable(&self.ctx);
        self.ctx.cleanup.volume.attach(Arc::clone(&host));
        self.tasks.push(tokio::spawn(crate::cleanup::run(
            Arc::clone(&self.ctx),
            host,
            pipeline,
            self.stop.subscribe(),
        )));
    }

    /// Lets the Games tab start games through `host` (Story 6.23) and keeps track of the ones
    /// that are running (Windows: the real PC; tests: a fake). Without this call a `game_launch`
    /// is refused as unsupported.
    pub fn games(&mut self, host: Arc<dyn crate::games::GameHost>) {
        self.ctx.games.set_host(host);
        self.tasks.push(tokio::spawn(crate::games::run(
            Arc::clone(&self.ctx),
            self.stop.subscribe(),
        )));
    }

    /// The shared state (tests and the hosts read it).
    pub fn ctx(&self) -> &Arc<Ctx> {
        &self.ctx
    }

    /// The Hub link state, as the app sees it.
    pub fn hub_state(&self) -> watch::Receiver<KioskHubState> {
        self.ctx.hub_state.subscribe()
    }

    /// Stops both halves and waits for them.
    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        for t in self.tasks {
            let _ = tokio::time::timeout(Duration::from_secs(5), t).await;
        }
    }
}
