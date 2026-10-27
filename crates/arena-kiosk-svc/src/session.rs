use std::sync::Arc;
use std::time::Duration;

use arena_proto::{LockReason, SessionScreen};
use tokio::sync::watch;
use tokio::time::Instant;

use crate::pc::{ScreenSession, set_locked};
use crate::runtime::Ctx;

pub fn sync(ctx: &Ctx, s: SessionScreen, started: bool) {
    if !started && ctx.pc.screen.borrow().locked {
        return;
    }
    let now = Instant::now();
    ctx.pc
        .deadline
        .send_replace(Some(now + Duration::from_millis(s.lock_after_ms)));
    if started {
        set_locked(ctx, false, None);
    }
    ctx.pc.screen.send_modify(|v| {
        let warning_min = v
            .session
            .as_ref()
            .and_then(|p| p.warning_min)
            .filter(|m| s.lock_after_ms <= u64::from(*m) * 60_000);
        v.session = Some(ScreenSession {
            device_name: s.device_name,
            started: now
                .checked_sub(Duration::from_millis(s.elapsed_ms))
                .unwrap_or(now),
            warning_min,
        });
    });
}

pub fn warn(ctx: &Ctx, minutes_left: u8) {
    ctx.pc.screen.send_if_modified(|v| match &mut v.session {
        Some(s) if s.warning_min != Some(minutes_left) => {
            s.warning_min = Some(minutes_left);
            true
        }
        _ => false,
    });
}

pub(crate) async fn run(ctx: Arc<Ctx>, mut stop: watch::Receiver<bool>) {
    let mut rx = ctx.pc.deadline.subscribe();
    loop {
        let deadline = *rx.borrow_and_update();
        tokio::select! {
            () = async {
                match deadline {
                    Some(d) => tokio::time::sleep_until(d).await,
                    None => std::future::pending().await,
                }
            } => {
                tracing::info!("the session reached its lock time; locked");
                set_locked(&ctx, true, Some(LockReason::TimesUp));
            }
            changed = rx.changed() => if changed.is_err() { return },
            _ = stop.changed() => return,
        }
    }
}
