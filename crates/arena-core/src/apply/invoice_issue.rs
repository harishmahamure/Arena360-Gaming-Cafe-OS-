//! Invoice numbers (Story 5.14): `invoice.issue` gives a settled sale the next number of the
//! branch's series. The Hub's invoice task submits it for each settled sale once the 8 s Undo
//! window has closed, oldest settle first, so a sale undone inside the window never takes a
//! number and the series has no gaps. [`issue_number`] is the one place a number is minted; a
//! settle that must not be undoable (Story 5.17) calls it inside its own transaction.

use arena_proto::{AppError, EffectKind, ErrorCode, Intent, InvoiceIssue, UNDO_WINDOW_MS};
use arena_store::{
    BranchSummary, ChangeOp, INVOICED_KINDS, InvoiceOrder, RowKey, StoreTx, find_branch_summary,
    find_invoice_order, insert_effect, set_invoice_no, take_sequence,
};

use super::branch_code::lock_branch_code;
use super::mutate::Written;
use super::session::invalid;
use crate::invoice::number;
use crate::status::Epoch;

#[derive(Default)]
pub(crate) struct IssueTarget {
    order: Option<InvoiceOrder>,
    branch: Option<BranchSummary>,
    /// Whether the order's Undo window is still open at the time of the intent.
    in_window: bool,
}

pub(super) fn load(
    tx: &StoreTx<'_>,
    p: &InvoiceIssue,
    now_ms: i64,
) -> Result<IssueTarget, AppError> {
    let order = find_invoice_order(tx, &p.order_id)?;
    let in_window = order
        .as_ref()
        .and_then(|o| o.settled_at)
        .is_some_and(|at| now_ms - at <= UNDO_WINDOW_MS);
    Ok(IssueTarget {
        order,
        branch: find_branch_summary(tx)?,
        in_window,
    })
}

/// Whether an order of this kind and total is a sale that takes an invoice number.
fn invoiced(kind: &str, total_minor: i64) -> bool {
    total_minor > 0 && INVOICED_KINDS.contains(&kind)
}

pub(super) fn validate(intent: &Intent, t: &IssueTarget) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "invoice.issue takes no base_rev"));
    }
    let Some(order) = &t.order else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "order_id",
            "no such order",
        ));
    };
    if t.branch.is_none() {
        return Err(invalid("branch", "the branch has not been set up"));
    }
    if order.invoice_no.is_some() {
        return Err(AppError::with_field(
            ErrorCode::Stale,
            "order_id",
            "this order already has an invoice number",
        ));
    }
    if order.status != "settled" || order.settled_at.is_none() {
        return Err(AppError::with_field(
            ErrorCode::Stale,
            "order_id",
            "this order is not settled",
        ));
    }
    if !invoiced(&order.kind, order.total_minor) {
        return Err(invalid("order_id", "this order takes no invoice number"));
    }
    if t.in_window {
        return Err(AppError::with_field(
            ErrorCode::Stale,
            "order_id",
            "the Undo window of this order is still open",
        ));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    t: &IssueTarget,
    epoch: &Epoch,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let (Some(order), Some(branch)) = (&t.order, &t.branch) else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "an invoice number validated without its target",
        ));
    };
    let mut written = Vec::new();
    issue_number(
        tx,
        &order.id,
        &order.fy,
        &branch.branch_code,
        epoch,
        Some(&intent.intent_id),
        now_ms,
        &mut written,
    )?;
    Ok(written)
}

/// Gives `order_id` the next number of the `(fy, segment)` series of the writer's epoch, in the
/// caller's transaction: advances the sequence by exactly one, writes `orders.invoice_no`, locks
/// the branch code at the first invoice and records the irreversible effect "invoice number
/// issued". Returns the number.
#[allow(clippy::too_many_arguments)]
pub(super) fn issue_number(
    tx: &StoreTx<'_>,
    order_id: &str,
    fy: &str,
    branch_code: &str,
    epoch: &Epoch,
    intent_id: Option<&str>,
    now_ms: i64,
    written: &mut Vec<Written>,
) -> Result<String, AppError> {
    let segment = number::segment(epoch.counter, &epoch.hub_instance_id);
    let (seq, opened) = take_sequence(tx, fy, &segment)?;
    let invoice_no = number::format(branch_code, fy, &segment, seq);
    set_invoice_no(tx, order_id, &invoice_no)?;
    insert_effect(
        tx,
        &ulid::Ulid::generate().to_string(),
        EffectKind::InvoiceNumberAssigned,
        intent_id,
        Some(order_id),
        now_ms,
    )?;
    written.push(Written {
        key: RowKey::InvoiceSequences {
            fy: fy.to_owned(),
            segment,
        },
        op: if opened {
            ChangeOp::Insert
        } else {
            ChangeOp::Update
        },
        before: None,
    });
    written.push(Written {
        key: RowKey::Orders {
            id: order_id.to_owned(),
        },
        op: ChangeOp::Update,
        before: None,
    });
    if let Some(locked) = lock_branch_code(tx, now_ms)? {
        written.push(Written {
            key: locked,
            op: ChangeOp::Update,
            before: None,
        });
    }
    Ok(invoice_no)
}
