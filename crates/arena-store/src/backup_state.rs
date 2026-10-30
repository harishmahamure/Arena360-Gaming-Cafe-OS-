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
    /// The epoch of the last change row the upgrade snapshot holds (Story 9.11); `None` until
    /// one is stored, and again after a lapse.
    pub base_epoch: Option<String>,
    /// That row's `seq`: the cloud holds no batches up to it, so gap detection starts after it.
    pub base_seq: Option<i64>,
    /// The branch is paid but its upgrade snapshot is not stored yet, so no batch goes up.
    pub first_snapshot_pending: bool,
    /// Why the last attempt at the upgrade snapshot failed (a code, never player data).
    pub first_snapshot_error: Option<String>,
    /// When (Hub clock, ms) it failed.
    pub first_snapshot_error_at: Option<i64>,
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
                keys_error_at, kek_escrowed_at, base_epoch, base_seq, first_snapshot_pending,
                first_snapshot_error, first_snapshot_error_at
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
                base_epoch: r.get(6)?,
                base_seq: r.get(7)?,
                first_snapshot_pending: r.get(8)?,
                first_snapshot_error: r.get(9)?,
                first_snapshot_error_at: r.get(10)?,
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

/// Marks the upgrade snapshot as pending (Story 9.11): the branch is paid and no batch goes up
/// until it is stored. `error` (cut to a short code) says why the last attempt failed; `None`
/// leaves the last error as it was.
pub fn write_first_snapshot_pending(
    conn: &Connection,
    error: Option<&str>,
    at: i64,
) -> Result<(), AppError> {
    match error {
        Some(error) => {
            let error: String = error.chars().take(MAX_ERROR_CHARS).collect();
            conn.execute(
                "UPDATE backup_state SET first_snapshot_pending = 1, first_snapshot_error = ?1,
                   first_snapshot_error_at = ?2 WHERE id = 1",
                params![error, at],
            )
        }
        None => conn.execute(
            "UPDATE backup_state SET first_snapshot_pending = 1 WHERE id = 1",
            [],
        ),
    }
    .map_err(db_error)?;
    Ok(())
}

/// Records that the upgrade snapshot was stored and that batching starts after its last change
/// row: the pending mark and its error are cleared, the base is kept for the heartbeat, and the
/// upload cursor is set to `epoch`/`seq`, all in the caller's transaction.
pub fn complete_first_snapshot(
    conn: &Connection,
    epoch: &str,
    seq: i64,
    at: i64,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE backup_state SET base_epoch = ?1, base_seq = ?2, first_snapshot_pending = 0,
           first_snapshot_error = NULL, first_snapshot_error_at = NULL WHERE id = 1",
        params![epoch, seq],
    )
    .map_err(db_error)?;
    crate::write_upload_cursor(
        conn,
        &crate::UploadCursor {
            epoch: epoch.to_owned(),
            last_seq_uploaded: seq,
            uploaded_at: at,
        },
    )
}

/// A lapse back to Free (Story 9.11): uploads stop and the next upgrade starts again with a fresh
/// full snapshot, so the cursor, the base and the pending mark are forgotten. Objects already in
/// the cloud stay there. Answers whether anything was forgotten.
pub fn stop_uploads(conn: &Connection) -> Result<bool, AppError> {
    let cursor = conn
        .execute("DELETE FROM upload_cursor WHERE id = 1", [])
        .map_err(db_error)?;
    let state = conn
        .execute(
            "UPDATE backup_state SET base_epoch = NULL, base_seq = NULL,
               first_snapshot_pending = 0, first_snapshot_error = NULL,
               first_snapshot_error_at = NULL
             WHERE id = 1 AND (base_epoch IS NOT NULL OR base_seq IS NOT NULL
               OR first_snapshot_pending = 1 OR first_snapshot_error IS NOT NULL)",
            [],
        )
        .map_err(db_error)?;
    Ok(cursor + state > 0)
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
    fn the_upgrade_snapshot_is_pending_with_its_error_until_it_sets_the_cursor_and_the_base() {
        let conn = conn();
        let s = read_backup_state(&conn).unwrap();
        assert!(!s.first_snapshot_pending);
        write_first_snapshot_pending(&conn, None, 1).unwrap();
        write_first_snapshot_pending(&conn, Some(&"x".repeat(500)), 2).unwrap();
        // A later call without an error leaves the last error in place.
        write_first_snapshot_pending(&conn, None, 3).unwrap();
        let s = read_backup_state(&conn).unwrap();
        assert!(s.first_snapshot_pending);
        assert_eq!(s.first_snapshot_error.unwrap().len(), MAX_ERROR_CHARS);
        assert_eq!(s.first_snapshot_error_at, Some(2));
        assert_eq!(crate::read_upload_cursor(&conn).unwrap(), None);

        complete_first_snapshot(&conn, "3:01JAAAAAAAAAAAAAAAAAAAAAAA", 41, 9).unwrap();
        let s = read_backup_state(&conn).unwrap();
        assert!(!s.first_snapshot_pending);
        assert_eq!(
            (s.first_snapshot_error, s.first_snapshot_error_at),
            (None, None)
        );
        assert_eq!(
            (s.base_epoch.as_deref(), s.base_seq),
            (Some("3:01JAAAAAAAAAAAAAAAAAAAAAAA"), Some(41))
        );
        let c = crate::read_upload_cursor(&conn).unwrap().unwrap();
        assert_eq!(
            (c.epoch.as_str(), c.last_seq_uploaded, c.uploaded_at),
            ("3:01JAAAAAAAAAAAAAAAAAAAAAAA", 41, 9)
        );
    }

    #[test]
    fn a_lapse_forgets_the_cursor_the_base_and_the_pending_mark_once() {
        let conn = conn();
        assert!(
            !stop_uploads(&conn).unwrap(),
            "a Free branch has nothing to forget"
        );
        complete_first_snapshot(&conn, "1:01JAAAAAAAAAAAAAAAAAAAAAAA", 5, 9).unwrap();
        assert!(stop_uploads(&conn).unwrap());
        let s = read_backup_state(&conn).unwrap();
        assert_eq!((s.base_epoch, s.base_seq), (None, None));
        assert_eq!(crate::read_upload_cursor(&conn).unwrap(), None);
        assert!(!stop_uploads(&conn).unwrap());
        write_first_snapshot_pending(&conn, Some("CLOUD_UNREACHABLE"), 1).unwrap();
        assert!(
            stop_uploads(&conn).unwrap(),
            "a pending upgrade is forgotten too"
        );
        let s = read_backup_state(&conn).unwrap();
        assert!(!s.first_snapshot_pending && s.first_snapshot_error.is_none());
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
