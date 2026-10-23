//! The Hub's shared runtime status, published through a `tokio::sync::watch` channel.

use std::fmt;

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
}

impl HubStatus {
    /// The status right after the database opened, before the lease is decided.
    pub fn opened(schema_version: u32, self_instance_id: impl Into<String>) -> Self {
        Self {
            db_open: true,
            self_instance_id: self_instance_id.into(),
            role: Role::Follower,
            lease: None,
            schema_version,
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
