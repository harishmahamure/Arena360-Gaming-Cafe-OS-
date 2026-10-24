//! The kiosk's admin exit, as pure state (Story 1.20): the failed-PIN throttle and the exit
//! timer. The service owns both (so restarting the app resets neither) and drives them with its
//! clock; nothing here sleeps or touches the platform.
//!
//! - **Throttle.** [`MAX_FAILURES`] wrong PINs in a row pause the prompt for [`PAUSE_MS`]; a
//!   success resets the count. While paused, no PIN is checked at all.
//! - **Timer.** An exit lasts [`ADMIN_EXIT_MS`] from its start. At most one runs; a new one
//!   (a remote unlock during an exit) replaces it, and the replaced one is reported re-locked.
//! - **Verifier.** A cached verifier is replaced only by a higher version.

pub use arena_proto::ADMIN_EXIT_MS;

/// Wrong PINs in a row before the prompt pauses.
pub const MAX_FAILURES: u32 = 5;
/// How long the prompt pauses, in ms.
pub const PAUSE_MS: i64 = 60_000;

/// What a PIN attempt may do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Check the PIN.
    Open,
    /// Paused: this many whole seconds are left (at least 1).
    Paused { seconds: u32 },
}

/// The failed-PIN throttle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Throttle {
    failures: u32,
    paused_until: Option<i64>,
}

fn seconds_left(until: i64, now: i64) -> u32 {
    let ms = (until - now).max(1);
    u32::try_from((ms + 999) / 1000).unwrap_or(u32::MAX)
}

impl Throttle {
    /// Whether a PIN may be checked at `now_ms`.
    pub fn gate(&self, now_ms: i64) -> Gate {
        match self.paused_until {
            Some(until) if until > now_ms => Gate::Paused {
                seconds: seconds_left(until, now_ms),
            },
            _ => Gate::Open,
        }
    }

    /// Wrong PINs in the current streak.
    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// Records a wrong PIN at `now_ms`; the [`MAX_FAILURES`]th pauses the prompt and answers
    /// its gate.
    pub fn fail(&mut self, now_ms: i64) -> Gate {
        if self.paused_until.is_some_and(|u| u <= now_ms) {
            self.paused_until = None;
        }
        self.failures += 1;
        if self.failures >= MAX_FAILURES {
            self.failures = 0;
            self.paused_until = Some(now_ms + PAUSE_MS);
        }
        self.gate(now_ms)
    }

    /// The Hub paused the PIN checks (`AUTH_LOCKED`): pause here too, for [`PAUSE_MS`].
    pub fn pause(&mut self, now_ms: i64) -> Gate {
        self.failures = 0;
        self.paused_until = Some(now_ms + PAUSE_MS);
        self.gate(now_ms)
    }

    /// A PIN was accepted: the streak ends.
    pub fn succeed(&mut self) {
        *self = Self::default();
    }
}

/// A running exit.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ActiveExit {
    pub exit_id: String,
    /// When it started, UTC ms.
    pub started_at: i64,
    /// When it re-locks, UTC ms.
    pub until: i64,
}

impl ActiveExit {
    /// An exit starting at `now_ms`.
    pub fn start(exit_id: impl Into<String>, now_ms: i64) -> Self {
        Self {
            exit_id: exit_id.into(),
            started_at: now_ms,
            until: now_ms.saturating_add(ADMIN_EXIT_MS),
        }
    }

    /// Whether it is over at `now_ms`.
    pub fn is_due(&self, now_ms: i64) -> bool {
        now_ms >= self.until
    }
}

/// What a service (re)start does with an exit it finds recorded: every recorded exit re-locks
/// at once (enforcement never stays suspended across a restart).
pub fn on_restart(recorded: Option<ActiveExit>) -> Option<ActiveExit> {
    recorded
}

/// Whether a pushed verifier of version `incoming` replaces the cached `current`.
pub fn replaces(current: Option<i64>, incoming: i64) -> bool {
    current.is_none_or(|c| incoming > c)
}

/// `"14:59"`: whole minutes and seconds left of an exit ending at `until`, at `now_ms`
/// (rounded up, never negative).
pub fn countdown(until: i64, now_ms: i64) -> String {
    let secs = ((until - now_ms).max(0) + 999) / 1000;
    format!("{}:{:02}", secs / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_wrong_pins_pause_for_a_minute_and_a_success_resets() {
        let mut t = Throttle::default();
        for i in 1..5 {
            assert_eq!(t.fail(0), Gate::Open);
            assert_eq!(t.failures(), i);
        }
        assert_eq!(t.fail(0), Gate::Paused { seconds: 60 });
        assert_eq!(t.gate(30_500), Gate::Paused { seconds: 30 });
        assert_eq!(t.gate(60_000), Gate::Open);
        // A fresh streak after the pause.
        assert_eq!(t.fail(60_000), Gate::Open);
        t.succeed();
        assert_eq!(t.failures(), 0);
        for _ in 0..4 {
            t.fail(70_000);
        }
        t.succeed();
        assert_eq!(t.fail(70_000), Gate::Open);
    }

    #[test]
    fn an_exit_lasts_fifteen_minutes() {
        let e = ActiveExit::start("E", 1_000);
        assert_eq!(e.until, 1_000 + 15 * 60 * 1000);
        assert!(!e.is_due(e.until - 1));
        assert!(e.is_due(e.until));
        assert_eq!(countdown(e.until, e.started_at + 1_000), "14:59");
        assert_eq!(countdown(e.until, e.until + 5), "0:00");
        assert_eq!(on_restart(Some(e.clone())), Some(e));
    }

    #[test]
    fn only_a_higher_verifier_replaces_the_cached_one() {
        assert!(replaces(None, 1));
        assert!(replaces(Some(2), 3));
        assert!(!replaces(Some(3), 3));
        assert!(!replaces(Some(3), 2));
    }
}
