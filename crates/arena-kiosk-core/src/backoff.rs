//! The reconnect policy: full-jitter exponential back-off between [`MIN_DELAY`] and
//! [`MAX_DELAY`], reset after a successful `hello`.
//!
//! The `n`-th consecutive failure (from 0) waits a uniformly random delay in
//! `[MIN_DELAY, min(MAX_DELAY, MIN_DELAY * 2^n)]`. The floor keeps a fleet of kiosks from
//! hammering a Hub that just restarted; the jitter spreads their reconnects out. The random
//! source is a seeded SplitMix64, so tests are deterministic.

use std::time::Duration;

/// The shortest wait before a reconnect.
pub const MIN_DELAY: Duration = Duration::from_secs(2);
/// The longest wait before a reconnect.
pub const MAX_DELAY: Duration = Duration::from_secs(30);

/// Consecutive-failure counter plus its jitter source.
#[derive(Debug, Clone)]
pub struct Backoff {
    failures: u32,
    rng: u64,
    min: Duration,
    max: Duration,
}

impl Backoff {
    /// A back-off at the default bounds, jittered from `seed`.
    pub fn new(seed: u64) -> Self {
        Self::with_bounds(seed, MIN_DELAY, MAX_DELAY)
    }

    /// A back-off with other bounds (tests use short ones). `max` below `min` is raised to it.
    pub fn with_bounds(seed: u64, min: Duration, max: Duration) -> Self {
        Self {
            failures: 0,
            rng: seed,
            min,
            max: max.max(min),
        }
    }

    /// How many failures in a row have been counted since the last reset.
    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// Counts a failure and returns how long to wait before the next attempt.
    pub fn next_delay(&mut self) -> Duration {
        let shift = self.failures.min(16);
        self.failures = self.failures.saturating_add(1);
        let ceiling = self
            .min
            .checked_mul(1u32 << shift)
            .unwrap_or(self.max)
            .min(self.max);
        let span = ceiling.saturating_sub(self.min).as_millis() as u64;
        let jitter = if span == 0 {
            0
        } else {
            self.next_u64() % (span + 1)
        };
        self.min + Duration::from_millis(jitter)
    }

    /// A successful connect: the next failure starts again from the floor.
    pub fn reset(&mut self) {
        self.failures = 0;
    }

    fn next_u64(&mut self) -> u64 {
        // SplitMix64.
        self.rng = self.rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_stay_within_the_bounds_and_grow() {
        for seed in 0..200 {
            let mut b = Backoff::new(seed);
            for n in 0..40u32 {
                let d = b.next_delay();
                assert!(d >= MIN_DELAY && d <= MAX_DELAY, "seed {seed} n {n}: {d:?}");
                let ceiling = MIN_DELAY
                    .checked_mul(1 << n.min(16))
                    .unwrap_or(MAX_DELAY)
                    .min(MAX_DELAY);
                assert!(d <= ceiling, "seed {seed} n {n}: {d:?} > {ceiling:?}");
            }
        }
        // The first failure waits exactly the floor.
        assert_eq!(Backoff::new(7).next_delay(), MIN_DELAY);
    }

    #[test]
    fn the_delays_are_jittered() {
        let mut seen = std::collections::HashSet::new();
        for seed in 0..50 {
            let mut b = Backoff::new(seed);
            for _ in 0..6 {
                b.next_delay();
            }
            seen.insert(b.next_delay());
        }
        assert!(seen.len() > 10, "{seen:?}");
    }

    #[test]
    fn a_success_resets_to_the_floor() {
        let mut b = Backoff::new(3);
        for _ in 0..10 {
            b.next_delay();
        }
        assert_eq!(b.failures(), 10);
        b.reset();
        assert_eq!(b.failures(), 0);
        assert_eq!(b.next_delay(), MIN_DELAY);
        let d = b.next_delay();
        assert!(d >= MIN_DELAY && d <= MIN_DELAY * 2, "{d:?}");
    }

    #[test]
    fn custom_bounds_are_respected() {
        let min = Duration::from_millis(10);
        let max = Duration::from_millis(40);
        let mut b = Backoff::with_bounds(1, min, max);
        for _ in 0..20 {
            let d = b.next_delay();
            assert!(d >= min && d <= max, "{d:?}");
        }
        let mut b = Backoff::with_bounds(1, max, min);
        assert_eq!(b.next_delay(), max);
    }
}
