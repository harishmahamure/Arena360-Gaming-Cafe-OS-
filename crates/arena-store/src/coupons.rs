//! Coupons (Story 5.21): the `coupons` and `coupon_targets` change-log tables, and the coupon an
//! order took. Only `apply()` writes them.

use arena_proto::{AppError, ErrorCode};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

/// A coupon with its targets as `(kind, id)`.
pub type CouponWithTargets = (CouponRow, Vec<(String, String)>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CouponRow {
    pub coupon_id: String,
    pub code: String,
    pub percent_bp: Option<i64>,
    pub amount_minor: Option<i64>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub usage_limit: Option<i64>,
    pub used: i64,
    pub min_bill_minor: i64,
    pub active: bool,
    pub rev: i64,
}

const COLUMNS: &str = "coupon_id, code, percent_bp, amount_minor, valid_from, valid_to, \
                       usage_limit, used, min_bill_minor, active, rev";

fn from(r: &rusqlite::Row<'_>) -> rusqlite::Result<CouponRow> {
    Ok(CouponRow {
        coupon_id: r.get(0)?,
        code: r.get(1)?,
        percent_bp: r.get(2)?,
        amount_minor: r.get(3)?,
        valid_from: r.get(4)?,
        valid_to: r.get(5)?,
        usage_limit: r.get(6)?,
        used: r.get(7)?,
        min_bill_minor: r.get(8)?,
        active: r.get(9)?,
        rev: r.get(10)?,
    })
}

pub fn find_coupon(conn: &Connection, id: &str) -> Result<Option<CouponRow>, AppError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM coupons WHERE coupon_id = ?1"),
        [id],
        from,
    )
    .optional()
    .map_err(db_error)
}

/// The coupon with this (already upper-cased) code.
pub fn find_coupon_by_code(conn: &Connection, code: &str) -> Result<Option<CouponRow>, AppError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM coupons WHERE code = ?1"),
        [code],
        from,
    )
    .optional()
    .map_err(db_error)
}

pub fn list_coupons(conn: &Connection) -> Result<Vec<CouponRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!("SELECT {COLUMNS} FROM coupons ORDER BY code"))
        .map_err(db_error)?;
    let rows = stmt.query_map([], from).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

/// A coupon's targets as `(kind, id)`, in key order.
pub fn list_coupon_targets(
    conn: &Connection,
    coupon_id: &str,
) -> Result<Vec<(String, String)>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT target_kind, target_id FROM coupon_targets WHERE coupon_id = ?1
             ORDER BY target_kind, target_id",
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map([coupon_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

pub fn insert_coupon(conn: &Connection, c: &CouponRow) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO coupons (coupon_id, code, percent_bp, amount_minor, valid_from, valid_to,
                              usage_limit, used, min_bill_minor, active, rev, created_at,
                              updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, 0, 0, 0)",
        params![
            c.coupon_id,
            c.code,
            c.percent_bp,
            c.amount_minor,
            c.valid_from,
            c.valid_to,
            c.usage_limit,
            c.min_bill_minor,
            c.active
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Updates everything but `used`, which only a settle and its Undo move.
pub fn update_coupon(conn: &Connection, c: &CouponRow) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE coupons SET code = ?2, percent_bp = ?3, amount_minor = ?4, valid_from = ?5,
                    valid_to = ?6, usage_limit = ?7, min_bill_minor = ?8, active = ?9
             WHERE coupon_id = ?1",
            params![
                c.coupon_id,
                c.code,
                c.percent_bp,
                c.amount_minor,
                c.valid_from,
                c.valid_to,
                c.usage_limit,
                c.min_bill_minor,
                c.active
            ],
        )
        .map_err(db_error)?;
    if n == 1 {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::Internal,
            format!("no coupons row {}", c.coupon_id),
        ))
    }
}

pub fn insert_coupon_target(
    conn: &Connection,
    coupon_id: &str,
    kind: &str,
    id: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO coupon_targets (coupon_id, target_kind, target_id) VALUES (?1, ?2, ?3)",
        params![coupon_id, kind, id],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Takes one use of the coupon. False when it has none left (`used` already at `usage_limit`).
pub fn take_coupon_use(conn: &Connection, coupon_id: &str) -> Result<bool, AppError> {
    let n = conn
        .execute(
            "UPDATE coupons SET used = used + 1
             WHERE coupon_id = ?1 AND (usage_limit IS NULL OR used < usage_limit)",
            [coupon_id],
        )
        .map_err(db_error)?;
    Ok(n == 1)
}

/// Gives one use back (never below zero).
pub fn give_coupon_use(conn: &Connection, coupon_id: &str) -> Result<(), AppError> {
    conn.execute(
        "UPDATE coupons SET used = MAX(used - 1, 0) WHERE coupon_id = ?1",
        [coupon_id],
    )
    .map_err(db_error)?;
    Ok(())
}

/// The coupon a settled order took: `(code, amount taken off)`.
pub fn order_coupon(conn: &Connection, order_id: &str) -> Result<Option<(String, i64)>, AppError> {
    let found: Option<(Option<String>, Option<i64>)> = conn
        .query_row(
            "SELECT coupon_code, coupon_minor FROM orders WHERE id = ?1",
            [order_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db_error)?;
    Ok(match found {
        Some((Some(code), Some(minor))) => Some((code, minor)),
        _ => None,
    })
}

/// Records the coupon on an order at settle. With `reduce_totals` the order's subtotal and total
/// go down by `minor` too (a session bill, which is priced before its coupon); a counter or plan
/// sale is written already net.
pub fn set_order_coupon(
    conn: &Connection,
    order_id: &str,
    code: &str,
    minor: i64,
    reduce_totals: bool,
) -> Result<(), AppError> {
    let cut = if reduce_totals { minor } else { 0 };
    conn.execute(
        "UPDATE orders SET coupon_code = ?2, coupon_minor = ?3,
                subtotal_minor = subtotal_minor - ?4, total_minor = total_minor - ?4
         WHERE id = ?1",
        params![order_id, code, minor, cut],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Takes the coupon off an order again (Undo of a session settle): the amount goes back onto the
/// subtotal and total and the coupon columns clear.
pub fn clear_order_coupon(conn: &Connection, order_id: &str) -> Result<(), AppError> {
    conn.execute(
        "UPDATE orders SET subtotal_minor = subtotal_minor + COALESCE(coupon_minor, 0),
                total_minor = total_minor + COALESCE(coupon_minor, 0),
                coupon_code = NULL, coupon_minor = NULL
         WHERE id = ?1 AND coupon_code IS NOT NULL",
        [order_id],
    )
    .map_err(db_error)?;
    Ok(())
}

/// One line of a bill as a coupon sees it: its amount, tax rate and what it is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CouponLineRow {
    pub amount_minor: i64,
    pub rate_bp: i64,
    /// The device group of a time line's device.
    pub group_id: Option<String>,
    pub plan_id: Option<String>,
    pub product_id: Option<String>,
}

/// The lines of an order, in position order.
pub fn list_coupon_lines(
    conn: &Connection,
    order_id: &str,
) -> Result<Vec<CouponLineRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT l.amount_minor, COALESCE(l.tax_rate_bp, 0), m.group_id, l.plan_id, l.product_id
             FROM order_lines l LEFT JOIN devices_meta m ON m.device_id = l.device_id
             WHERE l.order_id = ?1 ORDER BY l.position, l.id",
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map([order_id], |r| {
            Ok(CouponLineRow {
                amount_minor: r.get(0)?,
                rate_bp: r.get(1)?,
                group_id: r.get(2)?,
                plan_id: r.get(3)?,
                product_id: r.get(4)?,
            })
        })
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}
