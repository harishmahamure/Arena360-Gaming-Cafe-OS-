//! The daily snapshot (Story 8.8, AD-4, AD-24): a compressed copy of the branch database the Hub
//! stores in the cloud once a day, so an audit rebuild starts from a recent point instead of the
//! first change row.
//!
//! The copy is made on a connection of its own with `VACUUM INTO`, which reads one consistent
//! snapshot of the file while `apply()` keeps committing to the live database (WAL). Player
//! personal data is already ciphertext in the database (`*_enc` columns); the wrapped player keys
//! (`player_keys`) are dropped from the copy, so the file alone cannot open any of it.
//!
//! `snapshot_log` is local bookkeeping outside the change log, like `upload_cursor`: a row is
//! written only after the cloud answered 2xx for the file.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use arena_proto::{AppError, ErrorCode};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::db_error;

const ZSTD_LEVEL: i32 = 3;
/// Every scratch file of a snapshot starts with this and ends with [`TEMP_SUFFIX`].
const TEMP_PREFIX: &str = "snapshot-";
const TEMP_SUFFIX: &str = ".tmp";

/// A row of `snapshot_log`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotLogRow {
    /// The business day the snapshot was taken for (`yyyy-mm-dd`).
    pub date: String,
    /// The epoch of the last change row the copy holds (empty when it holds none).
    pub epoch: String,
    /// That row's `seq`.
    pub last_seq: i64,
    /// The compressed size.
    pub bytes: i64,
    /// The object key it was PUT to.
    pub key: String,
    /// When (Hub clock, ms) the cloud accepted it.
    pub uploaded_at: i64,
}

/// The latest day with a stored snapshot, if any.
pub fn latest_snapshot_date(conn: &Connection) -> Result<Option<String>, AppError> {
    conn.query_row("SELECT MAX(date) FROM snapshot_log", [], |r| r.get(0))
        .map_err(db_error)
}

/// The `snapshot_log` row of `date`, if the Hub has stored it.
pub fn find_snapshot_log(
    conn: &Connection,
    date: &str,
) -> Result<Option<SnapshotLogRow>, AppError> {
    conn.query_row(
        "SELECT date, epoch, last_seq, bytes, key, uploaded_at FROM snapshot_log WHERE date = ?1",
        [date],
        |r| {
            Ok(SnapshotLogRow {
                date: r.get(0)?,
                epoch: r.get(1)?,
                last_seq: r.get(2)?,
                bytes: r.get(3)?,
                key: r.get(4)?,
                uploaded_at: r.get(5)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

/// Records a stored snapshot (the day's row is replaced when it is stored again).
pub fn write_snapshot_log(conn: &Connection, row: &SnapshotLogRow) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO snapshot_log (date, epoch, last_seq, bytes, key, uploaded_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (date) DO UPDATE SET
           epoch = excluded.epoch,
           last_seq = excluded.last_seq,
           bytes = excluded.bytes,
           key = excluded.key,
           uploaded_at = excluded.uploaded_at",
        params![
            row.date,
            row.epoch,
            row.last_seq,
            row.bytes,
            row.key,
            row.uploaded_at
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// The bytes the database occupies on disk: the file and its write-ahead log.
pub fn database_bytes(db_path: &Path) -> u64 {
    let mut wal = db_path.as_os_str().to_owned();
    wal.push("-wal");
    [db_path.to_path_buf(), PathBuf::from(wal)]
        .iter()
        .filter_map(|p| fs::metadata(p).ok())
        .map(|m| m.len())
        .sum()
}

/// A finished snapshot file in the scratch directory. Dropping it deletes the file.
#[derive(Debug)]
pub struct SnapshotFile {
    path: PathBuf,
    /// The epoch of the last change row the copy holds (empty when it holds none).
    pub epoch: String,
    /// That row's `seq`.
    pub last_seq: i64,
    /// The compressed size.
    pub bytes: u64,
}

impl SnapshotFile {
    /// The compressed file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The compressed bytes, to PUT.
    pub fn read(&self) -> Result<Vec<u8>, AppError> {
        fs::read(&self.path).map_err(|e| io_error("read the snapshot", &e))
    }
}

impl Drop for SnapshotFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn io_error(what: &str, e: &std::io::Error) -> AppError {
    AppError::new(ErrorCode::Internal, format!("could not {what}: {e}"))
}

/// Deletes scratch files a snapshot that was cut short left in `dir`.
fn clear_leftovers(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(TEMP_PREFIX) && name.ends_with(TEMP_SUFFIX) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Copies the database at `db_path` into `dir` as `snapshot-<date>.db.zst.tmp`: `VACUUM INTO` a
/// scratch file, drop the player key table from the copy, zstd it, delete the scratch file. The
/// source is read on a connection of its own, so the live writer and the shared reader are never
/// held up. Blocking (disk).
pub fn build_snapshot_file(
    db_path: &Path,
    dir: &Path,
    date: &str,
) -> Result<SnapshotFile, AppError> {
    clear_leftovers(dir);
    let scratch = dir.join(format!("{TEMP_PREFIX}{date}.db{TEMP_SUFFIX}"));
    let packed = dir.join(format!("{TEMP_PREFIX}{date}.db.zst{TEMP_SUFFIX}"));
    let made = copy_and_pack(db_path, &scratch, &packed);
    let _ = fs::remove_file(&scratch);
    match made {
        Ok((epoch, last_seq)) => {
            let bytes = fs::metadata(&packed)
                .map_err(|e| io_error("measure the snapshot", &e))?
                .len();
            Ok(SnapshotFile {
                path: packed,
                epoch,
                last_seq,
                bytes,
            })
        }
        Err(e) => {
            let _ = fs::remove_file(&packed);
            Err(e)
        }
    }
}

fn copy_and_pack(db_path: &Path, scratch: &Path, packed: &Path) -> Result<(String, i64), AppError> {
    let source = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(db_error)?;
    source
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(db_error)?;
    source
        .execute("VACUUM INTO ?1", [scratch.to_string_lossy().as_ref()])
        .map_err(db_error)?;
    drop(source);

    let (epoch, last_seq) = {
        let copy = Connection::open(scratch).map_err(db_error)?;
        // No `-wal` file beside the copy, and the dropped table's pages are wiped and compacted
        // away rather than left in the free list.
        copy.execute_batch(
            "PRAGMA journal_mode = DELETE;
             PRAGMA secure_delete = ON;
             DROP TABLE IF EXISTS player_keys;
             VACUUM;",
        )
        .map_err(db_error)?;
        copy.query_row(
            "SELECT epoch, seq FROM changes
             ORDER BY epoch_counter DESC, epoch DESC, seq DESC LIMIT 1",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(db_error)?
        .unwrap_or_default()
    };

    let mut input = File::open(scratch).map_err(|e| io_error("open the snapshot copy", &e))?;
    let mut output = File::create(packed).map_err(|e| io_error("create the snapshot file", &e))?;
    zstd::stream::copy_encode(&mut input, &mut output, ZSTD_LEVEL)
        .map_err(|e| io_error("compress the snapshot", &e))?;
    output
        .sync_all()
        .map_err(|e| io_error("write the snapshot file", &e))?;
    Ok((epoch, last_seq))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database(dir: &Path) -> PathBuf {
        let path = dir.join("branch.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE players (player_id TEXT PRIMARY KEY, name_enc TEXT NOT NULL);
             CREATE TABLE player_keys (player_id TEXT PRIMARY KEY, dek_wrapped TEXT NOT NULL);
             CREATE TABLE changes (
                 epoch TEXT NOT NULL, epoch_counter INTEGER NOT NULL, seq INTEGER NOT NULL
             );
             CREATE TABLE snapshot_log (
                 date TEXT PRIMARY KEY, epoch TEXT NOT NULL, last_seq INTEGER NOT NULL,
                 bytes INTEGER NOT NULL, key TEXT NOT NULL, uploaded_at INTEGER NOT NULL
             );
             INSERT INTO players VALUES ('P1', 'ciphertext-of-a-name');
             INSERT INTO player_keys VALUES ('P1', 'SECRET-WRAPPED-DEK-VALUE');
             INSERT INTO changes VALUES ('1:a', 1, 1), ('1:a', 1, 2), ('2:b', 2, 1);",
        )
        .unwrap();
        path
    }

    #[test]
    fn the_copy_has_no_player_keys_and_knows_its_last_seq() {
        let dir = tempfile::tempdir().unwrap();
        let db = database(dir.path());
        let out = tempfile::tempdir().unwrap();
        let file = build_snapshot_file(&db, out.path(), "2026-10-29").unwrap();
        assert_eq!((file.epoch.as_str(), file.last_seq), ("2:b", 1));
        assert_eq!(file.bytes, fs::metadata(file.path()).unwrap().len());

        let plain = zstd::decode_all(&file.read().unwrap()[..]).unwrap();
        assert!(
            !String::from_utf8_lossy(&plain).contains("SECRET-WRAPPED-DEK-VALUE"),
            "the wrapped key is nowhere in the file, free pages included"
        );
        assert!(
            String::from_utf8_lossy(&plain).contains("ciphertext-of-a-name"),
            "the player rows themselves are there, as ciphertext"
        );
        let copy_path = out.path().join("opened.db");
        fs::write(&copy_path, &plain).unwrap();
        let copy = Connection::open(&copy_path).unwrap();
        let tables: Vec<String> = copy
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(tables.contains(&"players".to_owned()));
        assert!(!tables.contains(&"player_keys".to_owned()));
    }

    #[test]
    fn the_live_database_keeps_its_keys_and_no_scratch_file_stays() {
        let dir = tempfile::tempdir().unwrap();
        let db = database(dir.path());
        let out = tempfile::tempdir().unwrap();
        let file = build_snapshot_file(&db, out.path(), "2026-10-29").unwrap();
        let live = Connection::open(&db).unwrap();
        let keys: i64 = live
            .query_row("SELECT COUNT(*) FROM player_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(keys, 1);
        let names = |d: &Path| -> Vec<String> {
            fs::read_dir(d)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(
            names(out.path()),
            vec![file.path().file_name().unwrap().to_string_lossy()]
        );
        let path = file.path().to_path_buf();
        drop(file);
        assert!(!path.exists(), "dropping the file deletes it");
        assert!(names(out.path()).is_empty());
    }

    #[test]
    fn leftovers_of_an_interrupted_snapshot_are_cleared_first() {
        let dir = tempfile::tempdir().unwrap();
        let db = database(dir.path());
        let out = tempfile::tempdir().unwrap();
        fs::write(out.path().join("snapshot-2026-10-01.db.tmp"), b"half").unwrap();
        fs::write(out.path().join("keep.txt"), b"mine").unwrap();
        let file = build_snapshot_file(&db, out.path(), "2026-10-29").unwrap();
        assert!(!out.path().join("snapshot-2026-10-01.db.tmp").exists());
        assert!(out.path().join("keep.txt").exists());
        drop(file);
    }

    #[test]
    fn the_log_keeps_the_latest_day_and_replaces_a_repeated_one() {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(database(dir.path())).unwrap();
        assert_eq!(latest_snapshot_date(&conn).unwrap(), None);
        let row = |date: &str, last_seq: i64| SnapshotLogRow {
            date: date.into(),
            epoch: "1:a".into(),
            last_seq,
            bytes: 10,
            key: format!("snapshots/{date}.db.zst"),
            uploaded_at: 5,
        };
        write_snapshot_log(&conn, &row("2026-10-28", 4)).unwrap();
        write_snapshot_log(&conn, &row("2026-10-29", 9)).unwrap();
        write_snapshot_log(&conn, &row("2026-10-29", 12)).unwrap();
        assert_eq!(
            latest_snapshot_date(&conn).unwrap().as_deref(),
            Some("2026-10-29")
        );
        assert_eq!(
            find_snapshot_log(&conn, "2026-10-29")
                .unwrap()
                .unwrap()
                .last_seq,
            12
        );
    }

    #[test]
    fn the_footprint_counts_the_log_file() {
        let dir = tempfile::tempdir().unwrap();
        let db = database(dir.path());
        let live = Connection::open(&db).unwrap();
        live.execute_batch("INSERT INTO players VALUES ('P2', 'x')")
            .unwrap();
        assert!(database_bytes(&db) >= fs::metadata(&db).unwrap().len());
        assert_eq!(database_bytes(&dir.path().join("none.db")), 0);
    }
}
