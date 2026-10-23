//! The intent envelope: the only way a change is requested. Intents are named `domain.verb`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::actor::{Actor, ElevatedBy};
use crate::permission::{Permission, Requirement};
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
    /// Creates or edits a staff member (hashes computed outside the transaction).
    #[serde(rename = "staff.upsert")]
    StaffUpsert,
    /// Creates or edits a custom role and its permission rows (built-in roles are fixed).
    #[serde(rename = "role.upsert")]
    RoleUpsert,
}

impl IntentKind {
    /// Every kind.
    pub const ALL: [IntentKind; 8] = [
        Self::BranchBootstrap,
        Self::SettingsSet,
        Self::DeviceRegister,
        Self::DeviceRevoke,
        Self::DeviceVersionReported,
        Self::ConsolePlanRequested,
        Self::StaffUpsert,
        Self::RoleUpsert,
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
            Self::StaffUpsert => "staff.upsert",
            Self::RoleUpsert => "role.upsert",
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
            Self::StaffUpsert | Self::RoleUpsert => IntentCategory::Staff,
        }
    }

    /// What this kind requires of its actor (checked first in `apply()`). A total match: a new
    /// kind does not compile until it is mapped.
    pub fn requirement(self) -> Requirement {
        match self {
            Self::BranchBootstrap => Requirement::System,
            Self::SettingsSet => Requirement::Perm(Permission::SettingsEdit),
            Self::DeviceRegister => Requirement::Perm(Permission::DeviceManage),
            Self::DeviceRevoke => Requirement::Perm(Permission::DeviceRemote),
            Self::DeviceVersionReported => Requirement::System,
            Self::StaffUpsert | Self::RoleUpsert => Requirement::Perm(Permission::UserManage),
            Self::ConsolePlanRequested => Requirement::Phone,
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

/// The payload of `staff.upsert`. Without `staff_id` it creates a staff member (the writer
/// mints the id); with one it edits that member. `password_hash` and `pin_hash` are argon2id PHC
/// strings computed outside the transaction; `None` keeps the stored hash. A new PIN bumps the
/// member's `pin_verifier_version`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StaffUpsert {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub staff_id: Option<String>,
    pub login: String,
    pub display_name: String,
    pub role_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub password_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pin_hash: Option<String>,
    pub active: bool,
}

/// Whether a role row grants or withholds a permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum PermEffect {
    Allow,
    Deny,
}

impl PermEffect {
    /// The stored form, `allow` or `deny`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }
}

/// One permission row of a role. `permission` is a plain string so an unknown one fails
/// validation on `perms[i]` instead of failing to decode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RolePerm {
    pub permission: String,
    pub effect: PermEffect,
}

/// The payload of `role.upsert`. Without `role_id` it creates a custom role (`builtin = 0`);
/// with one it renames that custom role and replaces its permission rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RoleUpsert {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub role_id: Option<String>,
    pub name: String,
    pub perms: Vec<RolePerm>,
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
    #[serde(rename = "staff.upsert")]
    StaffUpsert(StaffUpsert),
    #[serde(rename = "role.upsert")]
    RoleUpsert(RoleUpsert),
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
            Self::StaffUpsert(_) => IntentKind::StaffUpsert,
            Self::RoleUpsert(_) => IntentKind::RoleUpsert,
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
        assert!(!IntentKind::ConsolePlanRequested.is_handled());
    }

    /// Permissions no intent requires yet: views, ops and the modules later stories build.
    const USED_BY_LATER_STORIES: &[Permission] = &[
        Permission::PlayerManage,
        Permission::PlayerView,
        Permission::WalletTopup,
        Permission::CreditManage,
        Permission::CreditCollect,
        Permission::SessionStart,
        Permission::SessionPause,
        Permission::SessionExtend,
        Permission::SessionTransfer,
        Permission::SessionEnd,
        Permission::SessionForceEnd,
        Permission::BookingManage,
        Permission::PosSell,
        Permission::PosVoid,
        Permission::RefundIssue,
        Permission::DiscountManual,
        Permission::KitchenUse,
        Permission::StockIn,
        Permission::StockAdjust,
        Permission::StockOverride,
        Permission::CatalogEdit,
        Permission::ShiftOpen,
        Permission::ShiftClose,
        Permission::CashManage,
        Permission::DevicePair,
        Permission::BackupExport,
        Permission::OpsPromote,
        Permission::OpsRestore,
        Permission::ReportView,
        Permission::AuditView,
    ];

    #[test]
    fn every_kind_has_exactly_one_requirement() {
        let mut seen = std::collections::HashSet::new();
        for kind in IntentKind::ALL {
            assert!(seen.insert(kind), "{} listed twice in ALL", kind.as_str());
            // `requirement()` is a total match returning one value; pin the current mapping.
            let want = match kind.as_str() {
                "branch.bootstrap" | "device.version_reported" => Requirement::System,
                "settings.set" => Requirement::Perm(Permission::SettingsEdit),
                "device.register" => Requirement::Perm(Permission::DeviceManage),
                "device.revoke" => Requirement::Perm(Permission::DeviceRemote),
                "staff.upsert" | "role.upsert" => Requirement::Perm(Permission::UserManage),
                "console.plan_requested" => Requirement::Phone,
                other => panic!("{other} has no pinned requirement"),
            };
            assert_eq!(kind.requirement(), want, "{}", kind.as_str());
        }
    }

    #[test]
    fn every_permission_is_required_or_reserved_for_a_later_story() {
        let required: Vec<Permission> = IntentKind::ALL
            .into_iter()
            .filter_map(|k| match k.requirement() {
                Requirement::Perm(p) => Some(p),
                _ => None,
            })
            .collect();
        for p in Permission::ALL {
            let used = required.contains(&p);
            let reserved = USED_BY_LATER_STORIES.contains(&p);
            assert!(
                used || reserved,
                "{} is neither required nor reserved",
                p.as_str()
            );
            assert!(
                !(used && reserved),
                "{} is required, so drop it from the allow-list",
                p.as_str()
            );
        }
    }

    #[test]
    fn staff_and_role_payloads_round_trip() {
        let payload = IntentPayload::RoleUpsert(RoleUpsert {
            role_id: None,
            name: "Floor lead".into(),
            perms: vec![RolePerm {
                permission: "pos.sell".into(),
                effect: PermEffect::Deny,
            }],
        });
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["kind"], "role.upsert");
        assert_eq!(json["perms"][0]["effect"], "deny");
        assert!(json.get("role_id").is_none());
        assert_eq!(
            serde_json::from_value::<IntentPayload>(json).unwrap(),
            payload
        );
        let staff: IntentPayload = serde_json::from_value(serde_json::json!({
            "kind": "staff.upsert", "login": "counter", "display_name": "Counter",
            "role_id": "R", "active": true
        }))
        .unwrap();
        assert_eq!(staff.kind(), IntentKind::StaffUpsert);
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
