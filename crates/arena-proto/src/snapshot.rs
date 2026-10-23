//! The Hub UI's branch snapshot, read over the loopback `GET /ui/snapshot`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::scope::ClientClass;

/// What the Hub UI needs to draw its shell and the floor placeholder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct HubSnapshot {
    /// The branch, or `null` before the branch is set up.
    pub branch: Option<SnapshotBranch>,
    /// Every device that is not revoked, ordered by id.
    pub devices: Vec<SnapshotDevice>,
}

/// The branch fields the shell shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SnapshotBranch {
    pub name: String,
    pub branch_code: String,
}

/// One device on the floor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SnapshotDevice {
    pub device_id: String,
    pub label: String,
    pub class: ClientClass,
    /// The floor zone; always `null` until zones exist, so every device is unassigned.
    pub zone: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_with_null_branch_and_zone() {
        let s = HubSnapshot {
            branch: None,
            devices: vec![SnapshotDevice {
                device_id: "D".into(),
                label: "Dev Kiosk".into(),
                class: ClientClass::Paired,
                zone: None,
            }],
        };
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            serde_json::json!({
                "branch": null,
                "devices": [{"device_id": "D", "label": "Dev Kiosk", "class": "paired", "zone": null}]
            })
        );
    }
}
