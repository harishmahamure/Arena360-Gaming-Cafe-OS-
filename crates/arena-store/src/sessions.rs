use arena_proto::{AppError, ErrorCode};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

pub struct NewSession<'a> {
    pub id: &'a str,
    pub player_id: Option<&'a str>,
    pub guest_no: Option<i64>,
    pub device_id: &'a str,
    pub group_id: &'a str,
    pub mode: &'a str,
    pub rate_id: Option<&'a str>,
    pub pack_id: Option<&'a str>,
    pub started_at: i64,
    pub ends_at: Option<i64>,
    pub reserved_minor: i64,
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
    pub ended_at: Option<i64>,
    pub ended_by: Option<String>,
    pub ends_at: Option<i64>,
    pub paused_at: Option<i64>,
    pub pause_reason: Option<String>,
    pub pause_note: Option<String>,
    pub pack_id: Option<String>,
    pub reserved_minor: i64,
    pub grace_min: u32,
    pub extended_min: i64,
    pub rate_id: Option<String>,
    pub group_id: String,
    pub group_bill_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentRow {
    pub id: String,
    pub session_id: String,
    pub from_at: i64,
    pub to_at: Option<i64>,
    pub device_id: String,
    pub rate_minor_per_hour: i64,
    pub paused_ms: i64,
    pub rule_id: Option<String>,
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
    pub id: String,
    pub order_id: String,
    pub device_id: String,
    pub from_at: i64,
    pub to_at: i64,
    pub minutes: i64,
    pub rate_minor_per_hour: i64,
    pub amount_minor: i64,
    pub rule_id: Option<String>,
    /// The product's name when this is a placed product line (`None` for a session line).
    pub label: Option<String>,
    pub qty: Option<i64>,
    /// Whether the line was served (a time line always is).
    pub served: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenOrder {
    pub id: String,
    pub player_id: Option<String>,
    pub wallet_minor: Option<i64>,
    pub credit: Option<crate::CreditRow>,
    pub rev: i64,
    pub session_id: Option<String>,
    pub session_rev: Option<i64>,
    pub device_id: String,
    pub subtotal_minor: i64,
    /// What settling the order takes; for an unpaid counter order (no session) this includes tax
    /// whatever the tax mode.
    pub total_minor: i64,
    pub group_bill_id: Option<String>,
}

pub fn insert_session(conn: &Connection, s: &NewSession<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO sessions (id, player_id, guest_no, device_id, group_id, mode, rate_id, state,
                               started_at, rounding_min, minimum_min, grace_min, time_up_mode,
                               epoch, business_day, fy, rev, created_at, updated_at, pack_id,
                               ends_at, reserved_minor)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'active', ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                 0, 0, 0, ?16, ?17, ?18)",
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
            s.fy,
            s.pack_id,
            s.ends_at,
            s.reserved_minor
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
    rule_id: Option<&str>,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO session_segments (id, session_id, from_at, device_id, rate_minor_per_hour,
                                       rule_id, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, 0)",
        params![
            id,
            session_id,
            from_at,
            device_id,
            rate_minor_per_hour,
            rule_id
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn device_has_live_session(conn: &Connection, device_id: &str) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sessions
                        WHERE device_id = ?1 AND (state IN ('active', 'paused', 'grace')
                               OR (state = 'expired' AND ended_at IS NULL)))",
        [device_id],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// The live session on `device_id` (same states as [`device_has_live_session`]): its id and its
/// player (`None` for a guest), if any. If several (which the engine does not allow), the
/// earliest started.
pub fn live_session_on_device(
    conn: &Connection,
    device_id: &str,
) -> Result<Option<(String, Option<String>)>, AppError> {
    conn.query_row(
        "SELECT id, player_id FROM sessions
         WHERE device_id = ?1 AND (state IN ('active', 'paused', 'grace')
                                   OR (state = 'expired' AND ended_at IS NULL))
         ORDER BY started_at, id LIMIT 1",
        [device_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(db_error)
}

/// The device a player has a live session on (`active`, `paused`, `grace`, or `expired` and not
/// yet ended), if any. If several (which the engine does not allow), the earliest started.
pub fn live_session_device_of_player(
    conn: &Connection,
    player_id: &str,
) -> Result<Option<String>, AppError> {
    conn.query_row(
        "SELECT device_id FROM sessions
         WHERE player_id = ?1 AND (state IN ('active', 'paused', 'grace')
                                   OR (state = 'expired' AND ended_at IS NULL))
         ORDER BY started_at, id LIMIT 1",
        [player_id],
        |r| r.get(0),
    )
    .optional()
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

pub(crate) const SESSION_COLUMNS: &str = "s.id, s.device_id, s.state, s.started_at, s.player_id, p.username,
        s.guest_no, s.time_up_mode, s.mode, s.rev, s.rounding_min, s.minimum_min,
        s.paused_total_ms, s.business_day, s.fy, s.ended_at, s.ended_by, s.ends_at, s.paused_at,
        s.pause_reason, s.pause_note, s.pack_id, s.reserved_minor, s.grace_min, s.extended_min, s.rate_id, s.group_id,
        s.group_bill_id";

pub(crate) fn live_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<LiveSession> {
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
        ended_at: r.get(15)?,
        ended_by: r.get(16)?,
        ends_at: r.get(17)?,
        paused_at: r.get(18)?,
        pause_reason: r.get(19)?,
        pause_note: r.get(20)?,
        pack_id: r.get(21)?,
        reserved_minor: r.get(22)?,
        grace_min: r.get(23)?,
        extended_min: r.get(24)?,
        rate_id: r.get(25)?,
        group_id: r.get(26)?,
        group_bill_id: r.get(27)?,
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
                OR (s.state = 'expired' AND s.ended_at IS NULL)
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
            "SELECT g.id, g.session_id, g.from_at, g.to_at, g.device_id, g.rate_minor_per_hour,
                    g.paused_ms, g.rule_id
             FROM session_segments g JOIN sessions s ON s.id = g.session_id
             WHERE (?1 IS NOT NULL AND g.session_id = ?1)
                OR (?1 IS NULL AND (s.state IN ('active', 'paused', 'grace')
                                   OR (s.state = 'expired' AND s.ended_at IS NULL)))
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
            paused_ms: r.get(6)?,
            rule_id: r.get(7)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

pub fn pause_session(
    conn: &Connection,
    id: &str,
    paused_at: i64,
    reason: &str,
    note: Option<&str>,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE sessions SET state = 'paused', paused_at = ?2, pause_reason = ?3, pause_note = ?4
         WHERE id = ?1",
        params![id, paused_at, reason, note],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Folds a finished pause into a session: the whole pause moves `ends_at` and the session's paused
/// total, and the open segment takes only the part of it after that segment began.
pub fn fold_pause(conn: &Connection, id: &str, paused_ms: i64) -> Result<(), AppError> {
    conn.execute(
        "UPDATE session_segments
         SET paused_ms = paused_ms + ?2 - MAX(0, from_at - COALESCE(
                 (SELECT paused_at FROM sessions WHERE id = ?1), from_at))
         WHERE session_id = ?1 AND to_at IS NULL",
        params![id, paused_ms],
    )
    .map_err(db_error)?;
    conn.execute(
        "UPDATE sessions SET paused_total_ms = paused_total_ms + ?2,
                             ends_at = CASE WHEN ends_at IS NULL THEN NULL ELSE ends_at + ?2 END,
                             paused_at = NULL, pause_reason = NULL, pause_note = NULL
         WHERE id = ?1",
        params![id, paused_ms],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn resume_session(conn: &Connection, id: &str, paused_ms: i64) -> Result<(), AppError> {
    fold_pause(conn, id, paused_ms)?;
    conn.execute("UPDATE sessions SET state = 'active' WHERE id = ?1", [id])
        .map_err(db_error)?;
    Ok(())
}

pub fn end_session(
    conn: &Connection,
    id: &str,
    ended_at: i64,
    ended_by: &str,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE sessions SET state = 'ended', ended_at = ?2, end_reason = 'staff', ended_by = ?3
         WHERE id = ?1",
        params![id, ended_at, ended_by],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn reopen_session(conn: &Connection, id: &str) -> Result<(), AppError> {
    conn.execute(
        "UPDATE sessions SET state = 'active', ended_at = NULL, end_reason = NULL, ended_by = NULL,
                forced_reason = NULL, forced_note = NULL, forced_left_min = NULL,
                forced_pack_min = NULL, controller_check = NULL, controller_checked_at = NULL,
                controller_checked_by = NULL
         WHERE id = ?1",
        [id],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Records whether a console's controllers came back after a session (`returned` or `missing`),
/// with who checked and when.
pub fn set_controller_check(
    conn: &Connection,
    id: &str,
    result: &str,
    at: i64,
    by: &str,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE sessions SET controller_check = ?2, controller_checked_at = ?3,
                controller_checked_by = ?4
         WHERE id = ?1",
        params![id, result, at, by],
    )
    .map_err(db_error)?;
    Ok(())
}

/// The newest ended session on this console whose controllers have not been checked.
pub fn unchecked_console_session(
    conn: &Connection,
    device_id: &str,
) -> Result<Option<String>, AppError> {
    conn.query_row(
        "SELECT s.id FROM sessions s JOIN devices_meta m ON m.device_id = s.device_id
         WHERE s.device_id = ?1 AND m.kind = 'console' AND s.ended_at IS NOT NULL
           AND s.controller_check IS NULL
         ORDER BY s.ended_at DESC, s.id DESC LIMIT 1",
        [device_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(db_error)
}

/// Every console with an ended session awaiting its controller check, as (device, session).
pub fn list_unchecked_consoles(conn: &Connection) -> Result<Vec<(String, String)>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT s.device_id, s.id FROM sessions s
             JOIN devices_meta m ON m.device_id = s.device_id
             WHERE m.kind = 'console' AND s.ended_at IS NOT NULL AND s.controller_check IS NULL
             ORDER BY s.ended_at, s.id",
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    Ok(rows)
}

/// What a forced end records on the session it just ended.
pub struct ForcedEnd<'a> {
    pub reason: &'a str,
    pub note: Option<&'a str>,
    pub left_min: i64,
    pub pack_min: i64,
}

/// Marks an ended session as force-ended: `end_reason` becomes `forced`, with the reason, note,
/// the whole minutes it had left and the pack minutes its end deducted.
pub fn mark_forced(conn: &Connection, id: &str, f: &ForcedEnd<'_>) -> Result<(), AppError> {
    conn.execute(
        "UPDATE sessions SET end_reason = 'forced', forced_reason = ?2, forced_note = ?3,
                forced_left_min = ?4, forced_pack_min = ?5
         WHERE id = ?1",
        params![id, f.reason, f.note, f.left_min, f.pack_min],
    )
    .map_err(db_error)?;
    Ok(())
}

/// The pack minutes a forced end deducted (0 for none); `None` when the session was not
/// force-ended.
pub fn forced_pack_minutes(conn: &Connection, id: &str) -> Result<Option<i64>, AppError> {
    conn.query_row(
        "SELECT COALESCE(forced_pack_min, 0) FROM sessions WHERE id = ?1 AND end_reason = 'forced'",
        [id],
        |r| r.get(0),
    )
    .optional()
    .map_err(db_error)
}

pub fn reopen_segments(conn: &Connection, session_id: &str, ended_at: i64) -> Result<(), AppError> {
    conn.execute(
        "UPDATE session_segments SET to_at = NULL WHERE session_id = ?1 AND to_at = ?2",
        params![session_id, ended_at],
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
            "SELECT o.id, o.session_id, COALESCE(o.device_id, ''), o.subtotal_minor, o.rev, o.player_id,
                    w.balance_minor, s.rev,
                    c.player_id, c.limit_minor, c.outstanding_minor, c.reminder_days, c.credit_paused,
                    c.last_override_day, c.rev, o.group_bill_id, o.total_minor
             FROM orders o LEFT JOIN wallets w ON w.player_id = o.player_id
                  LEFT JOIN credit_balances c ON c.player_id = o.player_id
                  LEFT JOIN sessions s ON s.id = o.session_id
             WHERE o.status = 'open' ORDER BY o.created_at, o.id",
        )
        .map_err(db_error)?;
    stmt.query_map([], |r| {
        Ok(OpenOrder {
            id: r.get(0)?,
            session_id: r.get(1)?,
            device_id: r.get(2)?,
            subtotal_minor: r.get(3)?,
            rev: r.get(4)?,
            player_id: r.get(5)?,
            wallet_minor: r.get(6)?,
            session_rev: r.get(7)?,
            credit: match r.get::<_, Option<String>>(8)? {
                Some(_) => Some(crate::credit::credit_from(r, 8)?),
                None => None,
            },
            group_bill_id: r.get(15)?,
            total_minor: r.get(16)?,
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
                    l.rate_minor_per_hour, l.amount_minor, g.rule_id,
                    CASE WHEN l.product_id IS NULL THEN NULL ELSE COALESCE(l.label, '') END, l.qty,
                    l.id, l.served
             FROM order_lines l JOIN orders o ON o.id = l.order_id
                  LEFT JOIN session_segments g ON g.id = l.session_segment_id
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
            rule_id: r.get(7)?,
            label: r.get(8)?,
            qty: r.get(9)?,
            id: r.get(10)?,
            served: r.get::<_, i64>(11)? != 0,
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
    pub session_id: Option<String>,
    pub settled_at: Option<i64>,
    pub settled_by: Option<String>,
    pub player_id: Option<String>,
    pub kind: String,
    pub group_bill_id: Option<String>,
    /// The PC an order is tied to (a counter order from a kiosk, Story 6.32).
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settlement {
    pub order_id: String,
    pub device_id: Option<String>,
    pub settled_at: i64,
    pub device_busy: bool,
    pub rev: i64,
    pub total_minor: i64,
    pub irreversible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ending {
    pub session_id: String,
    pub order_id: String,
    pub ended_at: i64,
    pub session_rev: i64,
    pub total_minor: i64,
    pub irreversible: bool,
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
        &format!("SELECT {ORDER_COLUMNS} FROM orders WHERE id = ?1"),
        [id],
        order_from,
    )
    .optional()
    .map_err(db_error)
}

pub(crate) const ORDER_COLUMNS: &str = "id, status, total_minor, rev, session_id, settled_at, settled_by, player_id, kind, group_bill_id, device_id";

pub(crate) fn order_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<OrderRow> {
    Ok(OrderRow {
        id: r.get(0)?,
        status: r.get(1)?,
        total_minor: r.get(2)?,
        rev: r.get(3)?,
        session_id: r.get(4)?,
        settled_at: r.get(5)?,
        settled_by: r.get(6)?,
        player_id: r.get(7)?,
        kind: r.get(8)?,
        group_bill_id: r.get(9)?,
        device_id: r.get(10)?,
    })
}

pub fn find_order_for_session(
    conn: &Connection,
    session_id: &str,
) -> Result<Option<OrderRow>, AppError> {
    conn.query_row(
        &format!("SELECT {ORDER_COLUMNS} FROM orders WHERE session_id = ?1"),
        [session_id],
        order_from,
    )
    .optional()
    .map_err(db_error)
}

pub fn list_order_line_ids(conn: &Connection, order_id: &str) -> Result<Vec<String>, AppError> {
    ids(
        conn,
        "SELECT id FROM order_lines WHERE order_id = ?1 ORDER BY position, id",
        order_id,
    )
}

/// The ids of an order's session (time) lines, leaving its product lines out.
pub fn list_segment_line_ids(conn: &Connection, order_id: &str) -> Result<Vec<String>, AppError> {
    ids(
        conn,
        "SELECT id FROM order_lines WHERE order_id = ?1 AND product_id IS NULL
         ORDER BY position, id",
        order_id,
    )
}

/// The total of an order's session (time) lines, leaving its product lines out.
pub fn sum_segment_amounts(conn: &Connection, order_id: &str) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COALESCE(SUM(amount_minor), 0) FROM order_lines
         WHERE order_id = ?1 AND product_id IS NULL",
        [order_id],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// A product line added by a placement, as its Undo needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedLine {
    pub id: String,
    pub amount_minor: i64,
    pub placed_by: Option<String>,
    pub placed_at: Option<i64>,
}

/// The lines one placement added to an order.
pub fn list_placement_lines(
    conn: &Connection,
    order_id: &str,
    placement_id: &str,
) -> Result<Vec<PlacedLine>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, amount_minor, placed_by, placed_at FROM order_lines
             WHERE order_id = ?1 AND placement_id = ?2 ORDER BY position, id",
        )
        .map_err(db_error)?;
    stmt.query_map(params![order_id, placement_id], |r| {
        Ok(PlacedLine {
            id: r.get(0)?,
            amount_minor: r.get(1)?,
            placed_by: r.get(2)?,
            placed_at: r.get(3)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// The session and order a placement put products on.
pub fn find_placed_order(
    conn: &Connection,
    placement_id: &str,
) -> Result<Option<(String, String)>, AppError> {
    conn.query_row(
        "SELECT o.session_id, o.id FROM order_lines l JOIN orders o ON o.id = l.order_id
         WHERE l.placement_id = ?1 AND o.session_id IS NOT NULL LIMIT 1",
        [placement_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(db_error)
}

/// How many product lines an order holds (the next placement's lines follow them).
pub fn count_product_lines(conn: &Connection, order_id: &str) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COUNT(*) FROM order_lines WHERE order_id = ?1 AND product_id IS NOT NULL",
        [order_id],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// The position the next product line of an order takes: after every line already placed, so
/// an Undone placement in the middle leaves no tie.
pub fn next_product_position(conn: &Connection, order_id: &str) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COALESCE(MAX(position) + 1, ?2) FROM order_lines
         WHERE order_id = ?1 AND product_id IS NOT NULL AND position >= ?2",
        params![order_id, PRODUCT_LINE_BASE],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// Raises (or, negative, lowers) an open bill's subtotal and total by `delta_minor`.
pub fn add_order_amount(
    conn: &Connection,
    order_id: &str,
    delta_minor: i64,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE orders SET subtotal_minor = subtotal_minor + ?2, total_minor = total_minor + ?2
         WHERE id = ?1",
        params![order_id, delta_minor],
    )
    .map_err(db_error)?;
    Ok(())
}

/// What a void records on the order (Story 5.23).
pub struct OrderVoid<'a> {
    pub at: i64,
    /// The approving staff member (the elevating manager, else the actor).
    pub by: &'a str,
    pub reason: &'a str,
    pub note: Option<&'a str>,
}

/// Marks an open session order void. The order is detached from its session (the session may
/// open a fresh bill) and keeps the session id in `void_session_id`; its lines stay.
pub fn void_order(conn: &Connection, order_id: &str, v: &OrderVoid<'_>) -> Result<(), AppError> {
    let changed = conn
        .execute(
            "UPDATE orders SET status = 'void', void_session_id = session_id, session_id = NULL,
                    voided_at = ?2, voided_by = ?3, void_reason = ?4, void_note = ?5
             WHERE id = ?1 AND status = 'open'",
            params![order_id, v.at, v.by, v.reason, v.note],
        )
        .map_err(db_error)?;
    if changed != 1 {
        return Err(AppError::new(
            ErrorCode::Stale,
            "the order is no longer open",
        ));
    }
    Ok(())
}

/// A product line as serve and cancel need it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineRow {
    pub id: String,
    pub order_id: String,
    pub product_id: Option<String>,
    pub label: String,
    pub qty: i64,
    pub amount_minor: i64,
    pub served: bool,
    pub placement_id: Option<String>,
}

pub fn find_line(conn: &Connection, line_id: &str) -> Result<Option<LineRow>, AppError> {
    conn.query_row(
        "SELECT id, order_id, product_id, COALESCE(label, ''), COALESCE(qty, 1), amount_minor,
                served, placement_id
         FROM order_lines WHERE id = ?1",
        [line_id],
        |r| {
            Ok(LineRow {
                id: r.get(0)?,
                order_id: r.get(1)?,
                product_id: r.get(2)?,
                label: r.get(3)?,
                qty: r.get(4)?,
                amount_minor: r.get(5)?,
                served: r.get::<_, i64>(6)? != 0,
                placement_id: r.get(7)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

/// Marks a line served by `by` at `at`.
pub fn mark_line_served(
    conn: &Connection,
    line_id: &str,
    at: i64,
    by: &str,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE order_lines SET served = 1, served_at = ?2, served_by = ?3 WHERE id = ?1",
        params![line_id, at, by],
    )
    .map_err(db_error)?;
    Ok(())
}

/// How many lines of an order are still to be served.
pub fn count_unserved_lines(conn: &Connection, order_id: &str) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COUNT(*) FROM order_lines WHERE order_id = ?1 AND served = 0",
        [order_id],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// The items placed on each session's bill order (the sum of its product line quantities), for
/// the sessions that have any.
pub fn list_ordered_counts(
    conn: &Connection,
) -> Result<std::collections::BTreeMap<String, i64>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT o.session_id, SUM(l.qty) FROM order_lines l JOIN orders o ON o.id = l.order_id
             WHERE o.session_id IS NOT NULL AND l.product_id IS NOT NULL
             GROUP BY o.session_id",
        )
        .map_err(db_error)?;
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)
}

pub fn list_payment_ids(conn: &Connection, order_id: &str) -> Result<Vec<String>, AppError> {
    ids(
        conn,
        "SELECT id FROM payments WHERE order_id = ?1 ORDER BY position, id",
        order_id,
    )
}

fn ids(conn: &Connection, sql: &str, key: &str) -> Result<Vec<String>, AppError> {
    let mut stmt = conn.prepare(sql).map_err(db_error)?;
    stmt.query_map([key], |r| r.get(0))
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)
}

pub fn reopen_order(conn: &Connection, id: &str) -> Result<(), AppError> {
    conn.execute(
        "UPDATE orders SET status = 'open', settled_at = NULL, settled_by = NULL WHERE id = ?1",
        [id],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn insert_effect(
    conn: &Connection,
    id: &str,
    kind: arena_proto::EffectKind,
    intent_id: Option<&str>,
    order_id: Option<&str>,
    recorded_at: i64,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO irreversible_effects (id, kind, intent_id, order_id, recorded_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, kind.as_str(), intent_id, order_id, recorded_at],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn forward_intent_id(
    conn: &Connection,
    tbl: &str,
    row_id: &str,
    intent_kind: &str,
) -> Result<Option<String>, AppError> {
    conn.query_row(
        "SELECT intent_id FROM changes WHERE tbl = ?1 AND row_id = ?2 AND intent_kind = ?3
         ORDER BY at DESC, seq DESC LIMIT 1",
        params![tbl, row_id, intent_kind],
        |r| r.get(0),
    )
    .optional()
    .map_err(db_error)
}

pub fn effect_recorded(
    conn: &Connection,
    intent_id: Option<&str>,
    order_id: &str,
) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM irreversible_effects
                        WHERE order_id = ?2 OR (?1 IS NOT NULL AND intent_id = ?1))",
        params![intent_id, order_id],
        |r| r.get(0),
    )
    .map_err(db_error)
}

pub fn find_ending(conn: &Connection, session_id: &str) -> Result<Option<Ending>, AppError> {
    type Found = (String, String, i64, i64, i64, String, Option<String>);
    let found: Option<Found> = conn
        .query_row(
            "SELECT s.id, o.id, s.ended_at, s.rev, o.total_minor, s.mode, s.end_reason
             FROM sessions s JOIN orders o ON o.session_id = s.id
             WHERE s.id = ?1 AND s.state = 'ended' AND s.ended_at IS NOT NULL",
            [session_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .optional()
        .map_err(db_error)?;
    let Some((session_id, order_id, ended_at, session_rev, total_minor, mode, end_reason)) = found
    else {
        return Ok(None);
    };
    let forced = end_reason.as_deref() == Some("forced");
    let kind = if forced {
        "session.force_end"
    } else {
        "session.end"
    };
    let forward = forward_intent_id(conn, "sessions", &session_id, kind)?;
    let irreversible =
        (mode != "postpaid" && !forced) || effect_recorded(conn, forward.as_deref(), &order_id)?;
    Ok(Some(Ending {
        session_id,
        order_id,
        ended_at,
        session_rev,
        total_minor,
        irreversible,
    }))
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
    let found = conn
        .query_row(
            "SELECT o.id, o.device_id, o.settled_at,
                    EXISTS (SELECT 1 FROM sessions s
                            WHERE s.device_id = o.device_id
                              AND (s.state IN ('active', 'paused', 'grace')
                                   OR (s.state = 'expired' AND s.ended_at IS NULL))),
                    o.rev, o.total_minor, o.kind
             FROM orders o WHERE o.id = ?1 AND o.status = 'settled' AND o.settled_at IS NOT NULL",
            [order_id],
            |r| {
                Ok((
                    Settlement {
                        order_id: r.get(0)?,
                        device_id: r.get(1)?,
                        settled_at: r.get(2)?,
                        device_busy: r.get(3)?,
                        rev: r.get(4)?,
                        total_minor: r.get(5)?,
                        irreversible: false,
                    },
                    r.get::<_, String>(6)?,
                ))
            },
        )
        .optional()
        .map_err(db_error)?;
    let Some((mut settlement, kind)) = found else {
        return Ok(None);
    };
    let intent_kind = match kind.as_str() {
        "topup" => "wallet.topup",
        "credit_collect" => "credit.collect",
        "plan_sale" => "plan.sell",
        "pos_sale" => "pos.sale",
        _ => "session.settle",
    };
    let forward = forward_intent_id(conn, "orders", order_id, intent_kind)?;
    settlement.irreversible = effect_recorded(conn, forward.as_deref(), order_id)?;
    Ok(Some(settlement))
}

pub fn assign_session_player(
    conn: &Connection,
    session_id: &str,
    player_id: &str,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE sessions SET player_id = ?2 WHERE id = ?1",
        params![session_id, player_id],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn assign_order_player(
    conn: &Connection,
    order_id: &str,
    player_id: &str,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE orders SET player_id = ?2 WHERE id = ?1",
        params![order_id, player_id],
    )
    .map_err(db_error)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionListing {
    pub id: String,
    pub device_id: String,
    pub device: String,
    pub username: Option<String>,
    pub guest_no: Option<i64>,
    pub mode: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub state: String,
    pub order_id: Option<String>,
    pub order_status: Option<String>,
    pub total_minor: Option<i64>,
    pub controller_check: Option<String>,
    /// The invoice number of the settled bill, when it has one (Story 5.17: Reprint).
    pub invoice_no: Option<String>,
}

pub fn list_sessions_for_day(
    conn: &Connection,
    business_day: &str,
) -> Result<Vec<SessionListing>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.device_id, COALESCE(m.label, s.device_id), p.username, s.guest_no,
                    s.mode, s.started_at, s.ended_at, s.state, o.id, o.status, o.total_minor,
                    s.controller_check, o.invoice_no
             FROM sessions s
             LEFT JOIN players p ON p.player_id = s.player_id
             LEFT JOIN devices_meta m ON m.device_id = s.device_id
             LEFT JOIN orders o ON o.session_id = s.id
             WHERE s.business_day = ?1
             ORDER BY s.started_at DESC, s.id DESC",
        )
        .map_err(db_error)?;
    stmt.query_map([business_day], |r| {
        Ok(SessionListing {
            id: r.get(0)?,
            device_id: r.get(1)?,
            device: r.get(2)?,
            username: r.get(3)?,
            guest_no: r.get(4)?,
            mode: r.get(5)?,
            started_at: r.get(6)?,
            ended_at: r.get(7)?,
            state: r.get(8)?,
            order_id: r.get(9)?,
            order_status: r.get(10)?,
            total_minor: r.get(11)?,
            controller_check: r.get(12)?,
            invoice_no: r.get(13)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

pub fn expire_session(conn: &Connection, id: &str, ended_at: i64) -> Result<(), AppError> {
    conn.execute(
        "UPDATE sessions SET state = 'expired', ended_at = ?2, end_reason = 'expired',
                             ended_by = 'hub'
         WHERE id = ?1",
        params![id, ended_at],
    )
    .map_err(db_error)?;
    Ok(())
}

/// The sum a player's live wallet sessions have set aside from the wallet balance.
pub fn sum_reserved_minor(conn: &Connection, player_id: &str) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COALESCE(SUM(reserved_minor), 0) FROM sessions
         WHERE player_id = ?1 AND mode = 'wallet'
           AND (state IN ('active', 'paused', 'grace') OR (state = 'expired' AND ended_at IS NULL))",
        [player_id],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// Moves the end of a running session out by `minutes` to `ends_at` and brings a session in
/// its grace period back to `active`.
pub fn extend_session(
    conn: &Connection,
    id: &str,
    ends_at: i64,
    minutes: i64,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE sessions SET ends_at = ?2, extended_min = extended_min + ?3,
                             state = CASE WHEN state = 'grace' THEN 'active' ELSE state END
         WHERE id = ?1",
        params![id, ends_at, minutes],
    )
    .map_err(db_error)?;
    Ok(())
}

/// The order and its one line for an extend, settled as it is written.
pub struct NewExtensionOrder<'a> {
    pub order_id: &'a str,
    pub line_id: &'a str,
    pub session_id: &'a str,
    pub segment_id: &'a str,
    pub player_id: Option<&'a str>,
    pub device_id: &'a str,
    pub from_at: i64,
    pub to_at: i64,
    pub minutes: i64,
    pub rate_minor_per_hour: i64,
    pub amount_minor: i64,
    pub settled_at: i64,
    pub settled_by: &'a str,
    pub business_day: &'a str,
    pub fy: &'a str,
}

/// Writes the settled order that charges an extend, tied to its session by
/// `orders.extends_session_id` (a session's own bill stays its one `orders.session_id` row).
pub fn insert_extension_order(
    conn: &Connection,
    o: &NewExtensionOrder<'_>,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO orders (id, session_id, player_id, guest_no, device_id, status, subtotal_minor,
                             total_minor, business_day, fy, kind, settled_at, settled_by,
                             extends_session_id, rev, created_at, updated_at)
         VALUES (?1, NULL, ?2, NULL, ?3, 'settled', ?4, ?4, ?5, ?6, 'session', ?7, ?8, ?9, 0, 0, 0)",
        params![
            o.order_id,
            o.player_id,
            o.device_id,
            o.amount_minor,
            o.business_day,
            o.fy,
            o.settled_at,
            o.settled_by,
            o.session_id
        ],
    )
    .map_err(db_error)?;
    insert_order_line(
        conn,
        &NewOrderLine {
            id: o.line_id,
            order_id: o.order_id,
            segment_id: o.segment_id,
            device_id: o.device_id,
            from_at: o.from_at,
            to_at: o.to_at,
            minutes: o.minutes,
            rate_minor_per_hour: o.rate_minor_per_hour,
            amount_minor: o.amount_minor,
            position: 0,
        },
    )
}

/// Moves a running session into its grace period after `ends_at`.
pub fn grace_session(conn: &Connection, id: &str) -> Result<(), AppError> {
    conn.execute("UPDATE sessions SET state = 'grace' WHERE id = ?1", [id])
        .map_err(db_error)?;
    Ok(())
}

/// Holds a session as `expired` until staff end it: no end time is recorded yet, and the device
/// stays in its session.
pub fn hold_session_expired(conn: &Connection, id: &str) -> Result<(), AppError> {
    conn.execute(
        "UPDATE sessions SET state = 'expired', end_reason = 'expired' WHERE id = ?1",
        [id],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Adds `paused_ms` to the paused time of a session's open segment, so a pause that spans a rate
/// change is split between the segment it closes and the one that opens.
pub fn add_segment_pause(conn: &Connection, id: &str, paused_ms: i64) -> Result<(), AppError> {
    conn.execute(
        "UPDATE session_segments SET paused_ms = paused_ms + ?2
         WHERE session_id = ?1 AND to_at IS NULL",
        params![id, paused_ms],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Sets a wallet session's end and the amount set aside from the wallet after a rate change.
pub fn reprice_wallet_session(
    conn: &Connection,
    id: &str,
    ends_at: i64,
    reserved_minor: i64,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE sessions SET ends_at = ?2, reserved_minor = ?3 WHERE id = ?1",
        params![id, ends_at, reserved_minor],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Moves a live session to another device and its group; `rate_id` is the new base rate of a
/// postpaid or wallet session and is left alone when `None`.
pub fn move_session(
    conn: &Connection,
    session_id: &str,
    device_id: &str,
    group_id: &str,
    rate_id: Option<&str>,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE sessions SET device_id = ?2, group_id = ?3, rate_id = COALESCE(?4, rate_id)
         WHERE id = ?1",
        params![session_id, device_id, group_id, rate_id],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Points a session's open bills at the device it moved to, returning their ids.
pub fn move_open_orders(
    conn: &Connection,
    session_id: &str,
    device_id: &str,
) -> Result<Vec<String>, AppError> {
    let mut stmt = conn
        .prepare("SELECT id FROM orders WHERE session_id = ?1 AND status = 'open' ORDER BY id")
        .map_err(db_error)?;
    let ids: Vec<String> = stmt
        .query_map([session_id], |r| r.get(0))
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)?;
    conn.execute(
        "UPDATE orders SET device_id = ?2 WHERE session_id = ?1 AND status = 'open'",
        params![session_id, device_id],
    )
    .map_err(db_error)?;
    Ok(ids)
}

/// A counter sale's order, written settled with its tax breakup.
pub struct NewPosOrder<'a> {
    pub id: &'a str,
    pub player_id: Option<&'a str>,
    /// The PC the order is tied to (a kiosk order, Story 6.32).
    pub device_id: Option<&'a str>,
    /// Written `open` and unpaid (an unpaid counter order) instead of settled.
    pub open: bool,
    pub subtotal_minor: i64,
    pub tax_minor: i64,
    pub total_minor: i64,
    /// The per-rate breakup as JSON.
    pub tax_breakup: &'a str,
    pub settled_at: i64,
    pub settled_by: &'a str,
    pub business_day: &'a str,
    pub fy: &'a str,
}

pub fn insert_pos_order(conn: &Connection, o: &NewPosOrder<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO orders (id, session_id, player_id, guest_no, device_id, status, subtotal_minor,
                             total_minor, business_day, fy, kind, settled_at, settled_by,
                             tax_minor, tax_breakup, rev, created_at, updated_at)
         VALUES (?1, NULL, ?2, NULL, ?11, ?12, ?3, ?4, ?5, ?6, 'pos_sale', ?7, ?8, ?9, ?10,
                 0, 0, 0)",
        params![
            o.id,
            o.player_id,
            o.subtotal_minor,
            o.total_minor,
            o.business_day,
            o.fy,
            (!o.open).then_some(o.settled_at),
            (!o.open).then_some(o.settled_by),
            o.tax_minor,
            o.tax_breakup,
            o.device_id,
            if o.open { "open" } else { "settled" },
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// What a PC owes the counter (Story 6.32): the totals of its open counter orders, which have
/// no session. A running session's own bill is not counted.
pub fn unpaid_counter_minor(conn: &Connection, device_id: &str) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COALESCE(SUM(total_minor), 0) FROM orders
         WHERE device_id = ?1 AND status = 'open' AND kind = 'pos_sale' AND session_id IS NULL",
        [device_id],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// The player of the latest session that ran on `device_id`, for a counter order placed after it
/// ended (Story 6.32).
pub fn last_session_player(conn: &Connection, device_id: &str) -> Result<Option<String>, AppError> {
    conn.query_row(
        "SELECT player_id FROM sessions WHERE device_id = ?1 ORDER BY started_at DESC, id DESC LIMIT 1",
        [device_id],
        |r| r.get(0),
    )
    .optional()
    .map(Option::flatten)
    .map_err(db_error)
}

/// One product line of a counter sale.
pub struct NewProductLine<'a> {
    pub id: &'a str,
    pub order_id: &'a str,
    pub product_id: &'a str,
    pub label: &'a str,
    pub qty: i64,
    pub unit_price_minor: i64,
    pub tax_rate_bp: i64,
    pub amount_minor: i64,
    pub position: i64,
    pub at: i64,
    /// The placement (`order.place` intent id) and the staff member that added the line; `None`
    /// for a counter sale's line.
    pub placement: Option<(&'a str, &'a str)>,
    /// Whether the line is served when written: a counter sale's always is, a placed kitchen line
    /// is not until staff mark it.
    pub served: bool,
}

/// Where the first product line of a session bill sits: after every session (time) line.
pub const PRODUCT_LINE_BASE: i64 = 1000;

pub fn insert_product_line(conn: &Connection, l: &NewProductLine<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO order_lines (id, order_id, session_segment_id, device_id, from_at, to_at,
                                  minutes, rate_minor_per_hour, amount_minor, position, rev,
                                  created_at, updated_at, label, product_id, qty,
                                  unit_price_minor, tax_rate_bp, placement_id, placed_by,
                                  placed_at, served, served_at, served_by)
         VALUES (?1, ?2, NULL, NULL, ?3, ?3, 0, 0, ?4, ?5, 0, 0, 0, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                 ?13, ?14, ?15, ?16)",
        params![
            l.id,
            l.order_id,
            l.at,
            l.amount_minor,
            l.position,
            l.label,
            l.product_id,
            l.qty,
            l.unit_price_minor,
            l.tax_rate_bp,
            l.placement.map(|p| p.0),
            l.placement.map(|p| p.1),
            l.placement.map(|_| l.at),
            i64::from(l.served),
            l.placement.filter(|_| l.served).map(|_| l.at),
            l.placement.filter(|_| l.served).map(|p| p.1)
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// The business day's settled counter sales, newest first, with their lines and payments.
pub fn list_pos_sales(
    conn: &Connection,
    business_day: &str,
) -> Result<Vec<arena_proto::PosSaleView>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT o.id, o.settled_at, COALESCE(st.display_name, o.settled_by), p.username,
                    o.subtotal_minor, o.tax_minor, o.total_minor, o.invoice_no
             FROM orders o
             LEFT JOIN staff st ON st.staff_id = o.settled_by
             LEFT JOIN players p ON p.player_id = o.player_id
             WHERE o.kind = 'pos_sale' AND o.status = 'settled' AND o.business_day = ?1
             ORDER BY o.settled_at DESC, o.id DESC",
        )
        .map_err(db_error)?;
    let heads = stmt
        .query_map([business_day], |r| {
            Ok(arena_proto::PosSaleView {
                order_id: r.get(0)?,
                settled_at: r.get::<_, Option<i64>>(1)?.unwrap_or_default(),
                staff: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                player: r.get(3)?,
                lines: Vec::new(),
                payments: Vec::new(),
                subtotal_minor: r.get(4)?,
                tax_minor: r.get(5)?,
                total_minor: r.get(6)?,
                invoice_no: r.get(7)?,
                print_state: None,
                whatsapp_state: None,
            })
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    let mut lines = conn
        .prepare(
            "SELECT COALESCE(product_id, ''), COALESCE(label, ''), COALESCE(qty, 1),
                    COALESCE(unit_price_minor, amount_minor), COALESCE(tax_rate_bp, 0),
                    amount_minor
             FROM order_lines WHERE order_id = ?1 ORDER BY position, id",
        )
        .map_err(db_error)?;
    let mut payments = conn
        .prepare(
            "SELECT method, amount_minor FROM payments
             WHERE order_id = ?1 AND amount_minor > 0 ORDER BY position, id",
        )
        .map_err(db_error)?;
    heads
        .into_iter()
        .map(|mut sale| {
            sale.lines = lines
                .query_map([&sale.order_id], |r| {
                    Ok(arena_proto::PosSaleLineView {
                        product_id: r.get(0)?,
                        name: r.get(1)?,
                        qty: r.get(2)?,
                        unit_price_minor: r.get(3)?,
                        rate_bp: r.get(4)?,
                        amount_minor: r.get(5)?,
                    })
                })
                .map_err(db_error)?
                .collect::<Result<_, _>>()
                .map_err(db_error)?;
            sale.payments = payments
                .query_map([&sale.order_id], |r| {
                    Ok(arena_proto::PosSalePaymentView {
                        method: r.get(0)?,
                        amount_minor: r.get(1)?,
                    })
                })
                .map_err(db_error)?
                .collect::<Result<_, _>>()
                .map_err(db_error)?;
            sale.print_state = crate::outbox::latest_print_state(conn, &sale.order_id)?;
            sale.whatsapp_state = crate::notify::latest_receipt_state(conn, &sale.order_id)?;
            Ok(sale)
        })
        .collect()
}

/// The kind of the intent that wrote changes under `intent_id`.
pub fn forward_kind(conn: &Connection, intent_id: &str) -> Result<Option<String>, AppError> {
    conn.query_row(
        "SELECT intent_kind FROM changes WHERE intent_id = ?1 LIMIT 1",
        [intent_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(db_error)
}
