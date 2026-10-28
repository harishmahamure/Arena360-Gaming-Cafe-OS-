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
/// `stock_moves.reason`: stock a cancelled, not-yet-served line gives back (order mode).
pub const MOVE_RETURN: &str = "return";

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
    /// The receipt choice (Story 5.17); `print` issues the invoice number in this transaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub receipt: Option<crate::ReceiptChoice>,
    /// The business the invoice is made out to (Story 5.20); a GST branch only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub business: Option<crate::BusinessBuyer>,
    /// A coupon code to apply to the bill (Story 5.21).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub coupon: Option<String>,
    /// A discount staff give by hand, applied after the coupon (Story 5.22). Needs `discount.manual`
    /// on the actor or on the approving manager.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub discount: Option<crate::ManualDiscount>,
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
    /// What the latest print of this sale came to (`queued`, `done`, `failed`); `null` when no
    /// receipt was asked for. A numbered sale can always be reprinted (Story 5.17).
    #[serde(default)]
    #[ts(optional = nullable)]
    pub print_state: Option<String>,
    /// What the WhatsApp receipt of this sale came to (`queued`, `sent`, `failed`, `skipped`);
    /// `null` when none was asked for (Story 5.19).
    #[serde(default)]
    #[ts(optional = nullable)]
    pub whatsapp_state: Option<String>,
}

/// `GET /ui/pos/sales`: the business day's counter sales, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PosSalesView {
    pub business_day: String,
    pub sales: Vec<PosSaleView>,
}

/// The payload of `order.place` (Story 5.12): products added to a running session's bill order.
/// The intent id is the placement's id. A short bundle part sells with `stock_override` (needs
/// `stock.override`), as in `pos.sale`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OrderPlace {
    pub session_id: String,
    pub lines: Vec<PosSaleLine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub stock_override: Option<bool>,
}

/// The payload of `order.place.reverted`: Undo of one placement within its window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OrderPlaceReverted {
    pub order_id: String,
    pub placement_id: String,
}

/// What a placement returns to the Hub, for the Undo toast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlaceOutcome {
    pub order_id: String,
    #[ts(type = "number")]
    pub order_rev: i64,
    pub placement_id: String,
    #[ts(type = "number")]
    pub placed_at: i64,
    #[ts(type = "number")]
    pub undo_until: i64,
    #[ts(type = "number")]
    pub total_minor: i64,
}

/// A placed order has been added to a running session's bill (the `order.placed` UI event).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OrderPlacedEvent {
    pub session_id: String,
    pub order_id: String,
}

/// The payload of `invoice.issue` (Story 5.14): number a settled sale. The system intent the Hub
/// applies once the Undo window of a settled order has closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct InvoiceIssue {
    pub order_id: String,
}

/// The payload of `order.line_served` (Story 5.13): a placed kitchen line is marked served. Its
/// open stock holds are taken (a `sale` move per held item) and closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OrderLineServed {
    pub line_id: String,
}

/// The payload of `order.line_cancel`: an unserved placed line is cancelled. `made` answers "Was
/// it already made?": `false` releases the hold (or returns the stock), `true` records waste
/// `cancelled_order` at cost. The intent's `reason` (`changed_mind`, `mistake`, `other`) is
/// required.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OrderLineCancel {
    pub line_id: String,
    pub made: bool,
}

/// A line was served or cancelled on a session bill (the `order.line_changed` UI event).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OrderLineEvent {
    pub line_id: String,
    /// `true` when the line was cancelled (and left the bill), `false` when it was served.
    pub cancelled: bool,
}
