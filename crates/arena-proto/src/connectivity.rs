//! The branch's WAN connectivity (Story 1.22), as the Hub's probe last saw it.
//!
//! The probe tells "the internet is unreachable" apart from "the internet works but the cloud
//! does not answer". It is the only outbound traffic a branch makes; nothing else waits on it.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Which link is down, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ConnectivityState {
    /// The internet and the cloud both answered (also the state before the first probe).
    #[default]
    Online,
    /// No internet endpoint answered.
    InternetOffline,
    /// The internet answered but the cloud's `GET /healthz` did not return 200.
    CloudUnreachable,
}

impl ConnectivityState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::InternetOffline => "internet_offline",
            Self::CloudUnreachable => "cloud_unreachable",
        }
    }
}

/// The probe's last answer: `/healthz`, the Hub UI snapshot and `event: connectivity.changed`
/// on `GET /ui/events`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct Connectivity {
    pub state: ConnectivityState,
    /// When the probe finished (UTC ms), or `null` before the first probe.
    #[ts(type = "number | null")]
    pub checked_at: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_snake_case_with_a_nullable_checked_at() {
        assert_eq!(
            serde_json::to_value(Connectivity::default()).unwrap(),
            serde_json::json!({"state": "online", "checked_at": null})
        );
        let c = Connectivity {
            state: ConnectivityState::CloudUnreachable,
            checked_at: Some(42),
        };
        assert_eq!(
            serde_json::to_value(c).unwrap(),
            serde_json::json!({"state": "cloud_unreachable", "checked_at": 42})
        );
        for s in [
            ConnectivityState::Online,
            ConnectivityState::InternetOffline,
            ConnectivityState::CloudUnreachable,
        ] {
            assert_eq!(serde_json::to_value(s).unwrap(), s.as_str());
        }
    }
}
