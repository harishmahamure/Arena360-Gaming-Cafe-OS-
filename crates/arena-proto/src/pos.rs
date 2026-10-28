use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::PaymentRow;

/// The most lines one counter sale takes.
pub const MAX_SALE_LINES: usize = 100;
/// The most units of one product on a sale line.
pub const MAX_SALE_QTY: i64 = 999;

/// `stock_moves.reason`: stock taken by a counter sale.
pub const MOVE_SALE: &str = "sale";
/// `stock_moves.reason`: the compensating move an Undo writes for a `sale`.
pub const MOVE_SALE_REVERTED: &str = "sale_reverted";
/// `stock_moves.reason`: stock raised to the counted quantity (a sale override).
pub const MOVE_COUNT_ADJUST: &str = "count_adjust";
/// The note a `count_adjust` written by a sale override carries.
pub const NOTE_SALE_OVERRIDE: &str = "Override at sale";

/// One line of a counter sale: `qty` of a product.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PosSaleLine {
    pub product_id: String,
    #[ts(type = "number")]
    pub qty: i64,
}

/// The payload of `pos.sale`: a walk-up sale settled in the same intent. The payments sum to the
/// total the Hub computes from the lines (tax included). A `player_id` attaches the sale to a
/// profile, which allows wallet and credit rows. `stock_override` lets a short bundle sell by
/// raising each short component to the quantity needed (needs `stock.override`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PosSale {
    pub lines: Vec<PosSaleLine>,
    pub payments: Vec<PaymentRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub player_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub credit_override: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub stock_override: Option<bool>,
}

/// The payload of `pos.sale.reverted`: Undo of a counter sale within its window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PosSaleReverted {
    pub order_id: String,
}

/// One line of a settled counter sale, as Recent sales shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PosSaleLineView {
    pub product_id: String,
    pub name: String,
    #[ts(type = "number")]
    pub qty: i64,
    #[ts(type = "number")]
    pub unit_price_minor: i64,
    #[ts(type = "number")]
    pub rate_bp: i64,
    #[ts(type = "number")]
    pub amount_minor: i64,
}

/// One payment row of a settled counter sale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PosSalePaymentView {
    pub method: String,
    #[ts(type = "number")]
    pub amount_minor: i64,
}

/// A settled counter sale, read-only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PosSaleView {
    pub order_id: String,
    #[ts(type = "number")]
    pub settled_at: i64,
    pub staff: String,
    /// The attached player's username, when the sale had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub player: Option<String>,
    pub lines: Vec<PosSaleLineView>,
    pub payments: Vec<PosSalePaymentView>,
    #[ts(type = "number")]
    pub subtotal_minor: i64,
    #[ts(type = "number")]
    pub tax_minor: i64,
    #[ts(type = "number")]
    pub total_minor: i64,
    /// The invoice number; `null` until Story 5.14 issues one.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub invoice_no: Option<String>,
}

/// `GET /ui/pos/sales`: the business day's counter sales, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PosSalesView {
    pub business_day: String,
    pub sales: Vec<PosSaleView>,
}
