use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{IMAGE_PREFIX, MAX_IMAGE_BYTES};

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

/// `cash_events.kind`: cash moved from the drawer to the safe (a negative amount, Story 5.25).
pub const CASH_DEPOSIT: &str = "deposit";
/// `cash_events.kind`: an expense; only one paid from the drawer has a (negative) amount.
pub const CASH_EXPENSE: &str = "expense";

/// The most a deposit or an expense moves in one go (minor units).
pub const MAX_CASH_MOVE_MINOR: i64 = 100_000_000;
/// The longest slip reference a deposit takes, in characters.
pub const DEPOSIT_REFERENCE_MAX_CHARS: usize = 40;
/// The longest expense note, in characters.
pub const EXPENSE_NOTE_MAX_CHARS: usize = 140;

/// What an expense was for, as the Expense form offers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ExpenseCategory {
    Supplies,
    Utilities,
    Repairs,
    FoodStock,
    Other,
}

impl ExpenseCategory {
    /// Every category, in the order the form offers them.
    pub const ALL: [ExpenseCategory; 5] = [
        Self::Supplies,
        Self::Utilities,
        Self::Repairs,
        Self::FoodStock,
        Self::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supplies => "supplies",
            Self::Utilities => "utilities",
            Self::Repairs => "repairs",
            Self::FoodStock => "food_stock",
            Self::Other => "other",
        }
    }
}

/// Where the money for an expense came from. Only `drawer` lowers the expected drawer cash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ExpensePaidFrom {
    Drawer,
    Owner,
}

impl ExpensePaidFrom {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Drawer => "drawer",
            Self::Owner => "owner",
        }
    }
}

/// The payload of `cash.deposit` (permission `cash.manage`; Counter elevates with the inline
/// PIN): cash taken from the open shift's drawer to the safe, with the slip reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CashDeposit {
    #[ts(type = "number")]
    pub amount_minor: i64,
    pub reference: String,
}

/// The payload of `expense.create` (permission `cash.manage`): an expense against the open
/// shift. The receipt photo is a JPEG data URI of at most 200 KB (the Hub resizes it first).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ExpenseCreate {
    pub category: ExpenseCategory,
    #[ts(type = "number")]
    pub amount_minor: i64,
    pub paid_from: ExpensePaidFrom,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub receipt_photo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<String>,
}

/// What is wrong with a receipt photo, if anything: it is a JPEG data URI of at most 200 KB.
pub fn receipt_photo_problem(photo: &str) -> Option<&'static str> {
    let Some(data) = photo.strip_prefix(IMAGE_PREFIX) else {
        return Some("the receipt photo must be a JPEG");
    };
    if data.len() > MAX_IMAGE_BYTES * 4 / 3 + 4 {
        return Some("the receipt photo is over 200 KB");
    }
    let Ok(bytes) = STANDARD.decode(data) else {
        return Some("the receipt photo is not valid");
    };
    if bytes.len() > MAX_IMAGE_BYTES {
        return Some("the receipt photo is over 200 KB");
    }
    if !bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("the receipt photo must be a JPEG");
    }
    None
}

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
