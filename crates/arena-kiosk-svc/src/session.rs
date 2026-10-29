use std::sync::Arc;
use std::time::Duration;

use arena_proto::{LockReason, SessionScreen, TimeUpMode};
use tokio::sync::watch;
use tokio::time::Instant;

use crate::pc::{ScreenSession, set_locked};
use crate::runtime::Ctx;

/// The branch's cached operating settings: the warning marks and the low-balance amount.
fn operating(ctx: &Ctx) -> crate::operating::KioskOperating {
    crate::operating::load(&ctx.queue.lock().unwrap_or_else(|p| p.into_inner()))
}

pub fn sync(ctx: &Ctx, mut s: SessionScreen, started: bool) {
    let operating = operating(ctx);
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
    crate::cleanup::mark_dirty(ctx);
    let unserved_items: u64 = s.orders.iter().map(|o| o.unserved).sum();
    let orders = std::mem::take(&mut s.orders);
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
            lock_at: now + Duration::from_millis(s.lock_after_ms),
            grace_ms: s.grace_ms,
            ordered_items: s.ordered_items,
            unserved_items,
            username: s.username,
            guest_no: s.guest_no,
            ends_at: s.remaining_ms.map(|ms| now + Duration::from_millis(ms)),
            balance_minor: s.balance_minor,
            low_balance_minor: s
                .wallet_left_minor
                .filter(|left| operating.low_balance().is_some_and(|t| *left <= t)),
            plan_name: s.plan_name,
            hold: s.time_up_mode == TimeUpMode::StaffHandles.as_str(),
        });
    });
    ctx.pc.track_orders(&orders, ctx.clock.now_ms());
}

/// The Hub is back (Story 6.36) and has said what it holds for this PC: its record wins. A live
/// session snaps the chip and HUD to the Hub's time, and a pause is followed either way; a
/// session the Hub ended meanwhile locks with the reason the Hub gives, which also runs the
/// session-end cleanup. A session the kiosk already ended on its own clock (time up) is gone
/// from the screen, so the Hub's later word about it changes nothing.
pub fn reconcile(ctx: &Ctx, hub: arena_proto::SessionSync) {
    let (has_session, locked, shown) = {
        let v = ctx.pc.screen.borrow();
        (v.session.is_some(), v.locked, v.reason)
    };
    if !has_session {
        return;
    }
    match hub.session {
        Some(s) => {
            if locked && shown == Some(LockReason::Paused) && hub.lock != Some(LockReason::Paused) {
                set_locked(ctx, false, None);
            }
            sync(ctx, s, false);
            if hub.lock == Some(LockReason::Paused) && !locked {
                set_locked(ctx, true, Some(LockReason::Paused));
            }
            tracing::info!("the hub's record of the session replaced the kiosk's clock");
        }
        None => {
            let reason = hub.lock.unwrap_or(LockReason::EndedByStaff);
            tracing::info!(
                ?reason,
                "the hub ended the session while the link was down; locking"
            );
            set_locked(ctx, true, Some(reason));
        }
    }
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

/// The next warning mark still ahead on a prepaid session ending at `ends_at`: the longest mark
/// whose moment is after `now` and shorter than the one already raised (Story 6.19). The kiosk
/// counts the marks on its own monotonic clock, so a Hub that cannot be reached does not
/// silence them; a mark the Hub already sent is the same value and changes nothing.
fn next_mark(
    marks: &[u8],
    ends_at: Instant,
    now: Instant,
    raised: Option<u8>,
) -> Option<(u8, Instant)> {
    marks.iter().find_map(|&m| {
        let at = ends_at.checked_sub(Duration::from_secs(u64::from(m) * 60))?;
        (at > now && raised.is_none_or(|r| m < r)).then_some((m, at))
    })
}

/// When the grace after a prepaid session's end time begins, if that is still ahead (Story 6.22).
/// The screen is told then too, so the HUD draws its grace card on the kiosk's own clock with
/// the Hub unreachable.
fn grace_start(s: &ScreenSession, now: Instant) -> Option<Instant> {
    s.ends_at?;
    (s.grace_ms > 0)
        .then(|| s.lock_at.checked_sub(Duration::from_millis(s.grace_ms)))
        .flatten()
        .filter(|at| *at > now)
}

pub(crate) async fn run(ctx: Arc<Ctx>, mut stop: watch::Receiver<bool>) {
    let mut rx = ctx.pc.deadline.subscribe();
    let mut screen = ctx.pc.screen.subscribe();
    loop {
        let deadline = *rx.borrow_and_update();
        let marks = operating(&ctx).marks();
        let (mark, grace_at) = {
            let v = screen.borrow_and_update();
            let session = v.session.as_ref().filter(|_| !v.locked);
            (
                session.and_then(|s| next_mark(&marks, s.ends_at?, Instant::now(), s.warning_min)),
                session.and_then(|s| grace_start(s, Instant::now())),
            )
        };
        tokio::select! {
            () = async {
                match grace_at {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            } => {
                tracing::info!("the grace began on the kiosk clock");
                // No value changes; the watchers are told so the app learns the grace began.
                ctx.pc.screen.send_modify(|_| {});
            }
            () = async {
                match mark {
                    Some((_, at)) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            } => {
                if let Some((minutes_left, _)) = mark {
                    tracing::info!(minutes_left, "a warning mark passed on the kiosk clock");
                    warn(&ctx, minutes_left);
                }
            }
            changed = screen.changed() => if changed.is_err() { return },
            () = async {
                match deadline {
                    Some(d) => tokio::time::sleep_until(d).await,
                    None => std::future::pending().await,
                }
            } => {
                tracing::info!("the session reached its lock time; locked");
                let hold = ctx.pc.screen.borrow().session.as_ref().is_some_and(|s| s.hold);
                set_locked(
                    &ctx,
                    true,
                    Some(if hold { LockReason::TimesUpSeeCounter } else { LockReason::TimesUp }),
                );
            }
            changed = rx.changed() => if changed.is_err() { return },
            _ = stop.changed() => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const MARKS: [u8; 3] = [10, 5, 1];

    fn at(base: Instant, secs: u64) -> Instant {
        base + Duration::from_secs(secs)
    }

    #[test]
    fn the_longest_mark_still_ahead_is_next() {
        let now = Instant::now();
        // 30 min left: the 10-min mark is 20 min away.
        let (m, when) = next_mark(&MARKS, at(now, 1800), now, None).unwrap();
        assert_eq!((m, when), (10, at(now, 1200)));
        // The 10-min mark raised: the 5-min mark is next.
        assert_eq!(
            next_mark(&MARKS, at(now, 1800), now, Some(10)).unwrap().0,
            5
        );
        assert_eq!(next_mark(&MARKS, at(now, 1800), now, Some(5)).unwrap().0, 1);
        assert_eq!(next_mark(&MARKS, at(now, 1800), now, Some(1)), None);
    }

    #[test]
    fn a_mark_already_past_is_not_raised_late() {
        let now = Instant::now();
        // 7 min left: the 10-min mark has gone; only 5 and 1 are ahead.
        assert_eq!(next_mark(&MARKS, at(now, 420), now, None).unwrap().0, 5);
        // 30 s left: nothing is ahead.
        assert_eq!(next_mark(&MARKS, at(now, 30), now, None), None);
    }

    fn session(now: Instant, ends_in: Option<u64>, lock_in: u64, grace_ms: u64) -> ScreenSession {
        ScreenSession {
            device_name: "PC".into(),
            started: now,
            warning_min: None,
            lock_at: at(now, lock_in),
            grace_ms,
            ordered_items: 0,
            unserved_items: 0,
            username: None,
            guest_no: None,
            ends_at: ends_in.map(|s| at(now, s)),
            balance_minor: None,
            plan_name: None,
            low_balance_minor: None,
            hold: false,
        }
    }

    #[test]
    fn the_grace_begins_one_grace_before_the_lock() {
        let now = Instant::now();
        // Ends in 60 s, locks in 180 s: a 120 s grace starts at the end time.
        let s = session(now, Some(60), 180, 120_000);
        assert_eq!(grace_start(&s, now), Some(at(now, 60)));
        // Already in the grace, or no grace, or postpaid: nothing ahead.
        assert_eq!(grace_start(&s, at(now, 60)), None);
        assert_eq!(grace_start(&session(now, Some(60), 60, 0), now), None);
        assert_eq!(grace_start(&session(now, None, 180, 120_000), now), None);
    }

    #[test]
    fn a_mark_the_hub_raised_late_does_not_go_backwards() {
        let now = Instant::now();
        // Warning 1 raised, then an extension: the next mark ahead may be 1 again only after
        // the sync cleared the warning, so a raised 1 is never followed by a longer mark.
        assert_eq!(next_mark(&MARKS, at(now, 3000), now, Some(1)), None);
    }
}
