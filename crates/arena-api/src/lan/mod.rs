//! The LAN protocol server.
//!
//! - `:7400`: TLS WebSocket at `/` for paired apps.
//! - `:7401`: the first byte is sniffed. `0x16` (a TLS ClientHello) is served exactly like
//!   `:7400`; anything else is plain HTTP: `GET /ws` upgrades to a WebSocket for the browser
//!   classes (`kds`, `phone`), `/kitchen` and `/console/<tv>` return a placeholder page, and
//!   every other path is 404.
//!
//! Every session starts with `hello` (see [`session`]); class scopes come from `arena-proto`.
//! Only a primary Hub serves intents; a follower answers `hello` and refuses every intent with
//! `NOT_PRIMARY`. Committed intents are pushed to every session ([`registry`]).

pub mod listener;
pub(crate) mod registry;
mod session;
mod throttle;
pub mod tls;

use std::sync::Arc;

use arena_core::{Clock, DeviceDirectory, HubStatus, WriterHandle};
use tokio::sync::watch;

pub use listener::MAX_CONNECTIONS;
pub use listener::{DEFAULT_LAN_PORT, DEFAULT_WEB_PORT, LanListeners, bind_lan, serve_lan};
pub use session::{
    HELLO_TIMEOUT, IDLE_TIMEOUT, LAN_SYSTEM_ACTOR, MAX_FRAME_BYTES, PING_INTERVAL, SESSION_QUEUE,
    VERIFY_CONCURRENCY,
};
pub use tls::HubCert;

/// A `major.minor` release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Release {
    pub major: u32,
    pub minor: u32,
}

impl Release {
    /// Parses the `major.minor` of an `x.y.z` version (`None` when it is not one).
    pub fn parse(version: &str) -> Option<Self> {
        let mut parts = version.split('.');
        let num = |p: Option<&str>| -> Option<u32> {
            let p = p?;
            if p.is_empty() || p.len() > 9 || !p.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            p.parse().ok()
        };
        let major = num(parts.next())?;
        let minor = num(parts.next())?;
        num(parts.next())?;
        parts.next().is_none().then_some(Self { major, minor })
    }

    /// The oldest release this Hub accepts from a paired client: the same major and at most two
    /// minors behind.
    pub fn minimum_client(self) -> Self {
        Self {
            major: self.major,
            minor: self.minor.saturating_sub(2),
        }
    }

    /// Whether a paired client at `client` may connect to this Hub.
    pub fn accepts(self, client: Self) -> bool {
        client.major == self.major && client.minor >= self.minimum_client().minor
    }
}

impl std::fmt::Display for Release {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// Everything a LAN session needs from the Hub.
pub struct LanHub {
    /// The writer, when this Hub started as primary.
    pub writer: Option<WriterHandle>,
    pub status: watch::Receiver<HubStatus>,
    pub clock: Arc<dyn Clock>,
    /// Reads `devices` for token checks (its own read-only connection).
    pub devices: Arc<DeviceDirectory>,
    pub tls: Arc<rustls::ServerConfig>,
    /// This Hub's version, `x.y.z`.
    pub version: String,
    registry: Arc<registry::Registry>,
    /// Bounds concurrent argon2id token checks across all connections.
    verify_permits: Arc<tokio::sync::Semaphore>,
    throttle: throttle::HelloThrottle,
}

impl LanHub {
    pub fn new(
        writer: Option<WriterHandle>,
        status: watch::Receiver<HubStatus>,
        clock: Arc<dyn Clock>,
        devices: Arc<DeviceDirectory>,
        tls: Arc<rustls::ServerConfig>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            writer,
            status,
            clock,
            devices,
            tls,
            version: version.into(),
            registry: Arc::new(registry::Registry::default()),
            verify_permits: Arc::new(tokio::sync::Semaphore::new(VERIFY_CONCURRENCY)),
            throttle: throttle::HelloThrottle::default(),
        }
    }

    /// How many sessions are connected.
    pub fn session_count(&self) -> usize {
        self.registry.len()
    }

    /// Whether intents can be served now: a writer exists and the Hub is primary.
    fn is_primary(&self) -> bool {
        self.writer.as_ref().is_some_and(|w| !w.is_closed())
            && self.status.borrow().role == arena_core::Role::Primary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(major: u32, minor: u32) -> Release {
        Release { major, minor }
    }

    #[test]
    fn version_window_is_same_major_and_two_minors_back() {
        let hub = Release::parse("0.5.3").unwrap();
        assert_eq!(hub, r(0, 5));
        assert_eq!(hub.minimum_client().to_string(), "0.3");
        assert!(!hub.accepts(r(0, 2)));
        assert!(hub.accepts(r(0, 3)));
        assert!(hub.accepts(r(0, 5)));
        assert!(hub.accepts(r(0, 9)));
        assert!(!hub.accepts(r(1, 5)));
        assert!(r(0, 1).accepts(r(0, 0)));
        for bad in ["", "1", "1.2", "1.2.3.4", "a.b.c", "1.2.x", "-1.2.3"] {
            assert_eq!(Release::parse(bad), None, "{bad}");
        }
    }
}
