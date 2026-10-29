//! `order.ready` (Story 6.33): staff mark an open order with an unserved line ready for the
//! player. The order is stamped `ready_at`; the Hub's session tick tells the kiosk, which shows
//! the seat or pick-up copy the branch's fulfilment setting asks for. Placing more lines on the
//! bill clears the mark (`order.place`), and serving the last line leaves it untouched.

use arena_proto::{AppError, ErrorCode, Intent, OrderReady};
use arena_store::{
    ChangeOp, OrderRow, RowKey, StoreTx, count_unserved_lines, find_order, set_order_ready,
};

use super::mutate::Written;
use super::order_line::is_open_bill;
use super::pos_sale::internal;
use super::session::invalid;

#[derive(Default)]
pub(crate) struct ReadyTarget {
    order: Option<OrderRow>,
    /// How many lines of the order are not served yet.
    unserved: i64,
}

pub(super) fn load_ready(tx: &StoreTx<'_>, r: &OrderReady) -> Result<ReadyTarget, AppError> {
    let Some(order) = find_order(tx, &r.order_id)? else {
        return Ok(ReadyTarget::default());
    };
    Ok(ReadyTarget {
        unserved: count_unserved_lines(tx, &order.id)?,
        order: Some(order),
    })
}

pub(super) fn validate_ready(intent: &Intent, t: &ReadyTarget) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "order.ready takes no base_rev"));
    }
    let Some(order) = &t.order else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "order_id",
            "no such order",
        ));
    };
    if order.status != "open" || !is_open_bill(order) {
        return Err(invalid("order_id", "this bill is no longer open"));
    }
    if t.unserved == 0 {
        return Err(invalid("order_id", "this order has nothing left to serve"));
    }
    if order.ready_at.is_some() {
        return Err(invalid("order_id", "this order is already marked ready"));
    }
    Ok(())
}

pub(super) fn mutate_ready(
    tx: &StoreTx<'_>,
    t: &ReadyTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let order = t
        .order
        .as_ref()
        .ok_or_else(|| internal("a ready mark validated without its order"))?;
    set_order_ready(tx, &order.id, Some(now_ms))?;
    Ok(vec![Written {
        key: RowKey::Orders {
            id: order.id.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    }])
}
