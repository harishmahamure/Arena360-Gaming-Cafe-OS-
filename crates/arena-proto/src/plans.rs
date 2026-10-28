use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const PLAN_KINDS: [&str; 3] = ["pack", "block", "pass"];
pub const PLAN_MAX_MINUTES: i64 = 100_000;
pub const PLAN_MAX_VALIDITY_DAYS: i64 = 3650;
pub const PLAN_MAX_GROUPS: usize = 50;
pub const MINUTES_PER_DAY: i64 = 1440;

fn yes() -> bool {
    true
}

/// The payload of `plan.upsert`: a time pack (`minutes`, `validity_days`), a block (a start
/// window of minutes from local midnight) or a pass (`validity_days` and a play window that
/// may wrap midnight), sold for the groups in `group_ids`. A plan with an `id` is an edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlanUpsert {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<String>,
    pub plan_kind: String,
    pub name: String,
    #[ts(type = "number")]
    pub price_minor: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub minutes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub validity_days: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub window_start_min: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub window_end_min: Option<i64>,
    pub group_ids: Vec<String>,
    #[serde(default = "yes")]
    pub active: bool,
}

/// One plan as `GET /ui/plans` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlanView {
    pub id: String,
    pub plan_kind: String,
    pub name: String,
    #[ts(type = "number")]
    pub price_minor: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub minutes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub validity_days: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub window_start_min: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub window_end_min: Option<i64>,
    pub group_ids: Vec<String>,
    pub active: bool,
    #[ts(type = "number")]
    pub rev: i64,
}

use crate::PaymentRow;

/// The payload of `plan.sell`: a pack or pass sold to a profiled player, settled in the same
/// intent (payments sum to the plan price; wallet and credit are allowed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlanSell {
    pub player_id: String,
    pub plan_id: String,
    pub payments: Vec<PaymentRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub credit_override: Option<bool>,
    /// The receipt choice (Story 5.17); `print` issues the invoice number in this transaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub receipt: Option<crate::ReceiptChoice>,
}

/// The payload of `plan.sell.reverted`: Undo of a plan sale within its window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlanSellReverted {
    pub order_id: String,
}

/// One pack or pass a player owns, with the terms it was sold under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnedPlanView {
    pub id: String,
    pub plan_id: String,
    pub plan_kind: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub minutes_left: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub minutes_total: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub window_start_min: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub window_end_min: Option<i64>,
    pub group_ids: Vec<String>,
    #[ts(type = "number")]
    pub bought_at: i64,
    #[ts(type = "number")]
    pub expires_at: i64,
    pub expired: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_defaults_to_active_without_optional_fields() {
        let u: PlanUpsert = serde_json::from_value(serde_json::json!({
            "plan_kind": "pack", "name": "5 h", "price_minor": 30000, "minutes": 300,
            "validity_days": 7, "group_ids": ["G"]
        }))
        .unwrap();
        assert!(u.active);
        assert_eq!(u.id, None);
        assert_eq!(u.window_start_min, None);
    }
}
