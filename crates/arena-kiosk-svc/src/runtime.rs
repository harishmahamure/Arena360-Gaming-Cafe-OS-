//! The running service: the Hub link and the app supervisor, sharing one [`Ctx`]. The console
//! mode, the Windows service host and the integration tests all start it the same way.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_kiosk_core::backoff::{MAX_DELAY, MIN_DELAY};
use arena_kiosk_core::watchdog::{PING_INTERVAL, RESTART_GRACE};
use arena_proto::{KioskHubState, KioskLogLevel};
use serde_json::Value;
use tokio::sync::{Notify, watch};
use tokio::task::JoinHandle;

use crate::clock::Clock;
use crate::config::KioskConfig;
use crate::ipc::IpcServer;
use crate::launcher::AppLauncher;
use crate::paths::DataDir;
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
        std::fs::create_dir_all(data.root())?;
        let queue = QueueDb::open(&data.queue())?;
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
        });
        let (stop, stop_rx) = watch::channel(false);
        let tasks = vec![
            tokio::spawn(crate::hub_link::run(Arc::clone(&ctx), stop_rx.clone())),
            tokio::spawn(crate::app::run(Arc::clone(&ctx), ipc, launcher, stop_rx)),
        ];
        Ok(Self { ctx, stop, tasks })
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
