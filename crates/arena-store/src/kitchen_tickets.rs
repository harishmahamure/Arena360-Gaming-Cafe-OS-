//! Kitchen tickets (Story 7.3): the `kitchen_tickets` and `kitchen_ticket_lines` change-log
//! tables. Only `apply()` writes them. A ticket holds the kitchen lines of one placement and moves
//! New, Preparing, Ready, Served; a ticket whose last line was cancelled is Cancelled.

use arena_proto::AppError;
use rusqlite::{Connection, OptionalExtension, params};

use crate::{HoldRow, db_error};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketRow {
    pub id: String,
    pub order_id: String,
    pub device_id: Option<String>,
    pub seat: String,
    pub number: i64,
    pub stage: String,
    pub placement_id: Option<String>,
    pub placed_at: i64,
    pub advanced_at: Option<i64>,
    pub advanced_by: Option<String>,
    pub cancel_reason: Option<String>,
    pub rev: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketLineRow {
    pub id: String,
    pub ticket_id: String,
    pub order_line_id: String,
    pub label: String,
    pub qty: i64,
    pub notes: Vec<String>,
}

/// What `apply()` writes for a new ticket (`rev` and the timestamps are set by the stamp step).
pub struct NewTicket<'a> {
    pub id: &'a str,
    pub order_id: &'a str,
    pub device_id: Option<&'a str>,
    pub seat: &'a str,
    pub number: i64,
    pub placement_id: Option<&'a str>,
    pub placed_at: i64,
}

pub struct NewTicketLine<'a> {
    pub id: &'a str,
    pub ticket_id: &'a str,
    pub order_line_id: &'a str,
    pub label: &'a str,
    pub qty: i64,
    pub notes: &'a [String],
}

const TICKET_COLUMNS: &str =
    "id, order_id, device_id, seat, number, stage, placement_id, placed_at,
                              advanced_at, advanced_by, cancel_reason, rev";

fn ticket(r: &rusqlite::Row<'_>) -> rusqlite::Result<TicketRow> {
    Ok(TicketRow {
        id: r.get(0)?,
        order_id: r.get(1)?,
        device_id: r.get(2)?,
        seat: r.get(3)?,
        number: r.get(4)?,
        stage: r.get(5)?,
        placement_id: r.get(6)?,
        placed_at: r.get(7)?,
        advanced_at: r.get(8)?,
        advanced_by: r.get(9)?,
        cancel_reason: r.get(10)?,
        rev: r.get(11)?,
    })
}

const LINE_COLUMNS: &str = "id, ticket_id, order_line_id, label, qty, notes";

fn line(r: &rusqlite::Row<'_>) -> rusqlite::Result<TicketLineRow> {
    let notes: Option<String> = r.get(5)?;
    Ok(TicketLineRow {
        id: r.get(0)?,
        ticket_id: r.get(1)?,
        order_line_id: r.get(2)?,
        label: r.get(3)?,
        qty: r.get(4)?,
        notes: notes
            .and_then(|n| serde_json::from_str(&n).ok())
            .unwrap_or_default(),
    })
}

/// The number the branch's next ticket takes: one past the highest, from 1.
pub fn next_ticket_number(conn: &Connection) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COALESCE(MAX(number), 0) + 1 FROM kitchen_tickets",
        [],
        |r| r.get(0),
    )
    .map_err(db_error)
}

pub fn insert_ticket(conn: &Connection, t: &NewTicket<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO kitchen_tickets (id, order_id, device_id, seat, number, stage, placement_id,
                                      placed_at, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'new', ?6, ?7, 0, 0, 0)",
        params![
            t.id,
            t.order_id,
            t.device_id,
            t.seat,
            t.number,
            t.placement_id,
            t.placed_at
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn insert_ticket_line(conn: &Connection, l: &NewTicketLine<'_>) -> Result<(), AppError> {
    let notes = if l.notes.is_empty() {
        None
    } else {
        Some(serde_json::to_string(l.notes).map_err(|_| {
            AppError::new(
                arena_proto::ErrorCode::Internal,
                "the notes cannot be written",
            )
        })?)
    };
    conn.execute(
        "INSERT INTO kitchen_ticket_lines (id, ticket_id, order_line_id, label, qty, notes, rev,
                                           created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, 0)",
        params![l.id, l.ticket_id, l.order_line_id, l.label, l.qty, notes],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn find_ticket(conn: &Connection, id: &str) -> Result<Option<TicketRow>, AppError> {
    conn.query_row(
        &format!("SELECT {TICKET_COLUMNS} FROM kitchen_tickets WHERE id = ?1"),
        [id],
        ticket,
    )
    .optional()
    .map_err(db_error)
}

/// An order's tickets, oldest first.
pub fn list_order_tickets(conn: &Connection, order_id: &str) -> Result<Vec<TicketRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {TICKET_COLUMNS} FROM kitchen_tickets WHERE order_id = ?1 ORDER BY number"
        ))
        .map_err(db_error)?;
    let rows = stmt.query_map([order_id], ticket).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

/// The ticket a placement opened, if it opened one.
pub fn find_placement_ticket(
    conn: &Connection,
    order_id: &str,
    placement_id: &str,
) -> Result<Option<TicketRow>, AppError> {
    conn.query_row(
        &format!(
            "SELECT {TICKET_COLUMNS} FROM kitchen_tickets
             WHERE order_id = ?1 AND placement_id = ?2"
        ),
        params![order_id, placement_id],
        ticket,
    )
    .optional()
    .map_err(db_error)
}

/// Every ticket that is not yet served or cancelled, oldest first.
pub fn list_open_tickets(conn: &Connection) -> Result<Vec<TicketRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {TICKET_COLUMNS} FROM kitchen_tickets
             WHERE stage IN ('new', 'preparing', 'ready') ORDER BY number"
        ))
        .map_err(db_error)?;
    let rows = stmt.query_map([], ticket).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

pub fn list_ticket_lines(
    conn: &Connection,
    ticket_id: &str,
) -> Result<Vec<TicketLineRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {LINE_COLUMNS} FROM kitchen_ticket_lines WHERE ticket_id = ?1 ORDER BY rowid"
        ))
        .map_err(db_error)?;
    let rows = stmt.query_map([ticket_id], line).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

/// The ticket line that carries an order line, if the line is a kitchen line on a ticket.
pub fn find_line_for_order_line(
    conn: &Connection,
    order_line_id: &str,
) -> Result<Option<TicketLineRow>, AppError> {
    conn.query_row(
        &format!("SELECT {LINE_COLUMNS} FROM kitchen_ticket_lines WHERE order_line_id = ?1"),
        [order_line_id],
        line,
    )
    .optional()
    .map_err(db_error)
}

/// Moves a ticket to `stage`, remembering who did it and when (its Undo window).
pub fn set_ticket_stage(
    conn: &Connection,
    id: &str,
    stage: &str,
    at: i64,
    by: &str,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE kitchen_tickets SET stage = ?2, advanced_at = ?3, advanced_by = ?4 WHERE id = ?1",
        params![id, stage, at, by],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Cancels a ticket, keeping the reason.
pub fn set_ticket_cancelled(
    conn: &Connection,
    id: &str,
    reason: &str,
    at: i64,
    by: &str,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE kitchen_tickets SET stage = 'cancelled', cancel_reason = ?2, advanced_at = ?3,
                advanced_by = ?4 WHERE id = ?1",
        params![id, reason, at, by],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Reopens a hold a serve closed (the Undo of a Served advance).
pub fn reopen_hold(conn: &Connection, hold_id: &str) -> Result<(), AppError> {
    conn.execute(
        "UPDATE stock_holds SET released_at = NULL WHERE id = ?1",
        [hold_id],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Marks a served line unserved again (the Undo of a Served advance).
pub fn unmark_line_served(conn: &Connection, line_id: &str) -> Result<(), AppError> {
    conn.execute(
        "UPDATE order_lines SET served = 0, served_at = NULL, served_by = NULL WHERE id = ?1",
        [line_id],
    )
    .map_err(db_error)?;
    Ok(())
}

/// The holds of one line closed exactly at `at`.
pub fn list_line_holds_released_at(
    conn: &Connection,
    line_id: &str,
    at: i64,
) -> Result<Vec<HoldRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, order_line_id, item_id, qty, released_at FROM stock_holds
             WHERE order_line_id = ?1 AND released_at = ?2 ORDER BY rowid",
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map(params![line_id, at], |r| {
            Ok(HoldRow {
                id: r.get(0)?,
                order_line_id: r.get(1)?,
                item_id: r.get(2)?,
                qty: r.get(3)?,
                released_at: r.get(4)?,
            })
        })
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

/// Puts a ticket back on `stage` after an Undo and clears its advance record, so the Undo itself
/// cannot be undone.
pub fn restore_ticket_stage(conn: &Connection, id: &str, stage: &str) -> Result<(), AppError> {
    conn.execute(
        "UPDATE kitchen_tickets SET stage = ?2, advanced_at = NULL, advanced_by = NULL
         WHERE id = ?1",
        params![id, stage],
    )
    .map_err(db_error)?;
    Ok(())
}

/// The order lines of a ticket that were served exactly at `at`: what one advance served.
pub fn list_ticket_lines_served_at(
    conn: &Connection,
    ticket_id: &str,
    at: i64,
) -> Result<Vec<String>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT l.id FROM order_lines l
             JOIN kitchen_ticket_lines k ON k.order_line_id = l.id
             WHERE k.ticket_id = ?1 AND l.served = 1 AND l.served_at = ?2 ORDER BY k.rowid",
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map(params![ticket_id, at], |r| r.get(0))
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}
