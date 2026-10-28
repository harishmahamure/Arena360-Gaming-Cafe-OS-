//! Shifts and the cash ledger (Story 5.10). The branch has one drawer, so one open shift at a
//! time. `shift.open` records the float staff confirmed beside the one the screen offered; every
//! cash payment row written afterwards adds a `sale` cash event against the open shift, and an
//! Undo cancels it with a compensating `sale_reverted` row. Without an open shift a cash row is
//! refused `SHIFT_NOT_OPEN`; UPI, card, wallet and credit never need one.

use arena_proto::{
    ActorKind, AppError, CASH_DEPOSIT, CASH_EXPENSE, CASH_FLOAT_IN, CASH_SALE, CASH_SALE_REVERTED,
    CashDeposit, DEPOSIT_REFERENCE_MAX_CHARS, EXPENSE_NOTE_MAX_CHARS, ErrorCode, ExpenseCreate,
    ExpensePaidFrom, Intent, MAX_CASH_MOVE_MINOR, MAX_FLOAT_MINOR, PaymentMethod, PaymentRow,
    Permission, ShiftClose, ShiftOpen, VARIANCE_NOTE_MAX_CHARS, VarianceReason,
    receipt_photo_problem,
};
use arena_store::{
    ChangeOp, NewCashEvent, NewCashMove, NewShift, RowKey, ShiftClosing, ShiftRow, StoreTx,
    close_shift, expected_cash, insert_cash_event, insert_cash_move, insert_shift,
    last_opening_float, open_sales_of_order, open_shift, variance_threshold,
};

use crate::rbac;

use super::mutate::Written;

#[derive(Default)]
pub(crate) struct ShiftTarget {
    bootstrapped: bool,
    open: Option<ShiftRow>,
    carried_minor: i64,
    /// The cash the open shift's drawer should hold (never sent to a client).
    expected_minor: i64,
    /// The branch's variance threshold (Story 5.26).
    threshold_minor: i64,
    /// Whether whoever approves the intent (the elevating member, else the actor) holds
    /// `shift.override_variance`.
    can_override: bool,
}

pub(super) fn load(tx: &StoreTx<'_>, intent: &Intent) -> Result<ShiftTarget, AppError> {
    let open = open_shift(tx)?;
    let expected_minor = match &open {
        Some(s) => expected_cash(tx, &s.id)?,
        None => 0,
    };
    Ok(ShiftTarget {
        bootstrapped: arena_store::branch_exists(tx)?,
        open,
        carried_minor: last_opening_float(tx)?,
        expected_minor,
        threshold_minor: variance_threshold(tx)?,
        can_override: can_override(tx, intent)?,
    })
}

/// Whether the approver of `intent` holds `shift.override_variance`: the owner, the elevating
/// member, or else the acting member.
fn can_override(tx: &StoreTx<'_>, intent: &Intent) -> Result<bool, AppError> {
    if !matches!(intent.payload, arena_proto::IntentPayload::ShiftClose(_)) {
        return Ok(false);
    }
    if intent.actor.kind == ActorKind::Owner {
        return Ok(true);
    }
    let holder = intent
        .elevated_by
        .as_ref()
        .map_or(intent.actor.id.as_str(), |e| e.id.as_str());
    rbac::staff_allows(tx, holder, Permission::ShiftOverrideVariance)
}

pub(super) fn validate_open(
    intent: &Intent,
    s: &ShiftOpen,
    t: &ShiftTarget,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(AppError::with_field(
            ErrorCode::ValidationFailed,
            "base_rev",
            "shift.open takes no base_rev",
        ));
    }
    if !(0..=MAX_FLOAT_MINOR).contains(&s.float_minor) {
        return Err(AppError::with_field(
            ErrorCode::ValidationFailed,
            "float_minor",
            "enter the cash in the drawer",
        ));
    }
    if !t.bootstrapped {
        return Err(AppError::with_field(
            ErrorCode::ValidationFailed,
            "branch",
            "the branch has not been set up",
        ));
    }
    if let Some(open) = &t.open {
        return Err(AppError::new(
            ErrorCode::ShiftAlreadyOpen,
            format!("{} has a shift open.", open.staff_name),
        ));
    }
    Ok(())
}

pub(super) fn mutate_open(
    tx: &StoreTx<'_>,
    intent: &Intent,
    s: &ShiftOpen,
    t: &ShiftTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let shift_id = ulid::Ulid::generate().to_string();
    insert_shift(
        tx,
        &NewShift {
            id: &shift_id,
            staff_id: &intent.actor.id,
            opened_at: now_ms,
            float_minor: s.float_minor,
            carried_minor: t.carried_minor,
        },
    )?;
    let event_id = ulid::Ulid::generate().to_string();
    insert_cash_event(
        tx,
        &NewCashEvent {
            id: &event_id,
            shift_id: &shift_id,
            kind: CASH_FLOAT_IN,
            amount_minor: s.float_minor,
            staff_id: &intent.actor.id,
            order_id: None,
            reverses: None,
        },
    )?;
    Ok(vec![
        Written {
            key: RowKey::Shifts { id: shift_id },
            op: ChangeOp::Insert,
            before: None,
        },
        Written {
            key: RowKey::CashEvents { id: event_id },
            op: ChangeOp::Insert,
            before: None,
        },
    ])
}

/// Whether any of the payment rows is cash.
pub(super) fn has_cash(payments: &[PaymentRow]) -> bool {
    payments.iter().any(|p| p.method == PaymentMethod::Cash)
}

/// The cash rule: a cash row needs an open shift (`SHIFT_NOT_OPEN`).
pub(super) fn gate(payments: &[PaymentRow], shift_open: bool) -> Result<(), AppError> {
    if has_cash(payments) && !shift_open {
        return Err(AppError::with_field(
            ErrorCode::ShiftNotOpen,
            "payments",
            "Open a shift to take cash.",
        ));
    }
    Ok(())
}

/// Writes a `sale` cash event against the open shift for each cash row of `order_id`.
pub(super) fn record_sales(
    tx: &StoreTx<'_>,
    staff_id: &str,
    order_id: &str,
    payments: &[PaymentRow],
    written: &mut Vec<Written>,
) -> Result<(), AppError> {
    if !has_cash(payments) {
        return Ok(());
    }
    let Some(shift) = open_shift(tx)? else {
        return Err(AppError::with_field(
            ErrorCode::ShiftNotOpen,
            "payments",
            "Open a shift to take cash.",
        ));
    };
    for p in payments.iter().filter(|p| p.method == PaymentMethod::Cash) {
        let id = ulid::Ulid::generate().to_string();
        insert_cash_event(
            tx,
            &NewCashEvent {
                id: &id,
                shift_id: &shift.id,
                kind: CASH_SALE,
                amount_minor: p.amount_minor,
                staff_id,
                order_id: Some(order_id),
                reverses: None,
            },
        )?;
        written.push(Written {
            key: RowKey::CashEvents { id },
            op: ChangeOp::Insert,
            before: None,
        });
    }
    Ok(())
}

/// An Undo: cancels each uncancelled `sale` of `order_id` with a `sale_reverted` row, against
/// the shift the sale was written to.
pub(super) fn reverse_sales(
    tx: &StoreTx<'_>,
    staff_id: &str,
    order_id: &str,
    written: &mut Vec<Written>,
) -> Result<(), AppError> {
    for sale in open_sales_of_order(tx, order_id)? {
        let id = ulid::Ulid::generate().to_string();
        insert_cash_event(
            tx,
            &NewCashEvent {
                id: &id,
                shift_id: &sale.shift_id,
                kind: CASH_SALE_REVERTED,
                amount_minor: -sale.amount_minor,
                staff_id,
                order_id: Some(order_id),
                reverses: Some(&sale.id),
            },
        )?;
        written.push(Written {
            key: RowKey::CashEvents { id },
            op: ChangeOp::Insert,
            before: None,
        });
    }
    Ok(())
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

/// The checks a deposit and an expense share: no `base_rev`, an amount in range, an open shift
/// (`SHIFT_NOT_OPEN`). Returns the open shift.
fn gate_move<'a>(
    intent: &Intent,
    kind: &str,
    amount_minor: i64,
    t: &'a ShiftTarget,
) -> Result<&'a ShiftRow, AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", &format!("{kind} takes no base_rev")));
    }
    if !(1..=MAX_CASH_MOVE_MINOR).contains(&amount_minor) {
        return Err(invalid("amount_minor", "enter an amount"));
    }
    t.open.as_ref().ok_or_else(|| {
        AppError::with_field(
            ErrorCode::ShiftNotOpen,
            "shift",
            "Open a shift to move cash.",
        )
    })
}

/// A drawer outflow above the expected cash is `DRAWER_SHORT`; the message never says by how
/// much, so the count stays blind.
fn drawer_has(t: &ShiftTarget, amount_minor: i64) -> Result<(), AppError> {
    if amount_minor > t.expected_minor {
        return Err(AppError::with_field(
            ErrorCode::DrawerShort,
            "amount_minor",
            "The drawer does not have that much cash.",
        ));
    }
    Ok(())
}

/// Plain text of 1 to `max` characters, trimmed.
fn plain(value: &str, max: usize) -> Option<&str> {
    let v = value.trim();
    (!v.is_empty() && v.chars().count() <= max && !v.chars().any(char::is_control)).then_some(v)
}

pub(super) fn validate_deposit(
    intent: &Intent,
    d: &CashDeposit,
    t: &ShiftTarget,
) -> Result<(), AppError> {
    if plain(&d.reference, DEPOSIT_REFERENCE_MAX_CHARS).is_none() {
        return Err(invalid("reference", "enter the slip reference"));
    }
    gate_move(intent, "cash.deposit", d.amount_minor, t)?;
    drawer_has(t, d.amount_minor)
}

pub(super) fn validate_expense(
    intent: &Intent,
    e: &ExpenseCreate,
    t: &ShiftTarget,
) -> Result<(), AppError> {
    if let Some(note) = &e.note
        && !note.trim().is_empty()
        && plain(note, EXPENSE_NOTE_MAX_CHARS).is_none()
    {
        return Err(invalid(
            "note",
            "the note is plain text of up to 140 characters",
        ));
    }
    if let Some(problem) = e.receipt_photo.as_deref().and_then(receipt_photo_problem) {
        return Err(invalid("receipt_photo", problem));
    }
    gate_move(intent, "expense.create", e.amount_minor, t)?;
    if e.paid_from == ExpensePaidFrom::Drawer {
        drawer_has(t, e.amount_minor)?;
    }
    Ok(())
}

/// What a close works out from the count: the signed difference and, over the threshold, the
/// approved reason.
struct CloseCheck {
    difference_minor: i64,
    over: bool,
    reason: Option<VarianceReason>,
    note: Option<String>,
}

fn check_close(intent: &Intent, c: &ShiftClose, t: &ShiftTarget) -> Result<CloseCheck, AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "shift.close takes no base_rev"));
    }
    if !(0..=MAX_CASH_MOVE_MINOR).contains(&c.counted_minor) {
        return Err(invalid("counted_minor", "enter the cash you counted"));
    }
    if t.open.is_none() {
        return Err(AppError::with_field(
            ErrorCode::ShiftNotOpen,
            "shift",
            "There is no shift to close.",
        ));
    }
    if let Some(d) = &c.deposit {
        if !(1..=MAX_CASH_MOVE_MINOR).contains(&d.amount_minor) || d.amount_minor > c.counted_minor
        {
            return Err(invalid(
                "deposit",
                "the deposit is more than the cash counted",
            ));
        }
        if plain(&d.reference, DEPOSIT_REFERENCE_MAX_CHARS).is_none() {
            return Err(invalid("reference", "enter the slip reference"));
        }
    }
    let difference_minor = c.counted_minor - t.expected_minor;
    let over = difference_minor.abs() > t.threshold_minor;
    if !over {
        return Ok(CloseCheck {
            difference_minor,
            over,
            reason: None,
            note: None,
        });
    }
    if !t.can_override {
        return Err(AppError::new(
            ErrorCode::VarianceOverThreshold,
            "The difference is over the limit. A manager must approve it.",
        ));
    }
    let Some(reason) = c.reason else {
        return Err(invalid("reason", "choose why the count is off"));
    };
    let note = if reason == VarianceReason::Other {
        let note = c.note.as_deref().map(str::trim).unwrap_or_default();
        if plain(note, VARIANCE_NOTE_MAX_CHARS).is_none() {
            return Err(invalid("note", "say why the count is off"));
        }
        Some(note.to_owned())
    } else {
        None
    };
    Ok(CloseCheck {
        difference_minor,
        over,
        reason: Some(reason),
        note,
    })
}

pub(super) fn validate_close(
    intent: &Intent,
    c: &ShiftClose,
    t: &ShiftTarget,
) -> Result<(), AppError> {
    check_close(intent, c, t).map(|_| ())
}

/// Closes the shift: stores the count, the expected cash and the signed variance (with the
/// reason and approver when it was over the threshold), writes the optional deposit after the
/// count, and records what the next shift's float is prefilled with.
pub(super) fn mutate_close(
    tx: &StoreTx<'_>,
    intent: &Intent,
    c: &ShiftClose,
    t: &ShiftTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let shift = t
        .open
        .as_ref()
        .ok_or_else(|| AppError::new(ErrorCode::ShiftNotOpen, "There is no shift to close."))?;
    let check = check_close(intent, c, t)?;
    let mut written = Vec::new();
    let mut deposited = 0;
    if let Some(d) = &c.deposit {
        let id = ulid::Ulid::generate().to_string();
        insert_cash_move(
            tx,
            &NewCashMove {
                id: &id,
                shift_id: &shift.id,
                kind: CASH_DEPOSIT,
                amount_minor: -d.amount_minor,
                staff_id: &intent.actor.id,
                reference: Some(d.reference.trim()),
                category: None,
                paid_from: None,
                spent_minor: None,
                note: None,
                receipt_photo: None,
            },
        )?;
        written.push(inserted(id));
        deposited = d.amount_minor;
    }
    let approver = check.over.then(|| {
        intent
            .elevated_by
            .as_ref()
            .map_or(intent.actor.id.as_str(), |e| e.id.as_str())
    });
    close_shift(
        tx,
        &ShiftClosing {
            shift_id: &shift.id,
            closed_at: now_ms,
            counted_minor: c.counted_minor,
            expected_minor: t.expected_minor,
            variance_minor: check.difference_minor,
            variance_reason: check.reason.map(VarianceReason::as_str),
            variance_note: check.note.as_deref(),
            closed_by: &intent.actor.id,
            approver_id: approver,
            next_float_minor: c.counted_minor - deposited,
        },
    )?;
    written.push(Written {
        key: RowKey::Shifts {
            id: shift.id.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    });
    Ok(written)
}

fn inserted(id: String) -> Written {
    Written {
        key: RowKey::CashEvents { id },
        op: ChangeOp::Insert,
        before: None,
    }
}

pub(super) fn mutate_deposit(
    tx: &StoreTx<'_>,
    intent: &Intent,
    d: &CashDeposit,
    t: &ShiftTarget,
) -> Result<Vec<Written>, AppError> {
    let shift = t
        .open
        .as_ref()
        .ok_or_else(|| AppError::new(ErrorCode::ShiftNotOpen, "Open a shift to move cash."))?;
    let id = ulid::Ulid::generate().to_string();
    insert_cash_move(
        tx,
        &NewCashMove {
            id: &id,
            shift_id: &shift.id,
            kind: CASH_DEPOSIT,
            amount_minor: -d.amount_minor,
            staff_id: &intent.actor.id,
            reference: Some(d.reference.trim()),
            category: None,
            paid_from: None,
            spent_minor: None,
            note: None,
            receipt_photo: None,
        },
    )?;
    Ok(vec![inserted(id)])
}

pub(super) fn mutate_expense(
    tx: &StoreTx<'_>,
    intent: &Intent,
    e: &ExpenseCreate,
    t: &ShiftTarget,
) -> Result<Vec<Written>, AppError> {
    let shift = t
        .open
        .as_ref()
        .ok_or_else(|| AppError::new(ErrorCode::ShiftNotOpen, "Open a shift to move cash."))?;
    let id = ulid::Ulid::generate().to_string();
    let note = e.note.as_deref().map(str::trim).filter(|n| !n.is_empty());
    insert_cash_move(
        tx,
        &NewCashMove {
            id: &id,
            shift_id: &shift.id,
            kind: CASH_EXPENSE,
            amount_minor: match e.paid_from {
                ExpensePaidFrom::Drawer => -e.amount_minor,
                ExpensePaidFrom::Owner => 0,
            },
            staff_id: &intent.actor.id,
            reference: None,
            category: Some(e.category.as_str()),
            paid_from: Some(e.paid_from.as_str()),
            spent_minor: Some(e.amount_minor),
            note,
            receipt_photo: e.receipt_photo.as_deref(),
        },
    )?;
    Ok(vec![inserted(id)])
}
