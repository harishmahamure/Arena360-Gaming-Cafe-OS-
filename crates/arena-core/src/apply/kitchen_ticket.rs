//! Kitchen tickets (Story 7.3). A placement (`order.place`, `pos.sale`) on a branch with the
//! `kitchen` flag opens one ticket holding only its kitchen-flagged lines, in the same
//! transaction. The ticket's stage (New, Preparing, Ready, Served) is the single source for the
//! Hub sheet, the KOT, the display and the READY badge:
//!
//! - `kitchen.advance` moves it exactly one stage on. Reaching Ready runs the `order.ready` effect
//!   for its order (the bill is stamped, so the HUD shows the delivered or pick-up copy); reaching
//!   Served serves each of its unserved lines (taking held stock), as `order.line_served` does. A
//!   walk-up ticket's lines were served at the sale, so only its stage changes.
//! - `kitchen.advance.reverted` takes the latest advance back within the Undo window, reversing
//!   its effects (a Served advance gives the stock back and reopens the holds).
//! - Cancelling a line takes it off its ticket; the last line cancelled cancels the ticket.
//!   Voiding an order, or undoing the placement that opened a ticket, ends the ticket too.
//!
//! A ticket names its order by seat label (`PC 07`, `TV 3`, `Counter`) and number (`K-<n>`),
//! never by player.

use std::collections::BTreeMap;

use arena_proto::{
    AlertKind, AppError, ErrorCode, FLAG_KITCHEN, Intent, KitchenAdvance, KitchenAdvanceReverted,
    MOVE_SALE, STAGE_READY, STAGE_SERVED, next_stage, previous_stage,
};
use arena_store::{
    ChangeOp, HoldRow, LotRow, MoveRow, NewAlert, NewTicket, NewTicketLine, OrderRow, RowKey,
    StockRow, StoreTx, Table, TicketRow, ack_alert, count_unserved_lines, delete_row, find_device,
    find_device_meta, find_open_alert, find_open_alert_for_ticket, find_order, find_stock,
    find_ticket, insert_alert, insert_ticket, insert_ticket_line, list_line_holds_released_at,
    list_lots, list_open_tickets, list_order_moves, list_order_tickets, list_ticket_lines,
    list_ticket_lines_served_at, next_ticket_number, read_row, reopen_hold, restore_ticket_stage,
    set_order_ready, set_ticket_cancelled, set_ticket_stage, unmark_line_served,
};

use super::mutate::Written;
use super::order_line::{LineTarget, check_servable, is_open_bill, load_line, serve_line};
use super::pos_sale::internal;
use super::session::{check_rev, in_window, undo_denied};

/// What a ticket records of one kitchen line a placement added.
pub(super) struct PlacedKitchenLine {
    pub line_id: String,
    pub label: String,
    pub qty: i64,
    pub notes: Vec<String>,
}

fn written(key: RowKey, op: ChangeOp) -> Written {
    Written {
        key,
        op,
        before: None,
    }
}

/// The seat label of a device: `TV 3` for a console TV, else its floor label.
fn seat_label(tx: &StoreTx<'_>, device_id: Option<&str>) -> Result<String, AppError> {
    let Some(id) = device_id else {
        return Ok("Counter".to_owned());
    };
    if let Some(meta) = find_device_meta(tx, id)? {
        if let Some(n) = meta.tv_number {
            return Ok(format!("TV {n}"));
        }
        if !meta.label.trim().is_empty() {
            return Ok(meta.label);
        }
    }
    Ok(find_device(tx, id)?.map_or_else(|| "Counter".to_owned(), |d| d.label))
}

/// Opens the ticket of a placement's kitchen lines. Nothing happens without a kitchen line or on
/// a branch without the `kitchen` flag: a Free branch keeps the Epic 5 and 6 behaviour.
pub(super) fn open(
    tx: &StoreTx<'_>,
    order_id: &str,
    device_id: Option<&str>,
    placement_id: &str,
    lines: &[PlacedKitchenLine],
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    if lines.is_empty() || !crate::entitlement::has_flag(tx, FLAG_KITCHEN)? {
        return Ok(Vec::new());
    }
    let id = ulid::Ulid::generate().to_string();
    let seat = seat_label(tx, device_id)?;
    insert_ticket(
        tx,
        &NewTicket {
            id: &id,
            order_id,
            device_id,
            seat: &seat,
            number: next_ticket_number(tx)?,
            placement_id: Some(placement_id),
            placed_at: now_ms,
        },
    )?;
    let mut out = vec![written(
        RowKey::KitchenTickets { id: id.clone() },
        ChangeOp::Insert,
    )];
    for l in lines {
        let line_id = ulid::Ulid::generate().to_string();
        insert_ticket_line(
            tx,
            &NewTicketLine {
                id: &line_id,
                ticket_id: &id,
                order_line_id: &l.line_id,
                label: &l.label,
                qty: l.qty,
                notes: &l.notes,
            },
        )?;
        out.push(written(
            RowKey::KitchenTicketLines { id: line_id },
            ChangeOp::Insert,
        ));
    }
    Ok(out)
}

/// Raises the order-ready alert of a ticket that reached Ready (Story 7.4). A device ticket
/// raises one per device (a second Ready ticket of the same device adds none); a walk-up ticket
/// raises a branch-level alert that records the ticket.
fn raise_ready_alert(
    tx: &StoreTx<'_>,
    ticket: &TicketRow,
    now_ms: i64,
    out: &mut Vec<Written>,
) -> Result<(), AppError> {
    let kind = AlertKind::OrderReady.as_str();
    let open = match ticket.device_id.as_deref() {
        Some(d) => find_open_alert(tx, kind, Some(d))?,
        None => find_open_alert_for_ticket(tx, kind, &ticket.id)?,
    };
    if open.is_some() {
        return Ok(());
    }
    let id = ulid::Ulid::generate().to_string();
    insert_alert(
        tx,
        &NewAlert {
            id: &id,
            kind,
            device_id: ticket.device_id.as_deref(),
            raised_at: now_ms,
            product_id: None,
            qty: None,
            threshold: None,
            ticket_id: ticket.device_id.is_none().then_some(ticket.id.as_str()),
        },
    )?;
    out.push(written(RowKey::Alerts { id }, ChangeOp::Insert));
    Ok(())
}

/// Closes the order-ready alert a ticket left behind once the ticket is no longer Ready (served,
/// cancelled or its Ready undone): a walk-up ticket's own alert, or a device's alert when none of
/// the device's tickets is still Ready. Call it after the ticket's stage has been written.
pub(super) fn close_ready_alert(
    tx: &StoreTx<'_>,
    ticket_id: &str,
    device_id: Option<&str>,
    by: &str,
    now_ms: i64,
    out: &mut Vec<Written>,
) -> Result<(), AppError> {
    let kind = AlertKind::OrderReady.as_str();
    let open = match device_id {
        Some(d) => {
            if list_open_tickets(tx)?
                .iter()
                .any(|t| t.device_id.as_deref() == Some(d) && t.stage == STAGE_READY)
            {
                return Ok(());
            }
            find_open_alert(tx, kind, Some(d))?
        }
        None => find_open_alert_for_ticket(tx, kind, ticket_id)?,
    };
    if let Some(a) = open {
        ack_alert(tx, &a.id, now_ms, by)?;
        out.push(written(RowKey::Alerts { id: a.id }, ChangeOp::Update));
    }
    Ok(())
}

/// Deletes a ticket's lines (the ticket row stays, or is deleted by the caller).
fn delete_lines(tx: &StoreTx<'_>, ticket_id: &str, out: &mut Vec<Written>) -> Result<(), AppError> {
    for l in list_ticket_lines(tx, ticket_id)? {
        let key = RowKey::KitchenTicketLines { id: l.id };
        let before = read_row(tx, &key)?;
        delete_row(tx, Table::KitchenTicketLines, &before)?;
        out.push(Written {
            key,
            op: ChangeOp::Delete,
            before: Some(before),
        });
    }
    Ok(())
}

/// Removes an order's tickets with their lines: the placement that opened them was undone.
pub(super) fn delete_order_tickets(
    tx: &StoreTx<'_>,
    tickets: &[TicketRow],
    out: &mut Vec<Written>,
) -> Result<(), AppError> {
    for t in tickets {
        delete_lines(tx, &t.id, out)?;
        let key = RowKey::KitchenTickets { id: t.id.clone() };
        let before = read_row(tx, &key)?;
        delete_row(tx, Table::KitchenTickets, &before)?;
        out.push(Written {
            key,
            op: ChangeOp::Delete,
            before: Some(before),
        });
    }
    Ok(())
}

/// A ticket that has moved past New cannot be taken back with its placement.
pub(super) fn check_undoable(tickets: &[TicketRow]) -> Result<(), AppError> {
    if tickets.iter().any(|t| t.stage != "new") {
        return Err(undo_denied("the kitchen has started this order"));
    }
    Ok(())
}

/// Cancels every live ticket of an order that is being voided, dropping their lines.
pub(super) fn cancel_order_tickets(
    tx: &StoreTx<'_>,
    order_id: &str,
    reason: &str,
    by: &str,
    now_ms: i64,
    out: &mut Vec<Written>,
) -> Result<(), AppError> {
    for t in list_order_tickets(tx, order_id)? {
        if !matches!(t.stage.as_str(), "new" | "preparing" | "ready") {
            continue;
        }
        delete_lines(tx, &t.id, out)?;
        set_ticket_cancelled(tx, &t.id, reason, now_ms, by)?;
        out.push(written(
            RowKey::KitchenTickets { id: t.id.clone() },
            ChangeOp::Update,
        ));
        close_ready_alert(tx, &t.id, t.device_id.as_deref(), by, now_ms, out)?;
    }
    Ok(())
}

// ---- kitchen.advance ----

#[derive(Default)]
pub(crate) struct AdvanceTarget {
    ticket: Option<TicketRow>,
    order: Option<OrderRow>,
    /// The unserved lines to serve when the ticket reaches Served.
    serves: Vec<LineTarget>,
    /// Whether the branch has the `kitchen` flag.
    kitchen: bool,
}

pub(super) fn load_advance(
    tx: &StoreTx<'_>,
    a: &KitchenAdvance,
    now_ms: i64,
) -> Result<AdvanceTarget, AppError> {
    let kitchen = crate::entitlement::has_flag(tx, FLAG_KITCHEN)?;
    let Some(ticket) = find_ticket(tx, &a.ticket_id)? else {
        return Ok(AdvanceTarget {
            kitchen,
            ..AdvanceTarget::default()
        });
    };
    let order = find_order(tx, &ticket.order_id)?;
    let mut serves = Vec::new();
    if a.to == STAGE_SERVED {
        for l in list_ticket_lines(tx, &ticket.id)? {
            let target = load_line(tx, &l.order_line_id, now_ms)?;
            if target.is_unserved() && target.on_open_bill() {
                serves.push(target);
            }
        }
    }
    Ok(AdvanceTarget {
        ticket: Some(ticket),
        order,
        serves,
        kitchen,
    })
}

pub(super) fn base_rev_advance(intent: &Intent, t: &AdvanceTarget) -> Result<(), AppError> {
    match &t.ticket {
        Some(ticket) => check_rev(intent, ticket.rev, "kitchen.advance"),
        None => Ok(()),
    }
}

/// A ticket that does not exist is `NOT_FOUND`, and `TIER_LOCKED` on a branch without the kitchen
/// flag (only a ticket created before a lapse can still be moved).
fn missing(kitchen: bool) -> AppError {
    if kitchen {
        AppError::with_field(ErrorCode::NotFound, "ticket_id", "no such ticket")
    } else {
        AppError::new(ErrorCode::TierLocked, "available on Basic and above")
    }
}

pub(super) fn validate_advance(
    _intent: &Intent,
    a: &KitchenAdvance,
    t: &AdvanceTarget,
) -> Result<(), AppError> {
    let Some(ticket) = &t.ticket else {
        return Err(missing(t.kitchen));
    };
    if next_stage(&ticket.stage) != Some(a.to.as_str()) {
        return Err(AppError::with_field(
            ErrorCode::BadStage,
            "to",
            "a ticket moves one stage at a time",
        ));
    }
    for line in &t.serves {
        check_servable(line)?;
    }
    Ok(())
}

pub(super) fn mutate_advance(
    tx: &StoreTx<'_>,
    intent: &Intent,
    a: &KitchenAdvance,
    t: &AdvanceTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let ticket = t
        .ticket
        .as_ref()
        .ok_or_else(|| internal("an advance validated without its ticket"))?;
    let by = intent.actor.id.as_str();
    set_ticket_stage(tx, &ticket.id, &a.to, now_ms, by)?;
    let mut out = vec![written(
        RowKey::KitchenTickets {
            id: ticket.id.clone(),
        },
        ChangeOp::Update,
    )];
    if a.to == STAGE_READY {
        // The order.ready effect: the open bill is stamped so the HUD shows the ready copy.
        if let Some(o) = t
            .order
            .as_ref()
            .filter(|o| o.status == "open" && is_open_bill(o))
            && count_unserved_lines(tx, &o.id)? > 0
        {
            set_order_ready(tx, &o.id, Some(now_ms))?;
            out.push(written(
                RowKey::Orders { id: o.id.clone() },
                ChangeOp::Update,
            ));
        }
        raise_ready_alert(tx, ticket, now_ms, &mut out)?;
    } else if a.to == STAGE_SERVED {
        for line in &t.serves {
            out.extend(serve_line(tx, by, line, now_ms)?);
        }
        close_ready_alert(
            tx,
            &ticket.id,
            ticket.device_id.as_deref(),
            by,
            now_ms,
            &mut out,
        )?;
    }
    Ok(out)
}

// ---- kitchen.advance.reverted ----

#[derive(Default)]
pub(crate) struct RevertTarget {
    ticket: Option<TicketRow>,
    order: Option<OrderRow>,
    /// For a Served ticket: the lines its last advance served, and the holds it closed.
    served: Vec<String>,
    holds: Vec<HoldRow>,
    moves: Vec<MoveRow>,
    stocks: BTreeMap<String, StockRow>,
    lots: BTreeMap<String, Vec<LotRow>>,
    /// Whether the branch has the `kitchen` flag.
    kitchen: bool,
    now: i64,
}

pub(super) fn load_advance_reverted(
    tx: &StoreTx<'_>,
    r: &KitchenAdvanceReverted,
    now_ms: i64,
) -> Result<RevertTarget, AppError> {
    let kitchen = crate::entitlement::has_flag(tx, FLAG_KITCHEN)?;
    let Some(ticket) = find_ticket(tx, &r.ticket_id)? else {
        return Ok(RevertTarget {
            kitchen,
            now: now_ms,
            ..RevertTarget::default()
        });
    };
    let order = find_order(tx, &ticket.order_id)?;
    let mut target = RevertTarget {
        kitchen,
        now: now_ms,
        ..RevertTarget::default()
    };
    if ticket.stage == STAGE_SERVED
        && let (Some(at), Some(o)) = (ticket.advanced_at, &order)
    {
        target.served = list_ticket_lines_served_at(tx, &ticket.id, at)?;
        for line_id in &target.served {
            target
                .holds
                .extend(list_line_holds_released_at(tx, line_id, at)?);
        }
        let items: Vec<&str> = target.holds.iter().map(|h| h.item_id.as_str()).collect();
        target.moves = list_order_moves(tx, &o.id, MOVE_SALE)?
            .into_iter()
            .filter(|m| m.at == at && items.contains(&m.product_id.as_str()))
            .collect();
        for m in &target.moves {
            if let Some(stock) = find_stock(tx, &m.product_id)? {
                target.stocks.insert(m.product_id.clone(), stock);
            }
            target
                .lots
                .insert(m.product_id.clone(), list_lots(tx, &m.product_id)?);
        }
    }
    target.ticket = Some(ticket);
    target.order = order;
    Ok(target)
}

pub(super) fn base_rev_advance_reverted(intent: &Intent, t: &RevertTarget) -> Result<(), AppError> {
    match &t.ticket {
        Some(ticket) => check_rev(intent, ticket.rev, "kitchen.advance.reverted"),
        None => Ok(()),
    }
}

pub(super) fn validate_advance_reverted(intent: &Intent, t: &RevertTarget) -> Result<(), AppError> {
    let Some(ticket) = &t.ticket else {
        return Err(missing(t.kitchen));
    };
    if previous_stage(&ticket.stage).is_none() {
        return Err(AppError::with_field(
            ErrorCode::BadStage,
            "ticket_id",
            "this ticket has nothing to undo",
        ));
    }
    if ticket.advanced_by.as_deref() != Some(intent.actor.id.as_str()) {
        return Err(undo_denied("only who moved the ticket can undo it"));
    }
    if !in_window(t.now, ticket.advanced_at) {
        return Err(undo_denied("the undo window has closed"));
    }
    if ticket.stage == STAGE_SERVED {
        let open = t
            .order
            .as_ref()
            .is_none_or(|o| o.status == "open" && is_open_bill(o));
        if !open {
            return Err(undo_denied("the bill is no longer open"));
        }
        if !super::stock::sale_restorable(&t.moves, &t.stocks, &t.lots) {
            return Err(undo_denied("the stock has changed since it was served"));
        }
    }
    Ok(())
}

pub(super) fn mutate_advance_reverted(
    tx: &StoreTx<'_>,
    intent: &Intent,
    t: &RevertTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let ticket = t
        .ticket
        .as_ref()
        .ok_or_else(|| internal("an undo validated without its ticket"))?;
    let previous = previous_stage(&ticket.stage)
        .ok_or_else(|| internal("an undo validated without a previous stage"))?;
    let mut out = Vec::new();
    if ticket.stage == STAGE_READY {
        // Clear the ready mark this advance set (a later placement or ready may own it now).
        if let Some(o) = t
            .order
            .as_ref()
            .filter(|o| o.ready_at.is_some() && o.ready_at == ticket.advanced_at)
        {
            set_order_ready(tx, &o.id, None)?;
            out.push(written(
                RowKey::Orders { id: o.id.clone() },
                ChangeOp::Update,
            ));
        }
    } else if ticket.stage == STAGE_SERVED
        && let Some(o) = &t.order
    {
        let (stock_rows, changed) = super::stock::restore_sale_moves(
            tx,
            &t.moves,
            &o.id,
            ticket.placement_id.as_deref(),
            &intent.actor.id,
            now_ms,
        )?;
        out.extend(stock_rows);
        for (product_id, before, after) in changed {
            out.extend(super::stock::low_after_change(
                tx,
                &product_id,
                before,
                after,
                &intent.actor.id,
                now_ms,
            )?);
        }
        for h in &t.holds {
            reopen_hold(tx, &h.id)?;
            out.push(written(
                RowKey::StockHolds { id: h.id.clone() },
                ChangeOp::Update,
            ));
            out.extend(super::stock::touch(tx, &h.item_id)?);
        }
        for line_id in &t.served {
            unmark_line_served(tx, line_id)?;
            out.push(written(
                RowKey::OrderLines {
                    id: line_id.clone(),
                },
                ChangeOp::Update,
            ));
        }
    }
    restore_ticket_stage(tx, &ticket.id, previous)?;
    out.push(written(
        RowKey::KitchenTickets {
            id: ticket.id.clone(),
        },
        ChangeOp::Update,
    ));
    if ticket.stage == STAGE_READY {
        let by = intent.actor.id.as_str();
        close_ready_alert(
            tx,
            &ticket.id,
            ticket.device_id.as_deref(),
            by,
            now_ms,
            &mut out,
        )?;
    }
    Ok(out)
}
