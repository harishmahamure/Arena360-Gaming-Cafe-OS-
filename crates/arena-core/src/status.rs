//! The Hub's shared runtime status, published through a `tokio::sync::watch` channel.

use std::fmt;

use arena_proto::Connectivity;
use arena_store::LeaseRow;
use tokio::sync::watch;

/// Whether this Hub is the branch's writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Primary,
    Follower,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Follower => "follower",
        }
    }
}

/// A writer epoch, ordered lexicographically by `(counter, hub_instance_id)`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Epoch {
    pub counter: i64,
    pub hub_instance_id: String,
}

impl fmt::Display for Epoch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.counter, self.hub_instance_id)
    }
}

/// The lease as last seen by this Hub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseInfo {
    pub counter: i64,
    pub hub_instance_id: String,
    pub provisional: bool,
    pub expires_at: i64,
}

impl LeaseInfo {
    pub fn epoch(&self) -> Epoch {
        Epoch {
            counter: self.counter,
            hub_instance_id: self.hub_instance_id.clone(),
        }
    }
}

impl From<&LeaseRow> for LeaseInfo {
    fn from(row: &LeaseRow) -> Self {
        Self {
            counter: row.counter,
            hub_instance_id: row.hub_instance_id.clone(),
            provisional: row.provisional,
            expires_at: row.expires_at,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PairingState {
    #[default]
    Active,
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloudTime {
    pub cloud_ms: i64,
    pub at: std::time::Instant,
}

/// What `/healthz` reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubStatus {
    /// `true` once the database is open. It turns `false` only on shutdown or when the writer
    /// thread dies; a demoted Hub keeps it `true`, because it can still read the database.
    pub db_open: bool,
    /// This process's own `hub_instance_id`.
    pub self_instance_id: String,
    pub role: Role,
    pub lease: Option<LeaseInfo>,
    pub schema_version: u32,
    /// The SHA-256 fingerprint (lower-case hex) of the LAN TLS certificate, once loaded.
    pub tls_fingerprint: Option<String>,
    /// The WAN connectivity probe's last answer (`online`, `checked_at: null` until the first
    /// probe finishes, so a Hub never shows the offline chip before it knows).
    pub connectivity: Connectivity,
    pub setup: Option<arena_proto::SetupStatus>,
    pub lan_addresses: Vec<std::net::Ipv4Addr>,
    /// The network the Hub is on ("Wi-Fi", "Ethernet"), when the host can name it (Android).
    pub lan_network: Option<String>,
    pub lan_ports: Option<(u16, u16)>,
    pub tier: Option<arena_proto::Tier>,
    pub pairing: PairingState,
    /// The branch name while this Hub replaces another and waits to be restored (Story 9.9): it
    /// writes nothing and serves no device until a restore completes.
    pub restore_pending: Option<String>,
    /// The owner released the branch key to this waiting Hub and it holds the key (Story 9.10):
    /// "Restore from a file" is offered. Never set while the Hub is not waiting.
    pub restore_key_released: bool,
    pub cloud_time: Option<CloudTime>,
    pub live: bool,
    pub licence: Option<arena_proto::LicenceStanding>,
    pub clock_wrong: bool,
    pub platform: arena_proto::HubPlatform,
    /// The change-log upload (Story 8.2), as the upload task last saw it.
    pub upload: UploadStatus,
    /// The Cloudflare Tunnel connector (Story 8.39), as the tunnel task last saw it.
    pub tunnel: arena_proto::TunnelLine,
}

/// How far the change-log upload has got. A Hub that does not upload (Free, follower) keeps the
/// default.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UploadStatus {
    /// When the cloud last accepted a batch (Hub clock, ms); `None` before the first.
    pub last_upload_at: Option<i64>,
    /// Change rows not yet in an accepted batch.
    pub pending_rows: i64,
    /// The last `seq` of the lease's epoch the cloud has accepted (the fork parent's, or 0,
    /// while the cursor is in an older epoch) (Story 8.3).
    pub last_seq_uploaded: i64,
    /// The `seq` the lease's epoch was forked from, if it was.
    pub fork_seq: Option<i64>,
    /// The last `seq` of the lease's epoch the upgrade snapshot holds in place of batches
    /// (Story 9.11), if it ended in that epoch.
    pub base_seq: Option<i64>,
    /// When the branch summary was last built and published (Hub clock, ms; `rollup_log`
    /// `latest`) (Story 8.4).
    pub last_summary_at: Option<i64>,
    /// The latest business day (`yyyy-mm-dd`) whose rollup is in `rollup_log` (Story 8.6).
    pub last_rollup_date: Option<String>,
    /// When the last snapshot was stored (Hub clock, ms; `snapshot_log`) (Story 9.17).
    pub last_snapshot_at: Option<i64>,
    /// When the last upload cycle failed, while `backup_state` still holds the failure (Hub
    /// clock, ms) (Story 9.17).
    pub failed_at: Option<i64>,
}

impl HubStatus {
    pub fn trusted_now(&self, wall_ms: i64) -> i64 {
        self.cloud_time.map_or(wall_ms, |c| {
            c.cloud_ms.saturating_add(c.at.elapsed().as_millis() as i64)
        })
    }

    /// The status right after the database opened, before the lease is decided.
    pub fn opened(schema_version: u32, self_instance_id: impl Into<String>) -> Self {
        Self {
            db_open: true,
            self_instance_id: self_instance_id.into(),
            role: Role::Follower,
            lease: None,
            schema_version,
            tls_fingerprint: None,
            connectivity: Connectivity::default(),
            setup: None,
            lan_addresses: Vec::new(),
            lan_network: None,
            lan_ports: None,
            tier: None,
            pairing: PairingState::Active,
            restore_pending: None,
            restore_key_released: false,
            cloud_time: None,
            live: false,
            licence: None,
            clock_wrong: false,
            platform: arena_proto::HubPlatform::Windows,
            upload: UploadStatus::default(),
            tunnel: arena_proto::TunnelLine::default(),
        }
    }
}

/// Creates the shared status channel.
pub fn status_channel(
    initial: HubStatus,
) -> (watch::Sender<HubStatus>, watch::Receiver<HubStatus>) {
    watch::channel(initial)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(counter: i64, id: &str) -> Epoch {
        Epoch {
            counter,
            hub_instance_id: id.into(),
        }
    }

    #[test]
    fn epochs_order_by_counter_then_instance_id() {
        assert!(e(2, "A") > e(1, "Z"));
        assert!(e(1, "B") > e(1, "A"));
        assert_eq!(e(3, "X"), e(3, "X"));
        assert_eq!(e(5, "01J").to_string(), "5:01J");
    }
}
