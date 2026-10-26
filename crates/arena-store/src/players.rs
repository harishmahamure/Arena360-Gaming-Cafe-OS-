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

pub fn phone_taken(conn: &Connection, phone_lookup: &str) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM players WHERE phone_lookup = ?1 AND deleted_at IS NULL)",
        [phone_lookup],
        |r| r.get(0),
    )
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
