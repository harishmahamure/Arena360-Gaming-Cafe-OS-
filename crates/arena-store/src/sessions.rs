use arena_proto::AppError;
use rusqlite::{Connection, params};

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

pub fn list_live_sessions(conn: &Connection) -> Result<Vec<LiveSession>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.device_id, s.state, s.started_at, s.player_id, p.username, s.guest_no,
                    s.time_up_mode
             FROM sessions s LEFT JOIN players p ON p.player_id = s.player_id
             WHERE s.state IN ('active', 'paused', 'grace')
             ORDER BY s.started_at, s.id",
        )
        .map_err(db_error)?;
    stmt.query_map([], |r| {
        Ok(LiveSession {
            id: r.get(0)?,
            device_id: r.get(1)?,
            state: r.get(2)?,
            started_at: r.get(3)?,
            player_id: r.get(4)?,
            username: r.get(5)?,
            guest_no: r.get(6)?,
            time_up_mode: r.get(7)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}
