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
    STAGE_READY, STAGE_SERVED,
};
use arena_store::{
    ChangeOp, HoldRow, LineRow, OrderRow, ProductRow, RowKey, StoreTx, Table, TicketLineRow,
    TicketRow, add_order_amount, delete_row, find_line, find_line_for_order_line, find_order,
    find_product, find_stock, find_ticket, list_components, list_line_holds, list_order_line_ids,
    list_ticket_lines, mark_line_served, read_row, release_hold, set_ticket_cancelled,
    set_ticket_stage,
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
    /// The kitchen ticket line that carries this line, and its ticket (Story 7.3).
    ticket: Option<(TicketRow, TicketLineRow)>,
    /// How many other lines of that ticket are still unserved.
    ticket_others_unserved: usize,
    /// How many other lines the ticket holds.
    ticket_others: usize,
}

impl LineTarget {
    /// Whether the line exists and is not served yet.
    pub(super) fn is_unserved(&self) -> bool {
        self.line.as_ref().is_some_and(|l| !l.served)
    }

    /// Whether the line sits on an open bill that can still change.
    pub(super) fn on_open_bill(&self) -> bool {
        self.order
            .as_ref()
            .is_some_and(|o| o.status == "open" && is_open_bill(o))
    }
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
    let mut ticket = None;
    let mut ticket_others = 0;
    let mut ticket_others_unserved = 0;
    if let Some(tl) = find_line_for_order_line(tx, &line.id)?
        && let Some(t) = find_ticket(tx, &tl.ticket_id)?
    {
        for other in list_ticket_lines(tx, &t.id)?
            .iter()
            .filter(|o| o.id != tl.id)
        {
            ticket_others += 1;
            if find_line(tx, &other.order_line_id)?.is_some_and(|l| !l.served) {
                ticket_others_unserved += 1;
            }
        }
        ticket = Some((t, tl));
    }
    Ok(LineTarget {
        all_lines: list_order_line_ids(tx, &line.order_id)?.len(),
        ticket,
        ticket_others,
        ticket_others_unserved,
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
    live_line(t)
}

/// The line, when it is a product line on an open bill.
fn live_line(t: &LineTarget) -> Result<&LineRow, AppError> {
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
    open_line(intent, t, "order.line_served")?;
    // A line on a kitchen ticket is served once the ticket is Ready (Story 7.3).
    if let Some((ticket, _)) = &t.ticket
        && ticket.stage != STAGE_READY
    {
        return Err(AppError::with_field(
            ErrorCode::BadStage,
            "line_id",
            "the kitchen ticket is not ready yet",
        ));
    }
    check_servable(t)
}

/// Whether a line can be served now: it is a live, unserved product line and the stock it holds
/// can be taken. Used by the line's own intent and by a ticket reaching Served.
pub(super) fn check_servable(t: &LineTarget) -> Result<(), AppError> {
    let line = live_line(t)?;
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
    let mut written = serve_line(tx, &intent.actor.id, t, now_ms)?;
    // The last unserved line of a Ready ticket serves the ticket.
    if let Some((ticket, _)) = &t.ticket
        && t.ticket_others_unserved == 0
    {
        set_ticket_stage(tx, &ticket.id, STAGE_SERVED, now_ms, &intent.actor.id)?;
        written.push(update(RowKey::KitchenTickets {
            id: ticket.id.clone(),
        }));
    }
    Ok(written)
}

/// Serves one line: takes the stock of its open holds, closes them and marks the line served.
pub(super) fn serve_line(
    tx: &StoreTx<'_>,
    staff_id: &str,
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
            tx, &h.item_id, h.qty, &tag, staff_id, now_ms,
        )?);
        written.extend(super::stock::low_after_change(
            tx,
            &h.item_id,
            before,
            before - h.qty,
            staff_id,
            now_ms,
        )?);
        release_hold(tx, &h.id, now_ms)?;
        written.push(update(RowKey::StockHolds { id: h.id.clone() }));
    }
    mark_line_served(tx, &line.id, now_ms, staff_id)?;
    written.push(update(RowKey::OrderLines {
        id: line.id.clone(),
    }));
    written.extend(super::alert::close_kiosk_order(
        tx, order, staff_id, now_ms,
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

/// Takes a cancelled line off its kitchen ticket (Story 7.3). The last line cancelled cancels the
/// ticket, keeping the reason; when the lines left are all served, a Ready ticket is Served.
fn drop_from_ticket(
    tx: &StoreTx<'_>,
    intent: &Intent,
    t: &LineTarget,
    now_ms: i64,
    written: &mut Vec<Written>,
) -> Result<(), AppError> {
    let Some((ticket, tl)) = &t.ticket else {
        return Ok(());
    };
    let key = RowKey::KitchenTicketLines { id: tl.id.clone() };
    let before = read_row(tx, &key)?;
    delete_row(tx, Table::KitchenTicketLines, &before)?;
    written.push(Written {
        key,
        op: ChangeOp::Delete,
        before: Some(before),
    });
    let by = intent.actor.id.as_str();
    if t.ticket_others == 0 {
        let reason = intent.reason.map_or("other", ReasonCode::as_str);
        set_ticket_cancelled(tx, &ticket.id, reason, now_ms, by)?;
    } else if t.ticket_others_unserved == 0 && ticket.stage == STAGE_READY {
        set_ticket_stage(tx, &ticket.id, STAGE_SERVED, now_ms, by)?;
    } else {
        return Ok(());
    }
    written.push(update(RowKey::KitchenTickets {
        id: ticket.id.clone(),
    }));
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
    drop_from_ticket(tx, intent, t, now_ms, &mut written)?;
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
