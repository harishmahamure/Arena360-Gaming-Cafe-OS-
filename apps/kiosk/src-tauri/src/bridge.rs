//! The app end of the Kiosk service channel. The webview never sees the channel: this Rust
//! shell answers the service's pings, forwards `hub_status` and `restore_state` to the webview
//! as the Tauri events `hub-status` and `restore-state`, and sends the webview's state up as
//! `app_state`. The webview has no Hub socket, no token and no admin rights.
//!
//! - **Liveness is the webview's.** The shell answers the service's `ping` only while the
//!   webview's `heartbeat` command (sent every second) was seen within [`HEARTBEAT_FRESH`], so a
//!   hung webview behind a live shell still gets restarted.
//! - **Restore once.** The first `restore_state` of this process is delivered; later ones (an
//!   IPC reconnect after a service restart) are ignored, since the app already has its state.
//! - **Latest state wins.** The webview's state is a single latest-value slot, never dropped
//!   when the line is busy, and re-sent on every (re)connect.
//! - **Windows:** the pipe's server must run as SYSTEM, or the connection is refused.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use arena_kiosk_core::ipc::{IpcMsg, read_msg, write_msg};
use arena_proto::{KioskHubState, KioskHubStatus};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::sync::{mpsc, watch};

/// The Tauri event carrying a [`KioskHubStatus`].
pub const HUB_STATUS_EVENT: &str = "hub-status";
/// The Tauri event carrying the state to restore (the JSON the webview reported).
pub const RESTORE_STATE_EVENT: &str = "restore-state";
/// The Tauri commands the webview calls: the `generate_handler!` list in `main.rs`, pinned
/// against `ipc-names.json` (the list the webview reads) by a test.
#[cfg(test)]
const COMMANDS: [&str; 4] = [
    "report_app_state",
    "hub_status",
    "take_restore_state",
    "heartbeat",
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
}

/// The shell's view of the channel, shared with the Tauri commands.
pub struct Bridge {
    /// The webview's latest state (the slot every connection sends from).
    state: watch::Sender<Option<Value>>,
    hub: Mutex<Option<KioskHubStatus>>,
    restore: Mutex<Option<Value>>,
    restore_seen: AtomicBool,
    heartbeat: Mutex<Option<Instant>>,
}

impl Default for Bridge {
    fn default() -> Self {
        Self {
            state: watch::channel(None).0,
            hub: Mutex::default(),
            restore: Mutex::default(),
            restore_seen: AtomicBool::new(false),
            heartbeat: Mutex::default(),
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

    /// The webview is alive.
    pub fn heartbeat(&self) {
        self.heartbeat_at(Instant::now());
    }

    fn heartbeat_at(&self, now: Instant) {
        *lock(&self.heartbeat) = Some(now);
    }

    /// The Hub link state, once known.
    pub fn hub_status(&self) -> Option<KioskHubStatus> {
        *lock(&self.hub)
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
        // The latest state goes out on every (re)connect: a restarted service lost it.
        let mut state = self.state.subscribe();
        state.mark_changed();
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
