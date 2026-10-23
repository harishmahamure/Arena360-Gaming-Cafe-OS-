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
