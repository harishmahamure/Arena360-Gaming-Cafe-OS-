use arena_proto::{
    AlertKind, AppError, DEFAULT_STOCK_EXPIRY_WARN_DAYS, ErrorCode, Intent, IntentPayload,
    KEY_STOCK_EXPIRY_WARN_DAYS, MAX_PRICE_MINOR, MAX_STOCK_IN_QTY, MAX_STOCK_QTY,
    MAX_SUPPLIER_CHARS, MOVE_COUNT_ADJUST, MOVE_EXPIRED, MOVE_RETURN, MOVE_SALE,
    MOVE_SALE_REVERTED, MOVE_STOCK_IN, MOVE_STOCK_IN_REVERTED, MOVE_WASTE, MOVE_WASTE_REVERTED,
    NOTE_SALE_OVERRIDE, STOCK_EXPIRY_WARN_RANGE, STOCK_UNDO_MS, StockIn, StockInReverted,
    StockThreshold, StockWaste, StockWasteReverted, StockWriteOff, add_days, consume_plan,
    lot_expired, lot_in_window, valid_date, weighted_avg_cost,
};
use arena_store::{
    AlertRow, ChangeOp, LotRow, MoveRow, NewAlert, ProductRow, RowKey, StockRow, StoreTx, Table,
    ack_alert, delete_row, find_lot, find_move, find_open_alert, find_open_alert_for_product,
    find_product, find_setting, find_stock, insert_alert, insert_lot, insert_move, insert_stock,
    latest_branch_alert_at, latest_move_id, list_expiring_lots, list_lots, low_stock_qty, read_row,
    set_alert_qty, set_alert_threshold, set_lot_remaining, set_low_stock_qty, update_stock,
};

use super::mutate::Written;

#[derive(Default)]
pub(crate) struct StockTarget {
    product: Option<ProductRow>,
    stock: Option<StockRow>,
    today: Option<String>,
    mv: Option<MoveRow>,
    lot: Option<LotRow>,
    latest: Option<String>,
    lots: Vec<LotRow>,
    now: i64,
    /// Days before expiry the branch warns (`stock.expiry_warn_days`).
    warn_days: i64,
    /// Lots with stock left that are expired or inside the warning window (with product name).
    window: Vec<(LotRow, String)>,
    /// The open "Expiring soon" alert, if any.
    open_alert: Option<AlertRow>,
    /// Whether an "Expiring soon" alert was already raised in today's business day.
    raised_today: bool,
    /// The product's low-stock threshold now (`stock.threshold`).
    threshold: Option<i64>,
    /// The open "Low stock" alert of the product (`stock.threshold`).
    low_alert: Option<AlertRow>,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>, p: &IntentPayload, now: i64) -> Result<StockTarget, AppError> {
    let mut t = StockTarget {
        now,
        ..StockTarget::default()
    };
    let product_id = match p {
        IntentPayload::StockIn(u) => {
            t.today = crate::locale::stamp(tx, now)?.map(|(day, _)| day);
            if u.expiry.is_some() {
                load_expiry(tx, &mut t)?;
            }
            Some(u.product_id.clone())
        }
        IntentPayload::StockInReverted(r) => {
            t.mv = find_move(tx, &r.move_id)?;
            if let Some(m) = &t.mv {
                t.lot = m
                    .lot_id
                    .as_deref()
                    .map(|l| find_lot(tx, l))
                    .transpose()?
                    .flatten();
                t.latest = latest_move_id(tx, &m.product_id)?;
            }
            t.mv.as_ref().map(|m| m.product_id.clone())
        }
        IntentPayload::StockWriteOff(w) => {
            t.today = crate::locale::stamp(tx, now)?.map(|(day, _)| day);
            t.lot = find_lot(tx, &w.lot_id)?;
            load_expiry(tx, &mut t)?;
            t.lot.as_ref().map(|l| l.product_id.clone())
        }
        IntentPayload::StockExpiryCheck(_) => {
            t.today = crate::locale::stamp(tx, now)?.map(|(day, _)| day);
            load_expiry(tx, &mut t)?;
            None
        }
        IntentPayload::StockWaste(w) => Some(w.product_id.clone()),
        IntentPayload::StockThreshold(w) => {
            t.threshold = low_stock_qty(tx, &w.product_id)?;
            t.low_alert =
                find_open_alert_for_product(tx, AlertKind::StockLow.as_str(), &w.product_id)?;
            Some(w.product_id.clone())
        }
        IntentPayload::StockWasteReverted(r) => {
            t.mv = find_move(tx, &r.move_id)?;
            if let Some(m) = &t.mv {
                t.lots = list_lots(tx, &m.product_id)?;
                t.latest = latest_move_id(tx, &m.product_id)?;
            }
            t.mv.as_ref().map(|m| m.product_id.clone())
        }
        _ => None,
    };
    if let Some(id) = product_id {
        t.product = find_product(tx, &id)?.filter(|r| r.deleted_at.is_none());
        t.stock = find_stock(tx, &id)?;
    }
    Ok(t)
}

pub(super) fn validate_in(u: &StockIn, t: &StockTarget) -> Result<(), AppError> {
    match &t.product {
        Some(p) if !p.bundle && p.track_stock => {}
        _ => {
            return Err(invalid(
                "product_id",
                "stock is recorded for tracked products that are not bundles",
            ));
        }
    }
    let held = t.stock.as_ref().map_or(0, |s| s.qty);
    if !(1..=MAX_STOCK_IN_QTY).contains(&u.qty) || held + u.qty > MAX_STOCK_QTY {
        return Err(invalid("qty", "quantity is out of range"));
    }
    if !(0..=MAX_PRICE_MINOR).contains(&u.unit_cost_minor) {
        return Err(invalid("unit_cost_minor", "cost is out of range"));
    }
    let supplier = u.supplier.trim();
    if supplier.chars().count() > MAX_SUPPLIER_CHARS || supplier.chars().any(char::is_control) {
        return Err(invalid("supplier", "supplier must be up to 80 characters"));
    }
    if let Some(e) = &u.expiry
        && (!valid_date(e) || t.today.as_deref().is_some_and(|d| e.as_str() < d))
    {
        return Err(invalid("expiry", "expiry must be today or later"));
    }
    Ok(())
}

pub(super) fn validate_revert(
    intent: &Intent,
    r: &StockInReverted,
    t: &StockTarget,
) -> Result<(), AppError> {
    let Some(m) = t.mv.as_ref().filter(|m| m.reason == MOVE_STOCK_IN) else {
        return Err(invalid("move_id", "no such stock-in"));
    };
    if intent.actor.id != m.staff_id {
        return Err(crate::rbac::denied());
    }
    if t.now - m.at > STOCK_UNDO_MS {
        return Err(AppError::with_field(
            ErrorCode::Expired,
            "move_id",
            "the undo window has closed",
        ));
    }
    let intact = t.lot.as_ref().is_some_and(|l| l.qty_remaining == l.qty_in);
    if t.latest.as_deref() != Some(r.move_id.as_str()) || !intact || t.stock.is_none() {
        return Err(AppError::with_field(
            ErrorCode::Stale,
            "move_id",
            "the stock changed since this stock-in",
        ));
    }
    Ok(())
}

fn write(key: RowKey, op: ChangeOp, before: Option<arena_store::Row>) -> Written {
    Written { key, op, before }
}

pub(super) fn mutate_in(
    tx: &StoreTx<'_>,
    intent: &Intent,
    u: &StockIn,
    t: &StockTarget,
    now: i64,
) -> Result<Vec<Written>, AppError> {
    let (qty, avg) = t
        .stock
        .as_ref()
        .map_or((0, 0), |s| (s.qty, s.avg_cost_minor));
    let new_avg = weighted_avg_cost(qty, avg, u.qty, u.unit_cost_minor);
    let stock_op = if t.stock.is_some() {
        update_stock(tx, &u.product_id, qty + u.qty, new_avg)?;
        ChangeOp::Update
    } else {
        insert_stock(tx, &u.product_id, qty + u.qty, new_avg)?;
        ChangeOp::Insert
    };
    let lot_id = ulid::Ulid::generate().to_string();
    insert_lot(
        tx,
        &LotRow {
            lot_id: lot_id.clone(),
            product_id: u.product_id.clone(),
            qty_in: u.qty,
            qty_remaining: u.qty,
            unit_cost_minor: u.unit_cost_minor,
            expiry: u.expiry.clone(),
            received_at: now,
        },
    )?;
    let move_id = ulid::Ulid::generate().to_string();
    insert_move(
        tx,
        &MoveRow {
            move_id: move_id.clone(),
            product_id: u.product_id.clone(),
            reason: MOVE_STOCK_IN.to_owned(),
            delta: u.qty,
            before_qty: qty,
            before_avg_cost_minor: avg,
            unit_cost_minor: u.unit_cost_minor,
            supplier: u.supplier.trim().to_owned(),
            lot_id: Some(lot_id.clone()),
            lots_used: "[]".to_owned(),
            staff_id: intent.actor.id.clone(),
            at: now,
            waste_reason: None,
            value_minor: 0,
            order_id: None,
            note: None,
            elevated_by: None,
            placement_id: None,
        },
    )?;
    let mut out = vec![
        write(
            RowKey::Stock {
                product_id: u.product_id.clone(),
            },
            stock_op,
            None,
        ),
        write(RowKey::StockLots { lot_id }, ChangeOp::Insert, None),
        write(RowKey::StockMoves { move_id }, ChangeOp::Insert, None),
    ];
    if stock_in_alerts(u, t) {
        out.push(raise_expiring(tx, now)?);
    }
    out.extend(low_after_change(
        tx,
        &u.product_id,
        qty,
        qty + u.qty,
        &intent.actor.id,
        now,
    )?);
    Ok(out)
}

pub(super) fn mutate_revert(
    tx: &StoreTx<'_>,
    intent: &Intent,
    t: &StockTarget,
    now: i64,
) -> Result<Vec<Written>, AppError> {
    let internal = || AppError::new(ErrorCode::Internal, "the stock target is incomplete");
    let (m, lot, stock) = match (&t.mv, &t.lot, &t.stock) {
        (Some(m), Some(l), Some(s)) => (m, l, s),
        _ => return Err(internal()),
    };
    update_stock(tx, &m.product_id, m.before_qty, m.before_avg_cost_minor)?;
    let key = RowKey::StockLots {
        lot_id: lot.lot_id.clone(),
    };
    let before = read_row(tx, &key)?;
    delete_row(tx, Table::StockLots, &before)?;
    let move_id = ulid::Ulid::generate().to_string();
    insert_move(
        tx,
        &MoveRow {
            move_id: move_id.clone(),
            product_id: m.product_id.clone(),
            reason: MOVE_STOCK_IN_REVERTED.to_owned(),
            delta: -m.delta,
            before_qty: stock.qty,
            before_avg_cost_minor: stock.avg_cost_minor,
            unit_cost_minor: m.unit_cost_minor,
            supplier: m.supplier.clone(),
            lot_id: Some(lot.lot_id.clone()),
            lots_used: used_json(&[(lot.lot_id.clone(), lot.qty_remaining)]),
            staff_id: intent.actor.id.clone(),
            at: now,
            waste_reason: None,
            value_minor: 0,
            order_id: None,
            note: None,
            elevated_by: None,
            placement_id: None,
        },
    )?;
    let mut out = vec![
        write(
            RowKey::Stock {
                product_id: m.product_id.clone(),
            },
            ChangeOp::Update,
            None,
        ),
        write(key, ChangeOp::Delete, Some(before)),
        write(RowKey::StockMoves { move_id }, ChangeOp::Insert, None),
    ];
    out.extend(low_after_change(
        tx,
        &m.product_id,
        stock.qty,
        m.before_qty,
        &intent.actor.id,
        now,
    )?);
    Ok(out)
}

fn used_json(used: &[(String, i64)]) -> String {
    let items: Vec<String> = used
        .iter()
        .map(|(lot_id, qty)| format!("{{\"lot_id\":\"{lot_id}\",\"qty\":{qty}}}"))
        .collect();
    format!("[{}]", items.join(","))
}

#[derive(serde::Deserialize)]
struct LotUse {
    lot_id: String,
    qty: i64,
}

fn parse_used(json: &str) -> Vec<(String, i64)> {
    serde_json::from_str::<Vec<LotUse>>(json)
        .unwrap_or_default()
        .into_iter()
        .map(|u| (u.lot_id, u.qty))
        .collect()
}

pub(super) fn validate_waste(w: &StockWaste, t: &StockTarget) -> Result<(), AppError> {
    match &t.product {
        Some(p) if !p.bundle && p.track_stock => {}
        _ => {
            return Err(invalid(
                "product_id",
                "waste is recorded for tracked products that are not bundles",
            ));
        }
    }
    if w.qty < 1 {
        return Err(invalid("qty", "quantity must be at least 1"));
    }
    if w.qty > t.stock.as_ref().map_or(0, |s| s.qty) {
        return Err(AppError::with_field(
            ErrorCode::OutOfStock,
            "qty",
            "not enough stock on hand",
        ));
    }
    Ok(())
}

pub(super) fn mutate_waste(
    tx: &StoreTx<'_>,
    intent: &Intent,
    w: &StockWaste,
    now: i64,
) -> Result<Vec<Written>, AppError> {
    let reason = intent.reason.map(|r| r.as_str());
    consume(
        tx,
        &w.product_id,
        w.qty,
        MOVE_WASTE,
        reason,
        &intent.actor.id,
        now,
    )
}

pub(super) fn validate_waste_revert(
    intent: &Intent,
    r: &StockWasteReverted,
    t: &StockTarget,
) -> Result<(), AppError> {
    let Some(m) = t.mv.as_ref().filter(|m| m.reason == MOVE_WASTE) else {
        return Err(invalid("move_id", "no such waste entry"));
    };
    if intent.actor.id != m.staff_id {
        return Err(crate::rbac::denied());
    }
    if t.now - m.at > STOCK_UNDO_MS {
        return Err(AppError::with_field(
            ErrorCode::Expired,
            "move_id",
            "the undo window has closed",
        ));
    }
    let used = parse_used(&m.lots_used);
    let intact = !used.is_empty()
        && used.iter().all(|(id, qty)| {
            t.lots
                .iter()
                .find(|l| &l.lot_id == id)
                .is_some_and(|l| l.qty_remaining + qty <= l.qty_in)
        });
    if t.latest.as_deref() != Some(r.move_id.as_str()) || !intact || t.stock.is_none() {
        return Err(AppError::with_field(
            ErrorCode::Stale,
            "move_id",
            "the stock changed since this waste entry",
        ));
    }
    Ok(())
}

/// Gives the quantity back to the lots the waste took it from, and appends the compensating
/// move (same reason and value, positive delta).
pub(super) fn mutate_waste_revert(
    tx: &StoreTx<'_>,
    intent: &Intent,
    t: &StockTarget,
    now: i64,
) -> Result<Vec<Written>, AppError> {
    let (Some(m), Some(stock)) = (&t.mv, &t.stock) else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "the stock target is incomplete",
        ));
    };
    let used = parse_used(&m.lots_used);
    let mut out = Vec::new();
    update_stock(tx, &m.product_id, m.before_qty, stock.avg_cost_minor)?;
    out.push(write(
        RowKey::Stock {
            product_id: m.product_id.clone(),
        },
        ChangeOp::Update,
        None,
    ));
    for (lot_id, qty) in &used {
        let held = t
            .lots
            .iter()
            .find(|l| &l.lot_id == lot_id)
            .map_or(0, |l| l.qty_remaining);
        set_lot_remaining(tx, lot_id, held + qty)?;
        out.push(write(
            RowKey::StockLots {
                lot_id: lot_id.clone(),
            },
            ChangeOp::Update,
            None,
        ));
    }
    let move_id = ulid::Ulid::generate().to_string();
    insert_move(
        tx,
        &MoveRow {
            move_id: move_id.clone(),
            product_id: m.product_id.clone(),
            reason: MOVE_WASTE_REVERTED.to_owned(),
            delta: -m.delta,
            before_qty: stock.qty,
            before_avg_cost_minor: stock.avg_cost_minor,
            unit_cost_minor: m.unit_cost_minor,
            supplier: String::new(),
            lot_id: None,
            lots_used: m.lots_used.clone(),
            staff_id: intent.actor.id.clone(),
            at: now,
            waste_reason: m.waste_reason.clone(),
            value_minor: m.value_minor,
            order_id: None,
            note: None,
            elevated_by: None,
            placement_id: None,
        },
    )?;
    out.push(write(
        RowKey::StockMoves { move_id },
        ChangeOp::Insert,
        None,
    ));
    out.extend(low_after_change(
        tx,
        &m.product_id,
        stock.qty,
        m.before_qty,
        &intent.actor.id,
        now,
    )?);
    Ok(out)
}

pub fn decrement(
    tx: &StoreTx<'_>,
    product_id: &str,
    qty: i64,
    reason: &str,
    staff_id: &str,
    now: i64,
) -> Result<Vec<Written>, AppError> {
    consume(tx, product_id, qty, reason, None, staff_id, now)
}

/// Lowers a product's stock in lot order (earliest expiry first) and appends the move, valued
/// at the average cost. `OUT_OF_STOCK` above the quantity on hand, `VALIDATION_FAILED qty` for
/// a quantity below 1.
fn consume(
    tx: &StoreTx<'_>,
    product_id: &str,
    qty: i64,
    reason: &str,
    waste_reason: Option<&str>,
    staff_id: &str,
    now: i64,
) -> Result<Vec<Written>, AppError> {
    consume_for(
        tx,
        product_id,
        qty,
        reason,
        waste_reason,
        None,
        staff_id,
        now,
    )
}

/// [`consume`] for a move that belongs to an order (the waste of a cancelled line).
#[allow(clippy::too_many_arguments)]
pub(super) fn consume_for(
    tx: &StoreTx<'_>,
    product_id: &str,
    qty: i64,
    reason: &str,
    waste_reason: Option<&str>,
    order_id: Option<&str>,
    staff_id: &str,
    now: i64,
) -> Result<Vec<Written>, AppError> {
    let short = || AppError::with_field(ErrorCode::OutOfStock, "qty", "not enough stock on hand");
    if qty <= 0 {
        return Err(invalid("qty", "quantity must be at least 1"));
    }
    let stock = find_stock(tx, product_id)?.ok_or_else(short)?;
    if qty > stock.qty {
        return Err(short());
    }
    let lots = list_lots(tx, product_id)?;
    let views: Vec<_> = lots.iter().map(LotRow::view).collect();
    let plan = consume_plan(&views, qty).ok_or_else(short)?;
    let mut out = Vec::new();
    update_stock(tx, product_id, stock.qty - qty, stock.avg_cost_minor)?;
    out.push(write(
        RowKey::Stock {
            product_id: product_id.to_owned(),
        },
        ChangeOp::Update,
        None,
    ));
    for (lot_id, take) in &plan {
        let left = lots
            .iter()
            .find(|l| &l.lot_id == lot_id)
            .map_or(0, |l| l.qty_remaining)
            - take;
        set_lot_remaining(tx, lot_id, left)?;
        out.push(write(
            RowKey::StockLots {
                lot_id: lot_id.clone(),
            },
            ChangeOp::Update,
            None,
        ));
    }
    let move_id = ulid::Ulid::generate().to_string();
    insert_move(
        tx,
        &MoveRow {
            move_id: move_id.clone(),
            product_id: product_id.to_owned(),
            reason: reason.to_owned(),
            delta: -qty,
            before_qty: stock.qty,
            before_avg_cost_minor: stock.avg_cost_minor,
            unit_cost_minor: stock.avg_cost_minor,
            supplier: String::new(),
            lot_id: None,
            lots_used: used_json(&plan),
            staff_id: staff_id.to_owned(),
            at: now,
            waste_reason: waste_reason.map(str::to_owned),
            value_minor: qty.saturating_mul(stock.avg_cost_minor),
            order_id: order_id.map(str::to_owned),
            note: None,
            elevated_by: None,
            placement_id: None,
        },
    )?;
    out.push(write(
        RowKey::StockMoves { move_id },
        ChangeOp::Insert,
        None,
    ));
    out.extend(low_after_change(
        tx,
        product_id,
        stock.qty,
        stock.qty - qty,
        staff_id,
        now,
    )?);
    Ok(out)
}

/// The branch's expiry warning in days: `stock.expiry_warn_days`, or the default when the row
/// is missing or holds something out of range.
pub(crate) fn warn_days(tx: &StoreTx<'_>) -> Result<i64, AppError> {
    let default = DEFAULT_STOCK_EXPIRY_WARN_DAYS.parse().unwrap_or(3);
    Ok(find_setting(tx, KEY_STOCK_EXPIRY_WARN_DAYS)?
        .and_then(|s| s.value.parse::<i64>().ok())
        .filter(|d| (STOCK_EXPIRY_WARN_RANGE.0..=STOCK_EXPIRY_WARN_RANGE.1).contains(d))
        .unwrap_or(default))
}

/// Loads what the expiry alert needs: the warning days, the lots with stock left that are
/// expired or inside the window, the open alert, and whether one was raised today.
fn load_expiry(tx: &StoreTx<'_>, t: &mut StockTarget) -> Result<(), AppError> {
    t.warn_days = warn_days(tx)?;
    if let Some(last) = t
        .today
        .as_deref()
        .and_then(|today| add_days(today, t.warn_days))
    {
        t.window = list_expiring_lots(tx, &last)?;
    }
    let kind = AlertKind::StockExpiring.as_str();
    t.open_alert = find_open_alert(tx, kind, None)?;
    t.raised_today = match (latest_branch_alert_at(tx, kind)?, t.today.as_deref()) {
        (Some(at), Some(today)) => {
            crate::locale::stamp(tx, at)?.is_some_and(|(day, _)| day == today)
        }
        _ => false,
    };
    Ok(())
}

fn raise_expiring(tx: &StoreTx<'_>, now: i64) -> Result<Written, AppError> {
    let id = ulid::Ulid::generate().to_string();
    insert_alert(
        tx,
        &NewAlert {
            id: &id,
            kind: AlertKind::StockExpiring.as_str(),
            device_id: None,
            raised_at: now,
            product_id: None,
            qty: None,
            threshold: None,
            ticket_id: None,
        },
    )?;
    Ok(write(RowKey::Alerts { id }, ChangeOp::Insert, None))
}

fn ack_row(tx: &StoreTx<'_>, alert: &AlertRow, by: &str, now: i64) -> Result<Written, AppError> {
    ack_alert(tx, &alert.id, now, by)?;
    Ok(write(
        RowKey::Alerts {
            id: alert.id.clone(),
        },
        ChangeOp::Update,
        None,
    ))
}

/// Acknowledges the open "Low stock" alert of a product that is deleted, untracked or made a
/// bundle (it no longer shows in Low stock, so its alert must not linger and sound).
pub(crate) fn ack_low_for(
    tx: &StoreTx<'_>,
    product_id: &str,
    by: &str,
    now: i64,
) -> Result<Vec<Written>, AppError> {
    match find_open_alert_for_product(tx, AlertKind::StockLow.as_str(), product_id)? {
        Some(alert) => Ok(vec![ack_row(tx, &alert, by, now)?]),
        None => Ok(Vec::new()),
    }
}

/// The supplier an imported opening quantity is booked under.
pub(super) const SUPPLIER_IMPORT: &str = "CSV import";

/// Opening stock for a product an import wrote (Story 5.33): a stock-in lot with no expiry, as
/// `stock.in` would write it. The product is already validated as tracked.
pub(super) fn opening_stock(
    tx: &StoreTx<'_>,
    intent: &Intent,
    product_id: &str,
    qty: i64,
    unit_cost_minor: i64,
    now: i64,
) -> Result<Vec<Written>, AppError> {
    let t = StockTarget {
        stock: find_stock(tx, product_id)?,
        now,
        ..StockTarget::default()
    };
    let u = StockIn {
        product_id: product_id.to_owned(),
        qty,
        unit_cost_minor,
        supplier: SUPPLIER_IMPORT.to_owned(),
        expiry: None,
    };
    mutate_in(tx, intent, &u, &t, now)
}

/// Whether a stock-in with this expiry lands inside the warning window of an alert-less branch.
pub(super) fn stock_in_alerts(u: &StockIn, t: &StockTarget) -> bool {
    t.open_alert.is_none()
        && match (&u.expiry, &t.today) {
            (Some(e), Some(today)) => lot_in_window(e, today, t.warn_days),
            _ => false,
        }
}

pub(super) fn validate_write_off(w: &StockWriteOff, t: &StockTarget) -> Result<(), AppError> {
    let Some(lot) = &t.lot else {
        return Err(invalid("lot_id", "no such lot"));
    };
    if lot.qty_remaining <= 0 {
        return Err(AppError::with_field(
            ErrorCode::Stale,
            "lot_id",
            "this lot has nothing left",
        ));
    }
    let expired = match (&lot.expiry, &t.today) {
        (Some(e), Some(today)) => lot_expired(e, today),
        _ => false,
    };
    if !expired {
        return Err(invalid(
            "lot_id",
            "only a lot past its expiry date is written off",
        ));
    }
    if t.stock.as_ref().is_none_or(|s| s.qty < lot.qty_remaining) || t.product.is_none() {
        return Err(AppError::new(
            ErrorCode::Internal,
            "the lot is not backed by stock",
        ));
    }
    debug_assert_eq!(w.lot_id, lot.lot_id);
    Ok(())
}

/// Empties the lot, lowers the product's stock, appends the `expired` move valued at the lot's
/// cost and, when no other lot is expired or expiring, clears the "Expiring soon" alert.
pub(super) fn mutate_write_off(
    tx: &StoreTx<'_>,
    intent: &Intent,
    t: &StockTarget,
    now: i64,
) -> Result<Vec<Written>, AppError> {
    let (Some(lot), Some(stock)) = (&t.lot, &t.stock) else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "the stock target is incomplete",
        ));
    };
    let qty = lot.qty_remaining;
    update_stock(tx, &lot.product_id, stock.qty - qty, stock.avg_cost_minor)?;
    set_lot_remaining(tx, &lot.lot_id, 0)?;
    let move_id = ulid::Ulid::generate().to_string();
    insert_move(
        tx,
        &MoveRow {
            move_id: move_id.clone(),
            product_id: lot.product_id.clone(),
            reason: MOVE_EXPIRED.to_owned(),
            delta: -qty,
            before_qty: stock.qty,
            before_avg_cost_minor: stock.avg_cost_minor,
            unit_cost_minor: lot.unit_cost_minor,
            supplier: String::new(),
            lot_id: Some(lot.lot_id.clone()),
            lots_used: used_json(&[(lot.lot_id.clone(), qty)]),
            staff_id: intent.actor.id.clone(),
            at: now,
            waste_reason: None,
            value_minor: qty.saturating_mul(lot.unit_cost_minor),
            order_id: None,
            note: None,
            elevated_by: None,
            placement_id: None,
        },
    )?;
    let mut out = vec![
        write(
            RowKey::Stock {
                product_id: lot.product_id.clone(),
            },
            ChangeOp::Update,
            None,
        ),
        write(
            RowKey::StockLots {
                lot_id: lot.lot_id.clone(),
            },
            ChangeOp::Update,
            None,
        ),
        write(RowKey::StockMoves { move_id }, ChangeOp::Insert, None),
    ];
    if let Some(alert) = &t.open_alert
        && t.window.iter().all(|(l, _)| l.lot_id == lot.lot_id)
    {
        out.push(ack_row(tx, alert, &intent.actor.id, now)?);
    }
    out.extend(low_after_change(
        tx,
        &lot.product_id,
        stock.qty,
        stock.qty - qty,
        &intent.actor.id,
        now,
    )?);
    Ok(out)
}

/// What the daily check does now: raise the alert (a lot is expired or in the window and none was
/// raised today; one still open from an earlier day is acknowledged and replaced) or clear a
/// stale one (nothing is in the window any more).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Check {
    Raise,
    Clear,
}

fn check_of(t: &StockTarget) -> Option<Check> {
    match (t.window.is_empty(), &t.open_alert) {
        (false, _) if t.today.is_some() && !t.raised_today => Some(Check::Raise),
        (true, Some(_)) if t.today.is_some() => Some(Check::Clear),
        _ => None,
    }
}

/// Whether `stock.expiry_check` would change anything now (the writer submits it only then).
pub(crate) fn check_due(t: &StockTarget) -> bool {
    check_of(t).is_some()
}

pub(crate) fn load_check(tx: &StoreTx<'_>, now: i64) -> Result<StockTarget, AppError> {
    load(
        tx,
        &IntentPayload::StockExpiryCheck(arena_proto::StockExpiryCheck {}),
        now,
    )
}

pub(super) fn validate_check(t: &StockTarget) -> Result<(), AppError> {
    check_of(t).map(|_| ()).ok_or_else(|| {
        AppError::new(
            ErrorCode::Stale,
            "no expiry alert needs to be raised or cleared",
        )
    })
}

pub(super) fn mutate_check(
    tx: &StoreTx<'_>,
    intent: &Intent,
    t: &StockTarget,
    now: i64,
) -> Result<Vec<Written>, AppError> {
    match (check_of(t), &t.open_alert) {
        (Some(Check::Raise), open) => {
            let mut out = Vec::new();
            if let Some(alert) = open {
                out.push(ack_row(tx, alert, &intent.actor.id, now)?);
            }
            out.push(raise_expiring(tx, now)?);
            // Touch the stock of every product with an expired lot so its availability
            // (`expired`) goes out on `catalog.changed` when the business day turns over.
            let today = t.today.as_deref().unwrap_or_default();
            let mut products: Vec<&str> = t
                .window
                .iter()
                .filter(|(l, _)| l.expiry.as_deref().is_some_and(|e| lot_expired(e, today)))
                .map(|(l, _)| l.product_id.as_str())
                .collect();
            products.sort_unstable();
            products.dedup();
            for product_id in products {
                out.push(write(
                    RowKey::Stock {
                        product_id: product_id.to_owned(),
                    },
                    ChangeOp::Update,
                    None,
                ));
            }
            Ok(out)
        }
        (Some(Check::Clear), Some(alert)) => Ok(vec![ack_row(tx, alert, &intent.actor.id, now)?]),
        _ => Err(AppError::new(
            ErrorCode::Internal,
            "the expiry check reached mutate with nothing to do",
        )),
    }
}

/// `stock.threshold` validation: a live tracked non-bundle product, a value in range, and a
/// value the product does not already have.
pub(super) fn validate_threshold(w: &StockThreshold, t: &StockTarget) -> Result<(), AppError> {
    match &t.product {
        Some(p) if !p.bundle && p.track_stock => {}
        _ => {
            return Err(invalid(
                "product_id",
                "a threshold is set for tracked products that are not bundles",
            ));
        }
    }
    if let Some(v) = w.low_stock_qty
        && !(0..=MAX_STOCK_QTY).contains(&v)
    {
        return Err(invalid("low_stock_qty", "threshold is out of range"));
    }
    if t.threshold == w.low_stock_qty {
        return Err(AppError::with_field(
            ErrorCode::Stale,
            "low_stock_qty",
            "the product already has this threshold",
        ));
    }
    Ok(())
}

/// Stores the threshold, then raises the alert (quantity at or under it, none open), refreshes
/// the open one (still at or under it) or acknowledges it (above it, or cleared).
pub(super) fn mutate_threshold(
    tx: &StoreTx<'_>,
    intent: &Intent,
    w: &StockThreshold,
    t: &StockTarget,
    now: i64,
) -> Result<Vec<Written>, AppError> {
    set_low_stock_qty(tx, &w.product_id, w.low_stock_qty)?;
    let mut out = vec![write(
        RowKey::Products {
            product_id: w.product_id.clone(),
        },
        ChangeOp::Update,
        None,
    )];
    let qty = t.stock.as_ref().map_or(0, |s| s.qty);
    match (w.low_stock_qty, &t.low_alert) {
        (Some(th), Some(alert)) if qty <= th => {
            set_alert_qty(tx, &alert.id, qty)?;
            set_alert_threshold(tx, &alert.id, th)?;
            out.push(alert_update(&alert.id));
        }
        (Some(th), None) if qty <= th => {
            out.push(raise_low(tx, &w.product_id, qty, th, now)?);
        }
        (_, Some(alert)) => out.push(ack_row(tx, alert, &intent.actor.id, now)?),
        _ => {}
    }
    Ok(out)
}

fn alert_update(id: &str) -> Written {
    write(RowKey::Alerts { id: id.to_owned() }, ChangeOp::Update, None)
}

fn raise_low(
    tx: &StoreTx<'_>,
    product_id: &str,
    qty: i64,
    threshold: i64,
    now: i64,
) -> Result<Written, AppError> {
    let id = ulid::Ulid::generate().to_string();
    insert_alert(
        tx,
        &NewAlert {
            id: &id,
            kind: AlertKind::StockLow.as_str(),
            device_id: None,
            raised_at: now,
            product_id: Some(product_id),
            qty: Some(qty),
            threshold: Some(threshold),
            ticket_id: None,
        },
    )?;
    Ok(write(RowKey::Alerts { id }, ChangeOp::Insert, None))
}

/// The low-stock hook every stock change runs after it moved the quantity of `product_id` from
/// `old_qty` to `new_qty`. A tracked non-bundle product with a threshold raises "Low stock" when
/// the quantity crosses from above the threshold to at or under it; while the alert is open and
/// the product still low, its quantity follows the stock; once the quantity is back above the
/// threshold the alert is acknowledged by `by`.
pub(crate) fn low_after_change(
    tx: &StoreTx<'_>,
    product_id: &str,
    old_qty: i64,
    new_qty: i64,
    by: &str,
    now: i64,
) -> Result<Vec<Written>, AppError> {
    let Some(threshold) = low_stock_qty(tx, product_id)? else {
        return Ok(Vec::new());
    };
    match find_product(tx, product_id)? {
        Some(p) if p.deleted_at.is_none() && p.track_stock && !p.bundle => {}
        _ => return Ok(Vec::new()),
    }
    let open = find_open_alert_for_product(tx, AlertKind::StockLow.as_str(), product_id)?;
    if new_qty > threshold {
        return match open {
            Some(alert) => Ok(vec![ack_row(tx, &alert, by, now)?]),
            None => Ok(Vec::new()),
        };
    }
    match open {
        Some(alert) if alert.qty != Some(new_qty) => {
            set_alert_qty(tx, &alert.id, new_qty)?;
            Ok(vec![alert_update(&alert.id)])
        }
        Some(_) => Ok(Vec::new()),
        None if old_qty > threshold => {
            Ok(vec![raise_low(tx, product_id, new_qty, threshold, now)?])
        }
        None => Ok(Vec::new()),
    }
}

/// A counter sale override: raises a short product's stock by `raise` units in a new lot (no
/// expiry, at the average cost) and appends the `count_adjust` move, so the sale that follows
/// never takes stock below zero. The note names the override and `elevated_by` the manager who
/// approved it, when the actor did not hold the right themselves. The raise stays when the sale is
/// undone: it corrects the stock record, which is not the sale's to take back.
pub(super) fn raise_for_sale(
    tx: &StoreTx<'_>,
    product_id: &str,
    raise: i64,
    order_id: &str,
    staff_id: &str,
    elevated_by: Option<&str>,
    now: i64,
) -> Result<Vec<Written>, AppError> {
    let stock = find_stock(tx, product_id)?;
    let (qty, avg) = stock.as_ref().map_or((0, 0), |s| (s.qty, s.avg_cost_minor));
    let stock_op = if stock.is_some() {
        update_stock(tx, product_id, qty + raise, avg)?;
        ChangeOp::Update
    } else {
        insert_stock(tx, product_id, qty + raise, avg)?;
        ChangeOp::Insert
    };
    let lot_id = ulid::Ulid::generate().to_string();
    insert_lot(
        tx,
        &LotRow {
            lot_id: lot_id.clone(),
            product_id: product_id.to_owned(),
            qty_in: raise,
            qty_remaining: raise,
            unit_cost_minor: avg,
            expiry: None,
            received_at: now,
        },
    )?;
    let move_id = ulid::Ulid::generate().to_string();
    insert_move(
        tx,
        &MoveRow {
            move_id: move_id.clone(),
            product_id: product_id.to_owned(),
            reason: MOVE_COUNT_ADJUST.to_owned(),
            delta: raise,
            before_qty: qty,
            before_avg_cost_minor: avg,
            unit_cost_minor: avg,
            supplier: String::new(),
            lot_id: Some(lot_id.clone()),
            lots_used: "[]".to_owned(),
            staff_id: staff_id.to_owned(),
            at: now,
            waste_reason: None,
            value_minor: raise.saturating_mul(avg),
            order_id: Some(order_id.to_owned()),
            note: Some(NOTE_SALE_OVERRIDE.to_owned()),
            elevated_by: elevated_by.map(str::to_owned),
            placement_id: None,
        },
    )?;
    Ok(vec![
        write(
            RowKey::Stock {
                product_id: product_id.to_owned(),
            },
            stock_op,
            None,
        ),
        write(RowKey::StockLots { lot_id }, ChangeOp::Insert, None),
        write(RowKey::StockMoves { move_id }, ChangeOp::Insert, None),
    ])
}

/// The sale a stock plan is carried out for.
pub(super) struct StockFor<'a> {
    pub(super) order_id: &'a str,
    /// The placement (`order.place` intent id) a session bill's stock moves are tagged with.
    pub(super) placement_id: Option<&'a str>,
    /// The business day, against which lots are expired.
    pub(super) today: &'a str,
}

/// A counter sale's stock: lowers a product by `qty` from its lots, earliest expiry first, never
/// from a lot past its expiry date on `today`, and appends the `sale` move valued at the average
/// cost. `OUT_OF_STOCK` when the usable lots hold less (the sale validated this already).
pub(super) fn take_for_sale(
    tx: &StoreTx<'_>,
    product_id: &str,
    qty: i64,
    sale: &StockFor<'_>,
    staff_id: &str,
    now: i64,
) -> Result<Vec<Written>, AppError> {
    let (order_id, placement_id, today) = (sale.order_id, sale.placement_id, sale.today);
    let short = || AppError::with_field(ErrorCode::OutOfStock, "lines", "not enough stock on hand");
    let stock = find_stock(tx, product_id)?.ok_or_else(short)?;
    let lots = list_lots(tx, product_id)?;
    let views: Vec<_> = lots
        .iter()
        .filter(|l| l.expiry.as_deref().is_none_or(|e| !lot_expired(e, today)))
        .map(LotRow::view)
        .collect();
    let plan = consume_plan(&views, qty).ok_or_else(short)?;
    if qty > stock.qty {
        return Err(short());
    }
    update_stock(tx, product_id, stock.qty - qty, stock.avg_cost_minor)?;
    let mut out = vec![write(
        RowKey::Stock {
            product_id: product_id.to_owned(),
        },
        ChangeOp::Update,
        None,
    )];
    for (lot_id, take) in &plan {
        let left = lots
            .iter()
            .find(|l| &l.lot_id == lot_id)
            .map_or(0, |l| l.qty_remaining)
            - take;
        set_lot_remaining(tx, lot_id, left)?;
        out.push(write(
            RowKey::StockLots {
                lot_id: lot_id.clone(),
            },
            ChangeOp::Update,
            None,
        ));
    }
    let move_id = ulid::Ulid::generate().to_string();
    insert_move(
        tx,
        &MoveRow {
            move_id: move_id.clone(),
            product_id: product_id.to_owned(),
            reason: MOVE_SALE.to_owned(),
            delta: -qty,
            before_qty: stock.qty,
            before_avg_cost_minor: stock.avg_cost_minor,
            unit_cost_minor: stock.avg_cost_minor,
            supplier: String::new(),
            lot_id: None,
            lots_used: used_json(&plan),
            staff_id: staff_id.to_owned(),
            at: now,
            waste_reason: None,
            value_minor: qty.saturating_mul(stock.avg_cost_minor),
            order_id: Some(order_id.to_owned()),
            note: None,
            elevated_by: None,
            placement_id: placement_id.map(str::to_owned),
        },
    )?;
    out.push(write(
        RowKey::StockMoves { move_id },
        ChangeOp::Insert,
        None,
    ));
    Ok(out)
}

/// What an Undo of a sale needs to know about the stock it took: whether every lot it drew from
/// still exists with room to take the units back.
pub(super) fn sale_restorable(
    moves: &[MoveRow],
    stocks: &std::collections::BTreeMap<String, StockRow>,
    lots: &std::collections::BTreeMap<String, Vec<LotRow>>,
) -> bool {
    moves.iter().all(|m| {
        let used = parse_used(&m.lots_used);
        let held = lots.get(&m.product_id).map_or(&[][..], Vec::as_slice);
        stocks.contains_key(&m.product_id)
            && !used.is_empty()
            && used.iter().all(|(id, qty)| {
                held.iter()
                    .find(|l| &l.lot_id == id)
                    .is_some_and(|l| l.qty_remaining + qty <= l.qty_in)
            })
    })
}

/// A product's quantity before and after a sale or its Undo, for the low-stock hook.
pub(super) type StockChange = (String, i64, i64);

/// Undo of a sale's stock: gives each `sale` move's quantity back to the lots it came from and
/// appends the compensating `sale_reverted` move (positive delta, same lots and value). Returns
/// the rows written, and per product the quantity before and after, for the low-stock hook.
pub(super) fn restore_sale_moves(
    tx: &StoreTx<'_>,
    moves: &[MoveRow],
    order_id: &str,
    placement_id: Option<&str>,
    staff_id: &str,
    now: i64,
) -> Result<(Vec<Written>, Vec<StockChange>), AppError> {
    let incomplete = || AppError::new(ErrorCode::Internal, "the sale's stock is incomplete");
    let mut out = Vec::new();
    let mut changed = Vec::new();
    for m in moves {
        let stock = find_stock(tx, &m.product_id)?.ok_or_else(incomplete)?;
        let give = -m.delta;
        update_stock(tx, &m.product_id, stock.qty + give, stock.avg_cost_minor)?;
        out.push(write(
            RowKey::Stock {
                product_id: m.product_id.clone(),
            },
            ChangeOp::Update,
            None,
        ));
        for (lot_id, qty) in parse_used(&m.lots_used) {
            let lot = find_lot(tx, &lot_id)?.ok_or_else(incomplete)?;
            set_lot_remaining(tx, &lot_id, lot.qty_remaining + qty)?;
            out.push(write(RowKey::StockLots { lot_id }, ChangeOp::Update, None));
        }
        let move_id = ulid::Ulid::generate().to_string();
        insert_move(
            tx,
            &MoveRow {
                move_id: move_id.clone(),
                product_id: m.product_id.clone(),
                reason: MOVE_SALE_REVERTED.to_owned(),
                delta: give,
                before_qty: stock.qty,
                before_avg_cost_minor: stock.avg_cost_minor,
                unit_cost_minor: m.unit_cost_minor,
                supplier: String::new(),
                lot_id: None,
                lots_used: m.lots_used.clone(),
                staff_id: staff_id.to_owned(),
                at: now,
                waste_reason: None,
                value_minor: m.value_minor,
                order_id: Some(order_id.to_owned()),
                note: None,
                elevated_by: None,
                placement_id: placement_id.map(str::to_owned),
            },
        )?;
        out.push(write(
            RowKey::StockMoves { move_id },
            ChangeOp::Insert,
            None,
        ));
        changed.push((m.product_id.clone(), stock.qty, stock.qty + give));
    }
    Ok((out, changed))
}

/// What a cancelled line gives back to the shelf in `order` mode, where the line took its stock
/// at placement: `qty` units in a new lot (no expiry) at the average cost, and a `return` move
/// tagged with the order and the placement. The line's share of the placement's lot draw is not
/// recorded per line, so the units go into a fresh lot, as a sale override does.
pub(super) fn return_to_stock(
    tx: &StoreTx<'_>,
    product_id: &str,
    qty: i64,
    sale: &StockFor<'_>,
    staff_id: &str,
    now: i64,
) -> Result<Vec<Written>, AppError> {
    let stock = find_stock(tx, product_id)?;
    let (before, avg) = stock.as_ref().map_or((0, 0), |s| (s.qty, s.avg_cost_minor));
    let stock_op = if stock.is_some() {
        update_stock(tx, product_id, before + qty, avg)?;
        ChangeOp::Update
    } else {
        insert_stock(tx, product_id, before + qty, avg)?;
        ChangeOp::Insert
    };
    let lot_id = ulid::Ulid::generate().to_string();
    insert_lot(
        tx,
        &LotRow {
            lot_id: lot_id.clone(),
            product_id: product_id.to_owned(),
            qty_in: qty,
            qty_remaining: qty,
            unit_cost_minor: avg,
            expiry: None,
            received_at: now,
        },
    )?;
    let move_id = ulid::Ulid::generate().to_string();
    insert_move(
        tx,
        &MoveRow {
            move_id: move_id.clone(),
            product_id: product_id.to_owned(),
            reason: MOVE_RETURN.to_owned(),
            delta: qty,
            before_qty: before,
            before_avg_cost_minor: avg,
            unit_cost_minor: avg,
            supplier: String::new(),
            lot_id: Some(lot_id.clone()),
            lots_used: "[]".to_owned(),
            staff_id: staff_id.to_owned(),
            at: now,
            waste_reason: None,
            value_minor: qty.saturating_mul(avg),
            order_id: Some(sale.order_id.to_owned()),
            note: None,
            elevated_by: None,
            placement_id: sale.placement_id.map(str::to_owned),
        },
    )?;
    let mut out = vec![
        write(
            RowKey::Stock {
                product_id: product_id.to_owned(),
            },
            stock_op,
            None,
        ),
        write(RowKey::StockLots { lot_id }, ChangeOp::Insert, None),
        write(RowKey::StockMoves { move_id }, ChangeOp::Insert, None),
    ];
    out.extend(low_after_change(
        tx,
        product_id,
        before,
        before + qty,
        staff_id,
        now,
    )?);
    Ok(out)
}

/// The units of a product that may be taken right now: on hand less lots past their expiry date on
/// `today` (open holds are not subtracted: serving a held line takes its own hold).
pub(super) fn takeable(tx: &StoreTx<'_>, product_id: &str, today: &str) -> Result<i64, AppError> {
    let held = find_stock(tx, product_id)?.map_or(0, |s| s.qty);
    let expired: i64 = list_lots(tx, product_id)?
        .iter()
        .filter(|l| l.expiry.as_deref().is_some_and(|e| lot_expired(e, today)))
        .map(|l| l.qty_remaining)
        .sum();
    Ok((held - expired).max(0))
}

/// A write that touches a product's stock row without changing it, so a hold opened or closed on
/// the product goes out on `catalog.changed` (availability follows the holds). `None` when the
/// product has no stock row (nothing sells from it anyway).
pub(super) fn touch(tx: &StoreTx<'_>, product_id: &str) -> Result<Option<Written>, AppError> {
    Ok(find_stock(tx, product_id)?.map(|_| {
        write(
            RowKey::Stock {
                product_id: product_id.to_owned(),
            },
            ChangeOp::Update,
            None,
        )
    }))
}
