//! Shifts and the cash ledger (Story 5.10). The branch has one drawer, so one open shift at a
//! time. `shift.open` records the float staff confirmed beside the one the screen offered; every
//! cash payment row written afterwards adds a `sale` cash event against the open shift, and an
//! Undo cancels it with a compensating `sale_reverted` row. Without an open shift a cash row is
//! refused `SHIFT_NOT_OPEN`; UPI, card, wallet and credit never need one.

use arena_proto::{
    AppError, CASH_FLOAT_IN, CASH_SALE, CASH_SALE_REVERTED, ErrorCode, Intent, MAX_FLOAT_MINOR,
    PaymentMethod, PaymentRow, ShiftOpen,
};
use arena_store::{
    ChangeOp, NewCashEvent, NewShift, RowKey, ShiftRow, StoreTx, insert_cash_event, insert_shift,
    last_opening_float, open_sales_of_order, open_shift,
};

use super::mutate::Written;

#[derive(Default)]
pub(crate) struct ShiftTarget {
    bootstrapped: bool,
    open: Option<ShiftRow>,
    carried_minor: i64,
}

pub(super) fn load(tx: &StoreTx<'_>) -> Result<ShiftTarget, AppError> {
    Ok(ShiftTarget {
        bootstrapped: arena_store::branch_exists(tx)?,
        open: open_shift(tx)?,
        carried_minor: last_opening_float(tx)?,
    })
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
