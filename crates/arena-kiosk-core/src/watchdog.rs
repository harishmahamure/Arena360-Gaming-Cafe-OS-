//! The Kiosk app watchdog, as a pure state machine driven by an injected clock (milliseconds
//! from any fixed origin).
//!
//! Every [`PING_INTERVAL`] the service pings the app. A ping that is still unanswered at the
//! next tick is a miss, and so is a tick with no app connected at all (it crashed or hung
//! before connecting). After [`MAX_MISSED`] misses in a row the service restarts the app, then
//! waits [`RESTART_GRACE`] plus a back-off for the new process to connect before counting
//! again. The back-off doubles with every restart that was not followed by a `pong` (2, 4, 8 …
//! up to 60 s at the default interval), so an app that cannot start is not relaunched in a
//! tight loop. A matching `pong` clears the misses and the back-off.
//!
//! While the app cannot run at all (no player session, or the pipe is not open to a resolved
//! player account) the service calls [`Watchdog::hold`]: nothing counts as a miss.

use std::time::Duration;

/// How often the service pings the app.
pub const PING_INTERVAL: Duration = Duration::from_secs(2);
/// Consecutive misses that restart the app.
pub const MAX_MISSED: u32 = 3;
/// How long a freshly restarted app has to connect before misses count again.
pub const RESTART_GRACE: Duration = Duration::from_secs(10);

/// What the service should do at a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogAction {
    /// Send `ping{n}` to the app.
    Ping(u64),
    /// Kill and relaunch the app.
    Restart,
    /// Nothing to send (no app is connected; the miss was counted).
    Wait,
}

/// The watchdog state.
#[derive(Debug, Clone)]
pub struct Watchdog {
    interval_ms: u64,
    grace_ms: u64,
    next_tick_ms: u64,
    outstanding: Option<u64>,
    missed: u32,
    last_n: u64,
    connected: bool,
    /// Restarts since the last pong.
    restarts: u32,
}

impl Watchdog {
    /// A watchdog started at `now_ms`, with no app connected yet: the first ticks fall after
    /// the start grace.
    pub fn new(now_ms: u64) -> Self {
        Self::with_timings(now_ms, PING_INTERVAL, RESTART_GRACE)
    }

    /// A watchdog with other timings (tests).
    pub fn with_timings(now_ms: u64, interval: Duration, grace: Duration) -> Self {
        let interval_ms = interval.as_millis().max(1) as u64;
        let grace_ms = grace.as_millis() as u64;
        Self {
            interval_ms,
            grace_ms,
            next_tick_ms: now_ms + grace_ms,
            outstanding: None,
            missed: 0,
            last_n: 0,
            connected: false,
            restarts: 0,
        }
    }

    /// When [`tick`](Self::tick) is next due.
    pub fn next_tick_ms(&self) -> u64 {
        self.next_tick_ms
    }

    /// Misses counted so far.
    pub fn missed(&self) -> u32 {
        self.missed
    }

    /// An app connected: ping it at the next tick (no miss carries over from before).
    pub fn on_connected(&mut self, now_ms: u64) {
        self.connected = true;
        self.outstanding = None;
        self.missed = 0;
        self.next_tick_ms = self.next_tick_ms.min(now_ms + self.interval_ms);
    }

    /// The app's connection closed: ticks count as misses until one connects.
    pub fn on_disconnected(&mut self) {
        self.connected = false;
        self.outstanding = None;
    }

    /// A `pong{n}`: only the answer to the ping outstanding clears the misses (and the restart
    /// back-off).
    pub fn on_pong(&mut self, n: u64) {
        if self.outstanding == Some(n) {
            self.outstanding = None;
            self.missed = 0;
            self.restarts = 0;
        }
    }

    /// The app cannot run now: count nothing, look again at the next interval.
    pub fn hold(&mut self, now_ms: u64) {
        self.missed = 0;
        self.outstanding = None;
        self.next_tick_ms = now_ms + self.interval_ms;
    }

    /// The back-off added after the `restarts`-th restart in a row: the interval doubled per
    /// restart, at most 30 intervals.
    fn restart_backoff_ms(&self) -> u64 {
        let shift = self.restarts.saturating_sub(1).min(16);
        (self.interval_ms << shift).min(self.interval_ms * 30)
    }

    /// Advances to `now_ms`. Returns `None` before the next tick is due.
    pub fn tick(&mut self, now_ms: u64) -> Option<WatchdogAction> {
        if now_ms < self.next_tick_ms {
            return None;
        }
        self.next_tick_ms = now_ms + self.interval_ms;
        if self.outstanding.is_some() || !self.connected {
            self.missed += 1;
        }
        if self.missed >= MAX_MISSED {
            self.missed = 0;
            self.outstanding = None;
            self.restarts = self.restarts.saturating_add(1);
            self.next_tick_ms =
                now_ms + self.grace_ms.max(self.interval_ms) + self.restart_backoff_ms();
            return Some(WatchdogAction::Restart);
        }
        if !self.connected {
            return Some(WatchdogAction::Wait);
        }
        self.last_n += 1;
        self.outstanding = Some(self.last_n);
        Some(WatchdogAction::Ping(self.last_n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const I: u64 = 2_000;
    const G: u64 = 10_000;

    fn connected() -> Watchdog {
        let mut w = Watchdog::new(0);
        w.on_connected(0);
        w
    }

    #[test]
    fn an_answering_app_is_pinged_every_two_seconds_and_never_restarted() {
        let mut w = connected();
        assert_eq!(w.tick(I - 1), None);
        let mut now = I;
        for n in 1..=50 {
            assert_eq!(w.tick(now), Some(WatchdogAction::Ping(n)));
            w.on_pong(n);
            assert_eq!(w.tick(now + I - 1), None);
            now += I;
        }
    }

    #[test]
    fn three_missed_pongs_restart_the_app() {
        let mut w = connected();
        assert_eq!(w.tick(I), Some(WatchdogAction::Ping(1)));
        assert_eq!(w.tick(2 * I), Some(WatchdogAction::Ping(2)));
        assert_eq!(w.missed(), 1);
        assert_eq!(w.tick(3 * I), Some(WatchdogAction::Ping(3)));
        assert_eq!(w.tick(4 * I), Some(WatchdogAction::Restart));
        // The new process gets the grace (plus the first back-off step) first.
        assert_eq!(w.next_tick_ms(), 4 * I + G + I);
        assert_eq!(w.tick(4 * I + G + I - 1), None);
    }

    #[test]
    fn a_late_or_stale_pong_does_not_clear_misses() {
        let mut w = connected();
        w.tick(I);
        w.tick(2 * I);
        w.on_pong(1); // the answer to the ping before the outstanding one
        assert_eq!(w.missed(), 1);
        w.on_pong(2);
        assert_eq!(w.missed(), 0);
    }

    #[test]
    fn a_crashed_app_that_never_reconnects_is_restarted() {
        let mut w = connected();
        assert_eq!(w.tick(I), Some(WatchdogAction::Ping(1)));
        w.on_pong(1);
        w.on_disconnected();
        assert_eq!(w.tick(2 * I), Some(WatchdogAction::Wait));
        assert_eq!(w.tick(3 * I), Some(WatchdogAction::Wait));
        assert_eq!(w.tick(4 * I), Some(WatchdogAction::Restart));
        // It reconnects: pinged at the next interval, misses cleared.
        w.on_connected(4 * I + 500);
        assert_eq!(w.next_tick_ms(), 4 * I + 500 + I);
        assert_eq!(w.tick(4 * I + 500 + I), Some(WatchdogAction::Ping(2)));
    }

    #[test]
    fn restarts_back_off_until_a_pong_and_holds_count_nothing() {
        let mut w = Watchdog::new(0);
        let mut now = G;
        let mut gaps = Vec::new();
        for _ in 0..8 {
            // Never connected: 3 waits, then a restart.
            while w.tick(now) != Some(WatchdogAction::Restart) {
                now = w.next_tick_ms();
            }
            gaps.push(w.next_tick_ms() - now - G);
            now = w.next_tick_ms();
        }
        assert_eq!(
            gaps,
            [I, 2 * I, 4 * I, 8 * I, 16 * I, 30 * I, 30 * I, 30 * I]
        );
        // A pong resets the back-off.
        w.on_connected(now);
        now = w.next_tick_ms();
        let Some(WatchdogAction::Ping(n)) = w.tick(now) else {
            panic!()
        };
        w.on_pong(n);
        w.on_disconnected();
        for _ in 0..3 {
            now = w.next_tick_ms();
            w.tick(now);
        }
        assert_eq!(w.next_tick_ms() - now - G, I);

        // While held (no player session), nothing counts.
        let mut w = Watchdog::new(0);
        for k in 0..20 {
            w.hold(G + k * I);
            assert_eq!(w.missed(), 0);
        }
        assert_eq!(w.tick(G + 20 * I), Some(WatchdogAction::Wait));
        assert_eq!(w.missed(), 1);
    }

    #[test]
    fn an_app_that_never_starts_is_restarted_after_the_grace() {
        let mut w = Watchdog::new(0);
        assert_eq!(w.tick(G - 1), None);
        assert_eq!(w.tick(G), Some(WatchdogAction::Wait));
        assert_eq!(w.tick(G + I), Some(WatchdogAction::Wait));
        assert_eq!(w.tick(G + 2 * I), Some(WatchdogAction::Restart));
    }
}
