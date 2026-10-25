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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub licence_unverified: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub clock_wrong: Option<bool>,
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
            }],
            locale: None,
            ui: SnapshotUi::default(),
            connectivity: crate::Connectivity::default(),
            setup: None,
            lan: None,
            paired: true,
            pairing_revoked: None,
            licence_unverified: None,
            clock_wrong: None,
        };
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            serde_json::json!({
                "branch": null,
                "groups": [],
                "zones": [],
                "devices": [{"device_id": "D", "label": "Dev Kiosk", "class": "paired", "zone": null, "online": true, "kind": "pc", "group_id": null, "meta_rev": 1, "tv_number": null, "floor_x": null, "floor_y": null}],
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
}
