//! Rollup schema v1 (Story 8.5, AD-35): `rollups/<yyyy-mm-dd>.json`, the Hub's own totals for one
//! business day, in one fixed, versioned shape. All money is integer minor units. The file is
//! computed from stamped columns and stored totals, never recomputed from rates, and its revenue,
//! payments and cash figures equal the Z-reports of the same day.
//!
//! The only player data in it is the signed-off credit list (id, username, amounts, the day the
//! oldest unpaid credit was spent, and whether a transactional consent is on file).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The schema of `rollups/<date>.json` this release writes natively: N (Story 8.7).
pub const ROLLUP_SCHEMA: u32 = 1;
/// The oldest schema this release still has a writer for: N−1, or N when N is the first.
pub const ROLLUP_MIN_SCHEMA: u32 = if ROLLUP_SCHEMA > 1 {
    ROLLUP_SCHEMA - 1
} else {
    1
};

/// The schema a Hub whose newest writer is `newest` (N) emits when its licence pins `pin`: the
/// pin, held within the writers it keeps (N−1 and N). A tier's file shape therefore changes only
/// when the cloud raises the pin, and never past what the Hub can write.
pub fn emitted_rollup_schema(pin: u32, newest: u32) -> u32 {
    pin.clamp(newest.saturating_sub(1).max(1), newest)
}

/// Whether a cloud whose newest rollup schema is `current` reads a file of `schema`: N or N−1.
pub fn rollup_schema_accepted(schema: u32, current: u32) -> bool {
    schema >= 1 && (schema == current || schema.saturating_add(1) == current)
}
/// The object key of the rollup of `date` (`yyyy-mm-dd`) under the branch's prefix of the data
/// bucket; what `rollup_log.key` records for that day.
pub fn rollup_object_key(date: &str) -> String {
    format!("rollups/{date}.json")
}

/// The object key of the daily snapshot of `date` (`yyyy-mm-dd`) under the branch's prefix of the
/// data bucket (Story 8.8); what `snapshot_log.key` records for that day.
pub fn snapshot_object_key(date: &str) -> String {
    format!("snapshots/{date}.db.zst")
}

/// Whether `s` is a `rollup_log.date` of a daily rollup (`yyyy-mm-dd`, month 01–12, day 01–31), as
/// opposed to the summary's `latest`.
pub fn is_rollup_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let num =
        |r: std::ops::Range<usize>| s.get(r).filter(|p| p.bytes().all(|c| c.is_ascii_digit()));
    let (Some(y), Some(m), Some(d)) = (num(0..4), num(5..7), num(8..10)) else {
        return false;
    };
    let (Ok(y), Ok(m), Ok(d)) = (y.parse::<u32>(), m.parse::<u32>(), d.parse::<u32>()) else {
        return false;
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let last = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    y >= 1 && (1..=last).contains(&d)
}

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

/// The payload of `rollup.publish` (Story 8.6, system only): the rollup of `date` was PUT to the
/// cloud. Upserts the `rollup_log` row of that day and writes nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RollupPublish {
    /// The business day, `yyyy-mm-dd`.
    pub date: String,
    /// The rollup's schema.
    pub schema: u32,
    /// When it was built (Hub clock, ms).
    #[ts(type = "number")]
    pub built_at: i64,
    /// The object key it was PUT to (`.../rollups/<date>.json`).
    pub key: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rollup_date_is_a_calendar_looking_date_not_latest() {
        assert!(is_rollup_date("2026-10-29"));
        assert!(!is_rollup_date("latest"));
        assert!(!is_rollup_date("2026-13-01"));
        assert!(!is_rollup_date("2026-02-31"));
        assert!(!is_rollup_date("2026-04-31"));
        assert!(!is_rollup_date("0000-01-01"));
        assert!(is_rollup_date("2028-02-29"));
        assert!(!is_rollup_date("2027-02-29"));
        assert!(!is_rollup_date("2026-1-001"));
        assert_eq!(rollup_object_key("2026-10-29"), "rollups/2026-10-29.json");
        assert_eq!(
            snapshot_object_key("2026-10-29"),
            "snapshots/2026-10-29.db.zst"
        );
    }

    #[test]
    fn the_emitted_schema_follows_the_pin_within_the_kept_writers() {
        // Newest writer 3: it keeps 2 and 3.
        assert_eq!(emitted_rollup_schema(0, 3), 2);
        assert_eq!(emitted_rollup_schema(2, 3), 2);
        assert_eq!(emitted_rollup_schema(3, 3), 3);
        assert_eq!(emitted_rollup_schema(9, 3), 3);
        // The first release has only v1.
        assert_eq!(emitted_rollup_schema(0, 1), 1);
        assert_eq!(emitted_rollup_schema(1, 1), 1);
        assert_eq!(emitted_rollup_schema(5, 1), 1);
        const { assert!(ROLLUP_MIN_SCHEMA <= ROLLUP_SCHEMA) };
    }

    #[test]
    fn the_cloud_accepts_the_current_and_the_previous_schema_only() {
        assert!(rollup_schema_accepted(3, 3));
        assert!(rollup_schema_accepted(2, 3));
        assert!(!rollup_schema_accepted(1, 3));
        assert!(!rollup_schema_accepted(4, 3));
        assert!(!rollup_schema_accepted(0, 1));
        assert!(rollup_schema_accepted(1, 1));
    }

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
