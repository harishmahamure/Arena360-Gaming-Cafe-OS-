//! Who is acting: the typed actor carried by every intent and recorded on every change.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The kind of principal behind an intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum ActorKind {
    Staff,
    Owner,
    Admin,
    System,
    Device,
}

impl ActorKind {
    /// The stored and wire form, e.g. `system`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Staff => "staff",
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::System => "system",
            Self::Device => "device",
        }
    }
}

/// A typed actor. Nothing is written anonymously or through a shared actor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct Actor {
    pub kind: ActorKind,
    pub id: String,
}

/// Where a manager PIN elevation was verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum ElevationMode {
    /// Verified live at the Hub.
    Hub,
    /// Verified offline against a cached, versioned verifier (kiosk admin exit).
    CachedVerifier,
}

impl ElevationMode {
    /// The stored and wire form, e.g. `cached_verifier`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hub => "hub",
            Self::CachedVerifier => "cached_verifier",
        }
    }
}

/// The actor whose PIN elevated an intent (Story 1.13 fills it in).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ElevatedBy {
    pub kind: ActorKind,
    pub id: String,
    pub mode: ElevationMode,
    pub verifier_version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_serialize_snake_case() {
        for (kind, text) in [
            (ActorKind::Staff, "staff"),
            (ActorKind::Owner, "owner"),
            (ActorKind::Admin, "admin"),
            (ActorKind::System, "system"),
            (ActorKind::Device, "device"),
        ] {
            assert_eq!(serde_json::to_string(&kind).unwrap(), format!("\"{text}\""));
            assert_eq!(kind.as_str(), text);
        }
        assert_eq!(
            serde_json::to_string(&ElevationMode::CachedVerifier).unwrap(),
            "\"cached_verifier\""
        );
        assert_eq!(ElevationMode::Hub.as_str(), "hub");
    }
}
