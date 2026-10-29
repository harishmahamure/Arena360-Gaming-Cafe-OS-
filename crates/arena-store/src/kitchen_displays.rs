//! Kitchen displays (Story 7.6): the `kitchen_displays` change-log table. A tablet pairs with a
//! code at the Hub and holds a scoped `kds` token; only the Argon2id hash of its secret is kept.
//! Only `apply()` writes the table (through these helpers); the LAN sessions read it to verify a
//! display's hello.

use arena_proto::{AppError, ErrorCode};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

/// One `kitchen_displays` row.
#[derive(Clone, PartialEq, Eq)]
pub struct DisplayRow {
    pub id: String,
    pub name: String,
    /// The Argon2id PHC string of the token's secret. Never sent to a client, never logged.
    pub token_hash: String,
    /// The Manager or Owner who issued the pairing code.
    pub created_by: Option<String>,
    pub last_seen: Option<i64>,
    pub revoked_at: Option<i64>,
    pub high_contrast: bool,
    pub rev: i64,
}

impl std::fmt::Debug for DisplayRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DisplayRow")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("revoked_at", &self.revoked_at)
            .finish_non_exhaustive()
    }
}

/// A new display (`rev` and the timestamps are set by the stamp step).
pub struct NewDisplay<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub token_hash: &'a str,
    pub created_by: Option<&'a str>,
}

const COLUMNS: &str = "id, name, token_hash, created_by, last_seen, revoked_at, high_contrast, rev";

fn display_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<DisplayRow> {
    Ok(DisplayRow {
        id: r.get(0)?,
        name: r.get(1)?,
        token_hash: r.get(2)?,
        created_by: r.get(3)?,
        last_seen: r.get(4)?,
        revoked_at: r.get(5)?,
        high_contrast: r.get::<_, i64>(6)? != 0,
        rev: r.get(7)?,
    })
}

pub fn find_display(conn: &Connection, id: &str) -> Result<Option<DisplayRow>, AppError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM kitchen_displays WHERE id = ?1"),
        [id],
        display_from,
    )
    .optional()
    .map_err(db_error)
}

/// Every display, revoked ones included, oldest first.
pub fn list_displays(conn: &Connection) -> Result<Vec<DisplayRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM kitchen_displays ORDER BY id"
        ))
        .map_err(db_error)?;
    let rows = stmt.query_map([], display_from).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

/// The displays that are not revoked.
pub fn count_live_displays(conn: &Connection) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COUNT(*) FROM kitchen_displays WHERE revoked_at IS NULL",
        [],
        |r| r.get(0),
    )
    .map_err(db_error)
}

pub fn insert_display(conn: &Connection, d: &NewDisplay<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO kitchen_displays (id, name, token_hash, created_by, last_seen, revoked_at,
                                       high_contrast, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, NULL, NULL, 0, 0, 0, 0)",
        params![d.id, d.name, d.token_hash, d.created_by],
    )
    .map_err(db_error)?;
    Ok(())
}

fn expect_one(n: usize, id: &str) -> Result<(), AppError> {
    if n == 1 {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::Internal,
            format!("no kitchen_displays row {id}"),
        ))
    }
}

pub fn set_display_revoked(conn: &Connection, id: &str, at_ms: i64) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE kitchen_displays SET revoked_at = ?2 WHERE id = ?1",
            params![id, at_ms],
        )
        .map_err(db_error)?;
    expect_one(n, id)
}

pub fn set_display_last_seen(conn: &Connection, id: &str, at_ms: i64) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE kitchen_displays SET last_seen = ?2 WHERE id = ?1",
            params![id, at_ms],
        )
        .map_err(db_error)?;
    expect_one(n, id)
}

pub fn set_display_high_contrast(conn: &Connection, id: &str, on: bool) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE kitchen_displays SET high_contrast = ?2 WHERE id = ?1",
            params![id, i64::from(on)],
        )
        .map_err(db_error)?;
    expect_one(n, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RowKey, Store, Table, stamp_row};

    fn new<'a>(id: &'a str, name: &'a str) -> NewDisplay<'a> {
        NewDisplay {
            id,
            name,
            token_hash: "$argon2id$x",
            created_by: Some("S"),
        }
    }

    #[test]
    fn displays_round_trip_and_revoked_ones_leave_the_live_count() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("branch.db"), 0).unwrap();
        assert!(store.kitchen_display("A").unwrap().is_none());
        store
            .write_tx(|tx| {
                insert_display(tx, &new("A", "Pass"))?;
                insert_display(tx, &new("B", "Grill"))?;
                assert_eq!(count_live_displays(tx)?, 2);
                set_display_last_seen(tx, "A", 50)?;
                set_display_high_contrast(tx, "A", true)?;
                set_display_revoked(tx, "B", 60)?;
                assert_eq!(count_live_displays(tx)?, 1);
                assert!(set_display_revoked(tx, "missing", 1).is_err());
                for key in ["A", "B"] {
                    stamp_row(tx, &RowKey::KitchenDisplays { id: key.into() }, 7, true)?;
                }
                Ok(())
            })
            .unwrap();
        let a = store.kitchen_display("A").unwrap().unwrap();
        assert_eq!(
            (a.name.as_str(), a.last_seen, a.high_contrast, a.revoked_at),
            ("Pass", Some(50), true, None)
        );
        assert_eq!(a.created_by.as_deref(), Some("S"));
        assert_eq!(a.rev, 1);
        let b = store.kitchen_display("B").unwrap().unwrap();
        assert_eq!((b.revoked_at, b.high_contrast), (Some(60), false));
        assert_eq!(store.kitchen_displays().unwrap().len(), 2);
        // A change-log table: it dumps by its `id` and its hash never shows in a debug print.
        assert_eq!(store.rows(Table::KitchenDisplays).unwrap().len(), 2);
        assert!(!format!("{a:?}").contains("argon2id"));
    }

    #[test]
    fn the_table_rejects_a_high_contrast_value_outside_zero_and_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("branch.db"), 0).unwrap();
        let bad = store.write_tx(|tx| {
            tx.execute(
                "INSERT INTO kitchen_displays (id, name, token_hash, high_contrast, rev, created_at, updated_at)
                 VALUES ('X', 'n', 'h', 2, 0, 0, 0)",
                [],
            )
            .map_err(crate::db_error)?;
            Ok(())
        });
        assert!(bad.is_err());
    }
}
