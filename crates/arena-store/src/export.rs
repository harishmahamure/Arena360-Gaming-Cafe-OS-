//! The full-database export (Story 9.4): a `.a360` file in a folder the owner chose, for a copy
//! of the branch they hold themselves. The layout is documented in `arena_proto::backup_export`.
//!
//! The copy is made on a connection of its own with `VACUUM INTO`, which reads one consistent
//! snapshot of the file while `apply()` keeps committing to the live database (WAL), so the writer
//! is never blocked. The copy lives beside the database (the disk `VACUUM` works on anyway), is
//! stripped of what must not leave the Hub (licence cache, device and kitchen display token
//! hashes), compressed to a second scratch file while it is hashed, and only then written, header
//! first, to the target as `.<name>.part` and renamed. Any failure removes the part file and both
//! scratch files. `player_keys` stay as stored: the keys are wrapped by the branch KEK, which is
//! not in the database.

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use arena_proto::{AppError, EXPORT_FORMAT_VERSION, ErrorCode, ExportHeader};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use sha2::{Digest, Sha256};

use crate::db_error;

/// Level 1 keeps a 1 GB database inside the 10 s budget on the reference HDD PC.
const ZSTD_LEVEL: i32 = 1;
const TEMP_PREFIX: &str = "export-";
const TEMP_SUFFIX: &str = ".tmp";
/// Scratch files older than this belong to an export that died with the process.
const STALE_AFTER: Duration = Duration::from_secs(60 * 60);

/// A finished export file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedFile {
    /// Where the file is.
    pub path: PathBuf,
    /// Its name inside the folder.
    pub file_name: String,
    /// The size of the whole file, header included.
    pub bytes: u64,
    /// What the header says.
    pub header: ExportHeader,
}

fn internal(what: &str, e: &io::Error) -> AppError {
    AppError::new(ErrorCode::Internal, format!("could not {what}: {e}"))
}

fn folder_problem(reason: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, "folder", reason)
}

/// The reason the target could not be written, from what the folder is now.
fn target_problem(folder: &Path) -> AppError {
    let reason = match fs::metadata(folder) {
        Err(_) => "not_found",
        Ok(m) if !m.is_dir() => "not_dir",
        Ok(_) => "not_writable",
    };
    folder_problem(reason)
}

/// Deletes the files of one export when it ends, whichever way.
struct Cleanup(Vec<PathBuf>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}

/// Deletes scratch files that an export killed with the process left beside the database.
fn clear_stale(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with(TEMP_PREFIX) && name.ends_with(TEMP_SUFFIX)) {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age >= STALE_AFTER);
        if old {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// A writer that hashes what passes through it.
struct HashingWriter<W> {
    inner: W,
    hasher: Sha256,
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [name],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// What the header says about the copy.
struct Facts {
    branch_id: String,
    branch_code: String,
    epoch: String,
    last_seq: i64,
    schema_version: u32,
}

/// `VACUUM INTO` a scratch file, then strip it and read its facts.
fn copy_and_strip(db_path: &Path, scratch: &Path) -> Result<Facts, AppError> {
    let source = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(db_error)?;
    source
        .busy_timeout(Duration::from_secs(5))
        .map_err(db_error)?;
    source
        .execute("VACUUM INTO ?1", [scratch.to_string_lossy().as_ref()])
        .map_err(db_error)?;
    drop(source);

    let copy = Connection::open(scratch).map_err(db_error)?;
    // No `-wal` file beside the copy, and the pages of what is cleared are wiped and compacted
    // away rather than left in the free list.
    copy.execute_batch("PRAGMA journal_mode = DELETE; PRAGMA secure_delete = ON;")
        .map_err(db_error)?;
    if table_exists(&copy, "licence_cache")? {
        copy.execute_batch("DELETE FROM licence_cache;")
            .map_err(db_error)?;
    }
    if table_exists(&copy, "devices")? {
        copy.execute_batch("UPDATE devices SET token_hash = '';")
            .map_err(db_error)?;
    }
    if table_exists(&copy, "kitchen_displays")? {
        copy.execute_batch("UPDATE kitchen_displays SET token_hash = '';")
            .map_err(db_error)?;
    }
    copy.execute_batch("VACUUM;").map_err(db_error)?;

    let (epoch, last_seq) = if table_exists(&copy, "changes")? {
        copy.query_row(
            "SELECT epoch, seq FROM changes
             ORDER BY epoch_counter DESC, epoch DESC, seq DESC LIMIT 1",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(db_error)?
        .unwrap_or_default()
    } else {
        Default::default()
    };
    let (branch_id, branch_code) = if table_exists(&copy, "branch")? {
        copy.query_row(
            "SELECT branch_id, branch_code FROM branch LIMIT 1",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(db_error)?
        .unwrap_or_default()
    } else {
        Default::default()
    };
    let schema_version = crate::current_version(&copy)?;
    Ok(Facts {
        branch_id,
        branch_code,
        epoch,
        last_seq,
        schema_version,
    })
}

/// zstd-compresses `scratch` into `packed`, hashing the compressed bytes.
fn compress(scratch: &Path, packed: &Path) -> Result<(String, u64), AppError> {
    let mut input = File::open(scratch).map_err(|e| internal("open the export copy", &e))?;
    let output = File::create(packed).map_err(|e| internal("create the export body", &e))?;
    let mut hashing = HashingWriter {
        inner: BufWriter::with_capacity(1 << 20, output),
        hasher: Sha256::new(),
    };
    zstd::stream::copy_encode(&mut input, &mut hashing, ZSTD_LEVEL)
        .map_err(|e| internal("compress the export", &e))?;
    hashing
        .flush()
        .map_err(|e| internal("write the export body", &e))?;
    let HashingWriter { inner, hasher } = hashing;
    let file = inner
        .into_inner()
        .map_err(|e| internal("write the export body", e.error()))?;
    file.sync_all()
        .map_err(|e| internal("write the export body", &e))?;
    let size = file
        .metadata()
        .map_err(|e| internal("measure the export body", &e))?
        .len();
    Ok((hex(&hasher.finalize()), size))
}

/// Writes `header` and the compressed `body` to `part`.
fn write_part(part: &Path, header: &[u8], body: &Path) -> io::Result<()> {
    let mut out = BufWriter::with_capacity(1 << 20, File::create(part)?);
    out.write_all(header)?;
    io::copy(&mut File::open(body)?, &mut out)?;
    out.flush()?;
    out.into_inner()
        .map_err(io::IntoInnerError::into_error)?
        .sync_all()
}

/// Exports the database at `db_path` into `folder` as `file_name`: the copy, the strip, zstd, the
/// header, `.<file_name>.part`, then the rename. The folder must already be a directory the Hub
/// can write (`VALIDATION_FAILED` on `folder` with `not_found`, `not_dir` or `not_writable`
/// otherwise). The source is read on a connection of its own, so the live writer and the shared
/// reader are never held up. Blocking (disk).
pub fn write_export(
    db_path: &Path,
    folder: &Path,
    file_name: &str,
    exported_at: i64,
) -> Result<ExportedFile, AppError> {
    let scratch_dir = db_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    clear_stale(&scratch_dir);
    let id = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    );
    let scratch = scratch_dir.join(format!("{TEMP_PREFIX}{id}.db{TEMP_SUFFIX}"));
    let packed = scratch_dir.join(format!("{TEMP_PREFIX}{id}.zst{TEMP_SUFFIX}"));
    let part = folder.join(format!(".{file_name}.part"));
    let target = folder.join(file_name);
    // The scratch copy's own journal, if any, goes with it.
    let mut scratch_journal = scratch.clone().into_os_string();
    scratch_journal.push("-journal");
    let _cleanup = Cleanup(vec![
        scratch.clone(),
        PathBuf::from(scratch_journal),
        packed.clone(),
        part.clone(),
    ]);

    if !fs::metadata(folder).is_ok_and(|m| m.is_dir()) {
        return Err(target_problem(folder));
    }
    let facts = copy_and_strip(db_path, &scratch)?;
    let (sha256, body_bytes) = compress(&scratch, &packed)?;
    let _ = fs::remove_file(&scratch);

    let header = ExportHeader {
        format_version: EXPORT_FORMAT_VERSION,
        branch_id: facts.branch_id,
        branch_code: facts.branch_code,
        epoch: facts.epoch,
        last_seq: facts.last_seq,
        schema_version: facts.schema_version,
        exported_at,
        sha256,
    };
    let encoded = header.encode();
    let written = write_part(&part, &encoded, &packed).and_then(|()| fs::rename(&part, &target));
    if written.is_err() {
        return Err(target_problem(folder));
    }
    Ok(ExportedFile {
        path: target,
        file_name: file_name.to_owned(),
        bytes: encoded.len() as u64 + body_bytes,
        header,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    const SECRET: &str = "SECRET-TOKEN-HASH-$argon2id$v=19$m=19456";

    fn database(dir: &Path) -> PathBuf {
        let path = dir.join("branch.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(&format!(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE schema_version (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);
             INSERT INTO schema_version VALUES (1, 0), (84, 0);
             CREATE TABLE branch (branch_id TEXT PRIMARY KEY, branch_code TEXT NOT NULL);
             INSERT INTO branch VALUES ('01JBRANCH', 'LVL01');
             CREATE TABLE players (player_id TEXT PRIMARY KEY, name_enc TEXT NOT NULL);
             INSERT INTO players VALUES ('P1', 'ciphertext-of-a-name');
             CREATE TABLE player_keys (player_id TEXT PRIMARY KEY, dek_wrapped TEXT NOT NULL);
             INSERT INTO player_keys VALUES ('P1', 'WRAPPED-DEK-1'), ('P2', 'WRAPPED-DEK-2');
             CREATE TABLE licence_cache (id INTEGER PRIMARY KEY, jwt TEXT NOT NULL, received_at INTEGER NOT NULL);
             INSERT INTO licence_cache VALUES (1, 'LICENCE-JWT-VALUE', 5);
             CREATE TABLE devices (device_id TEXT PRIMARY KEY, label TEXT NOT NULL, token_hash TEXT NOT NULL);
             INSERT INTO devices VALUES ('D1', 'PC 1', '{SECRET}'), ('D2', 'PC 2', '{SECRET}');
             CREATE TABLE kitchen_displays (id TEXT PRIMARY KEY, name TEXT NOT NULL, token_hash TEXT NOT NULL);
             INSERT INTO kitchen_displays VALUES ('K1', 'Kitchen', '{SECRET}');
             CREATE TABLE changes (
                 epoch TEXT NOT NULL, epoch_counter INTEGER NOT NULL, seq INTEGER NOT NULL
             );
             INSERT INTO changes VALUES ('1:a', 1, 1), ('1:a', 1, 2), ('2:b', 2, 1), ('2:b', 2, 2);"
        ))
        .unwrap();
        path
    }

    /// The file's header (its body hash checked) and the database bytes inside it.
    fn unpack(file: &Path) -> (ExportHeader, Vec<u8>) {
        let bytes = fs::read(file).unwrap();
        let (header, offset) = ExportHeader::decode(&bytes).unwrap();
        let body = &bytes[offset..];
        assert_eq!(
            hex(&Sha256::digest(body)),
            header.sha256,
            "the hash is of the body"
        );
        (header, zstd::decode_all(body).unwrap())
    }

    /// The header and the database inside the file, opened at `at`.
    fn open(file: &Path, at: &Path) -> (ExportHeader, Connection) {
        let (header, plain) = unpack(file);
        let db = at.join("opened.db");
        fs::write(&db, &plain).unwrap();
        (header, Connection::open(&db).unwrap())
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_header_names_the_copy_and_the_body_is_a_database() {
        let dir = tempfile::tempdir().unwrap();
        let db = database(dir.path());
        let out = tempfile::tempdir().unwrap();
        let done = write_export(
            &db,
            out.path(),
            "LVL01-2026-10-30-1842.a360",
            1_800_000_000_000,
        )
        .unwrap();
        assert_eq!(done.path, out.path().join("LVL01-2026-10-30-1842.a360"));
        assert_eq!(done.bytes, fs::metadata(&done.path).unwrap().len());
        assert_eq!(names(out.path()), vec!["LVL01-2026-10-30-1842.a360"]);

        let (header, copy) = open(&done.path, out.path());
        assert_eq!(header, done.header);
        assert_eq!(
            (
                header.format_version,
                header.branch_id.as_str(),
                header.branch_code.as_str(),
                header.epoch.as_str(),
                header.last_seq,
                header.schema_version,
                header.exported_at,
            ),
            (1, "01JBRANCH", "LVL01", "2:b", 2, 84, 1_800_000_000_000)
        );
        let players: i64 = copy
            .query_row("SELECT COUNT(*) FROM players", [], |r| r.get(0))
            .unwrap();
        assert_eq!(players, 1);
        let integrity: String = copy
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .unwrap();
        assert_eq!(integrity, "ok");
    }

    #[test]
    fn player_keys_stay_and_licence_and_token_hashes_go() {
        let dir = tempfile::tempdir().unwrap();
        let db = database(dir.path());
        let out = tempfile::tempdir().unwrap();
        let done = write_export(&db, out.path(), "x.a360", 1).unwrap();
        let (_, copy) = open(&done.path, out.path());

        let keys = |c: &Connection| -> Vec<(String, String)> {
            c.prepare("SELECT player_id, dek_wrapped FROM player_keys ORDER BY player_id")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        let live = Connection::open(&db).unwrap();
        assert_eq!(keys(&copy), keys(&live), "the keys are as stored");
        assert_eq!(keys(&copy).len(), 2);

        let count =
            |c: &Connection, sql: &str| -> i64 { c.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(count(&copy, "SELECT COUNT(*) FROM licence_cache"), 0);
        assert_eq!(
            count(&copy, "SELECT COUNT(*) FROM devices WHERE token_hash <> ''"),
            0
        );
        assert_eq!(
            count(&copy, "SELECT COUNT(*) FROM devices"),
            2,
            "the rows stay"
        );
        assert_eq!(
            count(
                &copy,
                "SELECT COUNT(*) FROM kitchen_displays WHERE token_hash <> ''"
            ),
            0
        );
        let (_, raw) = unpack(&done.path);
        let text = String::from_utf8_lossy(&raw);
        assert!(!text.contains(SECRET), "no hash survives in free pages");
        assert!(!text.contains("LICENCE-JWT-VALUE"));

        // The live database is untouched.
        assert_eq!(count(&live, "SELECT COUNT(*) FROM licence_cache"), 1);
        assert_eq!(
            count(&live, "SELECT COUNT(*) FROM devices WHERE token_hash <> ''"),
            2
        );
    }

    #[test]
    fn no_scratch_file_stays_beside_the_database_or_in_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let db = database(dir.path());
        let before = names(dir.path());
        let out = tempfile::tempdir().unwrap();
        write_export(&db, out.path(), "x.a360", 1).unwrap();
        assert_eq!(names(out.path()), vec!["x.a360"]);
        // The live WAL files may come and go; no `export-` file is left.
        assert!(
            names(dir.path())
                .iter()
                .all(|n| !n.starts_with(TEMP_PREFIX)),
            "{:?} (before: {before:?})",
            names(dir.path())
        );
    }

    #[test]
    fn a_missing_folder_a_file_and_an_unwritable_target_fail_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let db = database(dir.path());
        let out = tempfile::tempdir().unwrap();
        let reason = |e: AppError| {
            assert_eq!(e.code, ErrorCode::ValidationFailed);
            assert_eq!(e.field.as_deref(), Some("folder"));
            e.message
        };

        let gone = out.path().join("removed-drive");
        assert_eq!(
            reason(write_export(&db, &gone, "x.a360", 1).unwrap_err()),
            "not_found"
        );

        let file = out.path().join("a-file");
        fs::write(&file, b"x").unwrap();
        assert_eq!(
            reason(write_export(&db, &file, "x.a360", 1).unwrap_err()),
            "not_dir"
        );

        // The target name is taken by a directory: the rename fails after the part file exists.
        let folder = out.path().join("usb");
        fs::create_dir(&folder).unwrap();
        fs::create_dir(folder.join("x.a360")).unwrap();
        assert_eq!(
            reason(write_export(&db, &folder, "x.a360", 1).unwrap_err()),
            "not_writable"
        );
        assert_eq!(names(&folder), vec!["x.a360"], "the part file is gone");
        assert!(
            names(dir.path())
                .iter()
                .all(|n| !n.starts_with(TEMP_PREFIX)),
            "the scratch files are gone: {:?}",
            names(dir.path())
        );
    }

    #[test]
    fn a_database_that_cannot_be_copied_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let missing = dir.path().join("none.db");
        let e = write_export(&missing, out.path(), "x.a360", 1).unwrap_err();
        assert_eq!(e.code, ErrorCode::Internal);
        assert!(names(out.path()).is_empty());
        assert!(names(dir.path()).is_empty());
    }

    #[test]
    fn a_stale_scratch_file_is_cleared_and_a_fresh_one_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let db = database(dir.path());
        let out = tempfile::tempdir().unwrap();
        let old = dir.path().join("export-1-1.db.tmp");
        let fresh = dir.path().join("export-2-2.db.tmp");
        fs::write(&old, b"half").unwrap();
        fs::write(&fresh, b"in use").unwrap();
        let file = File::options().write(true).open(&old).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(3 * 60 * 60))
            .unwrap();
        drop(file);
        write_export(&db, out.path(), "x.a360", 1).unwrap();
        assert!(!old.exists());
        assert!(
            fresh.exists(),
            "another export's scratch file is not touched"
        );
    }

    #[test]
    fn trading_during_an_export_is_never_blocked_and_the_header_is_the_copys_point() {
        let dir = tempfile::tempdir().unwrap();
        let db = database(dir.path());
        let out = tempfile::tempdir().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let writer = {
            let (db, stop) = (db.clone(), Arc::clone(&stop));
            std::thread::spawn(move || {
                let conn = Connection::open(&db).unwrap();
                conn.busy_timeout(Duration::from_secs(5)).unwrap();
                let mut seq = 10;
                let mut slowest = Duration::ZERO;
                while !stop.load(Ordering::Relaxed) {
                    let started = std::time::Instant::now();
                    conn.execute("INSERT INTO changes VALUES ('2:b', 2, ?1)", [seq])
                        .unwrap();
                    slowest = slowest.max(started.elapsed());
                    seq += 1;
                    std::thread::sleep(Duration::from_micros(200));
                }
                (seq - 10, slowest)
            })
        };
        let mut headers = Vec::new();
        for i in 0..5 {
            let name = format!("run-{i}.a360");
            let done = write_export(&db, out.path(), &name, 1).unwrap();
            let (header, copy) = open(&done.path, out.path());
            let last: i64 = copy
                .query_row(
                    "SELECT seq FROM changes ORDER BY epoch_counter DESC, epoch DESC, seq DESC LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(
                header.last_seq, last,
                "the header is the copy's last change"
            );
            headers.push(header.last_seq);
            drop(copy);
            std::fs::remove_file(out.path().join("opened.db")).unwrap();
        }
        stop.store(true, Ordering::Relaxed);
        let (written, slowest) = writer.join().unwrap();
        assert!(written > 0, "the writer kept committing");
        assert!(
            slowest < Duration::from_secs(2),
            "no commit waited for an export ({slowest:?})"
        );
        assert!(headers.windows(2).all(|w| w[0] <= w[1]), "{headers:?}");
    }

    /// Tap-to-file-closed for a 1 GB database (target < 10 s on the reference PC). Slow; run with
    /// `cargo test -p arena-store --release -- --ignored export_one_gigabyte --nocapture`.
    #[test]
    #[ignore = "builds a 1 GB database; records the export time"]
    fn export_one_gigabyte_benchmark() {
        let dir = tempfile::tempdir().unwrap();
        let db = database(dir.path());
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE bulk (id INTEGER PRIMARY KEY, payload BLOB NOT NULL);",
            )
            .unwrap();
            // 16,384 rows of 64 KiB of random bytes = 1 GiB, which does not compress.
            conn.execute_batch(
                "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 16384)
                 INSERT INTO bulk SELECT i, randomblob(65536) FROM n;",
            )
            .unwrap();
        }
        let out = tempfile::tempdir().unwrap();
        let started = std::time::Instant::now();
        let done = write_export(&db, out.path(), "bench.a360", 1).unwrap();
        let took = started.elapsed();
        println!(
            "export of {} MiB database: {:?}, file {} MiB",
            crate::database_bytes(&db) / (1 << 20),
            took,
            done.bytes / (1 << 20)
        );
    }
}
