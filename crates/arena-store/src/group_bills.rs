use arena_proto::AppError;
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;
use crate::sessions::{
    LiveSession, ORDER_COLUMNS, OrderRow, SESSION_COLUMNS, SegmentRow, live_from, order_from,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupBillRow {
    pub id: String,
    pub payer_player_id: Option<String>,
    pub payer_username: Option<String>,
    pub payer_guest_no: Option<i64>,
    pub state: String,
    pub rev: i64,
}

const COLUMNS: &str = "g.id, g.payer_player_id, p.username, g.payer_guest_no, g.state, g.rev";

fn from(r: &rusqlite::Row<'_>) -> rusqlite::Result<GroupBillRow> {
    Ok(GroupBillRow {
        id: r.get(0)?,
        payer_player_id: r.get(1)?,
        payer_username: r.get(2)?,
        payer_guest_no: r.get(3)?,
        state: r.get(4)?,
        rev: r.get(5)?,
    })
}

pub fn insert_group_bill(
    conn: &Connection,
    id: &str,
    payer_player_id: Option<&str>,
    payer_guest_no: Option<i64>,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO group_bills (id, payer_player_id, payer_guest_no, state, rev, created_at,
                                  updated_at)
         VALUES (?1, ?2, ?3, 'open', 0, 0, 0)",
        params![id, payer_player_id, payer_guest_no],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn find_group_bill(conn: &Connection, id: &str) -> Result<Option<GroupBillRow>, AppError> {
    conn.query_row(
        &format!(
            "SELECT {COLUMNS} FROM group_bills g
             LEFT JOIN players p ON p.player_id = g.payer_player_id WHERE g.id = ?1"
        ),
        [id],
        from,
    )
    .optional()
    .map_err(db_error)
}

/// Group bills still open, or closed with an unsettled bill.
pub fn list_active_group_bills(conn: &Connection) -> Result<Vec<GroupBillRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM group_bills g
             LEFT JOIN players p ON p.player_id = g.payer_player_id
             WHERE g.state = 'open'
                OR EXISTS (SELECT 1 FROM orders o WHERE o.group_bill_id = g.id AND o.status = 'open')
             ORDER BY g.created_at, g.id"
        ))
        .map_err(db_error)?;
    stmt.query_map([], from)
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)
}

pub fn set_group_payer(
    conn: &Connection,
    id: &str,
    payer_player_id: Option<&str>,
    payer_guest_no: Option<i64>,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE group_bills SET payer_player_id = ?2, payer_guest_no = ?3 WHERE id = ?1",
        params![id, payer_player_id, payer_guest_no],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn set_group_state(conn: &Connection, id: &str, state: &str) -> Result<(), AppError> {
    conn.execute(
        "UPDATE group_bills SET state = ?2 WHERE id = ?1",
        params![id, state],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn set_session_group(
    conn: &Connection,
    session_id: &str,
    group_bill_id: &str,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE sessions SET group_bill_id = ?2 WHERE id = ?1",
        params![session_id, group_bill_id],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn set_order_group(
    conn: &Connection,
    order_id: &str,
    group_bill_id: &str,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE orders SET group_bill_id = ?2 WHERE id = ?1",
        params![order_id, group_bill_id],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn set_order_payer(
    conn: &Connection,
    order_id: &str,
    player_id: Option<&str>,
    guest_no: Option<i64>,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE orders SET player_id = ?2, guest_no = ?3 WHERE id = ?1",
        params![order_id, player_id, guest_no],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn find_group_order(
    conn: &Connection,
    group_bill_id: &str,
) -> Result<Option<OrderRow>, AppError> {
    conn.query_row(
        &format!("SELECT {ORDER_COLUMNS} FROM orders WHERE group_bill_id = ?1"),
        [group_bill_id],
        order_from,
    )
    .optional()
    .map_err(db_error)
}

/// Every session of a group bill, running or ended, oldest first.
pub fn list_group_sessions(
    conn: &Connection,
    group_bill_id: &str,
) -> Result<Vec<LiveSession>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {SESSION_COLUMNS} FROM sessions s
             LEFT JOIN players p ON p.player_id = s.player_id
             WHERE s.group_bill_id = ?1 ORDER BY s.started_at, s.id"
        ))
        .map_err(db_error)?;
    stmt.query_map([group_bill_id], live_from)
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)
}

/// The sessions of open group bills that have already ended.
pub fn list_ended_group_sessions(conn: &Connection) -> Result<Vec<LiveSession>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {SESSION_COLUMNS} FROM sessions s
             LEFT JOIN players p ON p.player_id = s.player_id
             JOIN group_bills g ON g.id = s.group_bill_id
             WHERE g.state = 'open' AND s.state = 'ended' ORDER BY s.started_at, s.id"
        ))
        .map_err(db_error)?;
    stmt.query_map([], live_from)
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)
}

/// The segments of those ended sessions.
pub fn list_ended_group_segments(conn: &Connection) -> Result<Vec<SegmentRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT g.id, g.session_id, g.from_at, g.to_at, g.device_id, g.rate_minor_per_hour,
                    g.paused_ms, g.rule_id
             FROM session_segments g JOIN sessions s ON s.id = g.session_id
                  JOIN group_bills b ON b.id = s.group_bill_id
             WHERE b.state = 'open' AND s.state = 'ended' ORDER BY g.from_at, g.id",
        )
        .map_err(db_error)?;
    stmt.query_map([], |r| {
        Ok(SegmentRow {
            id: r.get(0)?,
            session_id: r.get(1)?,
            from_at: r.get(2)?,
            to_at: r.get(3)?,
            device_id: r.get(4)?,
            rate_minor_per_hour: r.get(5)?,
            paused_ms: r.get(6)?,
            rule_id: r.get(7)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}
