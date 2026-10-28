//! The driver's side of the kiosk service's app channel (`<kiosk data>/ipc/kiosk.sock`): it
//! stands in for the Kiosk app. A reader task answers every `ping`, routes `reply` to the
//! waiting request and tracks the last `theme`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_kiosk_core::ipc::{IpcMsg, MAX_APP_LINE_BYTES, SecretPin, read_msg_limited, write_msg};
use arena_proto::AppError;
use serde_json::Value;
use tokio::io::BufReader;
use tokio::net::UnixStream;
use tokio::net::unix::OwnedWriteHalf;
use tokio::sync::{oneshot, watch};

/// How long a request waits for its reply.
const REPLY_TIMEOUT: Duration = Duration::from_secs(20);

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<IpcMsg>>>>;

/// A connected app channel.
pub struct Kiosk {
    writer: tokio::sync::Mutex<OwnedWriteHalf>,
    pending: Pending,
    next: AtomicU64,
    /// The last `theme{hc}` the service sent.
    pub theme: watch::Receiver<Option<bool>>,
}

impl Kiosk {
    /// Connects to the service's socket under `data_dir`, retrying for up to `wait`.
    pub async fn connect(data_dir: &Path, wait: Duration) -> Result<Arc<Self>, String> {
        let path = arena_kiosk_core::ipc::socket_path(data_dir);
        let deadline = tokio::time::Instant::now() + wait;
        let stream = loop {
            match UnixStream::connect(&path).await {
                Ok(s) => break s,
                Err(e) if tokio::time::Instant::now() >= deadline => {
                    return Err(format!("connect {}: {e}", path.display()));
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        };
        let (r, w) = stream.into_split();
        let pending: Pending = Arc::default();
        let (theme_tx, theme) = watch::channel(None);
        let (pong_tx, mut pong_rx) = tokio::sync::mpsc::channel::<u64>(16);
        let kiosk = Self {
            writer: tokio::sync::Mutex::new(w),
            pending: Arc::clone(&pending),
            next: AtomicU64::new(1),
            theme,
        };
        tokio::spawn(async move {
            let mut reader = BufReader::new(r);
            loop {
                match read_msg_limited(&mut reader, MAX_APP_LINE_BYTES).await {
                    Ok(IpcMsg::Ping { n }) => {
                        let _ = pong_tx.send(n).await;
                    }
                    Ok(IpcMsg::Theme { hc }) => {
                        theme_tx.send_replace(Some(hc));
                    }
                    Ok(msg @ IpcMsg::Reply { .. }) => {
                        let IpcMsg::Reply { rid, .. } = &msg else {
                            unreachable!()
                        };
                        let waiter = pending
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .remove(rid);
                        if let Some(w) = waiter {
                            let _ = w.send(msg);
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        // Closed, or a line that cannot be read: either way the channel is no
                        // use any more. Waiting requests fail at once instead of timing out.
                        if !matches!(e, arena_kiosk_core::ipc::IpcError::Closed) {
                            eprintln!("arena-e2e: ipc reader stopped: {e}");
                        }
                        pending.lock().unwrap_or_else(|p| p.into_inner()).clear();
                        return;
                    }
                }
            }
        });
        let kiosk = Arc::new(kiosk);
        let pinger = Arc::clone(&kiosk);
        tokio::spawn(async move {
            while let Some(n) = pong_rx.recv().await {
                if pinger.send(&IpcMsg::Pong { n }).await.is_err() {
                    return;
                }
            }
        });
        Ok(kiosk)
    }

    async fn send(&self, msg: &IpcMsg) -> Result<(), String> {
        write_msg(&mut *self.writer.lock().await, msg)
            .await
            .map_err(|e| format!("ipc write: {e}"))
    }

    async fn ask(&self, rid: String, msg: IpcMsg) -> Result<(Value, Option<AppError>), String> {
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(rid.clone(), tx);
        let forget = || {
            self.pending
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&rid);
        };
        if let Err(e) = self.send(&msg).await {
            forget();
            return Err(e);
        }
        match tokio::time::timeout(REPLY_TIMEOUT, rx).await {
            Ok(Ok(IpcMsg::Reply { body, error, .. })) => Ok((body, error)),
            Ok(_) => Err(format!("ipc {rid}: the channel closed")),
            Err(_) => {
                forget();
                Err(format!("ipc {rid}: no reply"))
            }
        }
    }

    fn rid(&self, what: &str) -> String {
        format!("e2e-{what}-{}", self.next.fetch_add(1, Ordering::SeqCst))
    }

    /// Sends the request `kind` (a `dev.*` hook or a service request); an error reply is `Err`.
    pub async fn request(&self, kind: &str, body: Value) -> Result<Value, String> {
        let rid = self.rid(kind);
        let msg = IpcMsg::Request {
            rid: rid.clone(),
            kind: kind.into(),
            body,
        };
        match self.ask(rid, msg).await? {
            (body, None) => Ok(body),
            (_, Some(e)) => Err(format!("{kind}: {} {}", e.code.as_str(), e.message)),
        }
    }

    /// `admin_exit{pin}`: `Ok(until)` when granted.
    pub async fn admin_exit(&self, pin: &str) -> Result<i64, String> {
        let rid = self.rid("admin-exit");
        let msg = IpcMsg::AdminExit {
            rid: rid.clone(),
            pin: SecretPin::new(pin),
        };
        match self.ask(rid, msg).await? {
            (body, None) => body["until"]
                .as_i64()
                .ok_or_else(|| format!("admin_exit: no until in {body}")),
            (_, Some(e)) => Err(format!("admin_exit refused: {}", e.code.as_str())),
        }
    }

    /// Waits until the service has sent `theme{hc: want}`.
    pub async fn wait_theme(&self, want: bool, wait: Duration) -> Result<(), String> {
        let mut rx = self.theme.clone();
        tokio::time::timeout(wait, rx.wait_for(|t| *t == Some(want)))
            .await
            .map_err(|_| format!("the kiosk never got theme hc={want}"))?
            .map(|_| ())
            .map_err(|_| "the ipc channel closed".to_owned())
    }
}
