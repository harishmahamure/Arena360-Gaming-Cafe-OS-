//! The Hub UI's branch snapshot, read over the loopback `GET /ui/snapshot`, and the events
//! pushed on `GET /ui/events`.

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
    /// The live device groups, in `sort_order` then name order.
    #[serde(default)]
    pub groups: Vec<SnapshotGroup>,
    /// The live floor zones, in `sort_order` then name order.
    #[serde(default)]
    pub zones: Vec<SnapshotZone>,
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<crate::SnapshotSession>>")]
    pub sessions: Vec<crate::SnapshotSession>,
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<crate::SnapshotBill>>")]
    pub bills: Vec<crate::SnapshotBill>,
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<crate::SnapshotGroupBill>>")]
    pub group_bills: Vec<crate::SnapshotGroupBill>,
    /// Every open alert, oldest first.
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<crate::SnapshotAlert>>")]
    pub alerts: Vec<crate::SnapshotAlert>,
    /// The open walk-up kitchen tickets (seat "Counter"), oldest first: the POS open-tickets strip
    /// (Story 7.4).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<crate::KitchenTicketView>>")]
    pub counter_tickets: Vec<crate::KitchenTicketView>,
    /// The kitchen displays that are not revoked, oldest first (Story 7.6). `connected` is filled by
    /// the LAN layer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<crate::KitchenDisplayView>>")]
    pub kitchen_displays: Vec<crate::KitchenDisplayView>,
    /// The branch's shift: who has the drawer open, and the float to prefill for the next one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub shift: Option<crate::ShiftState>,
    /// The branch locale in effect now, or `null` before the branch is set up.
    pub locale: Option<SnapshotLocale>,
    /// Hub UI behaviour settings.
    pub ui: SnapshotUi,
    /// Which WAN link is down, as the connectivity probe last saw it (the `chip-offline`).
    #[serde(default)]
    pub connectivity: crate::Connectivity,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub setup: Option<crate::SetupStatus>,
    #[serde(default)]
    pub lan: Option<SnapshotLan>,
    #[serde(default = "paired_default")]
    pub paired: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pairing_revoked: Option<bool>,
    /// The Hub replaces another and waits to be restored (Story 9.9): the name of the branch it
    /// restores. The Hub UI shows "Restore this branch" and nothing else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub restore_pending: Option<String>,
    /// This Windows Hub paired with the code of a Hub move and waits for the Android Hub to hand
    /// the branch over (Story 10.16): the name of the branch. The Hub UI shows "Waiting for your
    /// Android Hub" and nothing else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub migration_wait: Option<String>,
    /// The move this Android Hub is asked to hand over (Story 10.16): staff see a persistent
    /// "Move this branch to <PC name>" item in the alert list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub move_notice: Option<crate::MigrationNotice>,
    /// The owner released the branch key to this waiting Hub and it holds the key (Story 9.10):
    /// "Restore from a file" is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub restore_key_released: Option<bool>,
    /// Unpaired because the Hub lost the key that wrapped its secrets: the pairing screen says
    /// "This Hub needs to be paired again" (the branch data is intact).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub needs_repair: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub licence_unverified: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub clock_wrong: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub device_cap: Option<u32>,
    /// The effective tier (the licence's, or Free after a failed check); omitted until the Hub knows it.
    /// The Android ads gate reads it (Story 4.10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tier: Option<crate::Tier>,
    /// How the branch is billed, from a verified paid licence (Story 10.6): System shows it beside the tier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plan_cycle: Option<crate::BillingCycle>,
    /// When the paid licence ends (UTC ms), shown in branch time on System.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "number", optional)]
    pub licence_exp: Option<i64>,
}

fn paired_default() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SnapshotLan {
    pub addresses: Vec<String>,
    pub lan_port: u16,
    pub web_port: u16,
    pub fingerprint: String,
    /// The network the Hub is on ("Wi-Fi", "Ethernet"), when the host can name it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub network: Option<String>,
    /// An Android Hub with no LAN address: the header shows "No LAN connection".
    #[serde(default)]
    pub no_lan: bool,
}

/// The Hub UI settings every open view follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SnapshotUi {
    /// Minutes without input before a signed-in Hub UI locks (1–30; the setting
    /// `idle_lock_minutes`).
    pub idle_lock_minutes: u8,
    /// Whether this Hub's UI uses the high-contrast theme (the setting `display.hc.hub`).
    pub hc: bool,
}

impl SnapshotUi {
    /// The idle lock used when the setting is missing or unreadable.
    pub const DEFAULT_IDLE_LOCK_MINUTES: u8 = 5;
    /// The setting's allowed range.
    pub const IDLE_LOCK_RANGE: std::ops::RangeInclusive<u8> = 1..=30;

    /// From the settings' TEXT values (`idle_lock_minutes`, `display.hc.hub`): anything
    /// missing or out of range is the default (5 minutes, high contrast off).
    pub fn from_settings(idle_lock: Option<&str>, hc: Option<&str>) -> Self {
        let idle_lock_minutes = idle_lock
            .and_then(|v| v.parse::<u8>().ok())
            .filter(|m| Self::IDLE_LOCK_RANGE.contains(m))
            .unwrap_or(Self::DEFAULT_IDLE_LOCK_MINUTES);
        Self {
            idle_lock_minutes,
            hc: crate::display::parse_hc_value(hc),
        }
    }
}

impl Default for SnapshotUi {
    fn default() -> Self {
        Self {
            idle_lock_minutes: Self::DEFAULT_IDLE_LOCK_MINUTES,
            hc: false,
        }
    }
}

/// Where the currency symbol goes: `₹1.00` or `1.00 €`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
#[serde(rename_all = "lowercase")]
pub enum SymbolPosition {
    Before,
    After,
}

/// Digit grouping: `1,23,456` (Indian) or `123,456` (international).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
#[serde(rename_all = "lowercase")]
pub enum NumberFormat {
    Indian,
    International,
}

/// The branch locale in effect for the current business day (from the `branch.*` settings).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SnapshotLocale {
    /// ISO 4217 code.
    pub currency: String,
    /// Minor-unit exponent of `currency` (2 for INR, 0 for JPY, 3 for KWD).
    pub exponent: u8,
    pub symbol: String,
    pub symbol_position: SymbolPosition,
    pub number_format: NumberFormat,
    /// IANA time zone.
    pub tz: String,
    /// Minutes after local midnight at which the business day starts (0–1439).
    pub day_start_min: u16,
    /// The month (1–12) the financial year starts in.
    pub fy_start_month: u8,
    /// Timezone, day-start or financial-year changes still waiting for their business day.
    pub pending: Option<LocalePending>,
}

/// The deferred locale changes, each with the business day it applies from. At least one is
/// set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LocalePending {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tz: Option<PendingText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub day_start_min: Option<PendingNumber>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub fy_start_month: Option<PendingNumber>,
}

/// A deferred text value and the business day (`YYYY-MM-DD`) it applies from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PendingText {
    pub value: String,
    pub effective_from: String,
}

/// A deferred number and the business day (`YYYY-MM-DD`) it applies from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PendingNumber {
    pub value: u16,
    pub effective_from: String,
}

/// `event: settings.changed` on `GET /ui/events`: a committed intent wrote `settings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SettingsChangedEvent {
    /// The locale now in effect (`null` only before the branch is set up).
    pub locale: Option<SnapshotLocale>,
    /// The setting keys the intent wrote.
    pub keys: Vec<String>,
    /// The Hub UI settings now in effect.
    pub ui: SnapshotUi,
}

/// `event: entitlement.changed` on `GET /ui/events`: the Hub's effective tier moved (a licence applied,
/// expired or failed its check). `tier` is `null` when the Hub no longer knows it (unpaired). The
/// Android ads gate re-evaluates on it without a restart (Story 4.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct EntitlementChangedEvent {
    pub tier: Option<crate::Tier>,
}

/// `event: applied` on `GET /ui/events`: any other committed intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AppliedEvent {
    pub intent_id: String,
}

/// The branch fields the shell shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SnapshotBranch {
    pub name: String,
    pub branch_code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub code_locked: Option<bool>,
}

/// One device group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SnapshotGroup {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub colour: String,
    #[ts(type = "number")]
    pub sort_order: i64,
    #[ts(type = "number")]
    pub rev: i64,
    #[serde(default)]
    #[ts(type = "number | null", optional)]
    pub rate_minor: Option<i64>,
    /// The pricing rule that sets this group's rate now, when one does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub active_rule: Option<SnapshotActiveRule>,
    /// The earliest instant in the next 24 hours at which a rule of this group begins or ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "number | null", optional)]
    pub next_change_at: Option<i64>,
}

/// The pricing rule in effect for a group: what the header chip and the live rate show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SnapshotActiveRule {
    pub rule_id: String,
    pub name: String,
    /// `override_minor` or `percent_bp`.
    pub adjust_kind: String,
    #[ts(type = "number")]
    pub adjust_value: i64,
    /// The hourly rate in minor units while the rule applies.
    #[ts(type = "number")]
    pub rate_minor: i64,
    /// When another rule, or the base rate, takes over (within the next 24 hours).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "number | null", optional)]
    pub until: Option<i64>,
}

/// One floor zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SnapshotZone {
    pub id: String,
    pub name: String,
    #[ts(type = "number")]
    pub sort_order: i64,
    #[ts(type = "number")]
    pub rev: i64,
}

/// The one status a device shows. `InSession` and `Reserved` are set by later features.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum DeviceStatus {
    Online,
    #[default]
    Offline,
    InSession,
    Reserved,
    Maintenance,
}

impl DeviceStatus {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "online" => Self::Online,
            "offline" => Self::Offline,
            "in_session" => Self::InSession,
            "reserved" => Self::Reserved,
            "maintenance" => Self::Maintenance,
            _ => return None,
        })
    }

    pub fn held(self) -> bool {
        !matches!(self, Self::Online | Self::Offline)
    }
}

/// One device on the floor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SnapshotDevice {
    pub device_id: String,
    pub label: String,
    pub class: ClientClass,
    /// The id of the floor zone the device sits in, or `null` when it is Unassigned.
    pub zone: Option<String>,
    /// Whether the device has a live paired session with this Hub right now.
    #[serde(default)]
    pub online: bool,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub kind: Option<crate::kiosk_pairing::DeviceKind>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub group_id: Option<String>,
    #[serde(default)]
    #[ts(optional, type = "number | null")]
    pub meta_rev: Option<i64>,
    #[serde(default)]
    #[ts(optional, type = "number | null")]
    pub tv_number: Option<i64>,
    #[serde(default)]
    #[ts(optional, type = "number | null")]
    pub floor_x: Option<i64>,
    #[serde(default)]
    #[ts(optional, type = "number | null")]
    pub floor_y: Option<i64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(optional, type = "boolean")]
    pub manual: bool,
    #[serde(default)]
    #[ts(optional, as = "Option<DeviceStatus>")]
    pub status: DeviceStatus,
    #[serde(default)]
    #[ts(optional, type = "number | null")]
    pub last_seen_at: Option<i64>,
    #[serde(default)]
    #[ts(optional, type = "string | null")]
    pub maintenance_note: Option<String>,
    /// Over the plan's device limit: no session starts or moves onto it (Story 10.12). A running
    /// session is unaffected.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(optional, type = "boolean")]
    pub over_cap: bool,
    /// The ended console session whose controllers staff have not marked yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string | null")]
    pub controller_check: Option<String>,
    /// Connected through the Cloudflare Tunnel, not the LAN (Story 8.41); filled by the snapshot handler.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(optional, type = "boolean")]
    pub remote: bool,
    /// Restored from the cloud backup and not paired with this Hub yet: staff pair it again with
    /// the 6-digit code (Story 9.13).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(optional, type = "boolean")]
    pub pair_again: bool,
}

impl SnapshotDevice {
    pub fn apply_presence(&mut self, online: bool, last_seen_at: Option<i64>) {
        if self.status.held() || self.manual {
            return;
        }
        self.status = if online {
            DeviceStatus::Online
        } else {
            DeviceStatus::Offline
        };
        self.last_seen_at = last_seen_at;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_with_null_branch_and_zone() {
        let s = HubSnapshot {
            branch: None,
            groups: vec![],
            zones: vec![],
            sessions: vec![],
            bills: vec![],
            group_bills: vec![],
            alerts: vec![],
            counter_tickets: vec![],
            kitchen_displays: vec![],
            shift: None,
            devices: vec![SnapshotDevice {
                device_id: "D".into(),
                label: "Dev Kiosk".into(),
                class: ClientClass::Paired,
                zone: None,
                online: true,
                kind: Some(crate::kiosk_pairing::DeviceKind::Pc),
                group_id: None,
                meta_rev: Some(1),
                tv_number: None,
                floor_x: None,
                floor_y: None,
                manual: false,
                status: DeviceStatus::Online,
                last_seen_at: None,
                maintenance_note: None,
                over_cap: false,
                controller_check: None,
                remote: false,
                pair_again: false,
            }],
            locale: None,
            ui: SnapshotUi::default(),
            connectivity: crate::Connectivity::default(),
            setup: None,
            lan: None,
            paired: true,
            pairing_revoked: None,
            restore_pending: None,
            migration_wait: None,
            move_notice: None,
            restore_key_released: None,
            needs_repair: None,
            licence_unverified: None,
            clock_wrong: None,
            device_cap: None,
            tier: None,
            plan_cycle: None,
            licence_exp: None,
        };
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            serde_json::json!({
                "branch": null,
                "groups": [],
                "zones": [],
                "sessions": [],
                "bills": [],
                "group_bills": [],
                "alerts": [],
                "devices": [{"device_id": "D", "label": "Dev Kiosk", "class": "paired", "zone": null, "online": true, "kind": "pc", "group_id": null, "meta_rev": 1, "tv_number": null, "floor_x": null, "floor_y": null, "status": "online", "last_seen_at": null, "maintenance_note": null}],
                "locale": null,
                "ui": {"idle_lock_minutes": 5, "hc": false},
                "connectivity": {"state": "online", "checked_at": null},
                "setup": null,
                "lan": null,
                "paired": true
            })
        );
    }

    #[test]
    fn ui_block_reads_the_setting_or_falls_back() {
        assert_eq!(
            SnapshotUi::from_settings(Some("10"), None).idle_lock_minutes,
            10
        );
        for bad in [None, Some("0"), Some("31"), Some("five"), Some("")] {
            assert_eq!(
                SnapshotUi::from_settings(bad, None).idle_lock_minutes,
                5,
                "{bad:?}"
            );
        }
        assert!(SnapshotUi::from_settings(None, Some("true")).hc);
        for off in [None, Some("false"), Some("yes")] {
            assert!(!SnapshotUi::from_settings(None, off).hc, "{off:?}");
        }
    }
}

#[cfg(test)]
mod locale_tests {
    use super::*;

    #[test]
    fn locale_serializes_with_lowercase_enums_and_sparse_pending() {
        let l = SnapshotLocale {
            currency: "INR".into(),
            exponent: 2,
            symbol: "₹".into(),
            symbol_position: SymbolPosition::Before,
            number_format: NumberFormat::Indian,
            tz: "Asia/Kolkata".into(),
            day_start_min: 360,
            fy_start_month: 4,
            pending: Some(LocalePending {
                tz: Some(PendingText {
                    value: "Asia/Dubai".into(),
                    effective_from: "2027-01-02".into(),
                }),
                day_start_min: None,
                fy_start_month: None,
            }),
        };
        let v = serde_json::to_value(&l).unwrap();
        assert_eq!(v["symbol_position"], "before");
        assert_eq!(v["number_format"], "indian");
        assert_eq!(
            v["pending"],
            serde_json::json!({"tz": {"value": "Asia/Dubai", "effective_from": "2027-01-02"}})
        );
    }
    #[test]
    fn presence_moves_only_online_and_offline_paired_devices() {
        let dev = |manual, status| SnapshotDevice {
            device_id: "D".into(),
            label: "L".into(),
            class: ClientClass::Paired,
            zone: None,
            online: false,
            kind: None,
            group_id: None,
            meta_rev: None,
            tv_number: None,
            floor_x: None,
            floor_y: None,
            manual,
            status,
            last_seen_at: None,
            maintenance_note: None,
            over_cap: false,
            controller_check: None,
            remote: false,
            pair_again: false,
        };
        let mut d = dev(false, DeviceStatus::Offline);
        d.apply_presence(true, Some(9));
        assert_eq!((d.status, d.last_seen_at), (DeviceStatus::Online, Some(9)));
        d.apply_presence(false, Some(10));
        assert_eq!(
            (d.status, d.last_seen_at),
            (DeviceStatus::Offline, Some(10))
        );
        let mut m = dev(true, DeviceStatus::Online);
        m.apply_presence(false, Some(1));
        assert_eq!((m.status, m.last_seen_at), (DeviceStatus::Online, None));
        for held in [
            DeviceStatus::Maintenance,
            DeviceStatus::InSession,
            DeviceStatus::Reserved,
        ] {
            let mut h = dev(false, held);
            h.apply_presence(true, Some(1));
            assert_eq!((h.status, h.last_seen_at), (held, None));
        }
    }
}
