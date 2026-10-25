use arena_proto::{AppError, LotView};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockRow {
    pub product_id: String,
    pub qty: i64,
    pub avg_cost_minor: i64,
    pub rev: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LotRow {
    pub lot_id: String,
    pub product_id: String,
    pub qty_in: i64,
    pub qty_remaining: i64,
    pub unit_cost_minor: i64,
    pub expiry: Option<String>,
    pub received_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveRow {
    pub move_id: String,
    pub product_id: String,
    pub reason: String,
    pub delta: i64,
    pub before_qty: i64,
    pub before_avg_cost_minor: i64,
    pub unit_cost_minor: i64,
    pub supplier: String,
    pub lot_id: Option<String>,
    pub lots_used: String,
    pub staff_id: String,
    pub at: i64,
}

const LOT_COLUMNS: &str =
    "lot_id, product_id, qty_in, qty_remaining, unit_cost_minor, expiry, received_at";
const MOVE_COLUMNS: &str = "move_id, product_id, reason, delta, before_qty, before_avg_cost_minor, \
                            unit_cost_minor, supplier, lot_id, lots_used, staff_id, at";

fn lot(r: &rusqlite::Row<'_>) -> rusqlite::Result<LotRow> {
    Ok(LotRow {
        lot_id: r.get(0)?,
        product_id: r.get(1)?,
        qty_in: r.get(2)?,
        qty_remaining: r.get(3)?,
        unit_cost_minor: r.get(4)?,
        expiry: r.get(5)?,
        received_at: r.get(6)?,
    })
}

fn mv(r: &rusqlite::Row<'_>) -> rusqlite::Result<MoveRow> {
    Ok(MoveRow {
        move_id: r.get(0)?,
        product_id: r.get(1)?,
        reason: r.get(2)?,
        delta: r.get(3)?,
        before_qty: r.get(4)?,
        before_avg_cost_minor: r.get(5)?,
        unit_cost_minor: r.get(6)?,
        supplier: r.get(7)?,
        lot_id: r.get(8)?,
        lots_used: r.get(9)?,
        staff_id: r.get(10)?,
        at: r.get(11)?,
    })
}

impl LotRow {
    pub fn view(&self) -> LotView {
        LotView {
            lot_id: self.lot_id.clone(),
            expiry: self.expiry.clone(),
            received_at: self.received_at,
            remaining: self.qty_remaining,
        }
    }
}

pub fn find_stock(conn: &Connection, product_id: &str) -> Result<Option<StockRow>, AppError> {
    conn.query_row(
        "SELECT product_id, qty, avg_cost_minor, rev FROM stock WHERE product_id = ?1",
        [product_id],
        |r| {
            Ok(StockRow {
                product_id: r.get(0)?,
                qty: r.get(1)?,
                avg_cost_minor: r.get(2)?,
                rev: r.get(3)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

pub fn insert_stock(conn: &Connection, id: &str, qty: i64, avg: i64) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO stock (product_id, qty, avg_cost_minor, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, 0, 0, 0)",
        params![id, qty, avg],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn update_stock(conn: &Connection, id: &str, qty: i64, avg: i64) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE stock SET qty = ?2, avg_cost_minor = ?3 WHERE product_id = ?1",
            params![id, qty, avg],
        )
        .map_err(db_error)?;
    (n == 1).then_some(()).ok_or_else(|| {
        AppError::new(
            arena_proto::ErrorCode::Internal,
            format!("no stock row {id}"),
        )
    })
}

pub fn insert_lot(conn: &Connection, l: &LotRow) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO stock_lots (lot_id, product_id, qty_in, qty_remaining, unit_cost_minor,
                                 expiry, received_at, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 0, 0)",
        params![
            l.lot_id,
            l.product_id,
            l.qty_in,
            l.qty_remaining,
            l.unit_cost_minor,
            l.expiry,
            l.received_at
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn set_lot_remaining(conn: &Connection, lot_id: &str, remaining: i64) -> Result<(), AppError> {
    conn.execute(
        "UPDATE stock_lots SET qty_remaining = ?2 WHERE lot_id = ?1",
        params![lot_id, remaining],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn find_lot(conn: &Connection, lot_id: &str) -> Result<Option<LotRow>, AppError> {
    conn.query_row(
        &format!("SELECT {LOT_COLUMNS} FROM stock_lots WHERE lot_id = ?1"),
        [lot_id],
        lot,
    )
    .optional()
    .map_err(db_error)
}

pub fn list_lots(conn: &Connection, product_id: &str) -> Result<Vec<LotRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {LOT_COLUMNS} FROM stock_lots WHERE product_id = ?1 ORDER BY lot_id"
        ))
        .map_err(db_error)?;
    let rows = stmt.query_map([product_id], lot).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

pub fn insert_move(conn: &Connection, m: &MoveRow) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO stock_moves (move_id, product_id, reason, delta, before_qty,
                                  before_avg_cost_minor, unit_cost_minor, supplier, lot_id,
                                  lots_used, staff_id, at, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0, 0, 0)",
        params![
            m.move_id,
            m.product_id,
            m.reason,
            m.delta,
            m.before_qty,
            m.before_avg_cost_minor,
            m.unit_cost_minor,
            m.supplier,
            m.lot_id,
            m.lots_used,
            m.staff_id,
            m.at
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn find_move(conn: &Connection, move_id: &str) -> Result<Option<MoveRow>, AppError> {
    conn.query_row(
        &format!("SELECT {MOVE_COLUMNS} FROM stock_moves WHERE move_id = ?1"),
        [move_id],
        mv,
    )
    .optional()
    .map_err(db_error)
}

pub fn list_moves(conn: &Connection, product_id: &str) -> Result<Vec<MoveRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {MOVE_COLUMNS} FROM stock_moves WHERE product_id = ?1 ORDER BY rowid"
        ))
        .map_err(db_error)?;
    let rows = stmt.query_map([product_id], mv).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

pub fn latest_move_id(conn: &Connection, product_id: &str) -> Result<Option<String>, AppError> {
    conn.query_row(
        "SELECT move_id FROM stock_moves WHERE product_id = ?1
         ORDER BY rowid DESC LIMIT 1",
        [product_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(db_error)
}
