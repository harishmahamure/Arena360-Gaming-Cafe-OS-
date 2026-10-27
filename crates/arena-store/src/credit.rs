use arena_proto::AppError;
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreditRow {
    pub player_id: String,
    pub limit_minor: i64,
    pub outstanding_minor: i64,
    pub reminder_days: Vec<u16>,
    pub paused: bool,
    pub last_override_day: Option<String>,
    pub rev: i64,
}

pub fn encode_days(days: &[u16]) -> String {
    days.iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

pub fn decode_days(text: &str) -> Vec<u16> {
    text.split(',')
        .filter_map(|d| d.trim().parse().ok())
        .collect()
}

pub(crate) const CREDIT_COLUMNS: &str = "player_id, limit_minor, outstanding_minor, reminder_days, credit_paused, last_override_day, rev";

pub(crate) fn credit_from(r: &rusqlite::Row<'_>, at: usize) -> rusqlite::Result<CreditRow> {
    let days: String = r.get(at + 3)?;
    Ok(CreditRow {
        player_id: r.get(at)?,
        limit_minor: r.get(at + 1)?,
        outstanding_minor: r.get(at + 2)?,
        reminder_days: decode_days(&days),
        paused: r.get::<_, i64>(at + 4)? != 0,
        last_override_day: r.get(at + 5)?,
        rev: r.get(at + 6)?,
    })
}

pub fn find_credit(conn: &Connection, player_id: &str) -> Result<Option<CreditRow>, AppError> {
    conn.query_row(
        &format!("SELECT {CREDIT_COLUMNS} FROM credit_balances WHERE player_id = ?1"),
        [player_id],
        |r| credit_from(r, 0),
    )
    .optional()
    .map_err(db_error)
}

pub fn insert_credit(
    conn: &Connection,
    player_id: &str,
    limit_minor: i64,
    reminder_days: &[u16],
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO credit_balances (player_id, limit_minor, outstanding_minor, reminder_days,
                                      credit_paused, last_override_day, rev, created_at, updated_at)
         VALUES (?1, ?2, 0, ?3, 0, NULL, 0, 0, 0)",
        params![player_id, limit_minor, encode_days(reminder_days)],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn update_credit_limit(
    conn: &Connection,
    player_id: &str,
    limit_minor: i64,
    reminder_days: &[u16],
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE credit_balances SET limit_minor = ?2, reminder_days = ?3 WHERE player_id = ?1",
        params![player_id, limit_minor, encode_days(reminder_days)],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn adjust_outstanding(
    conn: &Connection,
    player_id: &str,
    delta_minor: i64,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE credit_balances SET outstanding_minor = outstanding_minor + ?2
         WHERE player_id = ?1",
        params![player_id, delta_minor],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn set_override_day(
    conn: &Connection,
    player_id: &str,
    day: Option<&str>,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE credit_balances SET last_override_day = ?2 WHERE player_id = ?1",
        params![player_id, day],
    )
    .map_err(db_error)?;
    Ok(())
}
