//! The app end of the Kiosk service channel. The webview never sees the channel: this Rust
//! shell answers the service's pings, forwards `hub_status`, `restore_state`, `toast` and
//! `theme` to the webview as the Tauri events `hub-status`, `restore-state`, `toast` and
//! `theme`, and sends the webview's state up as `app_state`. The webview has no Hub socket, no token and no admin rights.
//!
//! - **Liveness is the webview's.** The shell answers the service's `ping` only while the
//!   webview's `heartbeat` command (sent every second) was seen within [`HEARTBEAT_FRESH`], so a
//!   hung webview behind a live shell still gets restarted.
//! - **Restore once.** The first `restore_state` of this process is delivered; later ones (an
//!   IPC reconnect after a service restart) are ignored, since the app already has its state.
//! - **Latest state wins.** The webview's state is a single latest-value slot, never dropped
//!   when the line is busy, and re-sent on every (re)connect.
//! - **Admin exit (Story 1.20).** The webview's hidden panel sends the PIN through the
//!   `admin_exit` command: it goes to the service as IPC `admin_exit{pin}` only (never stored or
//!   logged) and the service's `reply` comes back as the command's result. `admin_exit_started`
//!   and `admin_exit_ended` become the `admin-exit` / `admin-exit-ended` events; `main.rs`
//!   suspends the lockdown and shrinks the window to the countdown badge in between.
//! - **Theme (Story 1.21).** The service's `theme{hc}` (on every connection and every change)
//!   is kept as the latest value: [`Bridge::wait_theme`] lets `main.rs` decide the first paint,
//!   and a change becomes the `theme` event (`main.rs` also saves it to `theme.json`).
//! - **Windows:** the pipe's server must run as SYSTEM, or the connection is refused.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use arena_kiosk_core::ipc::{IpcMsg, read_msg, write_msg};
use arena_proto::{DiscoveredHub, KioskHubState, KioskHubStatus};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::sync::{mpsc, oneshot, watch};

/// The Tauri event carrying a [`KioskHubStatus`].
pub const HUB_STATUS_EVENT: &str = "hub-status";
/// The Tauri event carrying the state to restore (the JSON the webview reported).
pub const RESTORE_STATE_EVENT: &str = "restore-state";
/// The Tauri event carrying a toast's catalogue key (e.g. `player.app.blocked`).
pub const TOAST_EVENT: &str = "toast";
/// The Tauri event of a started admin exit: `{until}` (UTC ms).
pub const ADMIN_EXIT_EVENT: &str = "admin-exit";
/// The Tauri event of an admin exit that is over.
pub const ADMIN_EXIT_ENDED_EVENT: &str = "admin-exit-ended";
/// The Tauri event carrying the theme: `{hc}`.
pub const THEME_EVENT: &str = "theme";
pub const TEST_SESSION_EVENT: &str = "test-session";
pub const TEST_SESSION_ENDED_EVENT: &str = "test-session-ended";
pub const PC_SCREEN_EVENT: &str = "pc-screen";
pub const DISCOVERY_FOUND_EVENT: &str = "discovery-found";
pub const DISCOVERY_DONE_EVENT: &str = "discovery-done";
/// How long the shell waits for the service's answer to a PIN.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
/// The Tauri commands the webview calls: the `generate_handler!` list in `main.rs`, pinned
/// against `ipc-names.json` (the list the webview reads) by a test.
#[cfg(test)]
const COMMANDS: [&str; 15] = [
    "report_app_state",
    "hub_status",
    "take_restore_state",
    "heartbeat",
    "admin_exit",
    "admin_exit_status",
    "test_session",
    "theme",
    "discover",
    "probe_ip",
    "select_hub",
    "pair",
    "pair_info",
    "pc_screen",
    "pc_dismiss",
];
/// How recent the webview's last heartbeat must be for a `ping` to be answered.
pub const HEARTBEAT_FRESH: Duration = Duration::from_secs(3);
/// Wait between attempts to reach the service.
const RETRY: Duration = Duration::from_millis(500);

/// What the webview should be told.
#[derive(Debug, Clone, PartialEq)]
pub enum Emit {
    HubStatus(KioskHubStatus),
    RestoreState(Value),
    /// A catalogue key to show as a persistent toast.
    Toast(String),
    /// An admin exit runs until this UTC ms.
    AdminExit(i64),
    /// The admin exit is over.
    AdminExitEnded,
    /// The theme changed: high contrast on (`true`) or off.
    Theme(bool),
    DiscoveryFound(u64, Vec<DiscoveredHub>),
    DiscoveryDone(u64, Vec<DiscoveredHub>),
    TestSession(u64),
    TestSessionEnded,
    PcScreen(PcScreen),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct PcScreen {
    pub locked: bool,
    pub reason: Option<arena_proto::LockReason>,
    pub message: Option<String>,
    pub session: Option<arena_kiosk_core::ipc::PcSession>,
}

/// What the service answered to a request: the body, or the error code (and the body that came
/// with it, e.g. `retry_after_s`).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Answer {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub body: Value,
}

/// The shell's view of the channel, shared with the Tauri commands.
pub struct Bridge {
    /// The webview's latest state (the slot every connection sends from).
    state: watch::Sender<Option<Value>>,
    hub: Mutex<Option<KioskHubStatus>>,
    restore: Mutex<Option<Value>>,
    restore_seen: AtomicBool,
    heartbeat: Mutex<Option<Instant>>,
    /// Whether the OS lockdown (the keyboard hook) is degraded; when true the service is told
    /// over IPC so it queues a `kiosk.log{error, lockdown_degraded}` for the Hub.
    lockdown_degraded: watch::Sender<bool>,
    /// The current connection's outbound queue (requests go out on it).
    outbox: Mutex<Option<mpsc::Sender<IpcMsg>>>,
    /// Requests waiting for their `reply`, by rid.
    pending: Mutex<HashMap<String, oneshot::Sender<IpcMsg>>>,
    next_rid: AtomicU64,
    /// Whether an admin exit runs, as last told: `admin_exit_ended` (sent on every connection)
    /// is passed on only when one ran, so a reconnect never re-covers the screen for nothing.
    exit_running: AtomicBool,
    /// This kiosk's high contrast, once the service has said (or `main.rs` has decided).
    theme: watch::Sender<Option<bool>>,
    test_deadline: Mutex<Option<Instant>>,
    screen: Mutex<(PcScreen, Instant)>,
}

impl Default for Bridge {
    fn default() -> Self {
        Self {
            state: watch::channel(None).0,
            hub: Mutex::default(),
            restore: Mutex::default(),
            restore_seen: AtomicBool::new(false),
            heartbeat: Mutex::default(),
            lockdown_degraded: watch::channel(false).0,
            outbox: Mutex::default(),
            pending: Mutex::default(),
            next_rid: AtomicU64::new(1),
            exit_running: AtomicBool::new(false),
            theme: watch::channel(None).0,
            test_deadline: Mutex::default(),
            screen: Mutex::new((PcScreen::default(), Instant::now())),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Bridge {
    /// The webview's state changed: it replaces the slot (sent to the service as soon as the
    /// line allows; only the latest is ever sent).
    pub fn report(&self, state: Value) {
        self.state.send_replace(Some(state));
    }

    /// Sends `build(rid)` to the service and waits for its `reply` (`None`: no service, or no
    /// answer in time).
    pub async fn request(&self, build: impl FnOnce(String) -> IpcMsg) -> Option<Answer> {
        let rid = format!("ui-{}", self.next_rid.fetch_add(1, Ordering::Relaxed));
        let out = lock(&self.outbox).clone()?;
        let (tx, rx) = oneshot::channel();
        lock(&self.pending).insert(rid.clone(), tx);
        if out.send(build(rid.clone())).await.is_err() {
            lock(&self.pending).remove(&rid);
            return None;
        }
        let reply = tokio::time::timeout(REQUEST_TIMEOUT, rx).await;
        lock(&self.pending).remove(&rid);
        match reply {
            Ok(Ok(IpcMsg::Reply { body, error, .. })) => Some(Answer {
                ok: error.is_none(),
                code: error.map(|e| e.code.as_str().to_owned()),
                body,
            }),
            _ => None,
        }
    }

    /// The webview is alive.
    pub fn heartbeat(&self) {
        self.heartbeat_at(Instant::now());
    }

    /// The OS lockdown became degraded (`true`) or healthy again (`false`). The service hears
    /// the change on its next pass and queues a `kiosk.log` for the Hub when degraded. Only the
    /// Windows input guard reports this; off Windows there is no OS hook.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn set_lockdown_degraded(&self, degraded: bool) {
        self.lockdown_degraded.send_replace(degraded);
    }

    /// The `log` request that reports the degraded lockdown to the service.
    fn degraded_log() -> IpcMsg {
        IpcMsg::Request {
            rid: "lockdown-degraded".into(),
            kind: "log".into(),
            body: serde_json::json!({
                "level": "error",
                "code": "lockdown_degraded",
                "detail": "the kiosk keyboard hook is not active",
            }),
        }
    }

    fn heartbeat_at(&self, now: Instant) {
        *lock(&self.heartbeat) = Some(now);
    }

    /// The Hub link state, once known.
    pub fn hub_status(&self) -> Option<KioskHubStatus> {
        *lock(&self.hub)
    }

    /// The theme the service sent, if any.
    pub fn theme(&self) -> Option<bool> {
        *self.theme.borrow()
    }

    /// Records the first-paint theme when the service has not said yet (so the `theme` command
    /// answers it, and a later different value from the service is a change).
    pub fn settle_theme(&self, hc: bool) {
        self.theme.send_if_modified(|t| {
            if t.is_some() {
                return false;
            }
            *t = Some(hc);
            true
        });
    }

    /// The service's theme, waiting at most `wait` for it (`None`: it did not say in time).
    pub async fn wait_theme(&self, wait: Duration) -> Option<bool> {
        let mut rx = self.theme.subscribe();
        match tokio::time::timeout(wait, rx.wait_for(Option::is_some)).await {
            Ok(Ok(v)) => *v,
            _ => None,
        }
    }

    pub fn pc_screen(&self) -> PcScreen {
        let (mut screen, at) = lock(&self.screen).clone();
        if let Some(s) = &mut screen.session {
            let gone = at.elapsed().as_millis() as u64;
            s.elapsed_ms += gone;
            s.grace_in_ms = s.grace_in_ms.map(|m| m.saturating_sub(gone));
        }
        screen
    }

    pub fn test_remaining_ms(&self) -> Option<u64> {
        let deadline = (*lock(&self.test_deadline))?;
        Some(
            deadline
                .saturating_duration_since(Instant::now())
                .as_millis() as u64,
        )
    }

    /// The state to restore, once (cleared on delivery).
    pub fn take_restore(&self) -> Option<Value> {
        lock(&self.restore).take()
    }

    /// Handles one message from the service: the reply to send back, and what to tell the
    /// webview.
    pub fn on_message(&self, msg: IpcMsg) -> (Option<IpcMsg>, Option<Emit>) {
        self.on_message_at(msg, Instant::now())
    }

    fn on_message_at(&self, msg: IpcMsg, now: Instant) -> (Option<IpcMsg>, Option<Emit>) {
        match msg {
            IpcMsg::Ping { n } => {
                let fresh = lock(&self.heartbeat)
                    .is_some_and(|at| now.saturating_duration_since(at) <= HEARTBEAT_FRESH);
                if fresh {
                    (Some(IpcMsg::Pong { n }), None)
                } else {
                    tracing::debug!(n, "no recent webview heartbeat; not answering the ping");
                    (None, None)
                }
            }
            IpcMsg::HubStatus { state } => {
                let status = KioskHubStatus { state };
                *lock(&self.hub) = Some(status);
                (None, Some(Emit::HubStatus(status)))
            }
            IpcMsg::RestoreState { json } => {
                if self.restore_seen.swap(true, Ordering::SeqCst) {
                    return (None, None);
                }
                *lock(&self.restore) = Some(json.clone());
                (None, Some(Emit::RestoreState(json)))
            }
            IpcMsg::Toast { key } => (None, Some(Emit::Toast(key))),
            IpcMsg::Theme { hc } => {
                let changed = self.theme.send_if_modified(|t| {
                    let new = *t != Some(hc);
                    *t = Some(hc);
                    new
                });
                (None, changed.then_some(Emit::Theme(hc)))
            }
            IpcMsg::AdminExitStarted { until } => {
                self.exit_running.store(true, Ordering::SeqCst);
                (None, Some(Emit::AdminExit(until)))
            }
            IpcMsg::AdminExitEnded => {
                if self.exit_running.swap(false, Ordering::SeqCst) {
                    (None, Some(Emit::AdminExitEnded))
                } else {
                    (None, None)
                }
            }
            IpcMsg::TestSessionStarted { remaining_ms } => {
                *lock(&self.test_deadline) = Some(now + Duration::from_millis(remaining_ms));
                (None, Some(Emit::TestSession(remaining_ms)))
            }
            IpcMsg::TestSessionEnded => {
                let ran = lock(&self.test_deadline).take().is_some();
                (None, ran.then_some(Emit::TestSessionEnded))
            }
            IpcMsg::PcScreen {
                locked,
                reason,
                message,
                session,
            } => {
                let screen = PcScreen {
                    locked,
                    reason,
                    message,
                    session,
                };
                *lock(&self.screen) = (screen.clone(), now);
                (None, Some(Emit::PcScreen(screen)))
            }
            IpcMsg::DiscoveryFound { search, hubs } => {
                (None, Some(Emit::DiscoveryFound(search, hubs)))
            }
            IpcMsg::DiscoveryDone { search, hubs } => {
                (None, Some(Emit::DiscoveryDone(search, hubs)))
            }
            IpcMsg::Reply { ref rid, .. } => {
                if let Some(tx) = lock(&self.pending).remove(rid) {
                    let _ = tx.send(msg);
                }
                (None, None)
            }
            // Replies to requests and anything app-side are not for the shell.
            _ => (None, None),
        }
    }

    /// The service is unreachable: as far as the player knows, the Hub link is down.
    pub fn on_service_lost(&self) -> Option<Emit> {
        let status = KioskHubStatus {
            state: KioskHubState::Reconnecting,
        };
        let mut hub = lock(&self.hub);
        if *hub == Some(status) {
            return None;
        }
        *hub = Some(status);
        Some(Emit::HubStatus(status))
    }

    /// Serves one connection until it closes. `emit` tells the webview.
    pub async fn serve<R, W>(&self, reader: R, mut writer: W, emit: &(dyn Fn(Emit) + Sync))
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (tx, mut rx) = mpsc::channel::<IpcMsg>(64);
        *lock(&self.outbox) = Some(tx.clone());
        // The latest state goes out on every (re)connect: a restarted service lost it.
        let mut state = self.state.subscribe();
        state.mark_changed();
        // A degraded lockdown is re-reported on every (re)connect, so a service restart does not
        // lose it.
        let mut degraded = self.lockdown_degraded.subscribe();
        degraded.mark_changed();
        let writer_task = tokio::spawn(async move {
            loop {
                let msg = tokio::select! {
                    m = rx.recv() => match m {
                        Some(m) => m,
                        None => break,
                    },
                    changed = state.changed() => {
                        if changed.is_err() {
                            break;
                        }
                        let Some(json) = state.borrow_and_update().clone() else {
                            continue;
                        };
                        IpcMsg::AppState { json }
                    }
                    changed = degraded.changed() => {
                        if changed.is_err() {
                            break;
                        }
                        if !*degraded.borrow_and_update() {
                            continue;
                        }
                        Self::degraded_log()
                    }
                };
                if write_msg(&mut writer, &msg).await.is_err() {
                    break;
                }
            }
        });
        let mut reader = BufReader::new(reader);
        loop {
            match read_msg(&mut reader).await {
                Ok(msg) => {
                    let (reply, event) = self.on_message(msg);
                    if let Some(r) = reply {
                        let _ = tx.send(r).await;
                    }
                    if let Some(e) = event {
                        emit(e);
                    }
                }
                Err(arena_kiosk_core::IpcError::Closed) => break,
                Err(e) => tracing::warn!(error = %e, "skipping a bad line from the kiosk service"),
            }
        }
        writer_task.abort();
        *lock(&self.outbox) = None;
        // Waiting requests fail at once (their senders drop).
        lock(&self.pending).clear();
    }

    /// Connects to the service and serves it, forever.
    pub async fn run(&self, emit: &(dyn Fn(Emit) + Sync)) {
        loop {
            match connect().await {
                Ok((r, w)) => self.serve(r, w, emit).await,
                Err(e) => tracing::debug!(error = %e, "kiosk service not reachable"),
            }
            if let Some(e) = self.on_service_lost() {
                emit(e);
            }
            tokio::time::sleep(RETRY).await;
        }
    }
}

type Halves = (
    Box<dyn AsyncRead + Send + Unpin>,
    Box<dyn AsyncWrite + Send + Unpin>,
);

#[cfg(windows)]
async fn connect() -> std::io::Result<Halves> {
    use tokio::net::windows::named_pipe::ClientOptions;
    let pipe = ClientOptions::new().open(arena_kiosk_core::ipc::PIPE_NAME)?;
    if !server_is_system(&pipe) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "the kiosk pipe is not served by SYSTEM; refusing it",
        ));
    }
    let (r, w) = tokio::io::split(pipe);
    Ok((Box::new(r), Box::new(w)))
}

/// Whether the pipe's server runs as SYSTEM: the server process's token user
/// (`GetNamedPipeServerProcessId`), or, when the player may not open that token, the pipe's
/// owner (the creator's; only SYSTEM can make it SYSTEM).
#[cfg(windows)]
fn server_is_system(pipe: &tokio::net::windows::named_pipe::NamedPipeClient) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT};
    use windows::Win32::Security::{
        GetTokenInformation, IsWellKnownSid, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        PSID, TOKEN_QUERY, TOKEN_USER, TokenUser, WinLocalSystemSid,
    };
    use windows::Win32::System::Pipes::GetNamedPipeServerProcessId;
    use windows::Win32::System::Threading::{
        OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let handle = HANDLE(pipe.as_raw_handle());
    // SAFETY: plain Win32 queries on live handles; every handle opened here is closed.
    unsafe {
        let mut pid = 0u32;
        if GetNamedPipeServerProcessId(handle, &mut pid).is_ok()
            && let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
        {
            let mut token = HANDLE::default();
            let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token).is_ok();
            let _ = CloseHandle(process);
            if opened {
                let mut buf = vec![0u8; 256];
                let mut len = 0u32;
                let ok = GetTokenInformation(
                    token,
                    TokenUser,
                    Some(buf.as_mut_ptr().cast()),
                    buf.len() as u32,
                    &mut len,
                )
                .is_ok();
                let system = ok && {
                    let user = &*(buf.as_ptr() as *const TOKEN_USER);
                    IsWellKnownSid(user.User.Sid, WinLocalSystemSid).as_bool()
                };
                let _ = CloseHandle(token);
                return system;
            }
        }
        let mut owner = PSID::default();
        let mut sd = PSECURITY_DESCRIPTOR::default();
        let rc = GetSecurityInfo(
            handle,
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            None,
            None,
            Some(&mut sd),
        );
        let system = rc.is_ok() && IsWellKnownSid(owner, WinLocalSystemSid).as_bool();
        if !sd.0.is_null() {
            let _ = LocalFree(Some(HLOCAL(sd.0)));
        }
        system
    }
}

#[cfg(unix)]
async fn connect() -> std::io::Result<Halves> {
    let dir = arena_kiosk_core::ipc::dev_data_dir(|k| std::env::var(k).ok());
    let stream = tokio::net::UnixStream::connect(arena_kiosk_core::ipc::socket_path(&dir)).await?;
    let (r, w) = stream.into_split();
    Ok((Box::new(r), Box::new(w)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn names_match_the_list_the_webview_reads() {
        let names: Value = serde_json::from_str(include_str!("../ipc-names.json")).unwrap();
        assert_eq!(names["events"]["hubStatus"], HUB_STATUS_EVENT);
        assert_eq!(names["events"]["restoreState"], RESTORE_STATE_EVENT);
        assert_eq!(names["events"]["toast"], TOAST_EVENT);
        assert_eq!(names["events"]["theme"], THEME_EVENT);
        assert_eq!(names["events"]["testSession"], TEST_SESSION_EVENT);
        assert_eq!(
            names["events"]["testSessionEnded"],
            TEST_SESSION_ENDED_EVENT
        );
        assert_eq!(names["events"]["pcScreen"], PC_SCREEN_EVENT);
        assert_eq!(names["events"]["discoveryFound"], DISCOVERY_FOUND_EVENT);
        assert_eq!(names["events"]["discoveryDone"], DISCOVERY_DONE_EVENT);
        let mut listed: Vec<&str> = names["commands"]
            .as_object()
            .unwrap()
            .values()
            .map(|v| v.as_str().unwrap())
            .collect();
        listed.sort_unstable();
        let mut ours = COMMANDS.to_vec();
        ours.sort_unstable();
        assert_eq!(listed, ours);
    }

    #[test]
    fn pings_are_answered_only_after_a_recent_webview_heartbeat() {
        let b = Bridge::default();
        let t0 = Instant::now();
        assert_eq!(b.on_message_at(IpcMsg::Ping { n: 1 }, t0), (None, None));
        b.heartbeat_at(t0);
        assert_eq!(
            b.on_message_at(IpcMsg::Ping { n: 2 }, t0 + Duration::from_secs(1)),
            (Some(IpcMsg::Pong { n: 2 }), None)
        );
        assert_eq!(
            b.on_message_at(IpcMsg::Ping { n: 3 }, t0 + Duration::from_secs(4)),
            (None, None),
            "a hung webview stops the pongs"
        );
    }

    #[tokio::test]
    async fn an_admin_exit_pin_goes_to_the_service_and_its_reply_comes_back() {
        let b = Arc::new(Bridge::default());
        assert_eq!(
            b.request(|rid| IpcMsg::AdminExit {
                rid,
                pin: arena_kiosk_core::ipc::SecretPin::new("2222"),
            })
            .await,
            None,
            "no service: no answer"
        );
        let (app, svc) = tokio::io::duplex(4096);
        let (ar, aw) = tokio::io::split(app);
        let (sr, mut sw) = tokio::io::split(svc);
        let served = {
            let b = Arc::clone(&b);
            tokio::spawn(async move { b.serve(ar, aw, &|_| {}).await })
        };
        let mut sr = BufReader::new(sr);
        tokio::time::sleep(Duration::from_millis(20)).await;
        let asking = {
            let b = Arc::clone(&b);
            tokio::spawn(async move {
                b.request(|rid| IpcMsg::AdminExit {
                    rid,
                    pin: arena_kiosk_core::ipc::SecretPin::new("2222"),
                })
                .await
            })
        };
        let IpcMsg::AdminExit { rid, pin } = read_msg(&mut sr).await.unwrap() else {
            panic!("expected admin_exit")
        };
        assert_eq!(pin.expose(), "2222");
        write_msg(
            &mut sw,
            &IpcMsg::Reply {
                rid,
                body: serde_json::json!({"retry_after_s": 60}),
                error: Some(arena_proto::AppError::new(
                    arena_proto::ErrorCode::AuthLocked,
                    "x",
                )),
            },
        )
        .await
        .unwrap();
        let answer = asking.await.unwrap().unwrap();
        assert!(!answer.ok);
        assert_eq!(answer.code.as_deref(), Some("AUTH_LOCKED"));
        assert_eq!(answer.body["retry_after_s"], 60);
        assert_eq!(
            b.on_message(IpcMsg::AdminExitStarted { until: 9 }).1,
            Some(Emit::AdminExit(9))
        );
        assert_eq!(
            b.on_message(IpcMsg::AdminExitEnded).1,
            Some(Emit::AdminExitEnded)
        );
        // Ended again (a reconnect with no exit running): nothing to undo.
        assert_eq!(b.on_message(IpcMsg::AdminExitEnded).1, None);
        drop(sw);
        drop(sr);
        let _ = tokio::time::timeout(Duration::from_secs(2), served).await;
    }

    #[tokio::test]
    async fn the_theme_is_waited_for_at_most_the_timeout_and_changes_are_emitted_once() {
        let b = Arc::new(Bridge::default());
        let started = Instant::now();
        assert_eq!(b.wait_theme(Duration::from_millis(50)).await, None);
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "a slow service never blocks"
        );
        let waiting = {
            let b = Arc::clone(&b);
            tokio::spawn(async move { b.wait_theme(Duration::from_secs(5)).await })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(
            b.on_message(IpcMsg::Theme { hc: true }),
            (None, Some(Emit::Theme(true)))
        );
        assert_eq!(waiting.await.unwrap(), Some(true));
        assert_eq!(b.theme(), Some(true));
        // The same value again (a reconnect) is nothing new; a change is.
        assert_eq!(b.on_message(IpcMsg::Theme { hc: true }), (None, None));
        assert_eq!(
            b.on_message(IpcMsg::Theme { hc: false }),
            (None, Some(Emit::Theme(false)))
        );
        assert_eq!(b.wait_theme(Duration::from_millis(1)).await, Some(false));
    }

    #[test]
    fn discovery_events_are_forwarded() {
        let b = Bridge::default();
        let hub = DiscoveredHub {
            addr: "10.0.0.2:7400".into(),
            branch_name: "B".into(),
            hub_name: "H".into(),
            fingerprint: "ab".repeat(32),
            protocol_version: 1,
            compatible: true,
            web_port: 7401,
        };
        assert_eq!(
            b.on_message(IpcMsg::DiscoveryFound {
                search: 4,
                hubs: vec![hub.clone()]
            }),
            (None, Some(Emit::DiscoveryFound(4, vec![hub.clone()])))
        );
        assert_eq!(
            b.on_message(IpcMsg::DiscoveryDone {
                search: 4,
                hubs: vec![hub.clone()]
            }),
            (None, Some(Emit::DiscoveryDone(4, vec![hub])))
        );
    }

    #[test]
    fn a_test_session_is_counted_on_the_shells_clock_and_ended_once() {
        let b = Bridge::default();
        assert_eq!(b.test_remaining_ms(), None);
        assert_eq!(b.on_message(IpcMsg::TestSessionEnded), (None, None));
        assert_eq!(
            b.on_message(IpcMsg::TestSessionStarted {
                remaining_ms: 120_000
            }),
            (None, Some(Emit::TestSession(120_000)))
        );
        let left = b.test_remaining_ms().unwrap();
        assert!((119_000..=120_000).contains(&left), "{left}");
        assert_eq!(
            b.on_message(IpcMsg::TestSessionEnded),
            (None, Some(Emit::TestSessionEnded))
        );
        assert_eq!(b.test_remaining_ms(), None);
        assert_eq!(b.on_message(IpcMsg::TestSessionEnded), (None, None));
    }

    #[test]
    fn the_staff_lock_and_message_are_kept_and_forwarded() {
        let b = Bridge::default();
        assert_eq!(b.pc_screen(), PcScreen::default());
        let shown = PcScreen {
            locked: true,
            reason: Some(arena_proto::LockReason::PayAtCounter),
            message: Some("hi".into()),
            session: None,
        };
        assert_eq!(
            b.on_message(IpcMsg::PcScreen {
                locked: true,
                reason: Some(arena_proto::LockReason::PayAtCounter),
                message: Some("hi".into()),
                session: None,
            }),
            (None, Some(Emit::PcScreen(shown.clone())))
        );
        assert_eq!(b.pc_screen(), shown);
    }

    #[test]
    fn a_session_screen_keeps_counting_between_service_messages() {
        let b = Bridge::default();
        let at = Instant::now();
        b.on_message_at(
            IpcMsg::PcScreen {
                locked: false,
                reason: None,
                message: None,
                session: Some(arena_kiosk_core::ipc::PcSession {
                    device_name: "PC 1".into(),
                    elapsed_ms: 1_000,
                    warning_min: None,
                    grace_in_ms: None,
                    grace_ms: 0,
                    ordered_items: 0,
                }),
            },
            at,
        );
        let session = b.pc_screen().session.unwrap();
        assert_eq!(session.device_name, "PC 1");
        assert!(session.elapsed_ms >= 1_000);
    }

    #[test]
    fn toasts_are_forwarded_every_time() {
        let b = Bridge::default();
        let key = arena_kiosk_core::ipc::TOAST_APP_BLOCKED.to_owned();
        for _ in 0..2 {
            assert_eq!(
                b.on_message(IpcMsg::Toast { key: key.clone() }),
                (None, Some(Emit::Toast(key.clone())))
            );
        }
    }

    #[test]
    fn status_is_forwarded_and_restore_is_delivered_once() {
        let b = Bridge::default();
        let status = KioskHubStatus {
            state: KioskHubState::Connected,
        };
        assert_eq!(
            b.on_message(IpcMsg::HubStatus {
                state: KioskHubState::Connected
            }),
            (None, Some(Emit::HubStatus(status)))
        );
        assert_eq!(b.hub_status(), Some(status));
        let json = serde_json::json!({"route": "/x"});
        assert_eq!(
            b.on_message(IpcMsg::RestoreState { json: json.clone() }).1,
            Some(Emit::RestoreState(json.clone()))
        );
        assert_eq!(b.take_restore(), Some(json));
        assert_eq!(b.take_restore(), None);
        // A reconnect's restore is ignored: the app already has its state.
        let again = serde_json::json!({"route": "/old"});
        assert_eq!(
            b.on_message(IpcMsg::RestoreState { json: again }),
            (None, None)
        );
        assert_eq!(b.take_restore(), None);
        // Losing the service reads as reconnecting, once.
        assert!(b.on_service_lost().is_some());
        assert!(b.on_service_lost().is_none());
    }

    #[tokio::test]
    async fn the_latest_state_is_sent_up_and_resent_after_a_reconnect() {
        let b = Arc::new(Bridge::default());
        b.report(serde_json::json!({"route": "/a"}));
        b.report(serde_json::json!({"route": "/b"}));
        for _ in 0..2 {
            let (app, svc) = tokio::io::duplex(4096);
            let (ar, aw) = tokio::io::split(app);
            let (sr, mut sw) = tokio::io::split(svc);
            let served = {
                let b = Arc::clone(&b);
                tokio::spawn(async move { b.serve(ar, aw, &|_| {}).await })
            };
            let mut sr = BufReader::new(sr);
            // Only the latest, on every connection.
            assert_eq!(
                read_msg(&mut sr).await.unwrap(),
                IpcMsg::AppState {
                    json: serde_json::json!({"route": "/b"})
                }
            );
            b.heartbeat();
            write_msg(&mut sw, &IpcMsg::Ping { n: 1 }).await.unwrap();
            assert_eq!(read_msg(&mut sr).await.unwrap(), IpcMsg::Pong { n: 1 });
            drop(sw);
            drop(sr);
            tokio::time::timeout(Duration::from_secs(2), served)
                .await
                .unwrap()
                .unwrap();
        }
    }
}
