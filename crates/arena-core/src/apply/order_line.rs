//! `order.line_served` and `order.line_cancel` (Story 5.13): a placed line on a session bill is
//! marked served, or cancelled while it is not.
//!
//! In `served` mode a kitchen line is placed unserved with one stock hold per tracked item (no stock
//! moves). Serving takes the stock of the line's open holds and closes them. Cancelling a held
//! line closes its holds (`made = false`) or also writes waste `cancelled_order` at cost
//! (`made = true`, capped at the stock on hand). A line without holds took its stock at placement
//! (`order` mode): cancelling it returns the units through a `return` move, and `made = true`
//! then writes the waste. The line leaves the bill and the order total shrinks.

use std::collections::BTreeMap;

use arena_proto::{
    AppError, ErrorCode, Intent, MOVE_WASTE, OrderLineCancel, OrderLineServed, ReasonCode,
};
use arena_store::{
    ChangeOp, HoldRow, LineRow, OrderRow, ProductRow, RowKey, StoreTx, Table, add_order_amount,
    delete_row, find_line, find_order, find_product, find_stock, list_components, list_line_holds,
    list_order_line_ids, mark_line_served, read_row, release_hold,
};

use super::mutate::Written;
use super::pos_sale::internal;
use super::session::invalid;
use super::stock::StockFor;

#[derive(Default)]
pub(crate) struct LineTarget {
    line: Option<LineRow>,
    order: Option<OrderRow>,
    /// The line's open stock holds.
    holds: Vec<HoldRow>,
    /// The tracked items the line sells, with the quantity of the whole line: the product itself,
    /// or a bundle's tracked parts times the line quantity.
    items: Vec<(ProductRow, i64)>,
    /// What each held item may be taken from right now (on hand less expired lots).
    takeable: BTreeMap<String, i64>,
    today: String,
    /// How many lines the order holds in all.
    all_lines: usize,
}

pub(super) fn load_line(
    tx: &StoreTx<'_>,
    line_id: &str,
    now_ms: i64,
) -> Result<LineTarget, AppError> {
    let Some(line) = find_line(tx, line_id)? else {
        return Ok(LineTarget::default());
    };
    let order = find_order(tx, &line.order_id)?;
    let holds = list_line_holds(tx, &line.id)?;
    let product = match &line.product_id {
        Some(id) => find_product(tx, id)?,
        None => None,
    };
    let mut items = Vec::new();
    if let Some(p) = product {
        if p.bundle {
            for (id, qty) in list_components(tx, &p.product_id)? {
                if let Some(c) = find_product(tx, &id)?.filter(|c| !c.bundle && c.track_stock) {
                    items.push((c, qty * line.qty));
                }
            }
        } else if p.track_stock {
            items.push((p, line.qty));
        }
    }
    let today = crate::locale::stamp(tx, now_ms)?
        .map(|(day, _)| day)
        .unwrap_or_default();
    let mut takeable = BTreeMap::new();
    for h in &holds {
        takeable.insert(
            h.item_id.clone(),
            super::stock::takeable(tx, &h.item_id, &today)?,
        );
    }
    Ok(LineTarget {
        all_lines: list_order_line_ids(tx, &line.order_id)?.len(),
        line: Some(line),
        order,
        holds,
        items,
        takeable,
        today,
    })
}

/// Whether an order is a bill that can still change: a session's, or an unpaid counter order
/// (Story 6.32: `pos_sale` and open, which only a counter order is).
pub(super) fn is_open_bill(o: &arena_store::OrderRow) -> bool {
    o.kind == "session" || (o.kind == "pos_sale" && o.session_id.is_none())
}

/// The line, when it is a product line on an open session bill.
fn open_line<'a>(intent: &Intent, t: &'a LineTarget, what: &str) -> Result<&'a LineRow, AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", &format!("{what} takes no base_rev")));
    }
    let Some(line) = &t.line else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "line_id",
            "no such line",
        ));
    };
    if line.product_id.is_none() {
        return Err(invalid("line_id", "this is not a product line"));
    }
    let open = t
        .order
        .as_ref()
        .is_some_and(|o| o.status == "open" && is_open_bill(o));
    if !open {
        return Err(invalid("line_id", "this bill is no longer open"));
    }
    Ok(line)
}

pub(super) fn validate_served(
    intent: &Intent,
    _s: &OrderLineServed,
    t: &LineTarget,
) -> Result<(), AppError> {
    let line = open_line(intent, t, "order.line_served")?;
    if line.served {
        return Err(invalid("line_id", "this line is already served"));
    }
    for h in &t.holds {
        let usable = t.takeable.get(&h.item_id).copied().unwrap_or(0);
        if usable < h.qty {
            let name = t
                .items
                .iter()
                .find(|(p, _)| p.product_id == h.item_id)
                .map_or("item", |(p, _)| p.name.as_str());
            return Err(AppError::with_field(
                ErrorCode::OutOfStock,
                "line_id",
                format!("Only {usable} {name} left."),
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_cancel(
    intent: &Intent,
    _c: &OrderLineCancel,
    t: &LineTarget,
) -> Result<(), AppError> {
    let line = open_line(intent, t, "order.line_cancel")?;
    if line.served {
        return Err(invalid("line_id", "a served line cannot be cancelled"));
    }
    Ok(())
}

fn update(key: RowKey) -> Written {
    Written {
        key,
        op: ChangeOp::Update,
        before: None,
    }
}

pub(super) fn mutate_served(
    tx: &StoreTx<'_>,
    intent: &Intent,
    t: &LineTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let (Some(line), Some(order)) = (&t.line, &t.order) else {
        return Err(internal("a serve validated without its line"));
    };
    let mut written = Vec::new();
    let tag = StockFor {
        order_id: &order.id,
        placement_id: line.placement_id.as_deref(),
        today: &t.today,
    };
    for h in &t.holds {
        let before = find_stock(tx, &h.item_id)?.map_or(0, |s| s.qty);
        written.extend(super::stock::take_for_sale(
            tx,
            &h.item_id,
            h.qty,
            &tag,
            &intent.actor.id,
            now_ms,
        )?);
        written.extend(super::stock::low_after_change(
            tx,
            &h.item_id,
            before,
            before - h.qty,
            &intent.actor.id,
            now_ms,
        )?);
        release_hold(tx, &h.id, now_ms)?;
        written.push(update(RowKey::StockHolds { id: h.id.clone() }));
    }
    mark_line_served(tx, &line.id, now_ms, &intent.actor.id)?;
    written.push(update(RowKey::OrderLines {
        id: line.id.clone(),
    }));
    written.extend(super::alert::close_kiosk_order(
        tx,
        order,
        &intent.actor.id,
        now_ms,
    )?);
    Ok(written)
}

/// Waste of a cancelled line that was made: `qty` of the item, valued at the average cost, never
/// more than the stock on hand.
fn waste(
    tx: &StoreTx<'_>,
    intent: &Intent,
    item_id: &str,
    qty: i64,
    order_id: &str,
    now_ms: i64,
    written: &mut Vec<Written>,
) -> Result<(), AppError> {
    let on_hand = find_stock(tx, item_id)?.map_or(0, |s| s.qty);
    let take = qty.min(on_hand);
    if take > 0 {
        written.extend(super::stock::consume_for(
            tx,
            item_id,
            take,
            MOVE_WASTE,
            Some(ReasonCode::CancelledOrder.as_str()),
            Some(order_id),
            &intent.actor.id,
            now_ms,
        )?);
    }
    Ok(())
}

/// Gives a line's stock back: a held line closes its holds, a line that took its stock (at
/// placement, or when it was served) returns the units through a `return` move. `made` also writes
/// the units off as waste `cancelled_order`. Used by cancel and by void (Story 5.23).
pub(super) fn give_back(
    tx: &StoreTx<'_>,
    intent: &Intent,
    t: &LineTarget,
    made: bool,
    now_ms: i64,
    written: &mut Vec<Written>,
) -> Result<(), AppError> {
    let (Some(line), Some(order)) = (&t.line, &t.order) else {
        return Err(internal("a line was given back without its row"));
    };
    if t.holds.is_empty() {
        // The line took its stock at placement: give it back, and write it off when it was made.
        let tag = StockFor {
            order_id: &order.id,
            placement_id: line.placement_id.as_deref(),
            today: &t.today,
        };
        for (item, qty) in &t.items {
            written.extend(super::stock::return_to_stock(
                tx,
                &item.product_id,
                *qty,
                &tag,
                &intent.actor.id,
                now_ms,
            )?);
            if made {
                waste(
                    tx,
                    intent,
                    &item.product_id,
                    *qty,
                    &order.id,
                    now_ms,
                    written,
                )?;
            }
        }
    } else {
        for h in &t.holds {
            release_hold(tx, &h.id, now_ms)?;
            written.push(update(RowKey::StockHolds { id: h.id.clone() }));
            written.extend(super::stock::touch(tx, &h.item_id)?);
            if made {
                waste(tx, intent, &h.item_id, h.qty, &order.id, now_ms, written)?;
            }
        }
    }
    Ok(())
}

pub(super) fn mutate_cancel(
    tx: &StoreTx<'_>,
    intent: &Intent,
    c: &OrderLineCancel,
    t: &LineTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let (Some(line), Some(order)) = (&t.line, &t.order) else {
        return Err(internal("a cancel validated without its line"));
    };
    let mut written = Vec::new();
    give_back(tx, intent, t, c.made, now_ms, &mut written)?;
    let key = RowKey::OrderLines {
        id: line.id.clone(),
    };
    let before = read_row(tx, &key)?;
    delete_row(tx, Table::OrderLines, &before)?;
    written.push(Written {
        key,
        op: ChangeOp::Delete,
        before: Some(before),
    });
    let key = RowKey::Orders {
        id: order.id.clone(),
    };
    if t.all_lines <= 1 {
        let before = read_row(tx, &key)?;
        delete_row(tx, Table::Orders, &before)?;
        written.push(Written {
            key,
            op: ChangeOp::Delete,
            before: Some(before),
        });
    } else {
        add_order_amount(tx, &order.id, -line.amount_minor)?;
        written.push(update(key));
    }
    written.extend(super::alert::close_kiosk_order(
        tx,
        order,
        &intent.actor.id,
        now_ms,
    )?);
    Ok(written)
}
