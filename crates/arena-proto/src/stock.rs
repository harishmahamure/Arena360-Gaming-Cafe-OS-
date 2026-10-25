use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const MAX_STOCK_IN_QTY: i64 = 1_000_000;
pub const MAX_STOCK_QTY: i64 = 100_000_000;
pub const MAX_SUPPLIER_CHARS: usize = 80;
pub const STOCK_UNDO_MS: i64 = 8_000;
pub const MOVE_STOCK_IN: &str = "stock_in";
pub const MOVE_STOCK_IN_REVERTED: &str = "stock_in_reverted";

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
