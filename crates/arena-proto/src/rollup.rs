//! Rollup schema v1 (Story 8.5, AD-35): `rollups/<yyyy-mm-dd>.json`, the Hub's own totals for one
//! business day, in one fixed, versioned shape. All money is integer minor units. The file is
//! computed from stamped columns and stored totals, never recomputed from rates, and its revenue,
//! payments and cash figures equal the Z-reports of the same day.
//!
//! The only player data in it is the signed-off credit list (id, username, amounts, the day the
//! oldest unpaid credit was spent, and whether a transactional consent is on file).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The schema of `rollups/<date>.json`.
pub const ROLLUP_SCHEMA: u32 = 1;
/// The most credit players one rollup lists (largest balance first); the totals cover everyone.
pub const ROLLUP_PLAYERS_CAP: usize = 500;

/// A count and the money it came to. Amounts are non-negative magnitudes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RollupAmount {
    #[ts(type = "number")]
    pub count: i64,
    #[ts(type = "number")]
    pub amount: i64,
}

/// Revenue by what was sold. `other` is discounts (negative) and the tax a tax-exclusive bill adds
/// on top, so the four add up to `revenue.total`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RollupByType {
    #[ts(type = "number")]
    pub play: i64,
    #[ts(type = "number")]
    pub products: i64,
    #[ts(type = "number")]
    pub plans: i64,
    #[ts(type = "number")]
    pub other: i64,
}

/// Payments taken, by method: the positive payment rows of the day's settled bills, wallet
/// top-ups and credit collections (the Z-report's payments).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RollupByMethod {
    #[ts(type = "number")]
    pub cash: i64,
    #[ts(type = "number")]
    pub upi: i64,
    #[ts(type = "number")]
    pub card: i64,
    #[ts(type = "number")]
    pub wallet: i64,
    #[ts(type = "number")]
    pub credit: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RollupRevenue {
    /// The settled sales' stored totals (the Z-report's `sales_minor`).
    #[ts(type = "number")]
    pub total: i64,
    pub by_type: RollupByType,
    pub by_method: RollupByMethod,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RollupSessions {
    /// Sessions that started on the business day (orphaned ones excluded).
    #[ts(type = "number")]
    pub count: i64,
    /// Whole minutes of session time on those sessions' segments.
    #[ts(type = "number")]
    pub minutes: i64,
}

/// A shift closed on the business day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RollupShift {
    pub shift_id: String,
    pub staff_name: String,
    #[ts(type = "number")]
    pub opened_at: i64,
    #[ts(type = "number")]
    pub closed_at: i64,
    #[ts(type = "number")]
    pub float: i64,
    #[ts(type = "number")]
    pub expected: i64,
    #[ts(type = "number")]
    pub counted: i64,
    /// Signed: counted less expected.
    #[ts(type = "number")]
    pub variance: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub approver_name: Option<String>,
}

/// A tracked product's stock at the end of the build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RollupStockLevel {
    pub product_id: String,
    pub name: String,
    #[ts(type = "number")]
    pub qty: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub low_qty: Option<i64>,
}

/// Stock written off on the day for one product and reason, at cost; an undone waste nets out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RollupWaste {
    pub product_id: String,
    #[ts(type = "number")]
    pub qty: i64,
    #[ts(type = "number")]
    pub value: i64,
    pub reason: String,
}

/// A lot with stock left that expires within the branch's warning window of the day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RollupExpiring {
    pub product_id: String,
    pub lot: String,
    #[ts(type = "number")]
    pub qty: i64,
    pub expires_on: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RollupStock {
    pub levels: Vec<RollupStockLevel>,
    pub waste: Vec<RollupWaste>,
    pub expiring: Vec<RollupExpiring>,
}

/// Credit outstanding by the age of the oldest unpaid spend, first in first out, as of the day.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RollupAged {
    #[ts(type = "number")]
    pub d0_7: i64,
    #[ts(type = "number")]
    pub d8_14: i64,
    #[ts(type = "number")]
    pub d15p: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RollupCreditPlayer {
    pub player_id: String,
    pub username: String,
    #[ts(type = "number")]
    pub outstanding: i64,
    /// The business day of the oldest credit spend not yet collected.
    pub oldest_unpaid_on: String,
    /// A transactional consent (reminders) is on file and granted.
    pub consent: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RollupCredit {
    /// Credit outstanding across every debtor.
    #[ts(type = "number")]
    pub outstanding: i64,
    pub aged: RollupAged,
    /// Largest balance first, at most [`ROLLUP_PLAYERS_CAP`].
    pub players: Vec<RollupCreditPlayer>,
}

/// Minutes played of one game (the library entry id) on the day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RollupPlayHours {
    pub game_id: String,
    #[ts(type = "number")]
    pub minutes: i64,
}

/// `rollups/<business_day>.json`, schema 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RollupV1 {
    pub schema: u32,
    pub branch_id: String,
    pub business_day: String,
    pub currency: String,
    pub revenue: RollupRevenue,
    pub refunds: RollupAmount,
    pub discounts: RollupAmount,
    pub voids: RollupAmount,
    pub sessions: RollupSessions,
    pub shifts: Vec<RollupShift>,
    pub stock: RollupStock,
    pub credit: RollupCredit,
    pub play_hours: Vec<RollupPlayHours>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unset_optional_field_has_no_key() {
        let s = RollupShift {
            shift_id: "s".into(),
            staff_name: "Asha".into(),
            opened_at: 1,
            closed_at: 2,
            float: 0,
            expected: 0,
            counted: 0,
            variance: 0,
            reason: None,
            approver_name: None,
        };
        let json = serde_json::to_value(&s).unwrap();
        assert!(json.get("reason").is_none() && json.get("approver_name").is_none());
        assert_eq!(serde_json::from_value::<RollupShift>(json).unwrap(), s);
    }
}
