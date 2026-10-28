//! The Stock reports on the Hub's Reports screen (Story 5.31): what is on hand and what it is
//! worth, every stock movement of a range, waste by reason, and the lots that expire. Read-only
//! views built from stored rows (`GET /ui/reports/stock`, `/stock/moves`, `/stock/waste` and
//! `/stock/expiring`, permission `report.view`). All amounts are minor units, at cost.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The most movement lines one answer carries (newest first); `StockMoves.total` counts them all.
pub const STOCK_MOVES_LIMIT: usize = 1000;

/// `WasteReasonRow.reason`: an expired lot written off (`stock.write_off`, move reason `expired`).
pub const WASTE_EXPIRED: &str = "expired";

/// The waste reasons in the order the report lists them. Each is always present, even at zero.
pub const WASTE_REASONS: [&str; 5] = [
    "spoiled",
    "broken",
    "free",
    "cancelled_order",
    WASTE_EXPIRED,
];

/// One tracked product on hand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StockLevel {
    pub product_id: String,
    pub name: String,
    #[ts(type = "number")]
    pub qty: i64,
    #[ts(type = "number")]
    pub avg_cost_minor: i64,
    /// `qty` at the average cost.
    #[ts(type = "number")]
    pub value_minor: i64,
}

/// Current stock per tracked product, by name, and the total value (`GET /ui/reports/stock`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StockReport {
    pub items: Vec<StockLevel>,
    #[ts(type = "number")]
    pub total_value_minor: i64,
}

/// One `stock_moves` row. `qty` is signed (a stock-in is positive, a sale or waste negative) and
/// `value_minor` carries the same sign.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StockMoveLine {
    pub move_id: String,
    pub product_id: String,
    pub product: String,
    /// The move reason: `stock_in`, `stock_in_reverted`, `waste`, `waste_reverted`, `expired`,
    /// `count_adjust`, `sale`, `sale_reverted` or `return`.
    pub reason: String,
    /// The waste reason (`spoiled`, `broken`, `free`, `cancelled_order`) of a waste move.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub waste_reason: Option<String>,
    #[ts(type = "number")]
    pub qty: i64,
    #[ts(type = "number")]
    pub value_minor: i64,
    pub staff_id: String,
    /// Empty for a staff member no longer in the staff table.
    pub staff: String,
    #[ts(type = "number")]
    pub at: i64,
}

/// The movements of a range, newest first (`GET /ui/reports/stock/moves`), cut to
/// [`STOCK_MOVES_LIMIT`] lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StockMoves {
    pub from: String,
    pub to: String,
    pub moves: Vec<StockMoveLine>,
    /// How many movements matched, before the cut.
    #[ts(type = "number")]
    pub total: i64,
    pub truncated: bool,
}

/// Waste of one reason: quantity and value at cost, net of undone waste.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct WasteReasonRow {
    pub reason: String,
    #[ts(type = "number")]
    pub qty: i64,
    #[ts(type = "number")]
    pub value_minor: i64,
}

/// Waste of one product, with its split by reason (reasons with nothing are left out).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct WasteProductRow {
    pub product_id: String,
    pub product: String,
    #[ts(type = "number")]
    pub qty: i64,
    #[ts(type = "number")]
    pub value_minor: i64,
    pub reasons: Vec<WasteReasonRow>,
}

/// Waste for a range (`GET /ui/reports/stock/waste`): every reason in [`WASTE_REASONS`] order,
/// and the products with waste, largest value first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct WasteReport {
    pub from: String,
    pub to: String,
    pub by_reason: Vec<WasteReasonRow>,
    pub by_product: Vec<WasteProductRow>,
    #[ts(type = "number")]
    pub total_qty: i64,
    #[ts(type = "number")]
    pub total_value_minor: i64,
}
