//! The branch summary (Story 8.4, AD-35): `state/latest.json`, the short picture of right now
//! that a paid Hub publishes every `summary_interval_s`, and `summary.publish`, the system intent
//! that records the publish in `rollup_log`.
//!
//! The file holds counts, totals, staff display names and the owner-editable configuration
//! (the catalogue, each row with its `rev`). It never holds a transaction row, and an attention
//! item names its subject by id and display parameters only, never a player's name, phone or
//! email.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

/// The schema of `state/latest.json`.
pub const SUMMARY_SCHEMA: u32 = 1;
/// The `rollup_log.date` of the summary file.
pub const SUMMARY_LOG_DATE: &str = "latest";
/// The object key of the summary under the branch's prefix of the data bucket; what
/// `rollup_log.key` records for `latest`.
pub const SUMMARY_OBJECT_KEY: &str = "state/latest.json";
/// The most a summary lists in any one of its lists (attention, each catalogue part).
pub const SUMMARY_LIST_CAP: usize = 500;

/// `attention.kind`: a shift closed today with a variance over the branch threshold.
pub const ATTENTION_SHIFT_VARIANCE: &str = "shift_variance";
/// `attention.kind`: a tracked product at or under its low-stock threshold.
pub const ATTENTION_LOW_STOCK: &str = "low_stock";
/// `attention.kind`: a player 15+ days overdue on credit (the ref is the player's id only).
pub const ATTENTION_CREDIT_OVERDUE: &str = "credit_overdue";
/// `attention.kind`: an open Hub alert (the ref is the alert id; the alert's kind is a parameter).
pub const ATTENTION_ALERT: &str = "alert";

/// An open shift.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SummaryShift {
    pub shift_id: String,
    pub staff_name: String,
    #[ts(type = "number")]
    pub opened_at: i64,
    /// Known only once a shift is counted; absent while it is open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub variance_minor: Option<i64>,
    /// When a counted shift closed (Hub clock, ms); absent while it is open (Story 8.13).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub closed_at: Option<i64>,
}

/// Something that needs the owner's eyes, by id and display parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SummaryAttention {
    pub kind: String,
    #[serde(rename = "ref")]
    #[ts(rename = "ref")]
    pub reference: String,
    pub text_params: BTreeMap<String, String>,
}

/// One owner-editable row: its id, a display name, its `rev` (the `base_rev` of an edit) and, for
/// the kinds that have one, whether it is active, one more display detail and the editable fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CatalogueRow {
    pub id: String,
    pub name: String,
    #[ts(type = "number")]
    pub rev: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub active: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub detail: Option<String>,
    /// The row as its upsert payload without `id` and `active`, for the kinds the Owner app can
    /// edit (rates, plans, pricing rules, coupons, products and bundles; Stories 8.23, 8.24).
    /// Additive: an older summary lacks it and its row can be listed but not edited. A product or
    /// bundle carries no image or kitchen notes and adds `low_stock_qty` (set by `stock.threshold`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "Record<string, unknown>")]
    pub fields: Option<serde_json::Value>,
}

/// A staff member as the owner sees them: no credentials of any kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CatalogueStaff {
    pub id: String,
    pub name: String,
    pub role: String,
    pub active: bool,
    #[ts(type = "number")]
    pub rev: i64,
}

/// A device: its name, group and kind, and whether it was revoked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CatalogueDevice {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub group_id: Option<String>,
    pub kind: String,
    pub revoked: bool,
    #[ts(type = "number")]
    pub rev: i64,
}

/// The owner-editable state of the branch, configuration only.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SummaryCatalogue {
    pub groups: Vec<CatalogueRow>,
    pub rates: Vec<CatalogueRow>,
    pub plans: Vec<CatalogueRow>,
    pub pricing_rules: Vec<CatalogueRow>,
    pub coupons: Vec<CatalogueRow>,
    pub products: Vec<CatalogueRow>,
    pub bundles: Vec<CatalogueRow>,
    pub staff: Vec<CatalogueStaff>,
    pub roles: Vec<CatalogueRow>,
    pub devices: Vec<CatalogueDevice>,
}

/// `state/latest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BranchSummary {
    pub schema: u32,
    pub branch_id: String,
    /// When the Hub built it (Hub clock, ms).
    #[ts(type = "number")]
    pub as_of: i64,
    pub business_day: String,
    /// A shift is open or a session is running.
    pub open: bool,
    #[ts(type = "number")]
    pub sessions_live: i64,
    /// The day's settled sales net of refunds, in minor units.
    #[ts(type = "number")]
    pub revenue_so_far_minor: i64,
    pub currency: String,
    /// The expected cash of the open shift's drawer (0 with none open).
    #[ts(type = "number")]
    pub cash_in_drawer_minor: i64,
    pub shifts: Vec<SummaryShift>,
    pub staff_on_shift: Vec<String>,
    pub attention: Vec<SummaryAttention>,
    pub catalogue: SummaryCatalogue,
}

/// The payload of `summary.publish` (system only): the summary was PUT to the cloud. Writes the
/// `rollup_log` row `latest` and nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SummaryPublish {
    /// The summary's schema.
    pub schema: u32,
    /// When it was built (Hub clock, ms).
    #[ts(type = "number")]
    pub built_at: i64,
    /// The object key it was PUT to (`.../state/latest.json`).
    pub key: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_attention_item_serialises_its_ref_as_ref() {
        let a = SummaryAttention {
            kind: ATTENTION_ALERT.into(),
            reference: "01JA".into(),
            text_params: BTreeMap::from([("kind".into(), "time_up".into())]),
        };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["ref"], "01JA");
        assert_eq!(serde_json::from_value::<SummaryAttention>(json).unwrap(), a);
    }

    #[test]
    fn an_open_shift_has_no_variance_key() {
        let s = SummaryShift {
            shift_id: "s".into(),
            staff_name: "Asha".into(),
            opened_at: 5,
            variance_minor: None,
            closed_at: None,
        };
        assert!(
            serde_json::to_value(&s)
                .unwrap()
                .get("variance_minor")
                .is_none()
        );
    }
}
