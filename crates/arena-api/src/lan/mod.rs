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
mod pair;
pub(crate) mod registry;
mod session;
mod throttle;
pub mod tls;

use std::sync::Arc;

use arena_core::{Clock, DeviceDirectory, HubStatus, SnapshotReader, StaffDirectory, WriterHandle};
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

/// Which devices have a live `paired` session with this Hub (the floor's Online / Offline).
#[derive(Clone, Default)]
pub struct Presence(Option<Arc<registry::Registry>>);

impl Presence {
    /// A handle that reports every device offline (no LAN server, tests).
    pub fn none() -> Self {
        Self(None)
    }

    #[cfg(test)]
    pub(crate) fn of(registry: Arc<registry::Registry>) -> Self {
        Self(Some(registry))
    }

    /// Whether `device_id` has a live paired session right now.
    pub fn online(&self, device_id: &str) -> bool {
        self.0.as_ref().is_some_and(|r| r.is_online(device_id))
    }

    /// The device's shown presence: online while connected and for 15 s after, and when it was
    /// last seen (epoch ms).
    pub fn status(&self, device_id: &str) -> (bool, Option<i64>) {
        self.0
            .as_ref()
            .map_or((false, None), |r| r.status(device_id))
    }

    /// A receiver that changes whenever a paired device session opens or closes (`None` for
    /// [`Presence::none`]).
    pub fn changes(&self) -> Option<tokio::sync::watch::Receiver<u64>> {
        self.0.as_ref().map(|r| r.presence_changes())
    }

    /// The remote unlocks kiosks acknowledge from now on (`None` for [`Presence::none`]).
    pub fn admin_unlock_acks(
        &self,
    ) -> Option<tokio::sync::broadcast::Receiver<arena_proto::AdminUnlockAckEvent>> {
        self.0.as_ref().map(|r| r.admin_unlock_acks())
    }

    pub fn pc_acks(&self) -> Option<tokio::sync::broadcast::Receiver<arena_proto::PcAckEvent>> {
        self.0.as_ref().map(|r| r.pc_acks())
    }

    pub fn push_pc_command(
        &self,
        device_id: &str,
        cmd_id: &str,
        action: arena_proto::PcAction,
        text: Option<String>,
        reason: Option<arena_proto::LockReason>,
    ) -> usize {
        self.0.as_ref().map_or(0, |r| {
            r.push_pc_command(device_id, cmd_id, action, text, reason)
        })
    }

    pub fn push_session_started(
        &self,
        device_id: &str,
        cmd_id: &str,
        session: arena_proto::SessionScreen,
    ) -> usize {
        self.0
            .as_ref()
            .map_or(0, |r| r.push_session_started(device_id, cmd_id, session))
    }
}

pub struct HubInfoSource {
    pub reader: Arc<SnapshotReader>,
    pub hub_name: String,
    pub lan_port: u16,
    pub web_port: u16,
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
    /// Checks admin-exit PINs (`kiosk.verify_admin_pin`); without it they are `UNSUPPORTED`.
    pub staff: Option<Arc<StaffDirectory>>,
    pub info: Option<Arc<HubInfoSource>>,
    pub pairing: Option<Arc<crate::pairing::Pairing>>,
    registry: Arc<registry::Registry>,
    /// Bounds concurrent argon2id token checks across all connections.
    verify_permits: Arc<tokio::sync::Semaphore>,
    throttle: throttle::HelloThrottle,
    kiosk_log_rate: throttle::KioskLogRate,
    admin_pin: throttle::AdminPinThrottle,
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
            staff: None,
            info: None,
            pairing: None,
            registry: Arc::new(registry::Registry::default()),
            verify_permits: Arc::new(tokio::sync::Semaphore::new(VERIFY_CONCURRENCY)),
            throttle: throttle::HelloThrottle::default(),
            kiosk_log_rate: throttle::KioskLogRate::default(),
            admin_pin: throttle::AdminPinThrottle::default(),
        }
    }

    /// Checks kiosk admin-exit PINs against `staff` (Story 1.20).
    pub fn with_staff(mut self, staff: Arc<StaffDirectory>) -> Self {
        self.staff = Some(staff);
        self
    }

    pub fn with_pairing(mut self, pairing: Arc<crate::pairing::Pairing>) -> Self {
        self.pairing = Some(pairing);
        self
    }

    pub fn with_info(mut self, info: HubInfoSource) -> Self {
        self.info = Some(Arc::new(info));
        self
    }

    pub async fn hub_info(&self) -> Option<arena_proto::HubInfo> {
        let source = Arc::clone(self.info.as_ref()?);
        let fingerprint = {
            let status = self.status.borrow();
            if status.role != arena_core::Role::Primary {
                return None;
            }
            status.tls_fingerprint.clone()?
        };
        let reader = Arc::clone(&source.reader);
        let branch = match tokio::task::spawn_blocking(move || reader.branch()).await {
            Ok(Ok(b)) => b?,
            Ok(Err(e)) => {
                tracing::warn!(code = e.code.as_str(), "hub-info: the branch read failed");
                return None;
            }
            Err(e) => {
                tracing::warn!(error = %e, "hub-info: the branch read task failed");
                return None;
            }
        };
        Some(arena_proto::HubInfo::new(
            branch.name,
            source.hub_name.clone(),
            fingerprint,
            source.lan_port,
            source.web_port,
        ))
    }

    /// How many sessions are connected.
    pub fn session_count(&self) -> usize {
        self.registry.len()
    }

    /// The handle the Hub UI's snapshot reads device presence through.
    pub fn presence(&self) -> Presence {
        Presence(Some(Arc::clone(&self.registry)))
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
