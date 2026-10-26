use std::sync::Arc;
use std::time::Duration;

use arena_kiosk_core::ipc::IpcMsg;
use tokio::sync::watch;
use tokio::time::Instant;

use crate::runtime::Ctx;

pub fn begin(ctx: &Ctx, lock_after_ms: u64) {
    ctx.test_session.send_if_modified(|d| {
        if d.is_some() {
            return false;
        }
        *d = Some(Instant::now() + Duration::from_millis(lock_after_ms));
        true
    });
}

pub(crate) fn message(deadline: Option<Instant>) -> IpcMsg {
    match deadline {
        Some(d) => IpcMsg::TestSessionStarted {
            remaining_ms: d.saturating_duration_since(Instant::now()).as_millis() as u64,
        },
        None => IpcMsg::TestSessionEnded,
    }
}

fn lock(ctx: &Ctx) {
    if ctx.test_session.send_replace(None).is_none() {
        return;
    }
    let device_id = ctx
        .config()
        .map_or_else(|| "unprovisioned".into(), |c| c.device_id);
    let intent = crate::intents::device_test_session_locked(&device_id, ctx.clock.now_ms());
    match ctx
        .queue
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(&intent)
    {
        Ok(()) => ctx.queued.notify_one(),
        Err(e) => tracing::error!(error = %e, "could not queue device.test_session.completed"),
    }
    tracing::info!("test session over; locked");
}

pub(crate) async fn run(ctx: Arc<Ctx>, mut stop: watch::Receiver<bool>) {
    let mut rx = ctx.test_session.subscribe();
    loop {
        let deadline = *rx.borrow_and_update();
        tokio::select! {
            () = async {
                match deadline {
                    Some(d) => tokio::time::sleep_until(d).await,
                    None => std::future::pending().await,
                }
            } => lock(&ctx),
            changed = rx.changed() => if changed.is_err() { return },
            _ = stop.changed() => return,
        }
    }
}
