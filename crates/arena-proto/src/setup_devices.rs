use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::AppError;
use crate::kiosk_pairing::DeviceKind;
use crate::snapshot::DeviceStatus;

pub const SETUP_ITEMS_MAX: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupDevice {
    pub id: String,
    pub label: String,
    pub kind: DeviceKind,
    pub group_id: Option<String>,
    pub status: DeviceStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupGroup {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub colour: String,
    #[ts(type = "number")]
    pub sort: i64,
    #[serde(default)]
    pub has_rate: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BranchSetup {
    pub devices: Vec<SetupDevice>,
    pub groups: Vec<SetupGroup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub status: Option<crate::SetupStatus>,
}

impl BranchSetup {
    pub fn valid(&self) -> bool {
        self.devices.len() <= SETUP_ITEMS_MAX && self.groups.len() <= SETUP_ITEMS_MAX
    }

    pub fn grouped(&self) -> bool {
        self.devices.iter().any(|d| d.group_id.is_some())
    }

    pub fn rated(&self) -> bool {
        !self.groups.is_empty() && self.groups.iter().all(|g| g.has_rate)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupCommandView {
    pub key: String,
    pub kind: String,
    #[ts(type = "unknown")]
    pub payload: serde_json::Value,
    pub status: crate::commands::CommandStatus,
    pub error: Option<AppError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupDevicesView {
    pub setup: Option<BranchSetup>,
    #[ts(type = "number | null")]
    pub reported_at: Option<i64>,
    #[ts(type = "number")]
    pub now: i64,
    pub hub_online: bool,
    pub commands: Vec<SetupCommandView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupCommandRequest {
    pub kind: String,
    #[ts(type = "unknown")]
    pub payload: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub branch_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouped_needs_a_device_in_a_group() {
        let mut s = BranchSetup::default();
        assert!(s.valid() && !s.grouped());
        s.devices.push(SetupDevice {
            id: "D".into(),
            label: "PC-1".into(),
            kind: DeviceKind::Pc,
            group_id: None,
            status: DeviceStatus::Online,
        });
        assert!(!s.grouped());
        s.devices[0].group_id = Some("G".into());
        assert!(s.grouped());
    }
}
