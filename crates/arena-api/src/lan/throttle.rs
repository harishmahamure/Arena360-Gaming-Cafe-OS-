//! Per-peer-IP throttle on failed hellos: after [`MAX_FAILURES`] credential failures within
//! [`WINDOW`], every hello from that IP is refused with `TOKEN_INVALID` for [`BLOCK`], without
//! touching argon2.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::Duration;

use tokio::time::Instant;

/// Failures that trigger a block.
pub const MAX_FAILURES: usize = 5;
/// The window the failures are counted in.
pub const WINDOW: Duration = Duration::from_secs(60);
/// How long a blocked IP stays blocked.
pub const BLOCK: Duration = Duration::from_secs(60);
/// Peers tracked before expired entries are swept.
const SWEEP_AT: usize = 4096;

#[derive(Default)]
struct Peer {
    failures: VecDeque<Instant>,
    blocked_until: Option<Instant>,
}

impl Peer {
    fn expired(&self, now: Instant) -> bool {
        self.blocked_until.is_none_or(|t| t <= now)
            && self
                .failures
                .back()
                .is_none_or(|t| now.duration_since(*t) >= WINDOW)
    }
}

/// Failed-hello bookkeeping, shared by every listener.
#[derive(Default)]
pub(crate) struct HelloThrottle {
    peers: Mutex<HashMap<IpAddr, Peer>>,
}

impl HelloThrottle {
    /// Whether `ip` is blocked at `now`.
    pub(crate) fn is_blocked(&self, ip: IpAddr, now: Instant) -> bool {
        self.peers
            .lock()
            .ok()
            .and_then(|m| m.get(&ip).and_then(|p| p.blocked_until))
            .is_some_and(|t| t > now)
    }

    /// Records a credential failure from `ip`; the fifth within the window blocks it.
    pub(crate) fn record_failure(&self, ip: IpAddr, now: Instant) {
        let Ok(mut map) = self.peers.lock() else {
            return;
        };
        if map.len() >= SWEEP_AT {
            map.retain(|_, p| !p.expired(now));
        }
        let peer = map.entry(ip).or_default();
        while peer
            .failures
            .front()
            .is_some_and(|t| now.duration_since(*t) >= WINDOW)
        {
            peer.failures.pop_front();
        }
        peer.failures.push_back(now);
        if peer.failures.len() >= MAX_FAILURES {
            peer.failures.clear();
            peer.blocked_until = Some(now + BLOCK);
            tracing::warn!(peer = %ip, "too many failed hellos; blocking this address for 60 s");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn five_failures_in_a_minute_block_for_a_minute() {
        let t = HelloThrottle::default();
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7));
        let other = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 8));
        let t0 = Instant::now();
        for i in 0..4 {
            t.record_failure(ip, t0 + Duration::from_secs(i));
        }
        assert!(!t.is_blocked(ip, t0 + Duration::from_secs(4)));
        t.record_failure(ip, t0 + Duration::from_secs(5));
        assert!(t.is_blocked(ip, t0 + Duration::from_secs(6)));
        assert!(!t.is_blocked(other, t0 + Duration::from_secs(6)));
        assert!(t.is_blocked(ip, t0 + Duration::from_secs(64)));
        assert!(!t.is_blocked(ip, t0 + Duration::from_secs(65)));
    }

    #[test]
    fn failures_older_than_the_window_do_not_count() {
        let t = HelloThrottle::default();
        let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let t0 = Instant::now();
        for i in 0..4 {
            t.record_failure(ip, t0 + Duration::from_secs(i));
        }
        t.record_failure(ip, t0 + Duration::from_secs(61));
        assert!(!t.is_blocked(ip, t0 + Duration::from_secs(62)));
    }
}

/// `kiosk.log` intents a device may burst.
pub const KIOSK_LOG_BURST: f64 = 60.0;
/// `kiosk.log` intents a device earns back per second (60 a minute).
pub const KIOSK_LOG_PER_SEC: f64 = 1.0;

/// A per-device token bucket for `kiosk.log` (shared by all of a device's sessions).
#[derive(Default)]
pub(crate) struct KioskLogRate {
    buckets: Mutex<HashMap<String, (f64, Instant)>>,
}

impl KioskLogRate {
    /// Takes one token for `device_id`; `false` when the device is over its rate.
    pub(crate) fn take(&self, device_id: &str) -> bool {
        self.take_at(device_id, Instant::now())
    }

    fn take_at(&self, device_id: &str, now: Instant) -> bool {
        let Ok(mut map) = self.buckets.lock() else {
            return false;
        };
        if map.len() >= SWEEP_AT {
            map.retain(|_, (_, at)| now.duration_since(*at) < Duration::from_secs(60));
        }
        let (tokens, at) = map
            .entry(device_id.to_owned())
            .or_insert((KIOSK_LOG_BURST, now));
        *tokens = (*tokens + now.duration_since(*at).as_secs_f64() * KIOSK_LOG_PER_SEC)
            .min(KIOSK_LOG_BURST);
        *at = now;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod kiosk_log_rate_tests {
    use super::*;

    #[test]
    fn sixty_a_minute_per_device() {
        let r = KioskLogRate::default();
        let t0 = Instant::now();
        for _ in 0..60 {
            assert!(r.take_at("D", t0));
        }
        assert!(
            !r.take_at("D", t0),
            "the 61st in the same instant is refused"
        );
        assert!(r.take_at("E", t0), "another device has its own bucket");
        assert!(r.take_at("D", t0 + Duration::from_secs(1)));
        assert!(!r.take_at("D", t0 + Duration::from_secs(1)));
    }
}

/// Consecutive wrong admin-exit PINs from one device before it is paused.
pub const ADMIN_PIN_MAX_FAILURES: u32 = 5;
/// How long a device is paused after [`ADMIN_PIN_MAX_FAILURES`].
pub const ADMIN_PIN_PAUSE: Duration = Duration::from_secs(60);

/// One device's admin-PIN state: (wrong PINs in a row, checks in flight, paused until).
type PinStreak = (u32, u32, Option<Instant>);

/// Per-device admin-exit PIN throttle (`kiosk.verify_admin_pin`): five wrong PINs in a row pause
/// that device's checks for a minute (`AUTH_LOCKED`), without touching argon2. Each check is
/// reserved before it runs and counts toward the five while in flight, so parallel attempts
/// cannot get past the limit. The kiosk keeps its own count too; this one holds even against a
/// kiosk that does not.
#[derive(Default)]
pub(crate) struct AdminPinThrottle {
    devices: Mutex<HashMap<String, PinStreak>>,
}

impl AdminPinThrottle {
    /// Reserves one check for `device_id` at `now`. `Err` with the seconds to wait when the device
    /// is paused, or when the wrong PINs so far plus the checks in flight already reach the limit.
    pub(crate) fn begin(&self, device_id: &str, now: Instant) -> Result<(), u32> {
        let Ok(mut map) = self.devices.lock() else {
            return Err(ADMIN_PIN_PAUSE.as_secs() as u32);
        };
        if map.len() >= SWEEP_AT {
            map.retain(|_, (n, f, until)| *n > 0 || *f > 0 || until.is_some_and(|u| u > now));
        }
        let entry = map.entry(device_id.to_owned()).or_insert((0, 0, None));
        if let Some(until) = entry.2 {
            if until > now {
                return Err(until.duration_since(now).as_secs_f64().ceil().max(1.0) as u32);
            }
            entry.2 = None;
        }
        if entry.0 + entry.1 >= ADMIN_PIN_MAX_FAILURES {
            return Err(ADMIN_PIN_PAUSE.as_secs() as u32);
        }
        entry.1 += 1;
        Ok(())
    }

    /// Gives back a reserved check that could not run (it counts neither way).
    pub(crate) fn cancel(&self, device_id: &str) {
        if let Ok(mut map) = self.devices.lock()
            && let Some(entry) = map.get_mut(device_id)
        {
            entry.1 = entry.1.saturating_sub(1);
        }
    }

    /// Ends a reserved check. A failure that completes the streak pauses the device and answers
    /// the pause in seconds.
    pub(crate) fn finish(&self, device_id: &str, ok: bool, now: Instant) -> Option<u32> {
        let Ok(mut map) = self.devices.lock() else {
            return None;
        };
        let entry = map.entry(device_id.to_owned()).or_insert((0, 0, None));
        entry.1 = entry.1.saturating_sub(1);
        if ok {
            entry.0 = 0;
            return None;
        }
        entry.0 += 1;
        if entry.0 >= ADMIN_PIN_MAX_FAILURES {
            entry.0 = 0;
            entry.2 = Some(now + ADMIN_PIN_PAUSE);
            return Some(ADMIN_PIN_PAUSE.as_secs() as u32);
        }
        None
    }
}

#[cfg(test)]
mod admin_pin_tests {
    use super::*;

    #[test]
    fn five_wrong_pins_in_a_row_pause_the_device_for_a_minute() {
        let t = AdminPinThrottle::default();
        let t0 = Instant::now();
        for _ in 0..4 {
            t.begin("D", t0).unwrap();
            assert_eq!(t.finish("D", false, t0), None);
        }
        t.begin("D", t0).unwrap();
        assert_eq!(t.finish("D", false, t0), Some(60));
        assert_eq!(t.begin("D", t0), Err(60));
        assert!(t.begin("E", t0).is_ok());
        assert!(t.begin("D", t0 + Duration::from_secs(60)).is_ok());
        // A success resets the streak.
        let t = AdminPinThrottle::default();
        for _ in 0..4 {
            t.begin("E", t0).unwrap();
            t.finish("E", false, t0);
        }
        t.begin("E", t0).unwrap();
        t.finish("E", true, t0);
        t.begin("E", t0).unwrap();
        assert_eq!(t.finish("E", false, t0), None);
    }

    #[test]
    fn checks_in_flight_count_toward_the_limit() {
        let t = AdminPinThrottle::default();
        let t0 = Instant::now();
        for _ in 0..5 {
            t.begin("D", t0).unwrap();
        }
        assert!(
            t.begin("D", t0).is_err(),
            "a sixth parallel check is refused"
        );
        t.finish("D", true, t0);
        assert!(t.begin("D", t0).is_ok());
    }
}
