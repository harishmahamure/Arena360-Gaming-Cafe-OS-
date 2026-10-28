use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const MAX_STOCK_IN_QTY: i64 = 1_000_000;
pub const MAX_STOCK_QTY: i64 = 100_000_000;
pub const MAX_SUPPLIER_CHARS: usize = 80;
pub const STOCK_UNDO_MS: i64 = 8_000;
pub const MOVE_STOCK_IN: &str = "stock_in";
pub const MOVE_STOCK_IN_REVERTED: &str = "stock_in_reverted";
pub const MOVE_WASTE: &str = "waste";
pub const MOVE_WASTE_REVERTED: &str = "waste_reverted";
pub const MOVE_EXPIRED: &str = "expired";

/// How many days before a lot's expiry date the Hub warns (0 to 30, default 3). Staff set it
/// through `settings.set`.
pub const KEY_STOCK_EXPIRY_WARN_DAYS: &str = "stock.expiry_warn_days";
pub const DEFAULT_STOCK_EXPIRY_WARN_DAYS: &str = "3";
pub const STOCK_EXPIRY_WARN_RANGE: (i64, i64) = (0, 30);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StockIn {
    pub product_id: String,
    #[ts(type = "number")]
    pub qty: i64,
    #[ts(type = "number")]
    pub unit_cost_minor: i64,
    #[serde(default)]
    pub supplier: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expiry: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StockInReverted {
    pub move_id: String,
}

/// The payload of `stock.waste`: `qty` of a tracked product written off. The waste reason
/// (`spoiled`, `broken`, `free`, `cancelled_order`) is the intent's `reason`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StockWaste {
    pub product_id: String,
    #[ts(type = "number")]
    pub qty: i64,
}

/// The payload of `stock.waste.reverted`: undo the waste move `move_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StockWasteReverted {
    pub move_id: String,
}

/// The payload of `stock.write_off`: write the whole remaining quantity of the expired lot
/// `lot_id` off the books (move reason `expired`, valued at the lot's cost).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StockWriteOff {
    pub lot_id: String,
}

/// The payload of `stock.expiry_check` (system only): raises the "Expiring soon" alert when a
/// lot is inside the warning window (once per business day) and clears it when none is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StockExpiryCheck {}

/// One lot on the Inventory → Expiring and Expired lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ExpiringLot {
    pub lot_id: String,
    pub product_id: String,
    pub product: String,
    #[ts(type = "number")]
    pub qty: i64,
    pub expiry: String,
    /// What the remaining quantity cost, in minor units.
    #[ts(type = "number")]
    pub value_minor: i64,
}

/// The two Inventory lists at one moment: the lots inside the warning window and the lots
/// already past their expiry date (both with quantity remaining), earliest expiry first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ExpiryView {
    pub today: String,
    #[ts(type = "number")]
    pub warn_days: i64,
    pub expiring: Vec<ExpiringLot>,
    pub expired: Vec<ExpiringLot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LotView {
    pub lot_id: String,
    pub expiry: Option<String>,
    pub received_at: i64,
    pub remaining: i64,
}

pub fn weighted_avg_cost(old_qty: i64, old_avg: i64, in_qty: i64, in_cost: i64) -> i64 {
    let total = i128::from(old_qty) + i128::from(in_qty);
    if total <= 0 {
        return in_cost;
    }
    let sum = i128::from(old_qty) * i128::from(old_avg) + i128::from(in_qty) * i128::from(in_cost);
    i64::try_from((2 * sum + total) / (2 * total)).unwrap_or(i64::MAX)
}

pub fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let num = |r: std::ops::Range<usize>| {
        s.get(r)
            .filter(|t| t.bytes().all(|c| c.is_ascii_digit()))
            .and_then(|t| t.parse::<u32>().ok())
    };
    let (Some(y), Some(m), Some(d)) = (num(0..4), num(5..7), num(8..10)) else {
        return false;
    };
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    y >= 1 && (1..=days).contains(&d)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `date` (`YYYY-MM-DD`) moved by `days`; `None` when `date` is not a real day or the result
/// leaves years 1 to 9999.
pub fn add_days(date: &str, days: i64) -> Option<String> {
    if !valid_date(date) {
        return None;
    }
    let n = |r: std::ops::Range<usize>| date.get(r)?.parse::<i64>().ok();
    let z = days_from_civil(n(0..4)?, n(5..7)?, n(8..10)?).checked_add(days)?;
    let (y, m, d) = civil_from_days(z);
    (1..=9999)
        .contains(&y)
        .then(|| format!("{y:04}-{m:02}-{d:02}"))
}

/// Whether a lot expiring on `expiry` is past its date on `today`: the date itself is still
/// sellable, the day after is not.
pub fn lot_expired(expiry: &str, today: &str) -> bool {
    expiry < today
}

/// Whether a lot expiring on `expiry` is expired, or expires within `warn_days` of `today`.
pub fn lot_in_window(expiry: &str, today: &str, warn_days: i64) -> bool {
    add_days(today, warn_days).is_some_and(|last| expiry <= last.as_str())
}

pub fn consume_plan(lots: &[LotView], qty: i64) -> Option<Vec<(String, i64)>> {
    if qty <= 0 {
        return None;
    }
    let mut live: Vec<&LotView> = lots.iter().filter(|l| l.remaining > 0).collect();
    live.sort_by(|a, b| {
        (a.expiry.is_none(), &a.expiry, a.received_at, &a.lot_id).cmp(&(
            b.expiry.is_none(),
            &b.expiry,
            b.received_at,
            &b.lot_id,
        ))
    });
    let mut left = qty;
    let mut plan = Vec::new();
    for l in live {
        if left == 0 {
            break;
        }
        let take = l.remaining.min(left);
        plan.push((l.lot_id.clone(), take));
        left -= take;
    }
    (left == 0).then_some(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lot(id: &str, expiry: Option<&str>, at: i64, remaining: i64) -> LotView {
        LotView {
            lot_id: id.into(),
            expiry: expiry.map(Into::into),
            received_at: at,
            remaining,
        }
    }

    #[test]
    fn the_golden_average_is_23_rupees() {
        assert_eq!(weighted_avg_cost(10, 2000, 30, 2400), 2300);
    }

    #[test]
    fn days_move_across_months_years_and_leap_days() {
        assert_eq!(add_days("2026-10-28", 3).as_deref(), Some("2026-10-31"));
        assert_eq!(add_days("2026-10-28", 4).as_deref(), Some("2026-11-01"));
        assert_eq!(add_days("2026-12-30", 30).as_deref(), Some("2027-01-29"));
        assert_eq!(add_days("2028-02-28", 1).as_deref(), Some("2028-02-29"));
        assert_eq!(add_days("2027-02-28", 1).as_deref(), Some("2027-03-01"));
        assert_eq!(add_days("2026-03-01", -1).as_deref(), Some("2026-02-28"));
        assert_eq!(add_days("2026-10-28", 0).as_deref(), Some("2026-10-28"));
        assert_eq!(add_days("2026-02-30", 1), None);
        assert_eq!(add_days("9999-12-31", 1), None);
        assert_eq!(add_days("0001-01-01", -1), None);
    }

    #[test]
    fn the_expiry_date_itself_is_sellable_and_inside_the_window() {
        assert!(!lot_expired("2026-10-28", "2026-10-28"));
        assert!(lot_expired("2026-10-27", "2026-10-28"));
        assert!(lot_in_window("2026-10-31", "2026-10-28", 3));
        assert!(!lot_in_window("2026-11-01", "2026-10-28", 3));
        assert!(lot_in_window("2026-10-01", "2026-10-28", 3));
        assert!(lot_in_window("2026-10-28", "2026-10-28", 0));
        assert!(!lot_in_window("2026-10-29", "2026-10-28", 0));
    }

    #[test]
    fn the_average_rounds_half_up_in_integers() {
        assert_eq!(weighted_avg_cost(1, 1, 1, 2), 2);
        assert_eq!(weighted_avg_cost(1, 2, 1, 1), 2);
        assert_eq!(weighted_avg_cost(2, 1, 1, 2), 1);
        assert_eq!(weighted_avg_cost(1, 1, 2, 2), 2);
        assert_eq!(weighted_avg_cost(3, 10, 1, 11), 10);
        assert_eq!(weighted_avg_cost(1, 10, 3, 11), 11);
        assert_eq!(weighted_avg_cost(1, 0, 1, 1), 1);
        assert_eq!(weighted_avg_cost(1, 0, 3, 1), 1);
    }

    #[test]
    fn an_empty_shelf_takes_the_new_cost() {
        assert_eq!(weighted_avg_cost(0, 900, 4, 100), 100);
        assert_eq!(weighted_avg_cost(0, 0, 1, 0), 0);
        assert_eq!(weighted_avg_cost(5, 0, 5, 0), 0);
        assert_eq!(weighted_avg_cost(3, 7, 2, 0), 4);
    }

    #[test]
    fn the_average_survives_the_extremes() {
        let top = 1_000_000_000_000;
        assert_eq!(weighted_avg_cost(MAX_STOCK_QTY - 1, top, 1, top), top);
        assert_eq!(
            weighted_avg_cost(MAX_STOCK_QTY - MAX_STOCK_IN_QTY, top, MAX_STOCK_IN_QTY, 0),
            top - top * MAX_STOCK_IN_QTY / MAX_STOCK_QTY
        );
    }

    #[test]
    fn dates_are_real_calendar_days() {
        for ok in [
            "2026-10-25",
            "2028-02-29",
            "2000-02-29",
            "0001-01-01",
            "2026-12-31",
        ] {
            assert!(valid_date(ok), "{ok}");
        }
        for bad in [
            "",
            "2026-02-30",
            "2027-02-29",
            "1900-02-29",
            "2026-13-01",
            "2026-00-10",
            "2026-10-00",
            "2026-10-32",
            "26-10-25",
            "2026/10/25",
            "2026-1-25",
            "2026-10-2x",
            "+026-10-25",
            "2026-10-25 ",
            "0000-01-01",
            "２０２６-10-25",
        ] {
            assert!(!valid_date(bad), "{bad}");
        }
    }

    #[test]
    fn decrements_take_the_earliest_expiry_first_and_no_expiry_last() {
        let lots = [
            lot("C", None, 1, 10),
            lot("B", Some("2027-02-01"), 2, 5),
            lot("A", Some("2027-01-01"), 3, 3),
        ];
        assert_eq!(
            consume_plan(&lots, 7),
            Some(vec![("A".into(), 3), ("B".into(), 4)])
        );
        assert_eq!(consume_plan(&lots, 18).unwrap().len(), 3);
        assert_eq!(consume_plan(&lots, 19), None);
        assert_eq!(consume_plan(&lots, 0), None);
        assert_eq!(consume_plan(&lots, -1), None);
        assert_eq!(consume_plan(&[], 1), None);
    }

    #[test]
    fn ties_break_by_receipt_then_id_and_empty_lots_are_skipped() {
        let lots = [
            lot("Z", Some("2027-01-01"), 1, 2),
            lot("Y", Some("2027-01-01"), 1, 2),
            lot("X", Some("2027-01-01"), 0, 0),
            lot("W", None, 5, 1),
            lot("V", None, 4, 1),
        ];
        assert_eq!(
            consume_plan(&lots, 6),
            Some(vec![
                ("Y".into(), 2),
                ("Z".into(), 2),
                ("V".into(), 1),
                ("W".into(), 1)
            ])
        );
    }
}
