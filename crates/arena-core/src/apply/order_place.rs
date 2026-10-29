//! `order.place` and `order.place.reverted` (Story 5.12): products added to a running session's
//! bill order. A placement adds product `order_lines` to the session's open bill order (created at
//! the first placement), takes stock through the same `sale` moves a counter sale writes (lot
//! order, expired lots never taken, a bundle's parts times the quantity) and treats every line as
//! served. The intent id is the placement's id: it is stored on the lines and on the stock moves,
//! so the 8 s Undo takes back that placement alone.
//!
//! The order total grows by the placed amount, so the ordinary settle, payments and cash events
//! work unchanged. No tax is computed on session bills in this story; the lines record the rate.

use std::collections::BTreeMap;

use arena_proto::{
    AppError, ErrorCode, Intent, KEY_INVENTORY_TAKE_STOCK_AT, MOVE_RETURN, MOVE_SALE,
    MOVE_SALE_REVERTED, OrderPlace, OrderPlaceReverted, SessionMode, TAKE_STOCK_AT_ORDER,
};
use arena_store::{
    ChangeOp, HoldRow, LiveSession, LotRow, MoveRow, NewHold, NewOrder, NewPosOrder,
    NewProductLine, OrderRow, PRODUCT_LINE_BASE, PlacedLine, RowKey, StockRow, StoreTx, Table,
    TicketRow, add_order_amount, delete_row, effect_recorded, find_order, find_order_for_session,
    find_placement_ticket, find_session, find_setting, find_stock, insert_hold, insert_order,
    insert_pos_order, insert_product_line, list_lots, list_order_line_ids, list_placement_holds,
    list_placement_lines, list_placement_moves, next_product_position, read_row, set_order_ready,
};

use super::mutate::Written;
use super::pos_sale::{
    Basket, StockFor, check_lines, internal, load_basket, move_stock, plan_stock, price,
};
use super::session::{check_rev, in_window, invalid, same_staff, undo_denied};

pub(crate) struct PlaceTarget {
    session: Option<LiveSession>,
    order: Option<OrderRow>,
    basket: Basket,
    /// Where the order's next product line goes.
    next_position: i64,
    /// Whether the branch takes stock when a line is served (`inventory.take_stock_at`).
    at_served: bool,
    /// For a counter order (Story 6.32): whether its PC exists, and whether its player does.
    counter_ok: (bool, bool),
}

/// Whether the branch takes stock at "served" (the default) rather than at placement.
pub(super) fn take_at_served(tx: &StoreTx<'_>) -> Result<bool, AppError> {
    Ok(find_setting(tx, KEY_INVENTORY_TAKE_STOCK_AT)?
        .is_none_or(|s| s.value != TAKE_STOCK_AT_ORDER))
}

/// Per line: its stock is held until served (a kitchen line while the branch takes stock at
/// served).
fn deferred(t: &PlaceTarget) -> Vec<bool> {
    t.basket
        .items
        .iter()
        .map(|i| t.at_served && i.product.as_ref().is_some_and(|p| p.kitchen))
        .collect()
}

pub(super) fn load_place(
    tx: &StoreTx<'_>,
    p: &OrderPlace,
    now_ms: i64,
) -> Result<PlaceTarget, AppError> {
    let session = find_session(tx, &p.session_id)?;
    let order = match &session {
        Some(s) => find_order_for_session(tx, &s.id)?,
        None => None,
    };
    let next_position = match &order {
        Some(o) => next_product_position(tx, &o.id)?,
        None => PRODUCT_LINE_BASE,
    };
    let counter_ok = match &p.counter {
        Some(c) => (
            arena_store::find_device(tx, &c.device_id)?.is_some(),
            match &c.player_id {
                Some(id) => arena_store::find_player(tx, id)?.is_some(),
                None => true,
            },
        ),
        None => (false, false),
    };
    Ok(PlaceTarget {
        session,
        order,
        basket: load_basket(tx, &p.lines, now_ms)?,
        next_position,
        at_served: take_at_served(tx)?,
        counter_ok,
    })
}

/// Whether an order can be placed on this session: a postpaid session that is not in a group
/// bill, or a block session, and it is running.
fn placeable(s: &LiveSession) -> bool {
    let running = matches!(s.state.as_str(), "active" | "paused" | "grace");
    let mode_ok = s.mode == SessionMode::Block.as_str()
        || (s.mode == SessionMode::Postpaid.as_str() && s.group_bill_id.is_none());
    running && mode_ok
}

pub(super) fn validate_place(
    intent: &Intent,
    p: &OrderPlace,
    t: &PlaceTarget,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "order.place takes no base_rev"));
    }
    if p.counter.is_some() {
        return validate_counter(intent, p, t);
    }
    let Some(s) = &t.session else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "session_id",
            "no such session",
        ));
    };
    if !placeable(s) {
        return Err(invalid(
            "session_id",
            "an order goes on a running postpaid or block session",
        ));
    }
    if let Some(o) = &t.order
        && (o.status != "open" || o.kind != "session")
    {
        return Err(invalid(
            "session_id",
            "this session's bill is already settled",
        ));
    }
    check_lines(&p.lines, &t.basket)?;
    super::kitchen_notes::check_lines(intent.actor.kind, &p.lines, &t.basket)?;
    price(&p.lines, &t.basket)?;
    plan_stock(
        &p.lines,
        &t.basket,
        p.stock_override == Some(true),
        &deferred(t),
    )?;
    Ok(())
}

/// An unpaid counter order (Story 6.32): the lines are tied to a PC, not a session, and nothing is
/// paid yet. The PC and the player (when named) must exist, no stock override applies, and the
/// lines follow the same rules as any order.
fn validate_counter(intent: &Intent, p: &OrderPlace, t: &PlaceTarget) -> Result<(), AppError> {
    if !p.session_id.is_empty() {
        return Err(invalid("session_id", "a counter order names no session"));
    }
    if p.stock_override == Some(true) {
        return Err(invalid(
            "stock_override",
            "a counter order takes no stock override",
        ));
    }
    if !t.counter_ok.0 {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "device_id",
            "no such PC",
        ));
    }
    if !t.counter_ok.1 {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "player_id",
            "no such player",
        ));
    }
    check_lines(&p.lines, &t.basket)?;
    super::kitchen_notes::check_lines(intent.actor.kind, &p.lines, &t.basket)?;
    price(&p.lines, &t.basket)?;
    plan_stock(&p.lines, &t.basket, false, &deferred(t))?;
    Ok(())
}

pub(super) fn mutate_place(
    tx: &StoreTx<'_>,
    intent: &Intent,
    p: &OrderPlace,
    t: &PlaceTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let (business_day, fy) = crate::locale::stamp(tx, now_ms)?
        .ok_or_else(|| internal("the branch has not been bootstrapped"))?;
    let priced = price(&p.lines, &t.basket)?;
    let plan = plan_stock(
        &p.lines,
        &t.basket,
        p.stock_override == Some(true),
        &deferred(t),
    )?;
    let amount: i64 = priced.lines.iter().map(|(_, a)| a).sum();
    let mut written = Vec::new();
    let order_id = match (&p.counter, &t.order) {
        (Some(c), _) => {
            let breakup = serde_json::to_string(&priced.bill.breakup)
                .map_err(|_| internal("the tax breakup cannot be written"))?;
            let id = ulid::Ulid::generate().to_string();
            insert_pos_order(
                tx,
                &NewPosOrder {
                    id: &id,
                    player_id: c.player_id.as_deref(),
                    device_id: Some(&c.device_id),
                    open: true,
                    subtotal_minor: priced.bill.subtotal_minor,
                    tax_minor: priced.bill.tax_minor,
                    total_minor: priced.bill.total_minor,
                    tax_breakup: &breakup,
                    settled_at: now_ms,
                    settled_by: &intent.actor.id,
                    business_day: &business_day,
                    fy: &fy,
                },
            )?;
            written.push(Written {
                key: RowKey::Orders { id: id.clone() },
                op: ChangeOp::Insert,
                before: None,
            });
            id
        }
        (None, Some(o)) => {
            add_order_amount(tx, &o.id, amount)?;
            // More food is coming: an earlier "ready" no longer covers the order.
            set_order_ready(tx, &o.id, None)?;
            written.push(Written {
                key: RowKey::Orders { id: o.id.clone() },
                op: ChangeOp::Update,
                before: None,
            });
            o.id.clone()
        }
        (None, None) => {
            let s = t
                .session
                .as_ref()
                .ok_or_else(|| internal("a placement validated without its session"))?;
            let id = ulid::Ulid::generate().to_string();
            insert_order(
                tx,
                &NewOrder {
                    id: &id,
                    session_id: &s.id,
                    player_id: s.player_id.as_deref(),
                    guest_no: s.guest_no,
                    device_id: &s.device_id,
                    subtotal_minor: amount,
                    business_day: &s.business_day,
                    fy: &s.fy,
                },
            )?;
            written.push(Written {
                key: RowKey::Orders { id: id.clone() },
                op: ChangeOp::Insert,
                before: None,
            });
            id
        }
    };
    let placement_id = intent.intent_id.as_str();
    let mut kitchen_lines = Vec::new();
    for (i, ((line, item), (rate_bp, line_amount))) in p
        .lines
        .iter()
        .zip(&t.basket.items)
        .zip(&priced.lines)
        .enumerate()
    {
        let product = item
            .product
            .as_ref()
            .ok_or_else(|| internal("a placed line written without its product"))?;
        let id = ulid::Ulid::generate().to_string();
        insert_product_line(
            tx,
            &NewProductLine {
                id: &id,
                order_id: &order_id,
                product_id: &product.product_id,
                label: &product.name,
                qty: line.qty,
                unit_price_minor: product.price_minor,
                tax_rate_bp: i64::from(*rate_bp),
                amount_minor: *line_amount,
                position: t.next_position + i as i64,
                at: now_ms,
                placement: Some((placement_id, &intent.actor.id)),
                served: !product.kitchen,
                kitchen_notes: &super::kitchen_notes::clean(&line.kitchen_notes),
            },
        )?;
        written.push(Written {
            key: RowKey::OrderLines { id: id.clone() },
            op: ChangeOp::Insert,
            before: None,
        });
        if product.kitchen {
            kitchen_lines.push(super::kitchen_ticket::PlacedKitchenLine {
                line_id: id.clone(),
                label: product.name.clone(),
                qty: line.qty,
                notes: super::kitchen_notes::clean(&line.kitchen_notes),
            });
        }
        for h in plan.holds.iter().filter(|h| h.line == i) {
            let hold_id = ulid::Ulid::generate().to_string();
            insert_hold(
                tx,
                &NewHold {
                    id: &hold_id,
                    order_line_id: &id,
                    item_id: &h.product_id,
                    qty: h.qty,
                },
            )?;
            written.push(Written {
                key: RowKey::StockHolds { id: hold_id },
                op: ChangeOp::Insert,
                before: None,
            });
            written.extend(super::stock::touch(tx, &h.product_id)?);
        }
    }
    let seat_device = match (&p.counter, &t.session) {
        (Some(c), _) => Some(c.device_id.as_str()),
        (None, Some(s)) => Some(s.device_id.as_str()),
        (None, None) => None,
    };
    written.extend(super::kitchen_ticket::open(
        tx,
        &order_id,
        seat_device,
        placement_id,
        &kitchen_lines,
        now_ms,
    )?);
    move_stock(
        tx,
        intent,
        &t.basket,
        &plan,
        &StockFor {
            order_id: &order_id,
            placement_id: Some(placement_id),
            today: &business_day,
        },
        now_ms,
        &mut written,
    )?;
    Ok(written)
}

#[derive(Default)]
pub(crate) struct RevertTarget {
    order: Option<OrderRow>,
    /// The lines the placement added (none once it was undone).
    placed: Vec<PlacedLine>,
    /// How many lines the order holds in all.
    all_lines: usize,
    moves: Vec<MoveRow>,
    /// Whether the placement's stock was already given back.
    given_back: bool,
    /// Whether a cancelled line of the placement returned its stock (order mode).
    returned: bool,
    /// The placement's open stock holds.
    holds: Vec<HoldRow>,
    stocks: BTreeMap<String, StockRow>,
    lots: BTreeMap<String, Vec<LotRow>>,
    effect: bool,
    /// The kitchen ticket the placement opened (Story 7.3).
    tickets: Vec<TicketRow>,
    now: i64,
}

pub(super) fn load_place_reverted(
    tx: &StoreTx<'_>,
    r: &OrderPlaceReverted,
    now: i64,
) -> Result<RevertTarget, AppError> {
    let Some(order) = find_order(tx, &r.order_id)? else {
        return Ok(RevertTarget {
            now,
            ..RevertTarget::default()
        });
    };
    let moves = list_placement_moves(tx, &r.placement_id, MOVE_SALE)?;
    let given_back = !list_placement_moves(tx, &r.placement_id, MOVE_SALE_REVERTED)?.is_empty();
    let mut stocks = BTreeMap::new();
    let mut lots = BTreeMap::new();
    for m in &moves {
        if let Some(stock) = find_stock(tx, &m.product_id)? {
            stocks.insert(m.product_id.clone(), stock);
        }
        lots.insert(m.product_id.clone(), list_lots(tx, &m.product_id)?);
    }
    Ok(RevertTarget {
        placed: list_placement_lines(tx, &order.id, &r.placement_id)?,
        all_lines: list_order_line_ids(tx, &order.id)?.len(),
        moves,
        given_back,
        returned: !list_placement_moves(tx, &r.placement_id, MOVE_RETURN)?.is_empty(),
        holds: list_placement_holds(tx, &order.id, &r.placement_id)?,
        stocks,
        lots,
        effect: effect_recorded(tx, Some(&r.placement_id), &order.id)?,
        tickets: find_placement_ticket(tx, &order.id, &r.placement_id)?
            .into_iter()
            .collect(),
        order: Some(order),
        now,
    })
}

pub(super) fn base_rev_place_reverted(intent: &Intent, t: &RevertTarget) -> Result<(), AppError> {
    let Some(o) = &t.order else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "order_id",
            "no such bill",
        ));
    };
    check_rev(intent, o.rev, "order.place.reverted")
}

pub(super) fn validate_place_reverted(intent: &Intent, t: &RevertTarget) -> Result<(), AppError> {
    let Some(o) = &t.order else {
        return Err(invalid("order_id", "no such bill"));
    };
    if o.kind != "session" || o.session_id.is_none() {
        return Err(invalid("order_id", "this bill is not a session bill"));
    }
    let Some(first) = t.placed.first() else {
        return Err(invalid("placement_id", "no such placement on this bill"));
    };
    if o.status != "open" {
        return Err(undo_denied("the bill is no longer open"));
    }
    if t.given_back {
        return Err(undo_denied("this order was already undone"));
    }
    if !same_staff(intent, first.placed_by.as_deref()) {
        return Err(undo_denied("only the staff member who placed it can undo"));
    }
    if !in_window(t.now, first.placed_at) {
        return Err(undo_denied("the undo window has closed"));
    }
    if t.returned {
        return Err(undo_denied("an item of this order was cancelled"));
    }
    if t.effect {
        return Err(undo_denied("a receipt, drawer or invoice already happened"));
    }
    super::kitchen_ticket::check_undoable(&t.tickets)?;
    if !super::stock::sale_restorable(&t.moves, &t.stocks, &t.lots) {
        return Err(undo_denied("the stock has changed since the order"));
    }
    Ok(())
}

pub(super) fn mutate_place_reverted(
    tx: &StoreTx<'_>,
    intent: &Intent,
    r: &OrderPlaceReverted,
    t: &RevertTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let Some(o) = &t.order else {
        return Err(internal("an undo validated without its target"));
    };
    let mut written = Vec::new();
    let (stock_rows, changed) = super::stock::restore_sale_moves(
        tx,
        &t.moves,
        &o.id,
        Some(&r.placement_id),
        &intent.actor.id,
        now_ms,
    )?;
    written.extend(stock_rows);
    for (product_id, before, after) in changed {
        written.extend(super::stock::low_after_change(
            tx,
            &product_id,
            before,
            after,
            &intent.actor.id,
            now_ms,
        )?);
    }
    for h in &t.holds {
        let key = RowKey::StockHolds { id: h.id.clone() };
        let before = read_row(tx, &key)?;
        delete_row(tx, Table::StockHolds, &before)?;
        written.push(Written {
            key,
            op: ChangeOp::Delete,
            before: Some(before),
        });
        written.extend(super::stock::touch(tx, &h.item_id)?);
    }
    super::kitchen_ticket::delete_order_tickets(tx, &t.tickets, now_ms, &mut written)?;
    let mut amount = 0;
    for line in &t.placed {
        amount += line.amount_minor;
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
    }
    let key = RowKey::Orders { id: o.id.clone() };
    if t.all_lines == t.placed.len() {
        let before = read_row(tx, &key)?;
        delete_row(tx, Table::Orders, &before)?;
        written.push(Written {
            key,
            op: ChangeOp::Delete,
            before: Some(before),
        });
    } else {
        add_order_amount(tx, &o.id, -amount)?;
        written.push(Written {
            key,
            op: ChangeOp::Update,
            before: None,
        });
    }
    Ok(written)
}
