use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::AppError;

pub const SETUP_COMMAND_KINDS: [&str; 7] = [
    "group.upsert",
    "device.assign",
    "device.register",
    "staff.upsert",
    "settings.set",
    "setup.test_session",
    "branch.go_live",
];

pub const SETTINGS_COMMAND_KEYS: [&str; 3] = ["branch.name", "branch.tz", "owner_alert.thresholds"];

pub const COMMAND_SCHEMA_VERSION: u32 = 1;
pub const COMMAND_PAYLOAD_MAX: usize = 16 * 1024;
/// A `product.upsert` may carry a product image (up to 200 KB as a base64 data URL, Story 8.24).
pub const PRODUCT_COMMAND_PAYLOAD_MAX: usize = 288 * 1024;

/// The largest payload the cloud queues for an owner command of `kind`.
pub fn command_payload_max(kind: &str) -> usize {
    if kind == "product.upsert" {
        PRODUCT_COMMAND_PAYLOAD_MAX
    } else {
        COMMAND_PAYLOAD_MAX
    }
}
pub const COMMANDS_PAGE: usize = 50;
/// A `fetched` command with no result after this long goes back to `pending` (Story 8.20).
pub const COMMAND_FETCH_TIMEOUT_MS: i64 = 10 * 60 * 1000;
/// A `pending` command older than this becomes `expired` (Story 8.20).
pub const COMMAND_EXPIRY_MS: i64 = 7 * 24 * 60 * 60 * 1000;

pub fn is_setup_command(kind: &str) -> bool {
    SETUP_COMMAND_KINDS.contains(&kind)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum CommandStatus {
    Pending,
    Fetched,
    Applied,
    Rejected,
    /// Waited 7 days unfetched; swept with `COMMAND_EXPIRED` (Story 8.20).
    Expired,
}

impl CommandStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Fetched => "fetched",
            Self::Applied => "applied",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            Self::Pending,
            Self::Fetched,
            Self::Applied,
            Self::Rejected,
            Self::Expired,
        ]
        .into_iter()
        .find(|k| k.as_str() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CommandWire {
    #[ts(type = "number")]
    pub id: i64,
    pub idempotency_key: String,
    pub kind: String,
    pub schema_version: u32,
    #[ts(type = "unknown")]
    pub payload: serde_json::Value,
    pub issued_by: String,
    /// The owner id when the cloud posted this command after a step-up (Story 8.25); the Hub records
    /// it as an MFA elevation on the change row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub elevated_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CommandsPage {
    pub commands: Vec<CommandWire>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CommandResult {
    pub epoch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub applied_seq: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<AppError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CommandResultAck {
    pub status: CommandStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_setup_kinds_are_commands() {
        assert!(SETUP_COMMAND_KINDS.iter().all(|k| is_setup_command(k)));
        assert!(!is_setup_command("branch.bootstrap"));
        assert!(!is_setup_command("pricing.set"));
    }

    #[test]
    fn statuses_round_trip() {
        for s in [
            CommandStatus::Pending,
            CommandStatus::Fetched,
            CommandStatus::Applied,
            CommandStatus::Rejected,
            CommandStatus::Expired,
        ] {
            assert_eq!(CommandStatus::parse(s.as_str()), Some(s));
        }
        assert_eq!(CommandStatus::parse("done"), None);
    }
}
