//! `arena-store`: the branch database. All SQL lives in this crate.
//!
//! [`Store::open`] opens SQLite in WAL mode with `synchronous=FULL` (both verified by reading
//! them back), refuses a schema newer than this binary and applies the embedded migrations.

mod migrations;

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::Duration;

use arena_proto::{AppError, ErrorCode};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

pub use migrations::LATEST_SCHEMA_VERSION;
/// The transaction a write job runs in (always `BEGIN IMMEDIATE`).
pub use rusqlite::Transaction;

/// The `hub_meta` key holding this Hub's instance id.
pub const META_HUB_INSTANCE_ID: &str = "hub_instance_id";

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Maps a SQLite failure to an `INTERNAL` error.
pub fn db_error(err: rusqlite::Error) -> AppError {
    AppError::new(ErrorCode::Internal, format!("database error: {err}"))
}

fn io_error(err: std::io::Error) -> AppError {
    AppError::new(ErrorCode::Internal, format!("i/o error: {err}"))
}

/// The single row of the `lease` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseRow {
    pub counter: i64,
    pub hub_instance_id: String,
    pub provisional: bool,
    pub renewed_at: i64,
    pub expires_at: i64,
}

/// An open branch database.
pub struct Store {
    conn: Connection,
    path: PathBuf,
    writer_lock: Option<File>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("path", &self.path)
            .field("writer_lock", &self.writer_lock.is_some())
            .finish()
    }
}

impl Store {
    /// Opens (creating if needed) the database at `path`, verifies the pragmas, takes the
    /// process writer lock and, only when it holds that lock, migrates it (recording
    /// `applied_at = now_ms`).
    ///
    /// Without the lock (another process on this data dir is the writer) the store is a
    /// read-only follower: it never migrates or writes, even when its schema is older than this
    /// binary's. See [`Store::holds_writer_lock`].
    ///
    /// Fails `SCHEMA_TOO_NEW` without writing anything when the file was written by a newer
    /// binary.
    pub fn open(path: impl AsRef<Path>, now_ms: i64) -> Result<Self, AppError> {
        let path = path.as_ref().to_path_buf();
        let conn = Connection::open(&path).map_err(db_error)?;
        conn.busy_timeout(BUSY_TIMEOUT).map_err(db_error)?;

        // Schema guard first: nothing below may run against a newer schema.
        let found = current_version(&conn)?;
        if found > LATEST_SCHEMA_VERSION {
            return Err(schema_too_new(found));
        }

        let mode: String = conn
            .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
            .map_err(db_error)?;
        conn.execute_batch("PRAGMA synchronous=FULL")
            .map_err(db_error)?;
        let mut store = Self {
            conn,
            path,
            writer_lock: None,
        };
        let journal = store.journal_mode()?;
        if !mode.eq_ignore_ascii_case("wal") || !journal.eq_ignore_ascii_case("wal") {
            return Err(AppError::new(
                ErrorCode::Internal,
                format!("journal_mode is {journal}, expected wal"),
            ));
        }
        let sync = store.synchronous()?;
        if sync != 2 {
            return Err(AppError::new(
                ErrorCode::Internal,
                format!("synchronous is {sync}, expected 2 (FULL)"),
            ));
        }
        if store.try_lock_writer()? {
            store.migrate(now_ms)?;
        }
        Ok(store)
    }

    fn migrate(&mut self, applied_at: i64) -> Result<(), AppError> {
        if current_version(&self.conn)? == LATEST_SCHEMA_VERSION {
            return Ok(());
        }
        self.write_tx(|tx| {
            // Re-read under the write lock: another process may have migrated meanwhile.
            let found = current_version(tx)?;
            if found > LATEST_SCHEMA_VERSION {
                return Err(schema_too_new(found));
            }
            for m in migrations::MIGRATIONS.iter().filter(|m| m.version > found) {
                tx.execute_batch(m.sql).map_err(db_error)?;
                record_schema_version(tx, m.version, applied_at)?;
            }
            Ok(())
        })
    }

    /// The database file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The highest applied migration.
    pub fn schema_version(&self) -> Result<u32, AppError> {
        current_version(&self.conn)
    }

    /// `PRAGMA journal_mode`, read back.
    pub fn journal_mode(&self) -> Result<String, AppError> {
        self.conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .map_err(db_error)
    }

    /// `PRAGMA synchronous`, read back (2 = FULL).
    pub fn synchronous(&self) -> Result<i64, AppError> {
        self.conn
            .query_row("PRAGMA synchronous", [], |r| r.get(0))
            .map_err(db_error)
    }

    /// `PRAGMA integrity_check` (`"ok"` when healthy).
    pub fn integrity_check(&self) -> Result<String, AppError> {
        self.conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(db_error)
    }

    /// Runs `f` in one `BEGIN IMMEDIATE` transaction: committed on `Ok`, rolled back on `Err`.
    pub fn write_tx<R>(
        &mut self,
        f: impl FnOnce(&Transaction<'_>) -> Result<R, AppError>,
    ) -> Result<R, AppError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let out = f(&tx)?; // dropping `tx` rolls back
        tx.commit().map_err(db_error)?;
        Ok(out)
    }

    /// Takes the process-level writer lock (`<db>.lock`), so two processes on one data dir can
    /// never both write. Returns `false` when another process holds it. The OS releases it when
    /// this process exits, however it exits. [`Store::open`] already tries once; this retries.
    pub fn try_lock_writer(&mut self) -> Result<bool, AppError> {
        if self.writer_lock.is_some() {
            return Ok(true);
        }
        let mut lock_path = self.path.clone().into_os_string();
        lock_path.push(".lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(PathBuf::from(lock_path))
            .map_err(io_error)?;
        match file.try_lock() {
            Ok(()) => {
                self.writer_lock = Some(file);
                Ok(true)
            }
            Err(TryLockError::WouldBlock) => Ok(false),
            Err(TryLockError::Error(e)) => Err(io_error(e)),
        }
    }

    /// Whether this store holds the process writer lock (taken by [`Store::open`]).
    pub fn holds_writer_lock(&self) -> bool {
        self.writer_lock.is_some()
    }

    /// Reads one `hub_meta` value (`None` on a database not yet migrated).
    pub fn meta(&self, key: &str) -> Result<Option<String>, AppError> {
        if self.schema_version()? == 0 {
            return Ok(None);
        }
        get_meta(&self.conn, key)
    }

    /// Reads the lease row outside any write transaction (`None` on a database not yet
    /// migrated).
    pub fn lease(&self) -> Result<Option<LeaseRow>, AppError> {
        if self.schema_version()? == 0 {
            return Ok(None);
        }
        read_lease(&self.conn)
    }
}

/// Inserts a `schema_version` row. Used by migrations (and by tests that fake a newer schema).
pub fn record_schema_version(
    conn: &Connection,
    version: u32,
    applied_at: i64,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO schema_version (version, applied_at) VALUES (?1, ?2)",
        params![version, applied_at],
    )
    .map_err(db_error)?;
    Ok(())
}

fn schema_too_new(found: u32) -> AppError {
    AppError::new(
        ErrorCode::SchemaTooNew,
        format!(
            "database schema version {found} is newer than this binary's version {LATEST_SCHEMA_VERSION}"
        ),
    )
}

/// The highest `schema_version`, or 0 for an empty database. Read-only.
fn current_version(conn: &Connection) -> Result<u32, AppError> {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_version')",
            [],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if !exists {
        return Ok(0);
    }
    let v: Option<i64> = conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))
        .map_err(db_error)?;
    Ok(v.unwrap_or(0).clamp(0, i64::from(u32::MAX)) as u32)
}

/// Reads one `hub_meta` value.
pub fn get_meta(conn: &Connection, key: &str) -> Result<Option<String>, AppError> {
    conn.query_row("SELECT value FROM hub_meta WHERE key = ?1", [key], |r| {
        r.get::<_, Option<String>>(0)
    })
    .optional()
    .map(Option::flatten)
    .map_err(db_error)
}

/// Inserts `value` for `key` unless a value exists; returns the stored value either way.
pub fn insert_meta_if_absent(
    conn: &Connection,
    key: &str,
    value: &str,
) -> Result<String, AppError> {
    conn.execute(
        "INSERT INTO hub_meta (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO NOTHING",
        params![key, value],
    )
    .map_err(db_error)?;
    get_meta(conn, key)?.ok_or_else(|| {
        AppError::new(
            ErrorCode::Internal,
            format!("hub_meta {key} missing after insert"),
        )
    })
}

/// Sets `key` to `value`, replacing any existing value.
pub fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO hub_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Reads the lease row, if any.
pub fn read_lease(conn: &Connection) -> Result<Option<LeaseRow>, AppError> {
    conn.query_row(
        "SELECT counter, hub_instance_id, provisional, renewed_at, expires_at FROM lease WHERE id = 1",
        [],
        |r| {
            Ok(LeaseRow {
                counter: r.get(0)?,
                hub_instance_id: r.get(1)?,
                provisional: r.get::<_, i64>(2)? != 0,
                renewed_at: r.get(3)?,
                expires_at: r.get(4)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

/// Deletes the lease row (used by tests and by a later manual-promotion flow).
pub fn delete_lease(conn: &Connection) -> Result<(), AppError> {
    conn.execute("DELETE FROM lease WHERE id = 1", [])
        .map_err(db_error)?;
    Ok(())
}

/// Writes (inserts or replaces) the lease row.
pub fn write_lease(conn: &Connection, row: &LeaseRow) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO lease (id, counter, hub_instance_id, provisional, renewed_at, expires_at)
         VALUES (1, ?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (id) DO UPDATE SET
           counter = excluded.counter,
           hub_instance_id = excluded.hub_instance_id,
           provisional = excluded.provisional,
           renewed_at = excluded.renewed_at,
           expires_at = excluded.expires_at",
        params![
            row.counter,
            row.hub_instance_id,
            i64::from(row.provisional),
            row.renewed_at,
            row.expires_at
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: i64 = 1_800_000_000_000;

    fn temp_db() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        (dir, path)
    }

    #[test]
    fn first_start_creates_file_with_wal_full_and_migrations() {
        let (_dir, path) = temp_db();
        assert!(!path.exists());
        let store = Store::open(&path, T).unwrap();
        assert!(path.exists());
        assert_eq!(store.journal_mode().unwrap(), "wal");
        assert_eq!(store.synchronous().unwrap(), 2);
        assert_eq!(store.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
        let rows: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, migrations::MIGRATIONS.len() as i64);
        assert_eq!(store.lease().unwrap(), None);
    }

    #[test]
    fn restart_does_not_reapply_migrations() {
        let (_dir, path) = temp_db();
        let first = Store::open(&path, T).unwrap();
        let applied: i64 = first
            .conn
            .query_row(
                "SELECT applied_at FROM schema_version WHERE version = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        drop(first);
        let again = Store::open(&path, T).unwrap();
        let (count, applied_again): (i64, i64) = again
            .conn
            .query_row(
                "SELECT COUNT(*), MAX(applied_at) FROM schema_version",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(applied_again, applied);
    }

    #[test]
    fn schema_too_new_fails_and_leaves_file_untouched() {
        let (_dir, path) = temp_db();
        {
            let store = Store::open(&path, T).unwrap();
            record_schema_version(&store.conn, 999, 0).unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        let err = Store::open(&path, T).unwrap_err();
        assert_eq!(err.code, ErrorCode::SchemaTooNew);
        assert!(err.message.contains("999"), "{}", err.message);
        assert!(
            err.message.contains(&LATEST_SCHEMA_VERSION.to_string()),
            "{}",
            err.message
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn meta_insert_if_absent_keeps_first_value() {
        let (_dir, path) = temp_db();
        let mut store = Store::open(&path, T).unwrap();
        let a = store
            .write_tx(|tx| insert_meta_if_absent(tx, META_HUB_INSTANCE_ID, "A"))
            .unwrap();
        let b = store
            .write_tx(|tx| insert_meta_if_absent(tx, META_HUB_INSTANCE_ID, "B"))
            .unwrap();
        assert_eq!((a.as_str(), b.as_str()), ("A", "A"));
        assert_eq!(
            store.meta(META_HUB_INSTANCE_ID).unwrap().as_deref(),
            Some("A")
        );
    }

    #[test]
    fn lease_round_trips_and_failed_tx_rolls_back() {
        let (_dir, path) = temp_db();
        let mut store = Store::open(&path, T).unwrap();
        let row = LeaseRow {
            counter: 1,
            hub_instance_id: "X".into(),
            provisional: true,
            renewed_at: 10,
            expires_at: 20,
        };
        store.write_tx(|tx| write_lease(tx, &row)).unwrap();
        assert_eq!(store.lease().unwrap(), Some(row.clone()));
        let err = store
            .write_tx(|tx| {
                write_lease(
                    tx,
                    &LeaseRow {
                        counter: 9,
                        ..row.clone()
                    },
                )?;
                Err::<(), _>(AppError::new(ErrorCode::Stale, "abort"))
            })
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Stale);
        assert_eq!(store.lease().unwrap(), Some(row));
    }

    fn hold_lock(path: &Path) -> File {
        let mut lock = path.as_os_str().to_owned();
        lock.push(".lock");
        let f = File::create(PathBuf::from(lock)).unwrap();
        f.try_lock().unwrap();
        f
    }

    #[test]
    fn applied_at_comes_from_the_caller() {
        let (_dir, path) = temp_db();
        let store = Store::open(&path, T).unwrap();
        let applied: i64 = store
            .conn
            .query_row("SELECT applied_at FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(applied, T);
    }

    #[test]
    fn follower_without_lock_never_migrates_a_new_file() {
        let (_dir, path) = temp_db();
        let _held = hold_lock(&path);
        let store = Store::open(&path, T).unwrap();
        assert!(!store.holds_writer_lock());
        assert_eq!(store.schema_version().unwrap(), 0);
        assert_eq!(store.lease().unwrap(), None);
        assert_eq!(store.meta(META_HUB_INSTANCE_ID).unwrap(), None);
        let tables: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tables, 0);
    }

    #[test]
    fn follower_without_lock_leaves_existing_file_untouched() {
        let (_dir, path) = temp_db();
        drop(Store::open(&path, T).unwrap());
        let before = std::fs::read(&path).unwrap();
        let _held = hold_lock(&path);
        let store = Store::open(&path, T + 1).unwrap();
        assert!(!store.holds_writer_lock());
        assert_eq!(store.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
        drop(store);
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn writer_lock_is_exclusive() {
        let (_dir, path) = temp_db();
        let mut a = Store::open(&path, T).unwrap();
        let mut b = Store::open(&path, T).unwrap();
        assert!(a.holds_writer_lock());
        assert!(!b.holds_writer_lock());
        assert!(a.try_lock_writer().unwrap());
        assert!(!b.try_lock_writer().unwrap());
        drop(a);
        assert!(b.try_lock_writer().unwrap());
    }
}
