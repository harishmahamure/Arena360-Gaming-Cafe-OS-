//! The rows a shift's Z-report is built from (Story 5.28). A shift's rows are its `cash_events`
//! (by `shift_id`) and, by time between `opened_at` and the close time, the settled sales and
//! payments, refunds, waste moves and voids. The windows of two shifts never overlap (one shift is
//! open at a time), so a row belongs to exactly one shift. The fold into the report lives in
//! `arena-core`.

use arena_proto::AppError;
use rusqlite::{Connection, params};

use crate::SalesReportRows;
use crate::db_error;
use crate::reports::{self, ReceivedRow};
use crate::shifts::ShiftDetail;

/// One positive payment row of a settled bill, top-up or credit collection in the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZPaymentRow {
    pub method: String,
    pub amount_minor: i64,
    pub reference: Option<String>,
    /// When the bill settled (UTC ms).
    pub at: i64,
    pub invoice_no: Option<String>,
}

/// A credit note issued in the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZRefundRow {
    pub credit_note_no: String,
    pub method: Option<String>,
    pub amount_minor: i64,
}

/// Stock written off in the window, one row per waste move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZWasteRow {
    pub reason: String,
    pub qty: i64,
    pub value_minor: i64,
}

/// An order voided in the window, with what its lines came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZVoidRow {
    pub reason: String,
    pub amount_minor: i64,
}

/// A `cash_events` row of the shift that is not a sale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZCashEventRow {
    pub kind: String,
    pub amount_minor: i64,
    pub reference: Option<String>,
    pub category: Option<String>,
    pub paid_from: Option<String>,
    pub spent_minor: Option<i64>,
    pub note: Option<String>,
}

/// Everything the Z-report of `shift` is folded from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZReportRows {
    pub shift: ShiftDetail,
    /// The shift's sales; its `received`, `refunded_minor` and `groups` stay empty (see the
    /// fields below).
    pub sales: SalesReportRows,
    pub received: Vec<ReceivedRow>,
    pub payments: Vec<ZPaymentRow>,
    pub refunds: Vec<ZRefundRow>,
    pub waste: Vec<ZWasteRow>,
    pub voids: Vec<ZVoidRow>,
    pub cash_events: Vec<ZCashEventRow>,
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [name],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// Loads the rows of `shift` for the window from its opening to `to_ms` (inclusive).
pub fn load_z_report_rows(
    conn: &Connection,
    shift: ShiftDetail,
    to_ms: i64,
) -> Result<ZReportRows, AppError> {
    let from = shift.opened_at;
    let sales = SalesReportRows {
        orders: reports::sales_orders_between(conn, from, to_ms)?,
        lines: reports::sales_lines_between(conn, from, to_ms)?,
        received: Vec::new(),
        refunded_minor: 0,
        groups: Vec::new(),
    };

    let received = conn
        .prepare(
            "SELECT o.kind, COALESCE(SUM(o.total_minor), 0), COUNT(*) FROM orders o
             WHERE o.status = 'settled' AND o.kind IN ('topup', 'credit_collect')
               AND o.settled_at BETWEEN ?1 AND ?2
             GROUP BY o.kind",
        )
        .map_err(db_error)?
        .query_map(params![from, to_ms], |r| {
            Ok(ReceivedRow {
                kind: r.get(0)?,
                amount_minor: r.get(1)?,
                count: r.get(2)?,
            })
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;

    let payments = conn
        .prepare(
            "SELECT p.method, p.amount_minor, p.reference, COALESCE(o.settled_at, 0), o.invoice_no
             FROM payments p JOIN orders o ON o.id = p.order_id
             WHERE o.status = 'settled'
               AND o.kind IN ('session', 'plan_sale', 'pos_sale', 'topup', 'credit_collect')
               AND o.settled_at BETWEEN ?1 AND ?2 AND p.amount_minor > 0
             ORDER BY o.settled_at, p.order_id, p.position, p.id",
        )
        .map_err(db_error)?
        .query_map(params![from, to_ms], |r| {
            Ok(ZPaymentRow {
                method: r.get(0)?,
                amount_minor: r.get(1)?,
                reference: r.get(2)?,
                at: r.get(3)?,
                invoice_no: r.get(4)?,
            })
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;

    let refunds = if table_exists(conn, "refunds")? {
        conn.prepare(
            "SELECT credit_note_no, method, amount_minor FROM refunds
             WHERE created_at BETWEEN ?1 AND ?2 ORDER BY created_at, id",
        )
        .map_err(db_error)?
        .query_map(params![from, to_ms], |r| {
            Ok(ZRefundRow {
                credit_note_no: r.get(0)?,
                method: r.get(1)?,
                amount_minor: r.get(2)?,
            })
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?
    } else {
        Vec::new()
    };

    let waste = conn
        .prepare(
            "SELECT waste_reason, ABS(delta), value_minor FROM stock_moves
             WHERE waste_reason IS NOT NULL AND at BETWEEN ?1 AND ?2 ORDER BY at, move_id",
        )
        .map_err(db_error)?
        .query_map(params![from, to_ms], |r| {
            Ok(ZWasteRow {
                reason: r.get(0)?,
                qty: r.get(1)?,
                value_minor: r.get(2)?,
            })
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;

    let voids = conn
        .prepare(
            "SELECT COALESCE(o.void_reason, 'other'),
                    COALESCE((SELECT SUM(l.amount_minor) FROM order_lines l
                              WHERE l.order_id = o.id), 0)
             FROM orders o WHERE o.voided_at BETWEEN ?1 AND ?2 ORDER BY o.voided_at, o.id",
        )
        .map_err(db_error)?
        .query_map(params![from, to_ms], |r| {
            Ok(ZVoidRow {
                reason: r.get(0)?,
                amount_minor: r.get(1)?,
            })
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;

    let cash_events = conn
        .prepare(
            "SELECT kind, amount_minor, reference, category, paid_from, spent_minor, note
             FROM cash_events WHERE shift_id = ?1 AND kind <> 'sale' AND kind <> 'sale_reverted'
             ORDER BY created_at, id",
        )
        .map_err(db_error)?
        .query_map([&shift.id], |r| {
            Ok(ZCashEventRow {
                kind: r.get(0)?,
                amount_minor: r.get(1)?,
                reference: r.get(2)?,
                category: r.get(3)?,
                paid_from: r.get(4)?,
                spent_minor: r.get(5)?,
                note: r.get(6)?,
            })
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;

    Ok(ZReportRows {
        shift,
        sales,
        received,
        payments,
        refunds,
        waste,
        voids,
        cash_events,
    })
}
