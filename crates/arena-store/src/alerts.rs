use arena_proto::AppError;
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertRow {
    pub id: String,
    pub kind: String,
    pub device_id: Option<String>,
    pub raised_at: i64,
    pub acked_at: Option<i64>,
    pub acked_by: Option<String>,
    pub rev: i64,
}

pub struct NewAlert<'a> {
    pub id: &'a str,
    pub kind: &'a str,
    pub device_id: Option<&'a str>,
    pub raised_at: i64,
}

const COLUMNS: &str = "id, kind, device_id, raised_at, acked_at, acked_by, rev";

fn from(r: &rusqlite::Row<'_>) -> rusqlite::Result<AlertRow> {
    Ok(AlertRow {
        id: r.get(0)?,
        kind: r.get(1)?,
        device_id: r.get(2)?,
        raised_at: r.get(3)?,
        acked_at: r.get(4)?,
        acked_by: r.get(5)?,
        rev: r.get(6)?,
    })
}

pub fn insert_alert(conn: &Connection, a: &NewAlert<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO alerts (id, kind, device_id, raised_at, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 0, 0, 0)",
        params![a.id, a.kind, a.device_id, a.raised_at],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn find_alert(conn: &Connection, id: &str) -> Result<Option<AlertRow>, AppError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM alerts WHERE id = ?1"),
        [id],
        from,
    )
    .optional()
    .map_err(db_error)
}

/// The open alert of `kind` for `device_id` (or for the branch when `None`), if any.
pub fn find_open_alert(
    conn: &Connection,
    kind: &str,
    device_id: Option<&str>,
) -> Result<Option<AlertRow>, AppError> {
    conn.query_row(
        &format!(
            "SELECT {COLUMNS} FROM alerts
             WHERE kind = ?1 AND device_id IS ?2 AND acked_at IS NULL"
        ),
        params![kind, device_id],
        from,
    )
    .optional()
    .map_err(db_error)
}

/// Every open alert, oldest first.
pub fn list_open_alerts(conn: &Connection) -> Result<Vec<AlertRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM alerts WHERE acked_at IS NULL ORDER BY raised_at, id"
        ))
        .map_err(db_error)?;
    let rows = stmt.query_map([], from).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

pub fn ack_alert(
    conn: &Connection,
    id: &str,
    acked_at: i64,
    acked_by: &str,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE alerts SET acked_at = ?2, acked_by = ?3 WHERE id = ?1 AND acked_at IS NULL",
        params![id, acked_at, acked_by],
    )
    .map_err(db_error)?;
    Ok(())
}

/// When the latest alert of `kind` for the branch (no device) was raised, open or not.
pub fn latest_branch_alert_at(conn: &Connection, kind: &str) -> Result<Option<i64>, AppError> {
    conn.query_row(
        "SELECT MAX(raised_at) FROM alerts WHERE kind = ?1 AND device_id IS NULL",
        [kind],
        |r| r.get(0),
    )
    .map_err(db_error)
}
