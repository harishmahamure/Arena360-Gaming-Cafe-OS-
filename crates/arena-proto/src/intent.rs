//! The intent envelope: the only way a change is requested. Intents are named `domain.verb`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::actor::{Actor, ElevatedBy};
use crate::permission::Permission;
use crate::scope::ClientClass;

/// The domain an intent kind belongs to. Class scopes are checked against it: no browser class
/// may reach a `staff`, `money` or `device` intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum IntentCategory {
    Branch,
    Settings,
    Device,
    Staff,
    Money,
    Kitchen,
    Console,
}

/// What an intent asks for, as `domain.verb`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum IntentKind {
    /// Creates the branch row and its default settings, once.
    #[serde(rename = "branch.bootstrap")]
    BranchBootstrap,
    /// Sets the value of one existing branch setting (an edit: carries `base_rev`).
    #[serde(rename = "settings.set")]
    SettingsSet,
    /// Registers a device and the argon2id hash of its token (dev/system only until pairing).
    #[serde(rename = "device.register")]
    DeviceRegister,
    /// Revokes a device: its sessions are closed and its token is refused from then on.
    #[serde(rename = "device.revoke")]
    DeviceRevoke,
    /// Records the app version a device reported in its `hello` (system actor, on change only).
    #[serde(rename = "device.version_reported")]
    DeviceVersionReported,
    /// A phone asks a console for a play plan. In the phone scope, but no handler yet.
    #[serde(rename = "console.plan_requested")]
    ConsolePlanRequested,
}

impl IntentKind {
    /// Every kind.
    pub const ALL: [IntentKind; 6] = [
        Self::BranchBootstrap,
        Self::SettingsSet,
        Self::DeviceRegister,
        Self::DeviceRevoke,
        Self::DeviceVersionReported,
        Self::ConsolePlanRequested,
    ];

    /// The wire and stored form, e.g. `branch.bootstrap`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BranchBootstrap => "branch.bootstrap",
            Self::SettingsSet => "settings.set",
            Self::DeviceRegister => "device.register",
            Self::DeviceRevoke => "device.revoke",
            Self::DeviceVersionReported => "device.version_reported",
            Self::ConsolePlanRequested => "console.plan_requested",
        }
    }

    /// The kind with this wire form, if any.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// The domain this kind belongs to.
    pub fn category(self) -> IntentCategory {
        match self {
            Self::BranchBootstrap => IntentCategory::Branch,
            Self::SettingsSet => IntentCategory::Settings,
            Self::DeviceRegister | Self::DeviceRevoke | Self::DeviceVersionReported => {
                IntentCategory::Device
            }
            Self::ConsolePlanRequested => IntentCategory::Console,
        }
    }

    /// The catalogue permission this kind requires, where the catalogue has one yet (Story
    /// 1.11 maps the rest).
    pub fn permission(self) -> Option<Permission> {
        match self {
            Self::DeviceRevoke => Some(Permission::DeviceRemote),
            Self::DeviceRegister => Some(Permission::DevicePair),
            _ => None,
        }
    }

    /// Whether the Hub has a handler (a payload variant) for this kind yet.
    pub fn is_handled(self) -> bool {
        !matches!(self, Self::ConsolePlanRequested)
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

/// The payload of `settings.set`: the setting `key` (which must already exist) and its new
/// TEXT `value`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SettingsSet {
    pub key: String,
    pub value: String,
}

/// The payload of `device.register`. `token_hash` is the argon2id PHC string of the token's
/// secret, computed outside the transaction; the writer mints the `device_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DeviceRegister {
    /// `paired` or `kds` (a phone is never registered).
    pub class: ClientClass,
    pub label: String,
    pub token_hash: String,
}

/// The payload of `device.revoke`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DeviceRevoke {
    pub device_id: String,
}

/// The payload of `device.version_reported`: the `x.y.z` app version from the device's hello.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DeviceVersionReported {
    pub device_id: String,
    pub version: String,
}

/// The intent's payload, tagged by its kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum IntentPayload {
    #[serde(rename = "branch.bootstrap")]
    BranchBootstrap(BranchBootstrap),
    #[serde(rename = "settings.set")]
    SettingsSet(SettingsSet),
    #[serde(rename = "device.register")]
    DeviceRegister(DeviceRegister),
    #[serde(rename = "device.revoke")]
    DeviceRevoke(DeviceRevoke),
    #[serde(rename = "device.version_reported")]
    DeviceVersionReported(DeviceVersionReported),
}

impl IntentPayload {
    /// The kind this payload belongs to.
    pub fn kind(&self) -> IntentKind {
        match self {
            Self::BranchBootstrap(_) => IntentKind::BranchBootstrap,
            Self::SettingsSet(_) => IntentKind::SettingsSet,
            Self::DeviceRegister(_) => IntentKind::DeviceRegister,
            Self::DeviceRevoke(_) => IntentKind::DeviceRevoke,
            Self::DeviceVersionReported(_) => IntentKind::DeviceVersionReported,
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
    /// A client ULID (26 Crockford base32 characters). A missing or malformed key fails
    /// `BAD_FRAME` before any transaction; a key already applied returns its stored outcome.
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
    fn every_kind_round_trips_its_wire_form() {
        for kind in IntentKind::ALL {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{}\"", kind.as_str()));
            assert_eq!(serde_json::from_str::<IntentKind>(&json).unwrap(), kind);
            assert_eq!(IntentKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(IntentKind::parse("nope.verb"), None);
        assert_eq!(
            IntentKind::DeviceRevoke.permission(),
            Some(Permission::DeviceRemote)
        );
        assert!(!IntentKind::ConsolePlanRequested.is_handled());
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
    fn settings_set_round_trips() {
        let intent = Intent {
            kind: IntentKind::SettingsSet,
            base_rev: Some(3),
            payload: IntentPayload::SettingsSet(SettingsSet {
                key: "idle_lock_minutes".into(),
                value: "7".into(),
            }),
            ..bootstrap()
        };
        let json = serde_json::to_value(&intent).unwrap();
        assert_eq!(json["kind"], "settings.set");
        assert_eq!(json["payload"]["kind"], "settings.set");
        assert_eq!(json["payload"]["key"], "idle_lock_minutes");
        assert_eq!(json["base_rev"], 3);
        let back: Intent = serde_json::from_value(json).unwrap();
        assert_eq!(back, intent);
        assert_eq!(back.payload.kind(), IntentKind::SettingsSet);
        assert_eq!(IntentKind::SettingsSet.as_str(), "settings.set");
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
