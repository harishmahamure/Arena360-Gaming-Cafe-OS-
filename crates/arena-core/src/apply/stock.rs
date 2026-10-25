use arena_proto::{
    AppError, ErrorCode, Intent, IntentPayload, MAX_PRICE_MINOR, MAX_STOCK_IN_QTY, MAX_STOCK_QTY,
    MAX_SUPPLIER_CHARS, MOVE_STOCK_IN, MOVE_STOCK_IN_REVERTED, STOCK_UNDO_MS, StockIn,
    StockInReverted, consume_plan, valid_date, weighted_avg_cost,
};
use arena_store::{
    ChangeOp, LotRow, MoveRow, ProductRow, RowKey, StockRow, StoreTx, Table, delete_row, find_lot,
    find_move, find_product, find_stock, insert_lot, insert_move, insert_stock, latest_move_id,
    list_lots, read_row, set_lot_remaining, update_stock,
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
    now: i64,
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
        },
    )?;
    Ok(vec![
        write(
            RowKey::Stock {
                product_id: u.product_id.clone(),
            },
            stock_op,
            None,
        ),
        write(RowKey::StockLots { lot_id }, ChangeOp::Insert, None),
        write(RowKey::StockMoves { move_id }, ChangeOp::Insert, None),
    ])
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
        },
    )?;
    Ok(vec![
        write(
            RowKey::Stock {
                product_id: m.product_id.clone(),
            },
            ChangeOp::Update,
            None,
        ),
        write(key, ChangeOp::Delete, Some(before)),
        write(RowKey::StockMoves { move_id }, ChangeOp::Insert, None),
    ])
}

fn used_json(used: &[(String, i64)]) -> String {
    let items: Vec<String> = used
        .iter()
        .map(|(lot_id, qty)| format!("{{\"lot_id\":\"{lot_id}\",\"qty\":{qty}}}"))
        .collect();
    format!("[{}]", items.join(","))
}

pub fn decrement(
    tx: &StoreTx<'_>,
    product_id: &str,
    qty: i64,
    reason: &str,
    staff_id: &str,
    now: i64,
) -> Result<Vec<Written>, AppError> {
    let short = || invalid("qty", "not enough stock");
    let stock = find_stock(tx, product_id)?.ok_or_else(short)?;
    if qty <= 0 || qty > stock.qty {
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
        },
    )?;
    out.push(write(
        RowKey::StockMoves { move_id },
        ChangeOp::Insert,
        None,
    ));
    Ok(out)
}
