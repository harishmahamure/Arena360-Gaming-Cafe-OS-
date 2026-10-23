//! The app supervisor: accepts the Kiosk app's IPC connection, keeps it fed with the Hub link
//! state, answers its requests, and runs the watchdog.
//!
//! - One app connection at a time: the newest wins and the older one is closed.
//! - On every connection the app gets `hub_status`, then `restore_state` with the last
//!   `app_state` it reported (when there is one).
//! - Every 2 s the app is pinged; after 3 misses in a row it is killed and relaunched through
//!   the [`AppLauncher`], and a `kiosk.log{warn, app_restarted}` is queued for the Hub.
//!   Restarts back off (see the watchdog). While the app cannot run at all (no player in the
//!   console session, or the pipe not open to a resolved player) nothing counts as a miss, and
//!   one `kiosk.log{warn, no_session}` is queued per such stretch.
//! - Requests: `log{level, code, detail}` queues a `kiosk.log`; `hub_status` answers the link
//!   state. Anything else is `UNSUPPORTED`. The app gets nothing else: no Hub socket, no token.

use std::sync::Arc;

use arena_kiosk_core::ipc::{IpcError, IpcMsg, read_msg, write_msg};
use arena_kiosk_core::{Watchdog, WatchdogAction};
use arena_proto::{AppError, ErrorCode, KioskLogLevel};
use serde::Deserialize;
use serde_json::Value;
use tokio::io::BufReader;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;

use crate::ipc::{IpcServer, IpcStream};
use crate::launcher::AppLauncher;
use crate::runtime::Ctx;

const OUT_DEPTH: usize = 64;

enum Event {
    Conn {
        id: u64,
        out: mpsc::Sender<IpcMsg>,
        kill: oneshot::Sender<()>,
    },
    Closed {
        id: u64,
    },
    Pong {
        id: u64,
        n: u64,
    },
}

struct Current {
    id: u64,
    out: mpsc::Sender<IpcMsg>,
    kill: oneshot::Sender<()>,
}

/// Runs the supervisor until `stop` is set.
pub(crate) async fn run<S: IpcServer>(
    ctx: Arc<Ctx>,
    mut server: S,
    launcher: Arc<dyn AppLauncher>,
    mut stop: watch::Receiver<bool>,
) {
    let ipc_ready = server.ready();
    let (tx, mut rx) = mpsc::channel::<Event>(64);
    let accept_ctx = Arc::clone(&ctx);
    let mut accept_stop = stop.clone();
    let acceptor = tokio::spawn(async move {
        let mut next_id = 0u64;
        loop {
            let stream = tokio::select! {
                s = server.accept() => s,
                _ = accept_stop.changed() => return,
            };
            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, "ipc accept failed");
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    continue;
                }
            };
            next_id += 1;
            let id = next_id;
            let (out_tx, out_rx) = mpsc::channel(OUT_DEPTH);
            let (kill_tx, kill_rx) = oneshot::channel();
            // Registered before it is served, so its Closed can never arrive first.
            if tx
                .send(Event::Conn {
                    id,
                    out: out_tx.clone(),
                    kill: kill_tx,
                })
                .await
                .is_err()
            {
                return;
            }
            tokio::spawn(serve(
                Arc::clone(&accept_ctx),
                id,
                stream,
                out_tx,
                out_rx,
                kill_rx,
                tx.clone(),
            ));
        }
    });

    let started = Instant::now();
    let now_ms = || started.elapsed().as_millis() as u64;
    let t = ctx.timings.clone();
    let mut dog = Watchdog::with_timings(now_ms(), t.ping_interval, t.restart_grace);
    let mut hub_state = ctx.hub_state.subscribe();
    let mut current: Option<Current> = None;
    let mut held = false;
    loop {
        let due = started + std::time::Duration::from_millis(dog.next_tick_ms());
        tokio::select! {
            ev = rx.recv() => {
                let Some(ev) = ev else { break };
                match ev {
                    Event::Conn { id, out, kill } => {
                        if let Some(old) = current.take() {
                            tracing::info!(old = old.id, new = id, "a newer app connection replaces the older one");
                            let _ = old.kill.send(());
                        }
                        let _ = out.try_send(IpcMsg::HubStatus { state: *hub_state.borrow_and_update() });
                        let last = ctx.app_state.lock().unwrap_or_else(|p| p.into_inner()).clone();
                        if let Some(json) = last {
                            let _ = out.try_send(IpcMsg::RestoreState { json });
                        }
                        dog.on_connected(now_ms());
                        tracing::info!(conn = id, "app connected");
                        current = Some(Current { id, out, kill });
                    }
                    Event::Closed { id } => {
                        if current.as_ref().is_some_and(|c| c.id == id) {
                            tracing::warn!(conn = id, "app disconnected");
                            current = None;
                            dog.on_disconnected();
                        }
                    }
                    Event::Pong { id, n } => {
                        if current.as_ref().is_some_and(|c| c.id == id) {
                            dog.on_pong(n);
                        }
                    }
                }
            }
            () = tokio::time::sleep_until(due) => {
                let l = Arc::clone(&launcher);
                let can_run = ipc_ready.load(std::sync::atomic::Ordering::SeqCst)
                    && tokio::task::spawn_blocking(move || l.ready()).await.unwrap_or(false);
                if !can_run {
                    if !held {
                        held = true;
                        tracing::warn!("the app cannot run now (no player session or pipe); watchdog on hold");
                        if let Err(e) = ctx.log_event(KioskLogLevel::Warn, "no_session", "no player session or app pipe; the watchdog is on hold") {
                            tracing::error!(error = %e, "could not queue the no_session event");
                        }
                    }
                    dog.hold(now_ms());
                    continue;
                }
                held = false;
                match dog.tick(now_ms()) {
                    Some(WatchdogAction::Ping(n)) => {
                        if let Some(c) = &current {
                            let _ = c.out.try_send(IpcMsg::Ping { n });
                        }
                    }
                    Some(WatchdogAction::Restart) => {
                        tracing::warn!(missed = arena_kiosk_core::watchdog::MAX_MISSED, "the app stopped answering; restarting it");
                        if let Some(old) = current.take() {
                            let _ = old.kill.send(());
                        }
                        dog.on_disconnected();
                        if let Err(e) = ctx.log_event(KioskLogLevel::Warn, "app_restarted", "no answer to 3 pings in a row (hung, crashed or not connected)") {
                            tracing::error!(error = %e, "could not queue the restart event");
                        }
                        let l = Arc::clone(&launcher);
                        match tokio::task::spawn_blocking(move || l.restart()).await {
                            Ok(Ok(())) => {}
                            Ok(Err(e)) => tracing::error!(detail = %e, "app relaunch failed"),
                            Err(e) => tracing::error!(error = %e, "app relaunch panicked"),
                        }
                    }
                    Some(WatchdogAction::Wait) | None => {}
                }
            }
            changed = hub_state.changed() => {
                if changed.is_err() { break; }
                let state = *hub_state.borrow_and_update();
                if let Some(c) = &current {
                    let _ = c.out.try_send(IpcMsg::HubStatus { state });
                }
            }
            _ = stop.changed() => break,
        }
    }
    if let Some(c) = current.take() {
        let _ = c.kill.send(());
    }
    acceptor.abort();
}

#[derive(Deserialize)]
struct LogBody {
    level: KioskLogLevel,
    code: String,
    #[serde(default)]
    detail: String,
}

fn handle_request(ctx: &Ctx, kind: &str, body: Value) -> Result<Value, AppError> {
    match kind {
        "log" => {
            let b: LogBody = serde_json::from_value(body)
                .map_err(|e| AppError::new(ErrorCode::BadFrame, format!("log body: {e}")))?;
            // The Hub's own rules, so nothing that would only be dropped there is queued.
            arena_proto::KioskLog {
                level: b.level,
                code: b.code.clone(),
                detail: b.detail.clone(),
            }
            .validate()?;
            ctx.log_event(b.level, &b.code, &b.detail)
                .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string()))?;
            Ok(Value::Object(Default::default()))
        }
        "hub_status" => Ok(serde_json::json!({ "state": *ctx.hub_state.borrow() })),
        other => Err(AppError::new(
            ErrorCode::Unsupported,
            format!("unknown request kind {other}"),
        )),
    }
}

async fn serve(
    ctx: Arc<Ctx>,
    id: u64,
    stream: IpcStream,
    out_tx: mpsc::Sender<IpcMsg>,
    mut out_rx: mpsc::Receiver<IpcMsg>,
    mut kill: oneshot::Receiver<()>,
    events: mpsc::Sender<Event>,
) {
    let IpcStream { reader, mut writer } = stream;
    let mut writer_task = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if write_msg(&mut writer, &msg).await.is_err() {
                break;
            }
        }
    });
    let mut reader = BufReader::new(reader);
    loop {
        let msg = tokio::select! {
            m = read_msg(&mut reader) => m,
            _ = &mut kill => break,
            _ = &mut writer_task => break,
        };
        match msg {
            Ok(IpcMsg::Pong { n }) => {
                let _ = events.send(Event::Pong { id, n }).await;
            }
            Ok(IpcMsg::AppState { json }) => {
                *ctx.app_state.lock().unwrap_or_else(|p| p.into_inner()) = Some(json);
            }
            Ok(IpcMsg::Request { rid, kind, body }) => {
                let reply = match handle_request(&ctx, &kind, body) {
                    Ok(body) => IpcMsg::Reply {
                        rid,
                        body,
                        error: None,
                    },
                    Err(e) => IpcMsg::Reply {
                        rid,
                        body: Value::Null,
                        error: Some(e),
                    },
                };
                let _ = out_tx.send(reply).await;
            }
            Ok(other) => {
                tracing::debug!(conn = id, msg = ?std::mem::discriminant(&other), "ignoring a service-side message from the app");
            }
            Err(IpcError::Closed) => break,
            Err(e) => tracing::warn!(conn = id, error = %e, "skipping a bad ipc line"),
        }
    }
    writer_task.abort();
    let _ = events.send(Event::Closed { id }).await;
}
