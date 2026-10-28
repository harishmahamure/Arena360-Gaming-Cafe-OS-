use arena_proto::AppError;
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRow {
    pub player_id: String,
    pub username: String,
    pub password_hash: String,
    pub phone_lookup: String,
    pub name_enc: String,
    pub phone_enc: String,
    pub email_enc: Option<String>,
    pub dob_enc: Option<String>,
    pub notes_enc: Option<String>,
    pub tags: String,
}

pub fn username_taken(conn: &Connection, username: &str) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM players WHERE username = ?1 AND deleted_at IS NULL)",
        [username],
        |r| r.get(0),
    )
    .map_err(db_error)
}

pub fn phone_holder(conn: &Connection, phone_lookup: &str) -> Result<Option<String>, AppError> {
    conn.query_row(
        "SELECT player_id FROM players WHERE phone_lookup = ?1 AND deleted_at IS NULL",
        [phone_lookup],
        |r| r.get(0),
    )
    .optional()
    .map_err(db_error)
}

pub fn insert_player(conn: &Connection, p: &PlayerRow) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO players (player_id, username, password_hash, phone_lookup, name_enc,
                              phone_enc, email_enc, dob_enc, notes_enc, tags, rev, created_at,
                              updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, 0, 0)",
        params![
            p.player_id,
            p.username,
            p.password_hash,
            p.phone_lookup,
            p.name_enc,
            p.phone_enc,
            p.email_enc,
            p.dob_enc,
            p.notes_enc,
            p.tags
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn insert_consent(
    conn: &Connection,
    consent_id: &str,
    player_id: &str,
    purpose: &str,
    policy_version: &str,
    granted: bool,
    at: i64,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO consents (consent_id, player_id, purpose, policy_version, granted, at, rev,
                               created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, 0)",
        params![consent_id, player_id, purpose, policy_version, granted, at],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn insert_player_key(
    conn: &Connection,
    player_id: &str,
    wrapped: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO player_keys (player_id, dek_wrapped) VALUES (?1, ?2)",
        params![player_id, wrapped],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn find_player_key(conn: &Connection, player_id: &str) -> Result<Option<String>, AppError> {
    conn.query_row(
        "SELECT dek_wrapped FROM player_keys WHERE player_id = ?1",
        [player_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(db_error)
}

pub fn update_player(conn: &Connection, p: &PlayerRow) -> Result<(), AppError> {
    conn.execute(
        "UPDATE players SET phone_lookup = ?2, name_enc = ?3, phone_enc = ?4, email_enc = ?5,
                            dob_enc = ?6, notes_enc = ?7, tags = ?8
         WHERE player_id = ?1",
        params![
            p.player_id,
            p.phone_lookup,
            p.name_enc,
            p.phone_enc,
            p.email_enc,
            p.dob_enc,
            p.notes_enc,
            p.tags
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn set_player_password(conn: &Connection, player_id: &str, hash: &str) -> Result<(), AppError> {
    conn.execute(
        "UPDATE players SET password_hash = ?2 WHERE player_id = ?1",
        params![player_id, hash],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn list_player_revs(conn: &Connection) -> Result<Vec<(String, i64)>, AppError> {
    let mut stmt = conn
        .prepare("SELECT player_id, rev FROM players WHERE deleted_at IS NULL")
        .map_err(db_error)?;
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)
}

pub fn find_player(
    conn: &Connection,
    player_id: &str,
) -> Result<Option<(PlayerRow, i64, String)>, AppError> {
    conn.query_row(
        "SELECT p.player_id, p.username, p.password_hash, p.phone_lookup, p.name_enc, p.phone_enc,
                p.email_enc, p.dob_enc, p.notes_enc, p.tags, p.rev, k.dek_wrapped
         FROM players p JOIN player_keys k ON k.player_id = p.player_id
         WHERE p.player_id = ?1 AND p.deleted_at IS NULL",
        [player_id],
        |r| {
            Ok((
                PlayerRow {
                    player_id: r.get(0)?,
                    username: r.get(1)?,
                    password_hash: r.get(2)?,
                    phone_lookup: r.get(3)?,
                    name_enc: r.get(4)?,
                    phone_enc: r.get(5)?,
                    email_enc: r.get(6)?,
                    dob_enc: r.get(7)?,
                    notes_enc: r.get(8)?,
                    tags: r.get(9)?,
                },
                r.get(10)?,
                r.get(11)?,
            ))
        },
    )
    .optional()
    .map_err(db_error)
}

/// The live player with this (already normalised) username and their wrapped key, for a kiosk
/// login (Story 6.8). A deleted player, or one with no key row, is not found.
pub fn find_player_by_username(
    conn: &Connection,
    username: &str,
) -> Result<Option<(PlayerRow, String)>, AppError> {
    conn.query_row(
        "SELECT p.player_id, p.username, p.password_hash, p.phone_lookup, p.name_enc, p.phone_enc,
                p.email_enc, p.dob_enc, p.notes_enc, p.tags, k.dek_wrapped
         FROM players p JOIN player_keys k ON k.player_id = p.player_id
         WHERE p.username = ?1 AND p.deleted_at IS NULL",
        [username],
        |r| {
            Ok((
                PlayerRow {
                    player_id: r.get(0)?,
                    username: r.get(1)?,
                    password_hash: r.get(2)?,
                    phone_lookup: r.get(3)?,
                    name_enc: r.get(4)?,
                    phone_enc: r.get(5)?,
                    email_enc: r.get(6)?,
                    dob_enc: r.get(7)?,
                    notes_enc: r.get(8)?,
                    tags: r.get(9)?,
                },
                r.get(10)?,
            ))
        },
    )
    .optional()
    .map_err(db_error)
}

pub fn latest_consent(
    conn: &Connection,
    player_id: &str,
    purpose: &str,
) -> Result<Option<(bool, String)>, AppError> {
    conn.query_row(
        "SELECT granted, policy_version FROM consents WHERE player_id = ?1 AND purpose = ?2
         ORDER BY at DESC, rowid DESC LIMIT 1",
        params![player_id, purpose],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(db_error)
}

/// Every live player's phone lookup and every username in use, for checking a CSV import (Story
/// 5.34) without a query per row.
pub fn player_import_keys(
    conn: &Connection,
) -> Result<
    (
        std::collections::HashSet<String>,
        std::collections::HashSet<String>,
    ),
    AppError,
> {
    let mut lookups = std::collections::HashSet::new();
    let mut names = std::collections::HashSet::new();
    let mut stmt = conn
        .prepare("SELECT phone_lookup, username FROM players WHERE deleted_at IS NULL")
        .map_err(db_error)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(db_error)?;
    for row in rows {
        let (lookup, name) = row.map_err(db_error)?;
        lookups.insert(lookup);
        names.insert(name);
    }
    Ok((lookups, names))
}
