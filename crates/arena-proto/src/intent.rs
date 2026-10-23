//! The intent envelope: the only way a change is requested. Intents are named `domain.verb`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::actor::{Actor, ElevatedBy};

/// What an intent asks for, as `domain.verb`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum IntentKind {
    /// Creates the branch row and its default settings, once.
    #[serde(rename = "branch.bootstrap")]
    BranchBootstrap,
}

impl IntentKind {
    /// The wire and stored form, e.g. `branch.bootstrap`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BranchBootstrap => "branch.bootstrap",
        }
    }
}

/// The payload of `branch.bootstrap`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BranchBootstrap {
    pub name: String,
    pub branch_code: String,
    /// ISO 4217 alpha-3 code, e.g. `INR`.
    pub currency: String,
    /// IANA time zone, e.g. `Asia/Kolkata`.
    pub tz: String,
    /// Minute of the local day at which the business day starts (0–1439).
    pub day_start_min: u16,
    /// Month in which the financial year starts (1–12).
    pub fy_start_month: u8,
}

/// The intent's payload, tagged by its kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum IntentPayload {
    #[serde(rename = "branch.bootstrap")]
    BranchBootstrap(BranchBootstrap),
}

impl IntentPayload {
    /// The kind this payload belongs to.
    pub fn kind(&self) -> IntentKind {
        match self {
            Self::BranchBootstrap(_) => IntentKind::BranchBootstrap,
        }
    }
}

/// A request to change branch state. Every write goes through `apply()` as one of these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct Intent {
    /// A ULID minted by the client.
    pub intent_id: String,
    pub kind: IntentKind,
    /// A client ULID; dedupe comes in Story 1.8.
    pub idempotency_key: String,
    /// When the client issued the intent, UTC ms.
    #[ts(type = "number")]
    pub issued_at: i64,
    /// The `rev` the edit was based on (edit intents only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub base_rev: Option<i64>,
    pub actor: Actor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub elevated_by: Option<ElevatedBy>,
    pub payload: IntentPayload,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::ActorKind;

    fn bootstrap() -> Intent {
        Intent {
            intent_id: "01JAAAAAAAAAAAAAAAAAAAAAAA".into(),
            kind: IntentKind::BranchBootstrap,
            idempotency_key: "01JBBBBBBBBBBBBBBBBBBBBBBB".into(),
            issued_at: 1_800_000_000_000,
            base_rev: None,
            actor: Actor {
                kind: ActorKind::System,
                id: "dev-seed".into(),
            },
            elevated_by: None,
            payload: IntentPayload::BranchBootstrap(BranchBootstrap {
                name: "Dev Branch".into(),
                branch_code: "DEV01".into(),
                currency: "INR".into(),
                tz: "Asia/Kolkata".into(),
                day_start_min: 360,
                fy_start_month: 4,
            }),
        }
    }

    #[test]
    fn kind_serializes_as_domain_verb() {
        assert_eq!(
            serde_json::to_string(&IntentKind::BranchBootstrap).unwrap(),
            "\"branch.bootstrap\""
        );
        assert_eq!(IntentKind::BranchBootstrap.as_str(), "branch.bootstrap");
    }

    #[test]
    fn intent_round_trips_with_tagged_payload() {
        let intent = bootstrap();
        let json = serde_json::to_value(&intent).unwrap();
        assert_eq!(json["kind"], "branch.bootstrap");
        assert_eq!(json["payload"]["kind"], "branch.bootstrap");
        assert_eq!(json["payload"]["currency"], "INR");
        assert!(json.get("base_rev").is_none());
        assert!(json.get("elevated_by").is_none());
        let back: Intent = serde_json::from_value(json).unwrap();
        assert_eq!(back, intent);
        assert_eq!(back.payload.kind(), IntentKind::BranchBootstrap);
    }
}
