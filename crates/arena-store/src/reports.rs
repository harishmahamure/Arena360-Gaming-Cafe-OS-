//! The rows the Reports screen is built from (Story 5.27). Everything is read by the stamped
//! `business_day` columns and the stored totals; nothing is recomputed from rates. A bill whose
//! settle was undone is open again and a voided one is `void`, so only `settled` orders count and
//! reversed pairs net to zero by construction. The aggregation (local hours, utilisation) lives in
//! `arena-core`, which knows the branch time zone.

use arena_proto::AppError;
use rusqlite::{Connection, params};

use crate::db_error;

/// A settled sales bill: a session bill, a plan sale or a counter sale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SalesOrderRow {
    pub id: String,
    /// `session`, `plan_sale` or `pos_sale`.
    pub kind: String,
    pub total_minor: i64,
    pub tax_minor: i64,
    pub coupon_minor: i64,
    pub discount_minor: i64,
    /// When the bill settled (UTC ms).
    pub settled_at: i64,
    /// The device group the bill belongs to; empty for a counter sale with no device.
    pub group_id: String,
}

/// One line of a settled sales bill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SalesLineRow {
    pub order_id: String,
    pub amount_minor: i64,
    pub qty: i64,
    pub is_product: bool,
    pub is_plan: bool,
    /// The product's category; empty when the product is gone or the line is not a product.
    pub category: String,
}

/// A wallet top-up or credit collection settled in the range: a payment, not a sale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedRow {
    pub kind: String,
    pub amount_minor: i64,
    pub count: i64,
}

/// A session that started in the range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionReportRow {
    pub id: String,
    pub device_id: String,
    pub group_id: String,
    /// Minutes billed on the session's time lines; 0 when the bill has none (a pack or pass).
    pub billed_min: i64,
    /// Play time billed to the session's settled bill, in minor units.
    pub revenue_minor: i64,
}

/// One slice of a session's time on one device (UTC ms); `to_at` is the end, or `now` while open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentSpan {
    pub session_id: String,
    pub group_id: String,
    pub from_at: i64,
    pub to_at: i64,
}

/// A device that can be sold time: its label and group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportDevice {
    pub device_id: String,
    pub label: String,
    pub group_id: String,
}

pub fn sales_orders(
    conn: &Connection,
    from: &str,
    to: &str,
) -> Result<Vec<SalesOrderRow>, AppError> {
    sales_orders_where(conn, "o.business_day BETWEEN ?1 AND ?2", &from, &to)
}

/// The sales settled from `from_ms` to `to_ms` inclusive (UTC ms), by settle time: the bills of one
/// shift (Story 5.28).
pub fn sales_orders_between(
    conn: &Connection,
    from_ms: i64,
    to_ms: i64,
) -> Result<Vec<SalesOrderRow>, AppError> {
    sales_orders_where(conn, "o.settled_at BETWEEN ?1 AND ?2", &from_ms, &to_ms)
}

fn sales_orders_where(
    conn: &Connection,
    cond: &str,
    a: &dyn rusqlite::ToSql,
    b: &dyn rusqlite::ToSql,
) -> Result<Vec<SalesOrderRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT o.id, o.kind, o.total_minor, o.tax_minor, COALESCE(o.coupon_minor, 0),
                    COALESCE(o.discount_minor, 0), COALESCE(o.settled_at, o.created_at),
                    COALESCE(s.group_id, es.group_id, dm.group_id, '')
             FROM orders o
             LEFT JOIN sessions s ON s.id = o.session_id
             LEFT JOIN sessions es ON es.id = o.extends_session_id
             LEFT JOIN devices_meta dm ON dm.device_id = o.device_id
             WHERE o.status = 'settled' AND o.kind IN ('session', 'plan_sale', 'pos_sale')
               AND {cond}",
        ))
        .map_err(db_error)?;
    stmt.query_map(params![a, b], |r| {
        Ok(SalesOrderRow {
            id: r.get(0)?,
            kind: r.get(1)?,
            total_minor: r.get(2)?,
            tax_minor: r.get(3)?,
            coupon_minor: r.get(4)?,
            discount_minor: r.get(5)?,
            settled_at: r.get(6)?,
            group_id: r.get(7)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

pub fn sales_lines(conn: &Connection, from: &str, to: &str) -> Result<Vec<SalesLineRow>, AppError> {
    sales_lines_where(conn, "o.business_day BETWEEN ?1 AND ?2", &from, &to)
}

/// The lines of the sales settled from `from_ms` to `to_ms` inclusive (Story 5.28).
pub fn sales_lines_between(
    conn: &Connection,
    from_ms: i64,
    to_ms: i64,
) -> Result<Vec<SalesLineRow>, AppError> {
    sales_lines_where(conn, "o.settled_at BETWEEN ?1 AND ?2", &from_ms, &to_ms)
}

fn sales_lines_where(
    conn: &Connection,
    cond: &str,
    a: &dyn rusqlite::ToSql,
    b: &dyn rusqlite::ToSql,
) -> Result<Vec<SalesLineRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT l.order_id, l.amount_minor, COALESCE(l.qty, 1), l.product_id IS NOT NULL,
                    l.plan_id IS NOT NULL, COALESCE(p.category, '')
             FROM order_lines l
             JOIN orders o ON o.id = l.order_id
             LEFT JOIN products p ON p.product_id = l.product_id
             WHERE o.status = 'settled' AND o.kind IN ('session', 'plan_sale', 'pos_sale')
               AND {cond}",
        ))
        .map_err(db_error)?;
    stmt.query_map(params![a, b], |r| {
        Ok(SalesLineRow {
            order_id: r.get(0)?,
            amount_minor: r.get(1)?,
            qty: r.get(2)?,
            is_product: r.get(3)?,
            is_plan: r.get(4)?,
            category: r.get(5)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// Top-ups and credit collections settled in the range, one row per kind.
pub fn received(conn: &Connection, from: &str, to: &str) -> Result<Vec<ReceivedRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT kind, COALESCE(SUM(total_minor), 0), COUNT(*) FROM orders
             WHERE status = 'settled' AND kind IN ('topup', 'credit_collect')
               AND business_day BETWEEN ?1 AND ?2
             GROUP BY kind",
        )
        .map_err(db_error)?;
    stmt.query_map(params![from, to], |r| {
        Ok(ReceivedRow {
            kind: r.get(0)?,
            amount_minor: r.get(1)?,
            count: r.get(2)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// What credit notes issued in the range paid back.
pub fn refunded(conn: &Connection, from: &str, to: &str) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COALESCE(SUM(amount_minor), 0) FROM refunds WHERE business_day BETWEEN ?1 AND ?2",
        params![from, to],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// The name of every device group, keyed by id (deleted groups too, so old sales keep a name).
pub fn group_names(conn: &Connection) -> Result<Vec<(String, String)>, AppError> {
    let mut stmt = conn
        .prepare("SELECT group_id, name FROM device_groups ORDER BY sort_order, name")
        .map_err(db_error)?;
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)
}

/// The sessions that started in the range, with the time and play-time revenue on their bills.
pub fn session_rows(
    conn: &Connection,
    from: &str,
    to: &str,
) -> Result<Vec<SessionReportRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.device_id, s.group_id,
                    COALESCE(SUM(l.minutes), 0), COALESCE(SUM(l.amount_minor), 0)
             FROM sessions s
             LEFT JOIN orders o ON o.session_id = s.id AND o.status = 'settled'
             LEFT JOIN order_lines l ON l.order_id = o.id
                                    AND l.product_id IS NULL AND l.plan_id IS NULL
             WHERE s.business_day BETWEEN ?1 AND ?2 AND s.state <> 'orphaned'
             GROUP BY s.id",
        )
        .map_err(db_error)?;
    stmt.query_map(params![from, to], |r| {
        Ok(SessionReportRow {
            id: r.get(0)?,
            device_id: r.get(1)?,
            group_id: r.get(2)?,
            billed_min: r.get(3)?,
            revenue_minor: r.get(4)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// The time slices of the sessions that started in the range; an open slice runs to `now_ms`.
pub fn segment_spans(
    conn: &Connection,
    from: &str,
    to: &str,
    now_ms: i64,
) -> Result<Vec<SegmentSpan>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT g.session_id, s.group_id, g.from_at, COALESCE(g.to_at, ?3)
             FROM session_segments g JOIN sessions s ON s.id = g.session_id
             WHERE s.business_day BETWEEN ?1 AND ?2 AND s.state <> 'orphaned'",
        )
        .map_err(db_error)?;
    stmt.query_map(params![from, to, now_ms], |r| {
        Ok(SegmentSpan {
            session_id: r.get(0)?,
            group_id: r.get(1)?,
            from_at: r.get(2)?,
            to_at: r.get(3)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// Every device that is still in the branch and sits in a group.
pub fn report_devices(conn: &Connection) -> Result<Vec<ReportDevice>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT device_id, label, group_id FROM devices_meta
             WHERE deleted_at IS NULL AND group_id IS NOT NULL ORDER BY label, device_id",
        )
        .map_err(db_error)?;
    stmt.query_map([], |r| {
        Ok(ReportDevice {
            device_id: r.get(0)?,
            label: r.get(1)?,
            group_id: r.get(2)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}
