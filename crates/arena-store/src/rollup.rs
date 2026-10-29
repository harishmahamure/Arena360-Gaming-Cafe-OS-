//! The rows the daily rollup (Story 8.5) is folded from. Everything is read by the stamped
//! `business_day` columns and stored totals, as the Reports screen does (Story 5.27); rows that
//! only carry a time (shifts, stock moves, game plays, voids) are read for a UTC window around the
//! day and the fold keeps the ones whose business day matches. Reads only. The fold into the v1
//! document lives in `arena-core`.

use arena_proto::AppError;
use rusqlite::{Connection, OptionalExtension, params};

use crate::reports::{self, SegmentSpan};
use crate::shifts::ShiftDetail;
use crate::stock::ThresholdRow;
use crate::{SalesReportRows, StockMovesRows, db_error};

/// One positive payment row of a bill settled on the day (the Z-report's payments).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollupPaymentRow {
    pub method: String,
    pub amount_minor: i64,
}

/// An order voided, with what its lines came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollupVoidRow {
    pub voided_at: i64,
    pub amount_minor: i64,
}

/// A lot with stock left, close to or past its expiry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollupLotRow {
    pub product_id: String,
    pub lot_id: String,
    pub qty: i64,
    pub expires_on: String,
}

/// A player with credit outstanding, aged first in, first out as of the day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollupDebtorRow {
    pub player_id: String,
    pub username: String,
    pub outstanding_minor: i64,
    pub aging: arena_proto::CreditAging,
    /// The latest transactional consent on file is granted.
    pub consent: bool,
}

/// A game played, by the time it ended (kiosk clock).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollupPlayRow {
    pub game_id: String,
    pub ended_at: i64,
    pub minutes: i64,
}

/// Everything the rollup of one business day is folded from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RollupRows {
    /// The day's settled sales (`received` and `refunded_minor` included).
    pub sales: SalesReportRows,
    pub payments: Vec<RollupPaymentRow>,
    /// Credit notes issued on the day: count and total.
    pub refund_count: i64,
    pub voids: Vec<RollupVoidRow>,
    pub session_count: i64,
    pub segments: Vec<SegmentSpan>,
    /// Closed shifts with `closed_at` in the window.
    pub shifts: Vec<ShiftDetail>,
    /// The branch's shift variance threshold (minor units); `None` leaves shifts unflagged.
    pub variance_threshold: Option<i64>,
    pub levels: Vec<ThresholdRow>,
    /// Waste moves and expired write-offs in the window.
    pub moves: StockMovesRows,
    pub lots: Vec<RollupLotRow>,
    pub debtors: Vec<RollupDebtorRow>,
    pub plays: Vec<RollupPlayRow>,
}

fn payments(conn: &Connection, day: &str) -> Result<Vec<RollupPaymentRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT p.method, p.amount_minor
             FROM payments p JOIN orders o ON o.id = p.order_id
             WHERE o.status = 'settled'
               AND o.kind IN ('session', 'plan_sale', 'pos_sale', 'topup', 'credit_collect')
               AND o.business_day = ?1 AND p.amount_minor > 0
             ORDER BY o.settled_at, p.order_id, p.position, p.id",
        )
        .map_err(db_error)?;
    stmt.query_map([day], |r| {
        Ok(RollupPaymentRow {
            method: r.get(0)?,
            amount_minor: r.get(1)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

fn refund_count(conn: &Connection, day: &str) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COUNT(*) FROM refunds WHERE business_day = ?1",
        [day],
        |r| r.get(0),
    )
    .map_err(db_error)
}

fn voids(conn: &Connection, from_ms: i64, to_ms: i64) -> Result<Vec<RollupVoidRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT o.voided_at,
                    COALESCE((SELECT SUM(l.amount_minor) FROM order_lines l
                              WHERE l.order_id = o.id), 0)
             FROM orders o WHERE o.voided_at BETWEEN ?1 AND ?2 ORDER BY o.voided_at, o.id",
        )
        .map_err(db_error)?;
    stmt.query_map(params![from_ms, to_ms], |r| {
        Ok(RollupVoidRow {
            voided_at: r.get(0)?,
            amount_minor: r.get(1)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

fn session_count(conn: &Connection, day: &str) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COUNT(*) FROM sessions WHERE business_day = ?1 AND state <> 'orphaned'",
        [day],
        |r| r.get(0),
    )
    .map_err(db_error)
}

fn lots(conn: &Connection, until: &str) -> Result<Vec<RollupLotRow>, AppError> {
    Ok(crate::stock::list_expiring_lots(conn, until)?
        .into_iter()
        .map(|(lot, _name)| RollupLotRow {
            product_id: lot.product_id,
            lot_id: lot.lot_id,
            qty: lot.qty_remaining,
            expires_on: lot.expiry.unwrap_or_default(),
        })
        .collect())
}

fn debtors(conn: &Connection, day: &str) -> Result<Vec<RollupDebtorRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT c.player_id, COALESCE(p.username, ''), c.outstanding_minor
             FROM credit_balances c
             LEFT JOIN players p ON p.player_id = c.player_id AND p.deleted_at IS NULL
             WHERE c.outstanding_minor > 0 ORDER BY c.outstanding_minor DESC, c.player_id",
        )
        .map_err(db_error)?;
    let rows: Vec<(String, String, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)?;
    rows.into_iter()
        .map(|(player_id, username, outstanding_minor)| {
            Ok(RollupDebtorRow {
                aging: crate::credit::credit_aging(conn, &player_id, day)?,
                consent: crate::players::latest_consent(conn, &player_id, "transactional")?
                    .is_some_and(|(granted, _)| granted),
                player_id,
                username,
                outstanding_minor,
            })
        })
        .collect()
}

fn plays(conn: &Connection, from_ms: i64, to_ms: i64) -> Result<Vec<RollupPlayRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT game_id, ended_at, minutes FROM game_plays
             WHERE ended_at BETWEEN ?1 AND ?2 ORDER BY ended_at, id",
        )
        .map_err(db_error)?;
    stmt.query_map(params![from_ms, to_ms], |r| {
        Ok(RollupPlayRow {
            game_id: r.get(0)?,
            ended_at: r.get(1)?,
            minutes: r.get(2)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// Loads the rows of `day` (about twenty queries). `from_ms..=to_ms` is a UTC window that surely
/// holds every instant of the business day; `expiry_until` is the last expiry date listed.
pub fn load_rollup_rows(
    conn: &Connection,
    day: &str,
    from_ms: i64,
    to_ms: i64,
    expiry_until: &str,
    now_ms: i64,
) -> Result<RollupRows, AppError> {
    let sales = SalesReportRows {
        orders: reports::sales_orders(conn, day, day)?,
        lines: reports::sales_lines(conn, day, day)?,
        received: reports::received(conn, day, day)?,
        refunded_minor: reports::refunded(conn, day, day)?,
        groups: Vec::new(),
    };
    Ok(RollupRows {
        sales,
        payments: payments(conn, day)?,
        refund_count: refund_count(conn, day)?,
        voids: voids(conn, from_ms, to_ms)?,
        session_count: session_count(conn, day)?,
        segments: reports::segment_spans(conn, day, day, now_ms)?,
        shifts: crate::shifts::closed_shifts_between(conn, from_ms, to_ms)?,
        variance_threshold: Some(crate::shifts::variance_threshold(conn)?),
        levels: crate::stock::list_tracked_thresholds(conn)?,
        moves: StockMovesRows {
            moves: crate::stock_reports::stock_moves_between(
                conn, from_ms, to_ms, None, None, true,
            )?,
            staff: Vec::new(),
        },
        lots: lots(conn, expiry_until)?,
        debtors: debtors(conn, day)?,
        plays: plays(conn, from_ms, to_ms)?,
    })
}

// ---- what decides which days to roll up (Story 8.6) ----

/// Tables whose change rows can alter a published rollup: the stamped ledgers it is folded from.
/// Configuration, credit balances and stock levels move with every sale of a later day, so a
/// change to them never re-opens a day that was already published.
pub const ROLLUP_LEDGER_TABLES: &[&str] = &[
    "orders",
    "order_lines",
    "payments",
    "refunds",
    "sessions",
    "session_segments",
    "shifts",
    "cash_events",
    "stock_moves",
    "game_plays",
];

/// A change row just committed, as the uploader hears about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedRow {
    pub epoch: String,
    pub seq: i64,
    pub tbl: String,
    pub row_id: String,
}

/// What the planner of the daily rollups reads in one snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RollupFacts {
    /// When the branch first uploaded (Hub clock, ms): the start of the paid era.
    pub paid_since: Option<i64>,
    /// The days (`yyyy-mm-dd`) with a `rollup_log` row.
    pub published: std::collections::BTreeSet<String>,
    /// `closed_at` of every shift closed from the start of the paid era on.
    pub shifts_closed_at: Vec<i64>,
    /// A shift is open now.
    pub shift_open: bool,
    /// The business days the given changes were stamped to, or touched rows stamped to.
    pub changed_days: std::collections::BTreeSet<String>,
}

fn published_days(conn: &Connection) -> Result<std::collections::BTreeSet<String>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT date FROM rollup_log
             WHERE date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]' ORDER BY date",
        )
        .map_err(db_error)?;
    stmt.query_map([], |r| r.get(0))
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)
}

fn shifts_closed_since(conn: &Connection, since_ms: i64) -> Result<Vec<i64>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT closed_at FROM shifts
             WHERE state = 'closed' AND closed_at >= ?1 ORDER BY closed_at",
        )
        .map_err(db_error)?;
    stmt.query_map([since_ms], |r| r.get(0))
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)
}

/// The business days a set of committed changes belong to: the day each change row was stamped
/// with, and (for orders, refunds and sessions) the day the touched row itself is stamped to, so
/// an Undo of an earlier day's sale re-opens that day. Only the ledger tables count.
fn changed_days(
    conn: &Connection,
    changes: &[ChangedRow],
) -> Result<std::collections::BTreeSet<String>, AppError> {
    let mut days = std::collections::BTreeSet::new();
    for c in changes
        .iter()
        .filter(|c| ROLLUP_LEDGER_TABLES.contains(&c.tbl.as_str()))
    {
        let stamped: Option<Option<String>> = conn
            .query_row(
                "SELECT business_day FROM changes WHERE epoch = ?1 AND seq = ?2",
                params![c.epoch, c.seq],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_error)?;
        days.extend(stamped.flatten());
        if matches!(c.tbl.as_str(), "orders" | "refunds" | "sessions") {
            let own: Option<String> = conn
                .query_row(
                    &format!("SELECT business_day FROM {} WHERE id = ?1", c.tbl),
                    [&c.row_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_error)?;
            days.extend(own);
        }
    }
    Ok(days)
}

/// Reads the facts the rollup planner needs, `changes` being the ledger changes heard since the
/// last plan.
pub fn load_rollup_facts(
    conn: &Connection,
    changes: &[ChangedRow],
) -> Result<RollupFacts, AppError> {
    let paid_since = crate::upload::read_paid_since(conn)?;
    Ok(RollupFacts {
        paid_since,
        published: published_days(conn)?,
        shifts_closed_at: match paid_since {
            Some(since) => shifts_closed_since(conn, since)?,
            None => Vec::new(),
        },
        shift_open: crate::shifts::open_shift_id(conn)?.is_some(),
        changed_days: changed_days(conn, changes)?,
    })
}
