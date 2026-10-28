use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The largest float a shift can open with (minor units).
pub const MAX_FLOAT_MINOR: i64 = 100_000_000;

/// `cash_events.kind`: the float counted into the drawer when a shift opens.
pub const CASH_FLOAT_IN: &str = "float_in";
/// `cash_events.kind`: cash kept from a payment (cash tendered minus change).
pub const CASH_SALE: &str = "sale";
/// `cash_events.kind`: the drawer was kicked open for a sale (amount 0; `order_id` names it).
pub const CASH_DRAWER_OPEN: &str = "drawer_open";
/// `cash_events.kind`: the compensating row an Undo writes for a `sale`.
pub const CASH_SALE_REVERTED: &str = "sale_reverted";
/// `cash_events.kind`: cash handed back for a refund (a negative amount; `order_id` names the
/// invoice refunded, Story 5.24).
pub const CASH_REFUND: &str = "refund";

/// The payload of `shift.open`: the float counted into the drawer. The carried amount (the
/// last shift's opening float) is read on the Hub and recorded beside it, so the audit shows
/// both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ShiftOpen {
    #[ts(type = "number")]
    pub float_minor: i64,
}

/// The open shift, as the Hub UI shows it. Never carries the expected drawer amount: the
/// count stays blind until close.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SnapshotShift {
    pub id: String,
    pub staff_id: String,
    pub staff_name: String,
    #[ts(type = "number")]
    pub opened_at: i64,
    #[ts(type = "number")]
    pub float_minor: i64,
    /// Cash events written against the shift so far (the float counts as one).
    #[ts(type = "number")]
    pub events: i64,
}

/// What the Shift screen and the settle bar need: the open shift, if any, and the float to
/// prefill when opening one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ShiftState {
    #[serde(default)]
    #[ts(optional = nullable)]
    pub open: Option<SnapshotShift>,
    /// The last shift's opening float (0 before the first shift).
    #[ts(type = "number")]
    pub carried_minor: i64,
}
