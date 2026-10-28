//! Matching scans to the catalogue and approving them into the library (Story 6.5): the
//! `game_matches`, `library_installs` and `library_dismissals` change-log tables. Only `apply()`
//! writes them, through these helpers; the matches are JSON text here (the payload types live in
//! `arena-proto`).

use arena_proto::{AppError, ErrorCode};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

/// One `game_matches` row: the catalogue matches found in a device's latest scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameMatchesRow {
    pub device_id: String,
    pub scanned_at: i64,
    /// A JSON array of `{catalog_id, path}`.
    pub matches: String,
    pub rev: i64,
}

fn matches_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<GameMatchesRow> {
    Ok(GameMatchesRow {
        device_id: r.get(0)?,
        scanned_at: r.get(1)?,
        matches: r.get(2)?,
        rev: r.get(3)?,
    })
}

/// The device's stored matches, if it has any.
pub fn find_game_matches(
    conn: &Connection,
    device_id: &str,
) -> Result<Option<GameMatchesRow>, AppError> {
    conn.query_row(
        "SELECT device_id, scanned_at, matches, rev FROM game_matches WHERE device_id = ?1",
        [device_id],
        matches_from,
    )
    .optional()
    .map_err(db_error)
}

/// Every device's stored matches, ordered by device id.
pub fn list_game_matches(conn: &Connection) -> Result<Vec<GameMatchesRow>, AppError> {
    let mut stmt = conn
        .prepare("SELECT device_id, scanned_at, matches, rev FROM game_matches ORDER BY device_id")
        .map_err(db_error)?;
    let rows = stmt.query_map([], matches_from).map_err(db_error)?;
    rows.collect::<Result<_, _>>().map_err(db_error)
}

/// Adds the device's first matches (the stamp step sets `rev = 1`).
pub fn insert_game_matches(
    conn: &Connection,
    device_id: &str,
    scanned_at: i64,
    matches: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO game_matches (device_id, scanned_at, matches, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, 0, 0, 0)",
        params![device_id, scanned_at, matches],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Replaces the device's stored matches (the stamp step then bumps `rev`).
pub fn replace_game_matches(
    conn: &Connection,
    device_id: &str,
    scanned_at: i64,
    matches: &str,
) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE game_matches SET scanned_at = ?2, matches = ?3 WHERE device_id = ?1",
            params![device_id, scanned_at, matches],
        )
        .map_err(db_error)?;
    if n == 1 {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::Internal,
            format!("no game_matches row {device_id}"),
        ))
    }
}

/// One `library_installs` row: where a device keeps a library entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRow {
    pub entry_id: String,
    pub device_id: String,
    pub path: String,
    /// The key of the proposal it was found as (empty when recorded by hand).
    pub match_key: String,
    pub rev: i64,
}

fn install_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<InstallRow> {
    Ok(InstallRow {
        entry_id: r.get(0)?,
        device_id: r.get(1)?,
        path: r.get(2)?,
        match_key: r.get(3)?,
        rev: r.get(4)?,
    })
}

/// The install paths of live entries, ordered by entry then device (the rows of a deleted entry
/// are not listed).
pub fn list_library_installs(conn: &Connection) -> Result<Vec<InstallRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT i.entry_id, i.device_id, i.path, i.match_key, i.rev
             FROM library_installs i
             JOIN library_entries e ON e.id = i.entry_id AND e.deleted_at IS NULL
             ORDER BY i.entry_id, i.device_id",
        )
        .map_err(db_error)?;
    let rows = stmt.query_map([], install_from).map_err(db_error)?;
    rows.collect::<Result<_, _>>().map_err(db_error)
}

/// Adds an install (the stamp step sets `rev = 1`).
pub fn insert_library_install(
    conn: &Connection,
    entry_id: &str,
    device_id: &str,
    path: &str,
    match_key: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO library_installs (entry_id, device_id, path, match_key, rev, created_at,
                                       updated_at)
         VALUES (?1, ?2, ?3, ?4, 0, 0, 0)",
        params![entry_id, device_id, path, match_key],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Rewrites an install's path and key (the stamp step then bumps `rev`).
pub fn update_library_install(
    conn: &Connection,
    entry_id: &str,
    device_id: &str,
    path: &str,
    match_key: &str,
) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE library_installs SET path = ?3, match_key = ?4
             WHERE entry_id = ?1 AND device_id = ?2",
            params![entry_id, device_id, path, match_key],
        )
        .map_err(db_error)?;
    if n == 1 {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::Internal,
            format!("no library_installs row {entry_id}/{device_id}"),
        ))
    }
}

/// The dismissed proposal keys, ordered.
pub fn list_library_dismissals(conn: &Connection) -> Result<Vec<String>, AppError> {
    let mut stmt = conn
        .prepare("SELECT key FROM library_dismissals ORDER BY key")
        .map_err(db_error)?;
    let rows = stmt.query_map([], |r| r.get(0)).map_err(db_error)?;
    rows.collect::<Result<_, _>>().map_err(db_error)
}

/// Dismisses a proposal (the stamp step sets `rev = 1`). A key already dismissed is an error.
pub fn insert_library_dismissal(conn: &Connection, key: &str) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO library_dismissals (key, rev, created_at, updated_at) VALUES (?1, 0, 0, 0)",
        [key],
    )
    .map_err(db_error)?;
    Ok(())
}
