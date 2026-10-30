//! The Cloudflare Tunnel of a branch with the `tunnel` licence flag (Story 8.39).
//!
//! The cloud decides on every heartbeat from the signed licence flags and the paired Hub's
//! platform and answers with a [`TunnelAnswer`]; the Hub tells the cloud whether it holds the
//! tunnel credential in a [`TunnelReport`]. The credential travels once, in a `grant`, and is
//! never logged or printed.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The licence flag that entitles a branch to a tunnel (Premium, or the Tunnel add-on).
pub const TUNNEL_FLAG: &str = "tunnel";

/// The domain the branches' tunnel hostnames live under.
pub const TUNNEL_DOMAIN: &str = "tunnel.arena360.cloud";

/// Where the connector forwards: the Hub's tunnel listener (arrives with Stories 8.40 and 8.41).
/// Every other request through the tunnel is answered 404.
pub const TUNNEL_ORIGIN: &str = "http://127.0.0.1:7403";

/// The hostname of a branch's tunnel.
pub fn tunnel_hostname(branch_id: &str) -> String {
    format!("{branch_id}.{TUNNEL_DOMAIN}")
}

/// What the cloud tells the Hub to do about its tunnel, in every heartbeat answer of a Free or
/// paid branch.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TunnelAnswer {
    /// The tunnel exists and the Hub holds no credential for it (or never had one): store this
    /// one and run the connector. Sent when the tunnel is created and when the Hub reports it
    /// holds none.
    Grant {
        hostname: String,
        /// The connector token (a secret: never logged).
        credential: String,
    },
    /// The tunnel exists and the Hub holds its credential: keep the connector running.
    On { hostname: String },
    /// No tunnel for this branch: stop the connector and delete the credential.
    Off,
    /// The paired Hub cannot run a tunnel (Android).
    UnsupportedPlatform,
}

impl std::fmt::Debug for TunnelAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Grant { hostname, .. } => f
                .debug_struct("Grant")
                .field("hostname", hostname)
                .field("credential", &"[redacted]")
                .finish(),
            Self::On { hostname } => f.debug_struct("On").field("hostname", hostname).finish(),
            Self::Off => f.write_str("Off"),
            Self::UnsupportedPlatform => f.write_str("UnsupportedPlatform"),
        }
    }
}

/// What the Hub tells the cloud about its tunnel, in the heartbeat request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct TunnelReport {
    /// The Hub holds a tunnel credential (it is `false` on a replaced PC or after a lost file).
    pub has_credential: bool,
}

/// Where the Hub's connector stands, for the System screen (`GET /ui/tunnel`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
#[serde(rename_all = "snake_case")]
pub enum TunnelState {
    /// No tunnel for this branch.
    #[default]
    Off,
    /// The connector was started and has not settled yet.
    Starting,
    /// The connector is running.
    On,
    /// The connector exited or could not start; it is restarted after a backoff.
    Reconnecting,
    /// The paired Hub cannot run a tunnel: the screen reads "Needs the Windows Hub".
    UnsupportedPlatform,
}

impl TunnelState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Starting => "starting",
            Self::On => "on",
            Self::Reconnecting => "reconnecting",
            Self::UnsupportedPlatform => "unsupported_platform",
        }
    }
}

/// The System screen's tunnel line: the state and the hostname when there is one.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct TunnelLine {
    pub state: TunnelState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hostname: Option<String>,
}

impl TunnelLine {
    pub fn new(state: TunnelState, hostname: Option<String>) -> Self {
        Self { state, hostname }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_hostname_is_the_branch_under_the_tunnel_domain() {
        assert_eq!(
            tunnel_hostname("01JBRANCH"),
            "01JBRANCH.tunnel.arena360.cloud"
        );
        assert_eq!(TUNNEL_ORIGIN, "http://127.0.0.1:7403");
    }

    #[test]
    fn answers_are_tagged_by_state() {
        let grant = TunnelAnswer::Grant {
            hostname: "b.tunnel.arena360.cloud".into(),
            credential: "secret-token".into(),
        };
        assert_eq!(
            serde_json::to_value(&grant).unwrap(),
            json!({"state": "grant", "hostname": "b.tunnel.arena360.cloud", "credential": "secret-token"})
        );
        for (answer, wire) in [
            (
                TunnelAnswer::On {
                    hostname: "h".into(),
                },
                json!({"state": "on", "hostname": "h"}),
            ),
            (TunnelAnswer::Off, json!({"state": "off"})),
            (
                TunnelAnswer::UnsupportedPlatform,
                json!({"state": "unsupported_platform"}),
            ),
        ] {
            assert_eq!(serde_json::to_value(&answer).unwrap(), wire);
            assert_eq!(
                serde_json::from_value::<TunnelAnswer>(wire).unwrap(),
                answer
            );
        }
    }

    #[test]
    fn the_credential_never_shows_in_debug() {
        let grant = TunnelAnswer::Grant {
            hostname: "h".into(),
            credential: "secret-token".into(),
        };
        let shown = format!("{grant:?}");
        assert!(!shown.contains("secret-token"), "{shown}");
        assert!(shown.contains("[redacted]"));
    }

    #[test]
    fn the_line_omits_a_missing_hostname() {
        assert_eq!(
            serde_json::to_value(TunnelLine::default()).unwrap(),
            json!({"state": "off"})
        );
        assert_eq!(
            serde_json::to_value(TunnelLine::new(TunnelState::On, Some("h".into()))).unwrap(),
            json!({"state": "on", "hostname": "h"})
        );
    }
}
