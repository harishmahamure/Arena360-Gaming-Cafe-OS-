//! The intent envelope: the only way a change is requested. Intents are named `domain.verb`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::actor::{Actor, ElevatedBy};
use crate::admin::{DeviceAdminExit, DeviceAdminRelock, DeviceAdminUnlock};
use crate::kiosk::KioskLog;
use crate::operating::AdsConsentSet;
use crate::permission::{Permission, Requirement};
use crate::printer::PrinterStatusSet;
use crate::process::KioskProcessBlocked;
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
    Audit,
    Kiosk,
    Catalog,
}

/// What an intent asks for, as `domain.verb`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum IntentKind {
    /// Creates the branch row and its default settings, once.
    #[serde(rename = "branch.bootstrap")]
    BranchBootstrap,
    /// Inserts whatever default rows an upgraded branch lacks (settings, built-in roles and
    /// their permission rows). System only; the Hub runs it when it becomes primary.
    #[serde(rename = "branch.seed_defaults")]
    BranchSeedDefaults,
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
    /// Records that a manager's PIN granted a read elevation (system only; the Hub submits it
    /// when it issues the read ticket), so the grant shows in the audit log.
    #[serde(rename = "audit.read_granted")]
    AuditReadGranted,
    /// One event a kiosk recorded (device actor), queued offline and replayed in order.
    #[serde(rename = "kiosk.log")]
    KioskLog,
    /// The kiosk killed a disallowed process in the player session (device actor), queued
    /// offline and replayed in order like `kiosk.log`.
    #[serde(rename = "kiosk.process_blocked")]
    KioskProcessBlocked,
    /// A kiosk granted an admin exit after a manager's PIN (device actor, elevated by the
    /// manager: checked by the Hub or, offline, against the cached verifier).
    #[serde(rename = "device.admin_exit")]
    DeviceAdminExit,
    /// A kiosk re-locked after an admin exit (device actor).
    #[serde(rename = "device.admin_relock")]
    DeviceAdminRelock,
    /// A manager unlocks a kiosk remotely from the Hub floor.
    #[serde(rename = "device.admin_unlock")]
    DeviceAdminUnlock,
    #[serde(rename = "ads.consent")]
    AdsConsent,
    #[serde(rename = "printer.status")]
    PrinterStatus,
    #[serde(rename = "group.upsert")]
    GroupUpsert,
    #[serde(rename = "group.delete")]
    GroupDelete,
    #[serde(rename = "device.assign")]
    DeviceAssign,
    #[serde(rename = "licence.apply")]
    LicenceApply,
    #[serde(rename = "zone.upsert")]
    ZoneUpsert,
    #[serde(rename = "zone.delete")]
    ZoneDelete,
    #[serde(rename = "device.place")]
    DevicePlace,
    #[serde(rename = "product.upsert")]
    ProductUpsert,
    #[serde(rename = "product.delete")]
    ProductDelete,
    #[serde(rename = "staff.set_credentials")]
    StaffSetCredentials,
    #[serde(rename = "stock.in")]
    StockIn,
    #[serde(rename = "stock.in.reverted")]
    StockInReverted,
    #[serde(rename = "device.maintenance")]
    DeviceMaintenance,
}

impl IntentKind {
    /// Every kind.
    pub const ALL: [IntentKind; 30] = [
        Self::BranchBootstrap,
        Self::BranchSeedDefaults,
        Self::SettingsSet,
        Self::DeviceRegister,
        Self::DeviceRevoke,
        Self::DeviceVersionReported,
        Self::ConsolePlanRequested,
        Self::StaffUpsert,
        Self::RoleUpsert,
        Self::AuditReadGranted,
        Self::KioskLog,
        Self::KioskProcessBlocked,
        Self::DeviceAdminExit,
        Self::DeviceAdminRelock,
        Self::DeviceAdminUnlock,
        Self::AdsConsent,
        Self::PrinterStatus,
        Self::GroupUpsert,
        Self::GroupDelete,
        Self::DeviceAssign,
        Self::LicenceApply,
        Self::ZoneUpsert,
        Self::ZoneDelete,
        Self::DevicePlace,
        Self::ProductUpsert,
        Self::ProductDelete,
        Self::StaffSetCredentials,
        Self::StockIn,
        Self::StockInReverted,
        Self::DeviceMaintenance,
    ];

    /// The wire and stored form, e.g. `branch.bootstrap`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BranchBootstrap => "branch.bootstrap",
            Self::BranchSeedDefaults => "branch.seed_defaults",
            Self::SettingsSet => "settings.set",
            Self::DeviceRegister => "device.register",
            Self::DeviceRevoke => "device.revoke",
            Self::DeviceVersionReported => "device.version_reported",
            Self::ConsolePlanRequested => "console.plan_requested",
            Self::StaffUpsert => "staff.upsert",
            Self::RoleUpsert => "role.upsert",
            Self::AuditReadGranted => "audit.read_granted",
            Self::KioskLog => "kiosk.log",
            Self::KioskProcessBlocked => "kiosk.process_blocked",
            Self::DeviceAdminExit => "device.admin_exit",
            Self::DeviceAdminRelock => "device.admin_relock",
            Self::DeviceAdminUnlock => "device.admin_unlock",
            Self::AdsConsent => "ads.consent",
            Self::PrinterStatus => "printer.status",
            Self::GroupUpsert => "group.upsert",
            Self::GroupDelete => "group.delete",
            Self::DeviceAssign => "device.assign",
            Self::LicenceApply => "licence.apply",
            Self::ZoneUpsert => "zone.upsert",
            Self::ZoneDelete => "zone.delete",
            Self::DevicePlace => "device.place",
            Self::ProductUpsert => "product.upsert",
            Self::ProductDelete => "product.delete",
            Self::StaffSetCredentials => "staff.set_credentials",
            Self::StockIn => "stock.in",
            Self::StockInReverted => "stock.in.reverted",
            Self::DeviceMaintenance => "device.maintenance",
        }
    }

    /// The kind with this wire form, if any.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// The domain this kind belongs to.
    pub fn category(self) -> IntentCategory {
        match self {
            Self::BranchBootstrap | Self::BranchSeedDefaults => IntentCategory::Branch,
            Self::SettingsSet | Self::AdsConsent | Self::PrinterStatus | Self::LicenceApply => {
                IntentCategory::Settings
            }
            Self::DeviceRegister
            | Self::DeviceRevoke
            | Self::DeviceVersionReported
            | Self::DeviceAdminExit
            | Self::DeviceAdminRelock
            | Self::DeviceAdminUnlock
            | Self::GroupUpsert
            | Self::GroupDelete
            | Self::DeviceAssign
            | Self::ZoneUpsert
            | Self::ZoneDelete
            | Self::DevicePlace
            | Self::DeviceMaintenance => IntentCategory::Device,
            Self::ConsolePlanRequested => IntentCategory::Console,
            Self::StaffUpsert | Self::RoleUpsert | Self::StaffSetCredentials => {
                IntentCategory::Staff
            }
            Self::AuditReadGranted => IntentCategory::Audit,
            Self::KioskLog | Self::KioskProcessBlocked => IntentCategory::Kiosk,
            Self::ProductUpsert | Self::ProductDelete | Self::StockIn | Self::StockInReverted => {
                IntentCategory::Catalog
            }
        }
    }

    /// What this kind requires of its actor (checked first in `apply()`). A total match: a new
    /// kind does not compile until it is mapped.
    pub fn requirement(self) -> Requirement {
        match self {
            Self::BranchBootstrap | Self::BranchSeedDefaults => Requirement::System,
            Self::SettingsSet | Self::AdsConsent | Self::PrinterStatus => {
                Requirement::Perm(Permission::SettingsEdit)
            }
            Self::DeviceRegister
            | Self::GroupUpsert
            | Self::GroupDelete
            | Self::DeviceAssign
            | Self::ZoneUpsert
            | Self::ZoneDelete
            | Self::DevicePlace
            | Self::DeviceMaintenance => Requirement::Perm(Permission::DeviceManage),
            Self::DeviceRevoke => Requirement::Perm(Permission::DeviceRemote),
            Self::DeviceVersionReported | Self::LicenceApply | Self::StaffSetCredentials => {
                Requirement::System
            }
            Self::StaffUpsert | Self::RoleUpsert => Requirement::Perm(Permission::UserManage),
            Self::ConsolePlanRequested => Requirement::Phone,
            Self::AuditReadGranted => Requirement::System,
            Self::KioskLog | Self::KioskProcessBlocked => Requirement::Device,
            // The kiosk itself acts; the approving manager must hold `device.remote`
            // ([`elevator_permission`](Self::elevator_permission)).
            Self::DeviceAdminExit | Self::DeviceAdminRelock => Requirement::Device,
            Self::DeviceAdminUnlock => Requirement::Perm(Permission::DeviceRemote),
            Self::ProductUpsert | Self::ProductDelete => Requirement::Perm(Permission::CatalogEdit),
            Self::StockIn | Self::StockInReverted => Requirement::Perm(Permission::StockIn),
        }
    }

    /// For a kind a device does on a manager's say-so: the permission the elevating staff
    /// member must hold (the actor's own requirement is still [`requirement`](Self::requirement)).
    /// Only `device.admin_exit` has one, and it is the only kind that accepts a
    /// `cached_verifier` elevation.
    pub fn elevator_permission(self) -> Option<Permission> {
        match self {
            Self::DeviceAdminExit => Some(Permission::DeviceRemote),
            _ => None,
        }
    }

    /// The reasons this kind accepts, when it requires one (`None`: it takes no reason). A
    /// total match, like [`requirement`](Self::requirement).
    pub fn reasons(self) -> Option<&'static [ReasonCode]> {
        match self {
            Self::DeviceRevoke => Some(&[
                ReasonCode::Lost,
                ReasonCode::Stolen,
                ReasonCode::Replaced,
                ReasonCode::Other,
            ]),
            Self::BranchBootstrap
            | Self::BranchSeedDefaults
            | Self::SettingsSet
            | Self::DeviceRegister
            | Self::DeviceVersionReported
            | Self::ConsolePlanRequested
            | Self::StaffUpsert
            | Self::RoleUpsert
            | Self::AuditReadGranted
            | Self::KioskLog
            | Self::KioskProcessBlocked
            | Self::DeviceAdminExit
            | Self::DeviceAdminRelock
            | Self::DeviceAdminUnlock
            | Self::AdsConsent
            | Self::PrinterStatus
            | Self::GroupUpsert
            | Self::GroupDelete
            | Self::DeviceAssign
            | Self::LicenceApply
            | Self::ZoneUpsert
            | Self::ZoneDelete
            | Self::DevicePlace
            | Self::ProductUpsert
            | Self::ProductDelete
            | Self::StaffSetCredentials
            | Self::StockIn
            | Self::StockInReverted => None,
            | Self::DeviceMaintenance => None,
        }
    }

    /// Whether the Hub has a handler (a payload variant) for this kind yet.
    pub fn is_handled(self) -> bool {
        !matches!(self, Self::ConsolePlanRequested)
    }
}

/// Why an intent was done, for kinds that require one ([`IntentKind::reasons`]). The UI shows
/// each code through the catalogue key `reason.<code>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum ReasonCode {
    Lost,
    Stolen,
    Replaced,
    Other,
}

impl ReasonCode {
    /// Every code.
    pub const ALL: [ReasonCode; 4] = [Self::Lost, Self::Stolen, Self::Replaced, Self::Other];

    /// The stored and wire form, e.g. `stolen`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lost => "lost",
            Self::Stolen => "stolen",
            Self::Replaced => "replaced",
            Self::Other => "other",
        }
    }
}

/// The [`IntentKind::reasons`] table as the JSON object the UI reads:
/// `{"device.revoke": ["lost", …]}` (kinds without reasons are left out).
pub fn reasons_json() -> String {
    let map: serde_json::Map<String, serde_json::Value> = IntentKind::ALL
        .into_iter()
        .filter_map(|k| {
            let codes = k.reasons()?;
            Some((
                k.as_str().to_owned(),
                codes.iter().map(|c| c.as_str()).collect::<Vec<_>>().into(),
            ))
        })
        .collect();
    let mut out = serde_json::to_string_pretty(&map).unwrap_or_else(|_| "{}".into());
    out.push('\n');
    out
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
    /// The cloud onboarding profile and the owner's staff row; absent for a dev fixture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub onboarding: Option<BootstrapOnboarding>,
}

/// What a paired branch takes from the cloud profile beyond the locale: tax registration and
/// the owner's staff row (no credentials; only the setup code's hash).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BootstrapOnboarding {
    pub country: String,
    pub gst_registered: bool,
    pub gstin: String,
    pub tax_mode: crate::tax::TaxMode,
    pub owner_staff: crate::pairing::BootstrapOwner,
}

/// The payload of `branch.seed_defaults` (nothing: the writer works out what is missing).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BranchSeedDefaults {}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub device_id: Option<String>,
    /// `paired` or `kds` (a phone is never registered).
    pub class: ClientClass,
    #[serde(default)]
    pub device_kind: crate::kiosk_pairing::DeviceKind,
    pub label: String,
    #[serde(default)]
    pub token_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub tv_number: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub group_id: Option<String>,
}

impl DeviceRegister {
    pub fn new(
        class: ClientClass,
        label: impl Into<String>,
        token_hash: impl Into<String>,
    ) -> Self {
        Self {
            device_id: None,
            class,
            device_kind: crate::kiosk_pairing::DeviceKind::Pc,
            label: label.into(),
            token_hash: token_hash.into(),
            tv_number: None,
            group_id: None,
        }
    }
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

/// The payload of `audit.read_granted`: who asked for a read elevation of `permission`, whose
/// PIN granted it, and the `pin_verifier_version` it was checked against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AuditReadGranted {
    pub permission: Permission,
    pub requester_staff_id: String,
    pub approver_staff_id: String,
    #[ts(type = "number")]
    pub verifier_version: i64,
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

/// The payload of `staff.set_credentials`: the member's own first password and PIN, argon2id
/// PHC strings computed outside the transaction. `setup_code_hash` is the stored hash the Hub
/// verified the typed code against and `trusted_now_ms` the Hub's trusted time at that check;
/// `apply()` accepts it only while the row still holds that hash, unexpired, and no password.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StaffSetCredentials {
    pub staff_id: String,
    pub setup_code_hash: String,
    #[ts(type = "number")]
    pub trusted_now_ms: i64,
    pub password_hash: String,
    pub pin_hash: String,
}

impl std::fmt::Debug for StaffSetCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaffSetCredentials")
            .field("staff_id", &self.staff_id)
            .finish_non_exhaustive()
    }
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

/// The payload of `group.upsert`. Without `id` it creates a group (the writer mints the id);
/// with one it edits that group. `group_kind` (not `kind`, which tags the payload) is one of
/// [`GROUP_KINDS`] and `colour` one of
/// [`GROUP_COLOURS`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GroupUpsert {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<String>,
    pub name: String,
    pub group_kind: String,
    pub colour: String,
    #[ts(type = "number")]
    pub sort_order: i64,
}

/// The payload of `group.delete`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GroupDelete {
    pub id: String,
}

/// The payload of `device.assign`: `group_id` `null` makes the device Unassigned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DeviceAssign {
    pub device_id: String,
    #[serde(default)]
    pub group_id: Option<String>,
}

/// The payload of `zone.upsert`. Without `id` it creates a zone (the writer mints the id);
/// with one it renames or reorders that zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ZoneUpsert {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<String>,
    pub name: String,
    #[ts(type = "number")]
    pub sort_order: i64,
}

/// The payload of `zone.delete`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ZoneDelete {
    pub id: String,
}

/// The payload of `device.place`: a zone with a cell (`floor_x` below [`FLOOR_COLS`], `floor_y`
/// below [`FLOOR_ROWS`]), or all three `null` to make the device Unassigned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DevicePlace {
    pub device_id: String,
    #[serde(default)]
    pub zone_id: Option<String>,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub floor_x: Option<i64>,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub floor_y: Option<i64>,
}

/// The payload of `device.maintenance`: `on` takes the device out of use with an optional
/// `note`; off returns it to service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DeviceMaintenance {
    pub device_id: String,
    pub on: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<String>,
}

pub const MAINTENANCE_NOTE_MAX: usize = 200;
pub const FLOOR_COLS: i64 = 12;
pub const FLOOR_ROWS: i64 = 8;
pub const GROUP_KINDS: [&str; 4] = ["pc", "console", "vr", "other"];
pub const GROUP_COLOURS: [&str; 6] = ["teal", "orange", "lime", "sand", "coral", "silver"];

/// The intent's payload, tagged by its kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum IntentPayload {
    #[serde(rename = "branch.bootstrap")]
    BranchBootstrap(BranchBootstrap),
    #[serde(rename = "branch.seed_defaults")]
    BranchSeedDefaults(BranchSeedDefaults),
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
    #[serde(rename = "audit.read_granted")]
    AuditReadGranted(AuditReadGranted),
    #[serde(rename = "kiosk.log")]
    KioskLog(KioskLog),
    #[serde(rename = "kiosk.process_blocked")]
    KioskProcessBlocked(KioskProcessBlocked),
    #[serde(rename = "device.admin_exit")]
    DeviceAdminExit(DeviceAdminExit),
    #[serde(rename = "device.admin_relock")]
    DeviceAdminRelock(DeviceAdminRelock),
    #[serde(rename = "device.admin_unlock")]
    DeviceAdminUnlock(DeviceAdminUnlock),
    #[serde(rename = "ads.consent")]
    AdsConsent(AdsConsentSet),
    #[serde(rename = "printer.status")]
    PrinterStatus(PrinterStatusSet),
    #[serde(rename = "group.upsert")]
    GroupUpsert(GroupUpsert),
    #[serde(rename = "group.delete")]
    GroupDelete(GroupDelete),
    #[serde(rename = "device.assign")]
    DeviceAssign(DeviceAssign),
    #[serde(rename = "licence.apply")]
    LicenceApply(crate::LicenceApply),
    #[serde(rename = "zone.upsert")]
    ZoneUpsert(ZoneUpsert),
    #[serde(rename = "zone.delete")]
    ZoneDelete(ZoneDelete),
    #[serde(rename = "device.place")]
    DevicePlace(DevicePlace),
    #[serde(rename = "product.upsert")]
    ProductUpsert(crate::ProductUpsert),
    #[serde(rename = "product.delete")]
    ProductDelete(crate::ProductDelete),
    #[serde(rename = "staff.set_credentials")]
    StaffSetCredentials(StaffSetCredentials),
    #[serde(rename = "stock.in")]
    StockIn(crate::StockIn),
    #[serde(rename = "stock.in.reverted")]
    StockInReverted(crate::StockInReverted),
    #[serde(rename = "device.maintenance")]
    DeviceMaintenance(DeviceMaintenance),
}

impl IntentPayload {
    /// The kind this payload belongs to.
    pub fn kind(&self) -> IntentKind {
        match self {
            Self::BranchBootstrap(_) => IntentKind::BranchBootstrap,
            Self::BranchSeedDefaults(_) => IntentKind::BranchSeedDefaults,
            Self::SettingsSet(_) => IntentKind::SettingsSet,
            Self::DeviceRegister(_) => IntentKind::DeviceRegister,
            Self::DeviceRevoke(_) => IntentKind::DeviceRevoke,
            Self::DeviceVersionReported(_) => IntentKind::DeviceVersionReported,
            Self::StaffUpsert(_) => IntentKind::StaffUpsert,
            Self::RoleUpsert(_) => IntentKind::RoleUpsert,
            Self::AuditReadGranted(_) => IntentKind::AuditReadGranted,
            Self::KioskLog(_) => IntentKind::KioskLog,
            Self::KioskProcessBlocked(_) => IntentKind::KioskProcessBlocked,
            Self::DeviceAdminExit(_) => IntentKind::DeviceAdminExit,
            Self::DeviceAdminRelock(_) => IntentKind::DeviceAdminRelock,
            Self::DeviceAdminUnlock(_) => IntentKind::DeviceAdminUnlock,
            Self::AdsConsent(_) => IntentKind::AdsConsent,
            Self::PrinterStatus(_) => IntentKind::PrinterStatus,
            Self::GroupUpsert(_) => IntentKind::GroupUpsert,
            Self::GroupDelete(_) => IntentKind::GroupDelete,
            Self::DeviceAssign(_) => IntentKind::DeviceAssign,
            Self::LicenceApply(_) => IntentKind::LicenceApply,
            Self::ZoneUpsert(_) => IntentKind::ZoneUpsert,
            Self::ZoneDelete(_) => IntentKind::ZoneDelete,
            Self::DevicePlace(_) => IntentKind::DevicePlace,
            Self::ProductUpsert(_) => IntentKind::ProductUpsert,
            Self::ProductDelete(_) => IntentKind::ProductDelete,
            Self::StaffSetCredentials(_) => IntentKind::StaffSetCredentials,
            Self::StockIn(_) => IntentKind::StockIn,
            Self::StockInReverted(_) => IntentKind::StockInReverted,
            Self::DeviceMaintenance(_) => IntentKind::DeviceMaintenance,
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
    /// Why, for kinds that require a reason; `None` otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<ReasonCode>,
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
            reason: None,
            payload: IntentPayload::BranchBootstrap(BranchBootstrap {
                name: "Dev Branch".into(),
                branch_code: "DEV01".into(),
                currency: "INR".into(),
                tz: "Asia/Kolkata".into(),
                day_start_min: 360,
                fy_start_month: 4,
                onboarding: None,
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
        Permission::StockAdjust,
        Permission::StockOverride,
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
    fn group_payloads_round_trip() {
        let up = IntentPayload::GroupUpsert(GroupUpsert {
            id: None,
            name: "PCs".into(),
            group_kind: "pc".into(),
            colour: "teal".into(),
            sort_order: 1,
        });
        let json = serde_json::to_value(&up).unwrap();
        assert_eq!(json["kind"], "group.upsert");
        assert_eq!(json["group_kind"], "pc");
        assert_eq!(serde_json::from_value::<IntentPayload>(json).unwrap(), up);
        let assign: IntentPayload = serde_json::from_value(
            serde_json::json!({"kind": "device.assign", "device_id": "D", "group_id": null}),
        )
        .unwrap();
        assert_eq!(
            assign,
            IntentPayload::DeviceAssign(DeviceAssign {
                device_id: "D".into(),
                group_id: None
            })
        );
    }

    #[test]
    fn zone_payloads_round_trip() {
        let up = IntentPayload::ZoneUpsert(ZoneUpsert {
            id: None,
            name: "PC hall".into(),
            sort_order: 1,
        });
        let json = serde_json::to_value(&up).unwrap();
        assert_eq!(json["kind"], "zone.upsert");
        assert_eq!(serde_json::from_value::<IntentPayload>(json).unwrap(), up);
        let place: IntentPayload = serde_json::from_value(
            serde_json::json!({"kind": "device.place", "device_id": "D", "zone_id": "Z", "floor_x": 2, "floor_y": 3}),
        )
        .unwrap();
        assert_eq!(
            place,
            IntentPayload::DevicePlace(DevicePlace {
                device_id: "D".into(),
                zone_id: Some("Z".into()),
                floor_x: Some(2),
                floor_y: Some(3)
            })
        );
    }

    #[test]
    fn maintenance_payload_round_trips() {
        let on: IntentPayload = serde_json::from_value(
            serde_json::json!({"kind": "device.maintenance", "device_id": "D", "on": true, "note": "fan"}),
        )
        .unwrap();
        assert_eq!(
            on,
            IntentPayload::DeviceMaintenance(DeviceMaintenance {
                device_id: "D".into(),
                on: true,
                note: Some("fan".into())
            })
        );
        assert_eq!(on.kind().as_str(), "device.maintenance");
    }

    #[test]
    fn every_kind_has_exactly_one_requirement() {
        let mut seen = std::collections::HashSet::new();
        for kind in IntentKind::ALL {
            assert!(seen.insert(kind), "{} listed twice in ALL", kind.as_str());
            // `requirement()` is a total match returning one value; pin the current mapping.
            let want = match kind.as_str() {
                "branch.bootstrap"
                | "branch.seed_defaults"
                | "device.version_reported"
                | "licence.apply"
                | "audit.read_granted" => Requirement::System,
                "settings.set" | "ads.consent" | "printer.status" => {
                    Requirement::Perm(Permission::SettingsEdit)
                }
                "device.register" | "group.upsert" | "group.delete" | "device.assign"
                | "zone.upsert" | "zone.delete" | "device.place" | "device.maintenance" => {
                    Requirement::Perm(Permission::DeviceManage)
                }
                "device.revoke" | "device.admin_unlock" => {
                    Requirement::Perm(Permission::DeviceRemote)
                }
                "device.admin_exit" | "device.admin_relock" => Requirement::Device,
                "staff.upsert" | "role.upsert" => Requirement::Perm(Permission::UserManage),
                "staff.set_credentials" => Requirement::System,
                "product.upsert" | "product.delete" => Requirement::Perm(Permission::CatalogEdit),
                "stock.in" | "stock.in.reverted" => Requirement::Perm(Permission::StockIn),
                "console.plan_requested" => Requirement::Phone,
                "kiosk.log" | "kiosk.process_blocked" => Requirement::Device,
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
    fn only_the_admin_exit_takes_an_elevating_permission() {
        for kind in IntentKind::ALL {
            let want = (kind == IntentKind::DeviceAdminExit).then_some(Permission::DeviceRemote);
            assert_eq!(kind.elevator_permission(), want, "{}", kind.as_str());
        }
        let p: IntentPayload = serde_json::from_value(serde_json::json!({
            "kind": "device.admin_exit", "exit_id": "01JAAAAAAAAAAAAAAAAAAAAAAA",
            "mode": "cached_verifier"
        }))
        .unwrap();
        assert_eq!(p.kind(), IntentKind::DeviceAdminExit);
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
    fn only_device_revoke_requires_a_reason() {
        for kind in IntentKind::ALL {
            let want: Option<&[ReasonCode]> = match kind {
                IntentKind::DeviceRevoke => Some(&ReasonCode::ALL),
                _ => None,
            };
            assert_eq!(kind.reasons(), want, "{}", kind.as_str());
        }
        for code in ReasonCode::ALL {
            let json = serde_json::to_string(&code).unwrap();
            assert_eq!(json, format!("\"{}\"", code.as_str()));
        }
        let intent = Intent {
            reason: Some(ReasonCode::Stolen),
            ..bootstrap()
        };
        let json = serde_json::to_value(&intent).unwrap();
        assert_eq!(json["reason"], "stolen");
        assert_eq!(serde_json::from_value::<Intent>(json).unwrap(), intent);
    }

    #[test]
    fn reasons_json_lists_kinds_with_reasons() {
        let v: serde_json::Value = serde_json::from_str(&reasons_json()).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"device.revoke": ["lost", "stolen", "replaced", "other"]})
        );
    }

    /// `pnpm gen:types` writes the reasons table next to the generated TypeScript types.
    #[cfg(feature = "export-ts")]
    #[test]
    fn export_reasons_json() {
        let dir = std::env::var("TS_RS_EXPORT_DIR").unwrap_or_else(|_| "./bindings".into());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join("reasons.json"),
            reasons_json(),
        )
        .unwrap();
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
        assert!(json.get("reason").is_none());
        let back: Intent = serde_json::from_value(json).unwrap();
        assert_eq!(back, intent);
        assert_eq!(back.payload.kind(), IntentKind::BranchBootstrap);
    }
}
