//! Ad requests and impressions (Story 4.14): one row per event and a cursor for each table.
//!
//! This is local bookkeeping outside the change log, like the licence cache: `apply()` writes
//! a row for the system intents `ad.request_recorded` / `ad.impression_recorded`, and the
//! heartbeat reads the rows after the cursor and moves the cursor when the cloud acknowledges.
//! A row holds its `id`, `slot` and `at` and nothing else: no device, creative, advertiser or
//! advertising id.

use arena_proto::AppError;
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

/// Which of the two tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdKind {
    Impression,
    Request,
}

impl AdKind {
    fn table(self) -> &'static str {
        match self {
            Self::Impression => "ad_impressions",
            Self::Request => "ad_requests",
        }
    }

    fn cursor_table(self) -> &'static str {
        match self {
            Self::Impression => "ad_upload_cursor",
            Self::Request => "ad_request_cursor",
        }
    }
}

/// One recorded event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdRow {
    pub id: i64,
    pub slot: String,
    pub at: i64,
}

/// Records one event and returns its id.
pub fn insert_ad(conn: &Connection, kind: AdKind, slot: &str, at: i64) -> Result<i64, AppError> {
    conn.execute(
        &format!("INSERT INTO {} (slot, at) VALUES (?1, ?2)", kind.table()),
        params![slot, at],
    )
    .map_err(db_error)?;
    Ok(conn.last_insert_rowid())
}

/// The last id the cloud acknowledged (0: none yet).
pub fn ad_cursor(conn: &Connection, kind: AdKind) -> Result<i64, AppError> {
    conn.query_row(
        &format!(
            "SELECT through_id FROM {} WHERE id = 1",
            kind.cursor_table()
        ),
        [],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// The rows with `id > after`, oldest first, at most `limit`.
pub fn ad_rows_after(
    conn: &Connection,
    kind: AdKind,
    after: i64,
    limit: usize,
) -> Result<Vec<AdRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT id, slot, at FROM {} WHERE id > ?1 ORDER BY id LIMIT ?2",
            kind.table()
        ))
        .map_err(db_error)?;
    stmt.query_map(
        params![after, i64::try_from(limit).unwrap_or(i64::MAX)],
        |r| {
            Ok(AdRow {
                id: r.get(0)?,
                slot: r.get(1)?,
                at: r.get(2)?,
            })
        },
    )
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// The highest id ever given in this table (0: none), whether or not the row is still there.
fn highest_id(conn: &Connection, kind: AdKind) -> Result<i64, AppError> {
    let seq: Option<i64> = conn
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = ?1",
            [kind.table()],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_error)?;
    Ok(seq.unwrap_or(0))
}

/// Moves the cursor to the cloud's acknowledgement and drops the rows it covers; answers the new
/// cursor. An acknowledgement beyond any id this table has given is held to the highest id (a
/// cursor never points past what exists).
pub fn ad_acknowledge(conn: &Connection, kind: AdKind, through: i64) -> Result<i64, AppError> {
    // Monotonic: a stale or lower answer never moves the cursor back over rows already dropped.
    let cursor = through
        .clamp(0, highest_id(conn, kind)?)
        .max(ad_cursor(conn, kind)?);
    conn.execute(
        &format!(
            "UPDATE {} SET through_id = ?1 WHERE id = 1",
            kind.cursor_table()
        ),
        [cursor],
    )
    .map_err(db_error)?;
    conn.execute(
        &format!("DELETE FROM {} WHERE id <= ?1", kind.table()),
        [cursor],
    )
    .map_err(db_error)?;
    Ok(cursor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    const T: i64 = 1_800_000_000_000;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("branch.db"), T).unwrap();
        (dir, store)
    }

    #[test]
    fn the_tables_hold_an_id_a_slot_and_a_time_and_nothing_else() {
        let (_dir, store) = store();
        for table in ["ad_impressions", "ad_requests"] {
            let columns: Vec<String> = store
                .conn
                .prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")
                .unwrap()
                .query_map([table], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(columns, ["id", "slot", "at"], "{table}");
        }
        for table in ["ad_upload_cursor", "ad_request_cursor"] {
            let rows: Vec<(i64, i64)> = store
                .conn
                .prepare(&format!("SELECT id, through_id FROM {table}"))
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(rows, [(1, 0)], "{table}: one row, nothing acknowledged");
            assert!(
                store
                    .conn
                    .execute(&format!("INSERT INTO {table} VALUES (2, 0)"), [])
                    .is_err(),
                "{table} is a single row"
            );
        }
    }

    #[test]
    fn a_slot_outside_banner_and_interstitial_is_refused() {
        let (_dir, store) = store();
        for kind in [AdKind::Impression, AdKind::Request] {
            assert!(insert_ad(&store.conn, kind, "banner", T).is_ok());
            assert!(insert_ad(&store.conn, kind, "interstitial", T).is_ok());
            assert!(insert_ad(&store.conn, kind, "rewarded", T).is_err());
        }
    }

    #[test]
    fn an_acknowledgement_moves_the_cursor_drops_its_rows_and_is_held_to_what_exists() {
        let (_dir, store) = store();
        let c = &store.conn;
        for n in 0..5 {
            assert_eq!(
                insert_ad(c, AdKind::Request, "banner", T + n).unwrap(),
                n + 1
            );
        }
        assert_eq!(ad_cursor(c, AdKind::Request).unwrap(), 0);
        assert_eq!(ad_rows_after(c, AdKind::Request, 0, 3).unwrap().len(), 3);
        assert_eq!(ad_acknowledge(c, AdKind::Request, 3).unwrap(), 3);
        let rest = ad_rows_after(c, AdKind::Request, 3, 10).unwrap();
        assert_eq!(rest.iter().map(|r| r.id).collect::<Vec<_>>(), [4, 5]);
        assert_eq!(ad_rows_after(c, AdKind::Request, 0, 10).unwrap().len(), 2);
        // Beyond anything given: held to the highest id; the other table is untouched.
        assert_eq!(ad_acknowledge(c, AdKind::Request, 900).unwrap(), 5);
        // A lower (stale) answer never moves the cursor back.
        assert_eq!(ad_acknowledge(c, AdKind::Request, 2).unwrap(), 5);
        assert_eq!(ad_cursor(c, AdKind::Request).unwrap(), 5);
        assert_eq!(ad_cursor(c, AdKind::Impression).unwrap(), 0);
        // An id is never used twice, even after its rows were dropped.
        assert_eq!(insert_ad(c, AdKind::Request, "banner", T).unwrap(), 6);
    }
}
