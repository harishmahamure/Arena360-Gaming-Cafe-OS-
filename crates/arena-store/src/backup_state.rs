//! The Hub's backup bookkeeping (Story 9.2, AD-24): `backup_state`, one row outside the change
//! log, and the read of the player key table that the key backup wraps.
//!
//! A trigger counts every change to `player_keys` into `keys_version`, in the transaction that
//! made it, so the key backup task can tell "changed since the last PUT" without comparing the
//! table. The row is written only by maintenance calls after the cloud has answered, never by
//! `apply()` as a change row.

use arena_proto::AppError;
use rusqlite::{Connection, params};

use crate::db_error;

/// The longest error text kept in `keys_error` (a code, never player data).
const MAX_ERROR_CHARS: usize = 120;

/// What `backup_state` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupState {
    /// Changes to `player_keys` so far (starts at 1).
    pub keys_version: i64,
    /// The version the last successful key backup held.
    pub keys_backed_up_version: i64,
    /// When (Hub clock, ms) the last key backup succeeded.
    pub keys_backed_up_at: Option<i64>,
    /// Why the last key backup attempt failed; cleared by a success.
    pub keys_error: Option<String>,
    /// When (Hub clock, ms) it failed.
    pub keys_error_at: Option<i64>,
    /// When (Hub clock, ms) the cloud accepted this Hub's escrowed branch key.
    pub kek_escrowed_at: Option<i64>,
}

impl BackupState {
    /// The key table changed since the last successful key backup.
    pub fn keys_dirty(&self) -> bool {
        self.keys_version > self.keys_backed_up_version
    }
}

/// One row of `player_keys`: a player and their data key wrapped by the branch key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerKeyRow {
    pub player_id: String,
    pub dek_wrapped: String,
}

/// The `backup_state` row.
pub fn read_backup_state(conn: &Connection) -> Result<BackupState, AppError> {
    conn.query_row(
        "SELECT keys_version, keys_backed_up_version, keys_backed_up_at, keys_error,
                keys_error_at, kek_escrowed_at
         FROM backup_state WHERE id = 1",
        [],
        |r| {
            Ok(BackupState {
                keys_version: r.get(0)?,
                keys_backed_up_version: r.get(1)?,
                keys_backed_up_at: r.get(2)?,
                keys_error: r.get(3)?,
                keys_error_at: r.get(4)?,
                kek_escrowed_at: r.get(5)?,
            })
        },
    )
    .map_err(db_error)
}

/// Every key row, by player id.
pub fn list_player_keys(conn: &Connection) -> Result<Vec<PlayerKeyRow>, AppError> {
    let mut stmt = conn
        .prepare("SELECT player_id, dek_wrapped FROM player_keys ORDER BY player_id")
        .map_err(db_error)?;
    stmt.query_map([], |r| {
        Ok(PlayerKeyRow {
            player_id: r.get(0)?,
            dek_wrapped: r.get(1)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// Records that a key backup holding `version` was stored at `at`. The recorded version never
/// moves back, so a slow cycle cannot hide a newer change; the last error is cleared.
pub fn write_keys_backed_up(conn: &Connection, version: i64, at: i64) -> Result<(), AppError> {
    conn.execute(
        "UPDATE backup_state SET
           keys_backed_up_version = MAX(keys_backed_up_version, ?1),
           keys_backed_up_at = ?2, keys_error = NULL, keys_error_at = NULL
         WHERE id = 1",
        params![version, at],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Records why the last key backup attempt failed (`error` is cut to a short code or phrase).
pub fn write_keys_error(conn: &Connection, error: &str, at: i64) -> Result<(), AppError> {
    let error: String = error.chars().take(MAX_ERROR_CHARS).collect();
    conn.execute(
        "UPDATE backup_state SET keys_error = ?1, keys_error_at = ?2 WHERE id = 1",
        params![error, at],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Records that the cloud holds this Hub's escrowed branch key as of `at`.
pub fn write_kek_escrowed(conn: &Connection, at: i64) -> Result<(), AppError> {
    conn.execute(
        "UPDATE backup_state SET kek_escrowed_at = ?1 WHERE id = 1",
        [at],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Forgets the escrow: the branch key was replaced (a re-pair after the old one was lost), so the
/// next check escrows the new one. The key backup is stale too: it was wrapped by the old key.
pub fn clear_kek_escrow(conn: &Connection) -> Result<(), AppError> {
    conn.execute(
        "UPDATE backup_state SET kek_escrowed_at = NULL, keys_backed_up_version = 0,
           keys_version = keys_version + 1
         WHERE id = 1",
        [],
    )
    .map_err(db_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
        for m in crate::migrations::MIGRATIONS {
            conn.execute_batch(m.sql).unwrap();
        }
        conn
    }

    fn player(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO players (player_id, username, password_hash, phone_lookup, tags, rev,
                                  created_at, updated_at, name_enc, phone_enc)
             VALUES (?1, 'u-' || lower(?1), '', ?1, '[]', 0, 0, 0, 'n', 'p')",
            [id],
        )
        .unwrap();
    }

    #[test]
    fn a_new_database_starts_dirty_and_every_key_change_bumps_the_version() {
        let conn = conn();
        let s = read_backup_state(&conn).unwrap();
        assert!(s.keys_dirty(), "the first key backup is always due");
        assert_eq!(s.kek_escrowed_at, None);
        player(&conn, "P1");
        let v0 = read_backup_state(&conn).unwrap().keys_version;
        conn.execute("INSERT INTO player_keys VALUES ('P1', 'w1')", [])
            .unwrap();
        conn.execute("UPDATE player_keys SET dek_wrapped = 'w2'", [])
            .unwrap();
        conn.execute("DELETE FROM player_keys", []).unwrap();
        assert_eq!(read_backup_state(&conn).unwrap().keys_version, v0 + 3);
    }

    #[test]
    fn a_backup_records_its_version_and_never_hides_a_newer_change() {
        let conn = conn();
        write_keys_error(&conn, "CLOUD_UNREACHABLE", 5).unwrap();
        let v = read_backup_state(&conn).unwrap().keys_version;
        write_keys_backed_up(&conn, v, 10).unwrap();
        let s = read_backup_state(&conn).unwrap();
        assert!(!s.keys_dirty());
        assert_eq!(
            (s.keys_backed_up_at, s.keys_error, s.keys_error_at),
            (Some(10), None, None)
        );
        // A late, older record cannot move the version back.
        write_keys_backed_up(&conn, v - 1, 11).unwrap();
        assert_eq!(read_backup_state(&conn).unwrap().keys_backed_up_version, v);
        player(&conn, "P1");
        conn.execute("INSERT INTO player_keys VALUES ('P1', 'w1')", [])
            .unwrap();
        assert!(read_backup_state(&conn).unwrap().keys_dirty());
    }

    #[test]
    fn the_error_is_cut_short_and_a_new_branch_key_forgets_the_escrow_and_the_backup() {
        let conn = conn();
        write_keys_error(&conn, &"x".repeat(500), 7).unwrap();
        assert_eq!(
            read_backup_state(&conn).unwrap().keys_error.unwrap().len(),
            MAX_ERROR_CHARS
        );
        write_kek_escrowed(&conn, 8).unwrap();
        let v = read_backup_state(&conn).unwrap().keys_version;
        write_keys_backed_up(&conn, v, 9).unwrap();
        clear_kek_escrow(&conn).unwrap();
        let s = read_backup_state(&conn).unwrap();
        assert_eq!(s.kek_escrowed_at, None);
        assert!(s.keys_dirty());
    }

    #[test]
    fn the_key_list_is_by_player_id() {
        let conn = conn();
        for id in ["P2", "P1"] {
            player(&conn, id);
            conn.execute("INSERT INTO player_keys VALUES (?1, 'w')", [id])
                .unwrap();
        }
        let ids: Vec<_> = list_player_keys(&conn)
            .unwrap()
            .into_iter()
            .map(|k| k.player_id)
            .collect();
        assert_eq!(ids, ["P1", "P2"]);
    }
}
