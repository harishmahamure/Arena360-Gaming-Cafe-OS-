use arena_proto::AppError;
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

pub struct NewSession<'a> {
    pub id: &'a str,
    pub player_id: Option<&'a str>,
    pub guest_no: Option<i64>,
    pub device_id: &'a str,
    pub group_id: &'a str,
    pub mode: &'a str,
    pub rate_id: &'a str,
    pub started_at: i64,
    pub rounding_min: u32,
    pub minimum_min: u32,
    pub grace_min: u32,
    pub time_up_mode: &'a str,
    pub epoch: &'a str,
    pub business_day: &'a str,
    pub fy: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSession {
    pub id: String,
    pub device_id: String,
    pub state: String,
    pub started_at: i64,
    pub player_id: Option<String>,
    pub username: Option<String>,
    pub guest_no: Option<i64>,
    pub time_up_mode: String,
    pub mode: String,
    pub rev: i64,
    pub rounding_min: u32,
    pub minimum_min: u32,
    pub paused_total_ms: i64,
    pub business_day: String,
    pub fy: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentRow {
    pub id: String,
    pub session_id: String,
    pub from_at: i64,
    pub to_at: Option<i64>,
    pub device_id: String,
    pub rate_minor_per_hour: i64,
}

pub struct NewOrder<'a> {
    pub id: &'a str,
    pub session_id: &'a str,
    pub player_id: Option<&'a str>,
    pub guest_no: Option<i64>,
    pub device_id: &'a str,
    pub subtotal_minor: i64,
    pub business_day: &'a str,
    pub fy: &'a str,
}

pub struct NewOrderLine<'a> {
    pub id: &'a str,
    pub order_id: &'a str,
    pub segment_id: &'a str,
    pub device_id: &'a str,
    pub from_at: i64,
    pub to_at: i64,
    pub minutes: i64,
    pub rate_minor_per_hour: i64,
    pub amount_minor: i64,
    pub position: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderLineRow {
    pub order_id: String,
    pub device_id: String,
    pub from_at: i64,
    pub to_at: i64,
    pub minutes: i64,
    pub rate_minor_per_hour: i64,
    pub amount_minor: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenOrder {
    pub id: String,
    pub rev: i64,
    pub session_id: Option<String>,
    pub device_id: String,
    pub subtotal_minor: i64,
}

pub fn insert_session(conn: &Connection, s: &NewSession<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO sessions (id, player_id, guest_no, device_id, group_id, mode, rate_id, state,
                               started_at, rounding_min, minimum_min, grace_min, time_up_mode,
                               epoch, business_day, fy, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'active', ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                 0, 0, 0)",
        params![
            s.id,
            s.player_id,
            s.guest_no,
            s.device_id,
            s.group_id,
            s.mode,
            s.rate_id,
            s.started_at,
            s.rounding_min,
            s.minimum_min,
            s.grace_min,
            s.time_up_mode,
            s.epoch,
            s.business_day,
            s.fy
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn insert_segment(
    conn: &Connection,
    id: &str,
    session_id: &str,
    from_at: i64,
    device_id: &str,
    rate_minor_per_hour: i64,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO session_segments (id, session_id, from_at, device_id, rate_minor_per_hour,
                                       rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, 0)",
        params![id, session_id, from_at, device_id, rate_minor_per_hour],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn device_has_live_session(conn: &Connection, device_id: &str) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sessions
                        WHERE device_id = ?1 AND state IN ('active', 'paused', 'grace'))",
        [device_id],
        |r| r.get(0),
    )
    .map_err(db_error)
}

pub fn next_guest_no(conn: &Connection, business_day: &str) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COALESCE(MAX(guest_no), 0) + 1 FROM sessions WHERE business_day = ?1",
        [business_day],
        |r| r.get(0),
    )
    .map_err(db_error)
}

const SESSION_COLUMNS: &str = "s.id, s.device_id, s.state, s.started_at, s.player_id, p.username,
        s.guest_no, s.time_up_mode, s.mode, s.rev, s.rounding_min, s.minimum_min,
        s.paused_total_ms, s.business_day, s.fy";

fn live_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<LiveSession> {
    Ok(LiveSession {
        id: r.get(0)?,
        device_id: r.get(1)?,
        state: r.get(2)?,
        started_at: r.get(3)?,
        player_id: r.get(4)?,
        username: r.get(5)?,
        guest_no: r.get(6)?,
        time_up_mode: r.get(7)?,
        mode: r.get(8)?,
        rev: r.get(9)?,
        rounding_min: r.get(10)?,
        minimum_min: r.get(11)?,
        paused_total_ms: r.get(12)?,
        business_day: r.get(13)?,
        fy: r.get(14)?,
    })
}

pub fn find_session(conn: &Connection, id: &str) -> Result<Option<LiveSession>, AppError> {
    conn.query_row(
        &format!(
            "SELECT {SESSION_COLUMNS} FROM sessions s
             LEFT JOIN players p ON p.player_id = s.player_id WHERE s.id = ?1"
        ),
        [id],
        live_from,
    )
    .optional()
    .map_err(db_error)
}

pub fn list_live_sessions(conn: &Connection) -> Result<Vec<LiveSession>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {SESSION_COLUMNS} FROM sessions s
             LEFT JOIN players p ON p.player_id = s.player_id
             WHERE s.state IN ('active', 'paused', 'grace')
             ORDER BY s.started_at, s.id"
        ))
        .map_err(db_error)?;
    stmt.query_map([], live_from)
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)
}

pub fn list_segments(
    conn: &Connection,
    session_id: Option<&str>,
) -> Result<Vec<SegmentRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT g.id, g.session_id, g.from_at, g.to_at, g.device_id, g.rate_minor_per_hour
             FROM session_segments g JOIN sessions s ON s.id = g.session_id
             WHERE (?1 IS NOT NULL AND g.session_id = ?1)
                OR (?1 IS NULL AND s.state IN ('active', 'paused', 'grace'))
             ORDER BY g.from_at, g.id",
        )
        .map_err(db_error)?;
    stmt.query_map([session_id], |r| {
        Ok(SegmentRow {
            id: r.get(0)?,
            session_id: r.get(1)?,
            from_at: r.get(2)?,
            to_at: r.get(3)?,
            device_id: r.get(4)?,
            rate_minor_per_hour: r.get(5)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

pub fn end_session(conn: &Connection, id: &str, ended_at: i64) -> Result<(), AppError> {
    conn.execute(
        "UPDATE sessions SET state = 'ended', ended_at = ?2, end_reason = 'staff' WHERE id = ?1",
        params![id, ended_at],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn close_segments(conn: &Connection, session_id: &str, to_at: i64) -> Result<(), AppError> {
    conn.execute(
        "UPDATE session_segments SET to_at = ?2 WHERE session_id = ?1 AND to_at IS NULL",
        params![session_id, to_at],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn insert_order(conn: &Connection, o: &NewOrder<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO orders (id, session_id, player_id, guest_no, device_id, status, subtotal_minor,
                             total_minor, business_day, fy, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'open', ?6, ?6, ?7, ?8, 0, 0, 0)",
        params![
            o.id,
            o.session_id,
            o.player_id,
            o.guest_no,
            o.device_id,
            o.subtotal_minor,
            o.business_day,
            o.fy
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn insert_order_line(conn: &Connection, l: &NewOrderLine<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO order_lines (id, order_id, session_segment_id, device_id, from_at, to_at,
                                  minutes, rate_minor_per_hour, amount_minor, position, rev,
                                  created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, 0, 0)",
        params![
            l.id,
            l.order_id,
            l.segment_id,
            l.device_id,
            l.from_at,
            l.to_at,
            l.minutes,
            l.rate_minor_per_hour,
            l.amount_minor,
            l.position
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn list_open_orders(conn: &Connection) -> Result<Vec<OpenOrder>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, COALESCE(device_id, ''), subtotal_minor, rev FROM orders
             WHERE status = 'open' ORDER BY created_at, id",
        )
        .map_err(db_error)?;
    stmt.query_map([], |r| {
        Ok(OpenOrder {
            id: r.get(0)?,
            session_id: r.get(1)?,
            device_id: r.get(2)?,
            subtotal_minor: r.get(3)?,
            rev: r.get(4)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

pub fn list_open_order_lines(conn: &Connection) -> Result<Vec<OrderLineRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT l.order_id, COALESCE(l.device_id, ''), l.from_at, l.to_at, l.minutes,
                    l.rate_minor_per_hour, l.amount_minor
             FROM order_lines l JOIN orders o ON o.id = l.order_id
             WHERE o.status = 'open' ORDER BY l.order_id, l.position",
        )
        .map_err(db_error)?;
    stmt.query_map([], |r| {
        Ok(OrderLineRow {
            order_id: r.get(0)?,
            device_id: r.get(1)?,
            from_at: r.get(2)?,
            to_at: r.get(3)?,
            minutes: r.get(4)?,
            rate_minor_per_hour: r.get(5)?,
            amount_minor: r.get(6)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderRow {
    pub id: String,
    pub status: String,
    pub total_minor: i64,
    pub rev: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settlement {
    pub order_id: String,
    pub device_id: Option<String>,
    pub settled_at: i64,
    pub device_busy: bool,
}

pub struct NewPayment<'a> {
    pub id: &'a str,
    pub order_id: &'a str,
    pub method: &'a str,
    pub amount_minor: i64,
    pub reference: Option<&'a str>,
    pub staff_id: &'a str,
    pub position: i64,
    pub business_day: &'a str,
    pub fy: &'a str,
}

pub fn find_order(conn: &Connection, id: &str) -> Result<Option<OrderRow>, AppError> {
    conn.query_row(
        "SELECT id, status, total_minor, rev FROM orders WHERE id = ?1",
        [id],
        |r| {
            Ok(OrderRow {
                id: r.get(0)?,
                status: r.get(1)?,
                total_minor: r.get(2)?,
                rev: r.get(3)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

pub fn settle_order(
    conn: &Connection,
    id: &str,
    settled_at: i64,
    settled_by: &str,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE orders SET status = 'settled', settled_at = ?2, settled_by = ?3 WHERE id = ?1",
        params![id, settled_at, settled_by],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn insert_payment(conn: &Connection, p: &NewPayment<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO payments (id, order_id, method, amount_minor, reference, staff_id, position,
                               business_day, fy, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, 0, 0)",
        params![
            p.id,
            p.order_id,
            p.method,
            p.amount_minor,
            p.reference,
            p.staff_id,
            p.position,
            p.business_day,
            p.fy
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn find_settlement(conn: &Connection, order_id: &str) -> Result<Option<Settlement>, AppError> {
    conn.query_row(
        "SELECT o.id, o.device_id, o.settled_at,
                EXISTS (SELECT 1 FROM sessions s
                        WHERE s.device_id = o.device_id AND s.state IN ('active', 'paused', 'grace'))
         FROM orders o WHERE o.id = ?1 AND o.status = 'settled' AND o.settled_at IS NOT NULL",
        [order_id],
        |r| {
            Ok(Settlement {
                order_id: r.get(0)?,
                device_id: r.get(1)?,
                settled_at: r.get(2)?,
                device_busy: r.get(3)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}
