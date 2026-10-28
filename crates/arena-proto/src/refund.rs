//! Refunds with credit notes (Story 5.24): the payload of `refund.create` and the views the Hub's
//! Refund screen reads (`GET /ui/refund/invoice`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{PaymentMethod, ReceiptChoice};

/// The longest note an `other` refund reason takes, in characters.
pub const REFUND_NOTE_MAX_CHARS: usize = 140;

/// The most lines one refund names.
pub const MAX_REFUND_LINES: usize = 50;

/// One line of an invoice to refund, with how many of its units. A line without a quantity (play
/// time, a plan) is refunded whole: its `qty` is 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RefundLineReq {
    pub line_id: String,
    #[ts(type = "number")]
    pub qty: i64,
    /// Put the units back into stock (a stock-tracked product line only), as a `return` move.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub return_to_stock: Option<bool>,
}

/// The payload of `refund.create`: selected lines of a settled invoice are refunded against it.
/// The intent's `reason` (`mistake`, `changed_mind`, `hardware_issue`, `other`) is required, with
/// a `note` of 1 to [`REFUND_NOTE_MAX_CHARS`] plain characters for `other`. The approver is the
/// actor, or the manager whose PIN elevated the intent (permission `refund.issue`).
///
/// `method` is how the money goes back (`cash` needs an open shift, `upi` the last 4 digits and
/// `card` the terminal reference in `reference`, `wallet` a profiled player); it is not needed when
/// a credit portion of the original absorbs the whole refund.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RefundCreate {
    pub invoice_no: String,
    pub lines: Vec<RefundLineReq>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub method: Option<PaymentMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<String>,
    /// `print` queues the credit note on the printer in the same transaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub receipt: Option<ReceiptChoice>,
}

/// A line of an invoice, with what can still be refunded of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RefundLineView {
    pub line_id: String,
    /// `time` (play time), `product` or `plan`.
    pub kind: String,
    pub label: String,
    #[ts(type = "number")]
    pub qty: i64,
    #[ts(type = "number")]
    pub refunded_qty: i64,
    #[ts(type = "number")]
    pub amount_minor: i64,
    #[ts(type = "number")]
    pub refunded_minor: i64,
    /// Whether the product line tracks stock, so "Return to stock" applies.
    pub tracked: bool,
    /// Why the line cannot be refunded (`plan_used`, `plan_not_refundable`), when it cannot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub refusal: Option<String>,
}

/// A credit note already issued against an invoice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RefundSummary {
    pub refund_id: String,
    pub credit_note_no: String,
    #[ts(type = "number")]
    pub amount_minor: i64,
    #[ts(type = "number")]
    pub created_at: i64,
    pub reason: String,
}

/// An invoice opened on the Refund screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RefundInvoice {
    pub order_id: String,
    pub invoice_no: String,
    #[ts(type = "number")]
    pub settled_at: i64,
    #[ts(type = "number")]
    pub total_minor: i64,
    /// Money taken on the invoice (all methods).
    #[ts(type = "number")]
    pub paid_minor: i64,
    /// Refunded so far, credit notes together.
    #[ts(type = "number")]
    pub refunded_minor: i64,
    /// The player the invoice was made out to, when there is one (so `wallet` can be offered).
    pub has_player: bool,
    pub lines: Vec<RefundLineView>,
    pub refunds: Vec<RefundSummary>,
}
