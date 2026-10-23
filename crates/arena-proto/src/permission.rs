//! The permission catalogue. Story 1.11 seeds the rest of it; until then every permission is a
//! "system actor allowed" stub inside `apply()`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A catalogue permission. Every intent maps to exactly one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum Permission {
    /// Act on a paired device remotely (revoke it, unlock it).
    #[serde(rename = "device.remote")]
    DeviceRemote,
    /// Pair a new device with the branch.
    #[serde(rename = "device.pair")]
    DevicePair,
}

impl Permission {
    /// The wire and stored form, e.g. `device.remote`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DeviceRemote => "device.remote",
            Self::DevicePair => "device.pair",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissions_serialize_as_domain_verb() {
        for p in [Permission::DeviceRemote, Permission::DevicePair] {
            assert_eq!(
                serde_json::to_string(&p).unwrap(),
                format!("\"{}\"", p.as_str())
            );
        }
    }
}
