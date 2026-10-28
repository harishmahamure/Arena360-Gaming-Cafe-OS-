//! The Stock reports' figures (Story 5.31), folded from the rows `arena-store` reads. Money is
//! the stored cost of each movement; nothing is recomputed from prices. Reading never goes
//! through `apply()`.

use std::collections::BTreeMap;

use arena_proto::{
    STOCK_MOVES_LIMIT, StockLevel, StockMoveLine, StockMoves, StockReport, WASTE_EXPIRED,
    WASTE_REASONS, WasteProductRow, WasteReasonRow, WasteReport,
};
use arena_store::{StockLevelRow, StockMoveReportRow, StockMovesRows};

/// Current stock per tracked product and the total value, each at the average cost.
pub fn stock_report(levels: &[StockLevelRow]) -> StockReport {
    let items: Vec<StockLevel> = levels
        .iter()
        .map(|l| StockLevel {
            product_id: l.product_id.clone(),
            name: l.name.clone(),
            qty: l.qty,
            avg_cost_minor: l.avg_cost_minor,
            value_minor: l.qty.saturating_mul(l.avg_cost_minor),
        })
        .collect();
    let total_value_minor = items.iter().map(|i| i.value_minor).sum();
    StockReport {
        items,
        total_value_minor,
    }
}

/// What a move was worth at cost, with the sign of its quantity. A stock-in stores no value, so
/// it is its quantity at the unit cost it came in at.
fn move_value(m: &StockMoveReportRow) -> i64 {
    let magnitude = if m.value_minor > 0 {
        m.value_minor
    } else {
        m.delta.abs().saturating_mul(m.unit_cost_minor)
    };
    magnitude.saturating_mul(m.delta.signum())
}

fn in_range(at: i64, from: &str, to: &str, day_of: &dyn Fn(i64) -> String) -> bool {
    let day = day_of(at);
    day.as_str() >= from && day.as_str() <= to
}

fn staff_name(rows: &StockMovesRows, id: &str) -> String {
    rows.staff
        .iter()
        .find(|(staff_id, _)| staff_id == id)
        .map(|(_, name)| name.clone())
        .unwrap_or_default()
}

/// The movements whose local business day (`day_of`) is from `from` to `to`, newest first, cut to
/// [`STOCK_MOVES_LIMIT`] lines.
pub fn stock_moves(
    rows: &StockMovesRows,
    from: &str,
    to: &str,
    day_of: &dyn Fn(i64) -> String,
) -> StockMoves {
    let mut kept: Vec<&StockMoveReportRow> = rows
        .moves
        .iter()
        .filter(|m| in_range(m.at, from, to, day_of))
        .collect();
    kept.sort_by_key(|m| std::cmp::Reverse(m.at));
    let total = i64::try_from(kept.len()).unwrap_or(i64::MAX);
    let truncated = kept.len() > STOCK_MOVES_LIMIT;
    kept.truncate(STOCK_MOVES_LIMIT);
    StockMoves {
        from: from.to_owned(),
        to: to.to_owned(),
        moves: kept
            .into_iter()
            .map(|m| StockMoveLine {
                move_id: m.move_id.clone(),
                product_id: m.product_id.clone(),
                product: m.product.clone(),
                reason: m.reason.clone(),
                waste_reason: m.waste_reason.clone(),
                qty: m.delta,
                value_minor: move_value(m),
                staff_id: m.staff_id.clone(),
                staff: staff_name(rows, &m.staff_id),
                at: m.at,
            })
            .collect(),
        total,
        truncated,
    }
}

/// Waste by reason and by product for the business days `from` to `to`, at cost. `rows` holds the
/// waste moves and expired write-offs; an undone waste counts negatively, so it nets out.
pub fn waste_report(
    rows: &StockMovesRows,
    from: &str,
    to: &str,
    day_of: &dyn Fn(i64) -> String,
) -> WasteReport {
    let mut by_reason: Vec<WasteReasonRow> = WASTE_REASONS
        .iter()
        .map(|r| WasteReasonRow {
            reason: (*r).to_owned(),
            qty: 0,
            value_minor: 0,
        })
        .collect();
    let mut by_product = BTreeMap::<String, (String, Vec<WasteReasonRow>)>::new();
    for m in rows
        .moves
        .iter()
        .filter(|m| in_range(m.at, from, to, day_of))
    {
        let reason = if m.reason == WASTE_EXPIRED {
            WASTE_EXPIRED.to_owned()
        } else if let Some(reason) = &m.waste_reason {
            reason.clone()
        } else {
            continue;
        };
        let (qty, value) = (-m.delta, -move_value(m));
        add(&mut by_reason, &reason, qty, value);
        let entry = by_product
            .entry(m.product_id.clone())
            .or_insert_with(|| (m.product.clone(), Vec::new()));
        add(&mut entry.1, &reason, qty, value);
    }
    let mut products: Vec<WasteProductRow> = by_product
        .into_iter()
        .map(|(product_id, (product, mut reasons))| {
            reasons.retain(|r| r.qty != 0 || r.value_minor != 0);
            WasteProductRow {
                product_id,
                product,
                qty: reasons.iter().map(|r| r.qty).sum(),
                value_minor: reasons.iter().map(|r| r.value_minor).sum(),
                reasons,
            }
        })
        .filter(|p| !p.reasons.is_empty())
        .collect();
    products.sort_by(|a, b| {
        b.value_minor
            .cmp(&a.value_minor)
            .then_with(|| a.product.to_lowercase().cmp(&b.product.to_lowercase()))
            .then_with(|| a.product_id.cmp(&b.product_id))
    });
    WasteReport {
        from: from.to_owned(),
        to: to.to_owned(),
        total_qty: by_reason.iter().map(|r| r.qty).sum(),
        total_value_minor: by_reason.iter().map(|r| r.value_minor).sum(),
        by_reason,
        by_product: products,
    }
}

/// Adds to the row of `reason`, appending a row for a reason the report does not know.
fn add(rows: &mut Vec<WasteReasonRow>, reason: &str, qty: i64, value_minor: i64) {
    if let Some(row) = rows.iter_mut().find(|r| r.reason == reason) {
        row.qty += qty;
        row.value_minor += value_minor;
    } else {
        rows.push(WasteReasonRow {
            reason: reason.to_owned(),
            qty,
            value_minor,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One business day per 1,000 ms, from 2026-10-10.
    fn day_of(ts_ms: i64) -> String {
        format!("2026-10-{:02}", 10 + ts_ms / 1000)
    }

    struct Mv<'a> {
        id: &'a str,
        product: &'a str,
        reason: &'a str,
        waste: Option<&'a str>,
        delta: i64,
        unit: i64,
        value: i64,
        at: i64,
    }

    fn mv(m: Mv<'_>) -> StockMoveReportRow {
        StockMoveReportRow {
            move_id: m.id.into(),
            product_id: m.product.to_lowercase(),
            product: m.product.into(),
            reason: m.reason.into(),
            waste_reason: m.waste.map(Into::into),
            delta: m.delta,
            unit_cost_minor: m.unit,
            value_minor: m.value,
            staff_id: "S1".into(),
            at: m.at,
        }
    }

    /// `(id, product, reason, waste reason, delta, unit cost, stored value, at)`.
    type Spec<'a> = (
        &'a str,
        &'a str,
        &'a str,
        Option<&'a str>,
        i64,
        i64,
        i64,
        i64,
    );

    fn rows(specs: &[Spec<'_>]) -> StockMovesRows {
        StockMovesRows {
            moves: specs
                .iter()
                .map(|&(id, product, reason, waste, delta, unit, value, at)| {
                    mv(Mv {
                        id,
                        product,
                        reason,
                        waste,
                        delta,
                        unit,
                        value,
                        at,
                    })
                })
                .collect(),
            staff: vec![("S1".into(), "Sita".into())],
        }
    }

    #[test]
    fn stock_is_valued_at_the_average_cost_and_totalled() {
        let levels = [
            StockLevelRow {
                product_id: "a".into(),
                name: "Cola".into(),
                qty: 10,
                avg_cost_minor: 20,
            },
            StockLevelRow {
                product_id: "b".into(),
                name: "Gum".into(),
                qty: 0,
                avg_cost_minor: 0,
            },
        ];
        let report = stock_report(&levels);
        assert_eq!(report.items[0].value_minor, 200);
        assert_eq!(report.items[1].value_minor, 0);
        assert_eq!(report.total_value_minor, 200);
        assert_eq!(stock_report(&[]).total_value_minor, 0);
    }

    #[test]
    fn movements_are_signed_valued_at_cost_and_kept_to_the_range() {
        let r = rows(&[
            ("M1", "Cola", "stock_in", None, 10, 20, 0, 0),
            ("M2", "Cola", "sale", None, -3, 20, 60, 1_500),
            (
                "M3",
                "Cola",
                "waste_reverted",
                Some("spoiled"),
                1,
                20,
                20,
                2_500,
            ),
            ("M4", "Cola", "sale", None, -1, 20, 20, 9_500),
        ]);
        let out = stock_moves(&r, "2026-10-10", "2026-10-12", &day_of);
        let seen: Vec<_> = out
            .moves
            .iter()
            .map(|m| (m.move_id.as_str(), m.qty, m.value_minor))
            .collect();
        assert_eq!(seen, [("M3", 1, 20), ("M2", -3, -60), ("M1", 10, 200)]);
        assert_eq!(out.moves[0].staff, "Sita");
        assert_eq!((out.total, out.truncated), (3, false));
    }

    #[test]
    fn a_long_list_is_cut_and_says_so() {
        let n = STOCK_MOVES_LIMIT + 5;
        let ids: Vec<String> = (0..n).map(|i| format!("M{i}")).collect();
        let specs: Vec<Spec<'_>> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let at = i64::try_from(i % 900).unwrap();
                (id.as_str(), "Cola", "sale", None, -1, 20, 20, at)
            })
            .collect();
        let out = stock_moves(&rows(&specs), "2026-10-10", "2026-10-10", &day_of);
        assert_eq!(out.moves.len(), STOCK_MOVES_LIMIT);
        assert_eq!(out.total, i64::try_from(n).unwrap());
        assert!(out.truncated);
    }

    #[test]
    fn waste_totals_by_reason_and_product_net_of_undone_waste() {
        let r = rows(&[
            ("W1", "Cola", "waste", Some("spoiled"), -4, 20, 80, 100),
            (
                "W2",
                "Cola",
                "waste_reverted",
                Some("spoiled"),
                1,
                20,
                20,
                200,
            ),
            ("W3", "Cola", "waste", Some("broken"), -1, 20, 20, 300),
            ("W4", "Apple", "waste", Some("free"), -2, 10, 20, 400),
            ("W5", "Apple", "expired", None, -5, 10, 50, 500),
            ("W6", "Apple", "waste", Some("spoiled"), -1, 10, 10, 5_000),
            ("W7", "Gum", "waste", Some("broken"), -1, 5, 5, 600),
            ("W8", "Gum", "waste_reverted", Some("broken"), 1, 5, 5, 700),
        ]);
        let out = waste_report(&r, "2026-10-10", "2026-10-10", &day_of);
        let by_reason: Vec<_> = out
            .by_reason
            .iter()
            .map(|r| (r.reason.as_str(), r.qty, r.value_minor))
            .collect();
        assert_eq!(
            by_reason,
            [
                ("spoiled", 3, 60),
                ("broken", 1, 20),
                ("free", 2, 20),
                ("cancelled_order", 0, 0),
                ("expired", 5, 50),
            ]
        );
        assert_eq!((out.total_qty, out.total_value_minor), (11, 150));
        // Gum nets to nothing and is left out; Cola is worth 80 - 20 + 20, Apple 70.
        let products: Vec<_> = out
            .by_product
            .iter()
            .map(|p| (p.product.as_str(), p.qty, p.value_minor))
            .collect();
        assert_eq!(products, [("Cola", 4, 80), ("Apple", 7, 70)]);
        assert_eq!(out.by_product[1].reasons.len(), 2);
    }

    #[test]
    fn no_waste_lists_every_reason_at_zero() {
        let out = waste_report(&rows(&[]), "2026-10-10", "2026-10-11", &day_of);
        assert_eq!(out.by_reason.len(), WASTE_REASONS.len());
        assert!(out.by_product.is_empty());
        assert_eq!((out.total_qty, out.total_value_minor), (0, 0));
    }
}
