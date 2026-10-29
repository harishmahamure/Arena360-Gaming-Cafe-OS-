//! `order.void` (Story 5.23): an unsettled session order is cancelled whole, with a reason and an
//! approver (`pos.void` on the actor or on the manager whose PIN elevated the intent).
//!
//! The order is the products placed on a running postpaid session's bill (Story 5.12). Voiding it
//! gives every line's stock back (open holds close; stock a line took, at placement or when it
//! was served, returns through a `return` move), marks the order `void` with the reason, the
//! note of an `other` reason and the approver, and detaches it from the session so the session
//! can open a fresh bill. The lines stay on the voided order for the record; nothing counts a
//! voided order as a sale. A settled order is `ORDER_SETTLED` (it is refunded, not voided); a
//! voided one is `STALE`. An order that carries play time (an ended or block session's bill) is
//! refused: the time was played and is settled, not voided.

use arena_proto::{AppError, ErrorCode, Intent, OrderVoid, ReasonCode, VOID_NOTE_MAX_CHARS};
use arena_store::{
    ChangeOp, OrderRow, OrderVoid as VoidRecord, RowKey, StoreTx, list_order_line_ids,
    list_segment_line_ids, void_order,
};

use super::mutate::Written;
use super::order_line::{LineTarget, give_back, load_line};
use super::pos_sale::internal;
use super::session::invalid;

#[derive(Default)]
pub(crate) struct VoidTarget {
    order: Option<OrderRow>,
    /// The order's product lines, each with its holds and the items it sells.
    lines: Vec<LineTarget>,
    /// How many play-time lines the order carries.
    time_lines: usize,
}

pub(super) fn load_void(
    tx: &StoreTx<'_>,
    v: &OrderVoid,
    now_ms: i64,
) -> Result<VoidTarget, AppError> {
    let Some(order) = arena_store::find_order(tx, &v.order_id)? else {
        return Ok(VoidTarget::default());
    };
    let time_lines = list_segment_line_ids(tx, &order.id)?;
    let mut lines = Vec::new();
    if order.status == "open" {
        for id in list_order_line_ids(tx, &order.id)? {
            if !time_lines.contains(&id) {
                lines.push(load_line(tx, &id, now_ms)?);
            }
        }
    }
    Ok(VoidTarget {
        order: Some(order),
        lines,
        time_lines: time_lines.len(),
    })
}

/// The note to store: only an `other` reason keeps one, and it is required.
fn note(intent: &Intent, v: &OrderVoid) -> Result<Option<String>, AppError> {
    if intent.reason != Some(ReasonCode::Other) {
        return Ok(None);
    }
    let note = v.note.as_deref().map(str::trim).unwrap_or_default();
    if note.is_empty() {
        return Err(invalid("note", "say why the order is voided"));
    }
    if note.chars().count() > VOID_NOTE_MAX_CHARS || note.chars().any(char::is_control) {
        return Err(invalid(
            "note",
            "the reason is plain text of up to 140 characters",
        ));
    }
    Ok(Some(note.to_owned()))
}

pub(super) fn validate_void(
    intent: &Intent,
    v: &OrderVoid,
    t: &VoidTarget,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "order.void takes no base_rev"));
    }
    let Some(order) = &t.order else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "order_id",
            "no such order",
        ));
    };
    match order.status.as_str() {
        "open" => {}
        "settled" => {
            return Err(AppError::with_field(
                ErrorCode::OrderSettled,
                "order_id",
                "this order is settled; refund it instead",
            ));
        }
        _ => {
            return Err(AppError::with_field(
                ErrorCode::Stale,
                "order_id",
                "this order is already void",
            ));
        }
    }
    if !super::order_line::is_open_bill(order)
        || (order.kind == "session" && order.session_id.is_none())
    {
        return Err(invalid(
            "order_id",
            "only an order on a session bill or an unpaid counter order can be voided",
        ));
    }
    if t.time_lines > 0 {
        return Err(invalid(
            "order_id",
            "this bill carries play time; settle it, or cancel items one by one",
        ));
    }
    if t.lines.is_empty() {
        return Err(invalid("order_id", "this order has no items"));
    }
    note(intent, v)?;
    Ok(())
}

pub(super) fn mutate_void(
    tx: &StoreTx<'_>,
    intent: &Intent,
    v: &OrderVoid,
    t: &VoidTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let Some(order) = &t.order else {
        return Err(internal("a void validated without its order"));
    };
    let reason = intent
        .reason
        .ok_or_else(|| internal("a void validated without its reason"))?;
    let note = note(intent, v)?;
    let mut written = Vec::new();
    for line in &t.lines {
        give_back(tx, intent, line, false, now_ms, &mut written)?;
    }
    let approver = intent
        .elevated_by
        .as_ref()
        .map_or(intent.actor.id.as_str(), |e| e.id.as_str());
    void_order(
        tx,
        &order.id,
        &VoidRecord {
            at: now_ms,
            by: approver,
            reason: reason.as_str(),
            note: note.as_deref(),
        },
    )?;
    written.push(Written {
        key: RowKey::Orders {
            id: order.id.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    });
    Ok(written)
}
