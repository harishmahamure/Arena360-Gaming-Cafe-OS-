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

    /// Every kind.
    pub const ALL: [ActorKind; 5] = [
        Self::Staff,
        Self::Owner,
        Self::Admin,
        Self::System,
        Self::Device,
    ];

    /// The kind with this stored form, if any.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
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

    /// The mode with this stored form, if any.
    pub fn parse(s: &str) -> Option<Self> {
        [Self::Hub, Self::CachedVerifier]
            .into_iter()
            .find(|m| m.as_str() == s)
    }
}

/// How an elevation was proven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum ElevationKind {
    /// A staff member's PIN.
    Pin,
    /// An owner's MFA approval (a later epic).
    Mfa,
}

impl ElevationKind {
    /// The stored and wire form, `pin` or `mfa`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pin => "pin",
            Self::Mfa => "mfa",
        }
    }

    /// The kind with this stored form, if any.
    pub fn parse(s: &str) -> Option<Self> {
        [Self::Pin, Self::Mfa].into_iter().find(|k| k.as_str() == s)
    }
}

/// Who approved an intent the actor could not do alone. Only the Hub sets it (from a redeemed
/// elevation ticket); a client-supplied value is stripped at every edge. `id` is the approving
/// staff member and `verifier_version` the `pin_verifier_version` their PIN was checked
/// against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ElevatedBy {
    pub kind: ElevationKind,
    pub id: String,
    pub mode: ElevationMode,
    #[ts(type = "number")]
    pub verifier_version: i64,
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
            assert_eq!(ActorKind::parse(text), Some(kind));
        }
        assert_eq!(ActorKind::parse("nobody"), None);
        assert_eq!(
            ElevationMode::parse("cached_verifier"),
            Some(ElevationMode::CachedVerifier)
        );
        assert_eq!(ElevationKind::parse("mfa"), Some(ElevationKind::Mfa));
        assert_eq!(
            serde_json::to_string(&ElevationMode::CachedVerifier).unwrap(),
            "\"cached_verifier\""
        );
        assert_eq!(ElevationMode::Hub.as_str(), "hub");
        for (kind, text) in [(ElevationKind::Pin, "pin"), (ElevationKind::Mfa, "mfa")] {
            assert_eq!(serde_json::to_string(&kind).unwrap(), format!("\"{text}\""));
            assert_eq!(kind.as_str(), text);
        }
    }
}
