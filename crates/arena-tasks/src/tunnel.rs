//! The Hub's Cloudflare Tunnel connector (Story 8.39).
//!
//! The heartbeat leaves the cloud's [`TunnelAnswer`] in the [`TunnelInbox`]. [`TunnelTask`]
//! applies it: a `grant` stores the credential through the platform keystore (DPAPI on Windows)
//! and starts the bundled `cloudflared` as a supervised child process, outbound only; `off` (the
//! flag is gone) and `unsupported_platform` stop the child and delete the credential. A connector
//! that exits is restarted after a [`backoff`](crate::heartbeat::backoff). The task never calls
//! the writer, so a tunnel that flaps cannot touch trading. The credential is never logged.

use std::future::pending;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_core::{HubStatus, Role};
use arena_proto::{TunnelAnswer, TunnelLine, TunnelReport, TunnelState};
use tokio::sync::{Notify, oneshot, watch};
use tokio::time::Instant;

use crate::heartbeat::{backoff, unit};
use crate::keystore::{Keystore, named_keystore};

/// The DPAPI-protected file the tunnel credential is kept in, beside the Hub token.
pub const TUNNEL_CREDENTIAL_FILE: &str = "tunnel-credential.bin";
/// The longest wait between two starts of a connector that keeps exiting.
pub const MAX_RESTART_BACKOFF: Duration = Duration::from_secs(60);
/// How long a started connector must stay up before the line reads `on`.
pub const SETTLE: Duration = Duration::from_secs(5);
/// A connector that ran this long and then exited starts its backoff over.
pub const STABLE: Duration = Duration::from_secs(60);
/// How long a stopping connector is given to exit before the task goes on.
const STOP_WAIT: Duration = Duration::from_secs(5);

/// Where the heartbeat leaves the cloud's tunnel answer for [`TunnelTask`], and where the task
/// says whether it holds a credential, for the next heartbeat to report.
#[derive(Default)]
pub struct TunnelInbox {
    order: Mutex<Option<TunnelAnswer>>,
    held: AtomicBool,
    wake: Notify,
}

impl TunnelInbox {
    pub fn new() -> Arc<Self> {
        Arc::default()
    }

    /// The cloud's answer. A `grant` not yet taken is not replaced by an `on` (it carries the
    /// only copy of the credential the cloud will send); anything else replaces what waits.
    pub fn offer(&self, answer: TunnelAnswer) {
        if let Ok(mut slot) = self.order.lock() {
            let keep_grant = matches!(
                (&*slot, &answer),
                (Some(TunnelAnswer::Grant { .. }), TunnelAnswer::On { .. })
            );
            if !keep_grant {
                *slot = Some(answer);
            }
        }
        self.wake.notify_one();
    }

    /// What the next heartbeat tells the cloud.
    pub fn report(&self) -> TunnelReport {
        TunnelReport {
            has_credential: self.held.load(Ordering::SeqCst),
        }
    }

    pub(crate) fn set_held(&self, held: bool) {
        self.held.store(held, Ordering::SeqCst);
    }

    pub(crate) fn take(&self) -> Option<TunnelAnswer> {
        self.order.lock().ok()?.take()
    }
}

/// The stored tunnel credential: a DPAPI-protected file on Windows (a 0600 file on a dev build).
pub struct TunnelCredential {
    keystore: Box<dyn Keystore>,
    file: PathBuf,
}

impl TunnelCredential {
    /// The credential kept in `data_dir`.
    pub fn in_dir(data_dir: &Path) -> Self {
        Self {
            keystore: named_keystore(data_dir, TUNNEL_CREDENTIAL_FILE),
            file: data_dir.join(TUNNEL_CREDENTIAL_FILE),
        }
    }

    pub fn get(&self) -> io::Result<Option<String>> {
        Ok(self
            .keystore
            .get()?
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .filter(|c| !c.is_empty()))
    }

    pub fn put(&self, credential: &str) -> io::Result<()> {
        self.keystore.put(credential.as_bytes())
    }

    /// Deletes the stored credential; a missing one is fine.
    pub fn delete(&self) -> io::Result<()> {
        match std::fs::remove_file(&self.file) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// A started connector: `exited` resolves when it is gone, and `stop` ends it.
pub struct Running {
    exited: oneshot::Receiver<()>,
    stop: Option<oneshot::Sender<()>>,
}

impl Running {
    pub fn new(exited: oneshot::Receiver<()>, stop: oneshot::Sender<()>) -> Self {
        Self {
            exited,
            stop: Some(stop),
        }
    }

    /// Asks the connector to stop and waits (briefly) until it has.
    async fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let _ = tokio::time::timeout(STOP_WAIT, &mut self.exited).await;
    }
}

/// Starts the connector process.
pub trait Connector: Send + Sync {
    /// Starts a connector that carries `credential`. Dropping the returned [`Running`] stops it.
    fn start(&self, credential: &str) -> io::Result<Running>;
}

/// Runs the bundled `cloudflared` as a child process. The credential goes to the child in
/// `TUNNEL_TOKEN`, not on its command line, where other processes could read it. Ingress is the
/// tunnel's own configuration in Cloudflare (only the Hub's tunnel listener, else 404), so the
/// child opens no port and needs no config file.
pub struct ProcessConnector {
    program: PathBuf,
}

impl ProcessConnector {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    pub fn program(&self) -> &Path {
        &self.program
    }
}

impl Connector for ProcessConnector {
    fn start(&self, credential: &str) -> io::Result<Running> {
        let mut cmd = tokio::process::Command::new(&self.program);
        cmd.args(["tunnel", "--no-autoupdate", "run"])
            .env("TUNNEL_TOKEN", credential)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let mut child = cmd.spawn()?;
        let (exited_tx, exited) = oneshot::channel();
        let (stop, stop_rx) = oneshot::channel();
        tokio::spawn(async move {
            tokio::select! {
                _ = child.wait() => {}
                _ = stop_rx => {
                    let _ = child.kill().await;
                }
            }
            let _ = exited_tx.send(());
        });
        Ok(Running::new(exited, stop))
    }
}

struct Live {
    running: Running,
    started: Instant,
}

/// Supervises the connector from the cloud's answers.
pub struct TunnelTask {
    inbox: Arc<TunnelInbox>,
    credential: TunnelCredential,
    connector: Arc<dyn Connector>,
    status: watch::Sender<HubStatus>,
    hostname: Option<String>,
    /// The cloud wants the connector up (an `on` or a stored `grant`).
    want: bool,
    fails: u32,
}

impl TunnelTask {
    /// Reads whether a credential is stored now, so the first heartbeat reports it correctly.
    pub fn new(
        inbox: Arc<TunnelInbox>,
        credential: TunnelCredential,
        connector: Arc<dyn Connector>,
        status: watch::Sender<HubStatus>,
    ) -> Self {
        let held = match credential.get() {
            Ok(c) => c.is_some(),
            Err(e) => {
                tracing::warn!(error = %e, "tunnel: the stored credential cannot be read");
                false
            }
        };
        inbox.set_held(held);
        Self {
            inbox,
            credential,
            connector,
            status,
            hostname: None,
            want: false,
            fails: 0,
        }
    }

    fn publish(&self, state: TunnelState) {
        let line = TunnelLine::new(state, self.hostname.clone());
        self.status.send_if_modified(|s| {
            let changed = s.tunnel != line;
            s.tunnel = line;
            changed
        });
    }

    /// Applies the cloud's answer. A connector that must change is stopped; the loop starts it.
    async fn apply(&mut self, order: TunnelAnswer, live: &mut Option<Live>) {
        match order {
            TunnelAnswer::Grant {
                hostname,
                credential,
            } => {
                // A short keystore call on this task's own thread: nothing else waits on it.
                match self.credential.put(&credential) {
                    Ok(()) => {
                        self.inbox.set_held(true);
                        self.hostname = Some(hostname);
                        self.want = true;
                        self.fails = 0;
                        // A new credential: the connector restarts with it.
                        if let Some(l) = live.take() {
                            l.running.stop().await;
                        }
                    }
                    Err(e) => tracing::warn!(
                        error = %e,
                        "tunnel: the credential could not be stored; it is asked for again"
                    ),
                }
            }
            TunnelAnswer::On { hostname } => {
                if self.inbox.report().has_credential {
                    self.hostname = Some(hostname);
                    self.want = true;
                }
            }
            TunnelAnswer::Off | TunnelAnswer::UnsupportedPlatform => {
                let unsupported = order == TunnelAnswer::UnsupportedPlatform;
                self.want = false;
                self.hostname = None;
                self.fails = 0;
                if let Some(l) = live.take() {
                    l.running.stop().await;
                }
                match self.credential.delete() {
                    Ok(()) => self.inbox.set_held(false),
                    Err(e) => {
                        tracing::warn!(error = %e, "tunnel: the credential file could not be deleted");
                    }
                }
                self.publish(if unsupported {
                    TunnelState::UnsupportedPlatform
                } else {
                    TunnelState::Off
                });
            }
        }
    }

    /// Starts the connector from the stored credential.
    async fn start(&mut self) -> Result<Live, ()> {
        let credential = match self.credential.get() {
            Ok(Some(c)) => c,
            Ok(None) => {
                // Nothing to run: the next heartbeat reports it and the cloud grants again.
                self.inbox.set_held(false);
                self.want = false;
                self.publish(TunnelState::Off);
                return Err(());
            }
            Err(e) => {
                tracing::warn!(error = %e, "tunnel: the stored credential cannot be read");
                return Err(());
            }
        };
        match self.connector.start(&credential) {
            Ok(running) => Ok(Live {
                running,
                started: Instant::now(),
            }),
            Err(e) => {
                tracing::warn!(error = %e, "tunnel: the connector could not be started");
                Err(())
            }
        }
    }

    pub async fn run(mut self) {
        let mut roles = self.status.subscribe();
        let mut live: Option<Live> = None;
        let mut retry_at: Option<Instant> = None;
        let mut settle_at: Option<Instant> = None;
        loop {
            if let Some(order) = self.inbox.take() {
                self.apply(order, &mut live).await;
                if !self.want {
                    retry_at = None;
                    settle_at = None;
                } else if self.fails == 0 {
                    // A fresh credential does not wait out the backoff of the old one.
                    retry_at = None;
                }
            }
            let run_now = self.want && roles.borrow().role == Role::Primary;
            if !run_now {
                // A follower never runs the connector (only the primary talks to the cloud).
                if let Some(l) = live.take() {
                    l.running.stop().await;
                    self.publish(if self.want {
                        TunnelState::Starting
                    } else {
                        TunnelState::Off
                    });
                }
                retry_at = None;
                settle_at = None;
            } else if live.is_none() && retry_at.is_none() {
                match self.start().await {
                    Ok(l) => {
                        live = Some(l);
                        settle_at = Some(Instant::now() + SETTLE);
                        self.publish(TunnelState::Starting);
                    }
                    Err(()) if self.want => {
                        self.fails = self.fails.saturating_add(1);
                        retry_at =
                            Some(Instant::now() + backoff(self.fails, MAX_RESTART_BACKOFF, unit()));
                        self.publish(TunnelState::Reconnecting);
                    }
                    Err(()) => {}
                }
            }
            tokio::select! {
                () = self.inbox.wake.notified() => {}
                changed = roles.changed() => {
                    if changed.is_err() {
                        return;
                    }
                }
                _ = async {
                    match live.as_mut() {
                        Some(l) => { let _ = (&mut l.running.exited).await; }
                        None => pending::<()>().await,
                    }
                } => {
                    let ran = live.take().map(|l| l.started.elapsed()).unwrap_or_default();
                    if ran >= STABLE {
                        self.fails = 0;
                    }
                    self.fails = self.fails.saturating_add(1);
                    settle_at = None;
                    retry_at = Some(Instant::now() + backoff(self.fails, MAX_RESTART_BACKOFF, unit()));
                    tracing::warn!(fails = self.fails, "tunnel: the connector exited; restarting after a backoff");
                    self.publish(TunnelState::Reconnecting);
                }
                () = async {
                    match retry_at {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => pending::<()>().await,
                    }
                } => {
                    retry_at = None;
                }
                () = async {
                    match settle_at {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => pending::<()>().await,
                    }
                } => {
                    settle_at = None;
                    if live.is_some() {
                        self.publish(TunnelState::On);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_core::status_channel;
    use std::sync::atomic::AtomicUsize;

    /// Hands out connectors the test can crash.
    #[derive(Default)]
    struct FakeConnector {
        starts: Mutex<Vec<String>>,
        crashes: Mutex<Vec<oneshot::Sender<()>>>,
        stops: AtomicUsize,
        fail_start: AtomicBool,
    }

    impl FakeConnector {
        fn starts(&self) -> Vec<String> {
            self.starts.lock().unwrap().clone()
        }

        fn crash(&self) {
            if let Some(tx) = self.crashes.lock().unwrap().pop() {
                let _ = tx.send(());
            }
        }
    }

    impl Connector for Arc<FakeConnector> {
        fn start(&self, credential: &str) -> io::Result<Running> {
            if self.fail_start.load(Ordering::SeqCst) {
                return Err(io::Error::other("no cloudflared"));
            }
            self.starts.lock().unwrap().push(credential.to_owned());
            let (exited_tx, exited) = oneshot::channel();
            let (stop, stop_rx) = oneshot::channel::<()>();
            let (crash_tx, crash_rx) = oneshot::channel::<()>();
            self.crashes.lock().unwrap().push(crash_tx);
            let me = Arc::clone(self);
            tokio::spawn(async move {
                tokio::select! {
                    _ = crash_rx => {}
                    _ = stop_rx => { me.stops.fetch_add(1, Ordering::SeqCst); }
                }
                let _ = exited_tx.send(());
            });
            Ok(Running::new(exited, stop))
        }
    }

    struct Fx {
        dir: tempfile::TempDir,
        inbox: Arc<TunnelInbox>,
        connector: Arc<FakeConnector>,
        status: watch::Sender<HubStatus>,
    }

    fn fx() -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let inbox = TunnelInbox::new();
        let connector = Arc::new(FakeConnector::default());
        let (status, _rx) = status_channel(HubStatus::opened(1, "01JSELF"));
        status.send_modify(|s| s.role = Role::Primary);
        Fx {
            dir,
            inbox,
            connector,
            status,
        }
    }

    impl Fx {
        fn spawn(&self) -> tokio::task::JoinHandle<()> {
            let task = TunnelTask::new(
                Arc::clone(&self.inbox),
                TunnelCredential::in_dir(self.dir.path()),
                Arc::new(Arc::clone(&self.connector)),
                self.status.clone(),
            );
            tokio::spawn(task.run())
        }

        fn line(&self) -> TunnelLine {
            self.status.borrow().tunnel.clone()
        }

        fn stored(&self) -> Option<String> {
            TunnelCredential::in_dir(self.dir.path()).get().unwrap()
        }
    }

    fn grant(credential: &str) -> TunnelAnswer {
        TunnelAnswer::Grant {
            hostname: "B.tunnel.arena360.cloud".into(),
            credential: credential.into(),
        }
    }

    async fn settle() {
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_grant_is_stored_and_runs_the_connector() {
        let f = fx();
        assert!(!f.inbox.report().has_credential);
        let job = f.spawn();
        f.inbox.offer(grant("tok-1"));
        settle().await;
        assert_eq!(f.stored().as_deref(), Some("tok-1"));
        assert!(f.inbox.report().has_credential);
        assert_eq!(f.connector.starts(), ["tok-1"]);
        assert_eq!(f.line().state, TunnelState::Starting);
        tokio::time::sleep(SETTLE + Duration::from_millis(10)).await;
        assert_eq!(
            f.line(),
            TunnelLine::new(TunnelState::On, Some("B.tunnel.arena360.cloud".into()))
        );
        job.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn the_credential_on_disk_is_not_plain_text_on_windows() {
        let f = fx();
        let job = f.spawn();
        f.inbox.offer(grant("tok-plain-secret"));
        settle().await;
        let file = std::fs::read(f.dir.path().join(TUNNEL_CREDENTIAL_FILE)).unwrap();
        if cfg!(windows) {
            assert!(!file.windows(10).any(|w| w == b"plain-secr"));
        }
        job.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn off_stops_the_connector_and_deletes_the_credential() {
        let f = fx();
        let job = f.spawn();
        f.inbox.offer(grant("tok-1"));
        settle().await;
        f.inbox.offer(TunnelAnswer::Off);
        settle().await;
        assert_eq!(f.connector.stops.load(Ordering::SeqCst), 1);
        assert_eq!(f.stored(), None);
        assert!(!f.dir.path().join(TUNNEL_CREDENTIAL_FILE).exists());
        assert!(!f.inbox.report().has_credential);
        assert_eq!(f.line(), TunnelLine::default());
        job.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn an_unsupported_platform_runs_nothing_and_says_so() {
        let f = fx();
        let job = f.spawn();
        f.inbox.offer(TunnelAnswer::UnsupportedPlatform);
        settle().await;
        assert!(f.connector.starts().is_empty());
        assert_eq!(f.line().state, TunnelState::UnsupportedPlatform);
        job.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_crash_restarts_after_a_backoff_and_reads_reconnecting() {
        let f = fx();
        let job = f.spawn();
        f.inbox.offer(grant("tok-1"));
        settle().await;
        tokio::time::sleep(SETTLE + Duration::from_millis(10)).await;
        assert_eq!(f.line().state, TunnelState::On);
        f.connector.crash();
        settle().await;
        assert_eq!(f.line().state, TunnelState::Reconnecting);
        assert_eq!(f.connector.starts().len(), 1, "not restarted at once");
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert_eq!(f.connector.starts(), ["tok-1", "tok-1"]);
        assert_eq!(f.line().state, TunnelState::Starting);
        job.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_connector_that_keeps_crashing_waits_longer_each_time() {
        let f = fx();
        let job = f.spawn();
        f.inbox.offer(grant("tok-1"));
        settle().await;
        let mut waits = Vec::new();
        for _ in 0..4 {
            f.connector.crash();
            settle().await;
            let before = f.connector.starts().len();
            let mut waited = Duration::ZERO;
            while f.connector.starts().len() == before {
                tokio::time::sleep(Duration::from_millis(100)).await;
                settle().await;
                waited += Duration::from_millis(100);
                assert!(waited < MAX_RESTART_BACKOFF + Duration::from_secs(1));
            }
            waits.push(waited);
        }
        assert!(waits[3] > waits[0], "{waits:?}");
        job.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_connector_that_cannot_start_is_retried_with_backoff() {
        let f = fx();
        f.connector.fail_start.store(true, Ordering::SeqCst);
        let job = f.spawn();
        f.inbox.offer(grant("tok-1"));
        settle().await;
        assert_eq!(f.line().state, TunnelState::Reconnecting);
        f.connector.fail_start.store(false, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert_eq!(f.connector.starts(), ["tok-1"]);
        job.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_grant_replaces_the_credential_and_restarts() {
        let f = fx();
        let job = f.spawn();
        f.inbox.offer(grant("tok-1"));
        settle().await;
        f.inbox.offer(grant("tok-2"));
        settle().await;
        assert_eq!(f.connector.starts(), ["tok-1", "tok-2"]);
        assert_eq!(f.stored().as_deref(), Some("tok-2"));
        job.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn on_keeps_the_running_connector_and_a_follower_runs_none() {
        let f = fx();
        let job = f.spawn();
        f.inbox.offer(grant("tok-1"));
        settle().await;
        f.inbox.offer(TunnelAnswer::On {
            hostname: "B.tunnel.arena360.cloud".into(),
        });
        settle().await;
        assert_eq!(f.connector.starts().len(), 1);
        f.status.send_modify(|s| s.role = Role::Follower);
        settle().await;
        assert_eq!(f.connector.stops.load(Ordering::SeqCst), 1);
        assert_eq!(f.stored().as_deref(), Some("tok-1"), "kept for a promotion");
        f.status.send_modify(|s| s.role = Role::Primary);
        settle().await;
        assert_eq!(f.connector.starts().len(), 2);
        job.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_stored_credential_is_reported_after_a_restart_and_runs_on_on() {
        let f = fx();
        TunnelCredential::in_dir(f.dir.path()).put("tok-9").unwrap();
        let job = f.spawn();
        assert!(f.inbox.report().has_credential);
        settle().await;
        assert!(
            f.connector.starts().is_empty(),
            "waits for the cloud's word"
        );
        f.inbox.offer(TunnelAnswer::On {
            hostname: "h".into(),
        });
        settle().await;
        assert_eq!(f.connector.starts(), ["tok-9"]);
        job.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn an_on_without_a_credential_runs_nothing() {
        let f = fx();
        let job = f.spawn();
        f.inbox.offer(TunnelAnswer::On {
            hostname: "h".into(),
        });
        settle().await;
        assert!(f.connector.starts().is_empty());
        assert_eq!(f.line(), TunnelLine::default());
        job.abort();
    }

    #[tokio::test]
    async fn a_missing_program_is_a_start_failure_not_a_panic() {
        let missing = ProcessConnector::new("definitely-not-cloudflared-xyz");
        assert!(missing.start("tok").is_err());
    }

    #[test]
    fn an_unread_grant_is_not_replaced_by_an_on() {
        let inbox = TunnelInbox::new();
        inbox.offer(grant("tok-1"));
        inbox.offer(TunnelAnswer::On {
            hostname: "h".into(),
        });
        assert!(matches!(inbox.take(), Some(TunnelAnswer::Grant { .. })));
        inbox.offer(grant("tok-1"));
        inbox.offer(TunnelAnswer::Off);
        assert_eq!(inbox.take(), Some(TunnelAnswer::Off));
    }
}
