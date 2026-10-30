//! Restoring a branch from a `.a360` export file as a new epoch (Story 9.7).
//!
//! A hot swap would have to rebuild the writer, the lease task and every reader that holds the
//! database open (and Windows cannot rename an open file), so the restore is done in two parts.
//! The Hub **stages** a checked file ([`stage_export`]) and stops. At the next start, before
//! [`Store::open`] serves, [`apply_pending`] builds the restored database beside the live one:
//! the body is decompressed to a scratch file, migrated forward by `Store::open`, every erasure
//! of the replaced database is applied again, and the lease is written as a new provisional epoch
//! forked at the file's `{epoch, last seq}`. Only then are the two files swapped (the replaced
//! database goes to `orphaned/<epoch>-<yyyymmddThhmmss>.db`, it is never deleted by a restore).
//! Anything that fails before the swap leaves the live database where it is, and the pending
//! files are removed.
//!
//! Layout inside the data directory (beside `branch.db`):
//! `restore-pending/restore.a360` and `restore-pending/restore.json` (the marker, written last),
//! `orphaned/`, and `restore-result.json` (what the Hub records as `ops.restore`, kept until it
//! has).

use std::fs::{self, File};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use arena_proto::{
    AppError, CloudRestoreFacts, ErrorCode, ExportHeader, ORPHAN_KEEP_DAYS, OpsRestore,
    PAIR_AGAIN_TOKEN, RESTORE_SYSTEM_ACTOR,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{BatchFork, LATEST_SCHEMA_VERSION, Store, db_error};

/// The folder of a staged restore, inside the data directory.
pub const PENDING_DIR: &str = "restore-pending";
const PENDING_FILE: &str = "restore.a360";
const PENDING_MARKER: &str = "restore.json";
/// The folder replaced databases are moved to, inside the data directory.
pub const ORPHANED_DIR: &str = "orphaned";
/// What a finished restore leaves for the Hub to record.
pub const RESULT_FILE: &str = "restore-result.json";
const WORK_FILE: &str = "restore-work.db";
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
const RENAME_TRIES: u32 = 40;
const RENAME_WAIT: Duration = Duration::from_millis(250);
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

fn data_dir(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

fn internal(what: &str, e: &dyn std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::Internal, format!("could not {what}: {e}"))
}

fn corrupt(why: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::RestoreCorrupt, why)
}

fn path_problem(reason: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, "path", reason)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// `<counter>` of an epoch text `<counter>:<instance>` (0 for an empty or malformed one).
fn epoch_counter(epoch: &str) -> i64 {
    epoch
        .split_once(':')
        .and_then(|(c, _)| c.parse::<i64>().ok())
        .unwrap_or(0)
}

/// What a checked file holds and what a restore of it would set aside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inspection {
    pub header: ExportHeader,
    /// The size of the whole file.
    pub file_bytes: u64,
    /// The branch's name, from the live database.
    pub branch_name: String,
    /// Change rows of the live database above the file's last `seq`.
    pub rows_set_aside: i64,
    /// The live database holds at least one erased player.
    pub has_tombstones: bool,
}

struct HashWriter(Sha256);

impl Write for HashWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn open_read_only(db_path: &Path) -> Result<Connection, AppError> {
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(db_error)?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(db_error)?;
    Ok(conn)
}

/// Checks `file` against the live database at `db_path`, in the order the story fixes: the file
/// itself (magic, header, size and sha256 of the body) is `RESTORE_CORRUPT`, another branch's
/// file is `RESTORE_WRONG_BRANCH`, a schema newer than this binary is `SCHEMA_TOO_NEW`. A file
/// that cannot be opened is `VALIDATION_FAILED` on `path`. Writes nothing. Blocking (disk).
pub fn inspect_export(db_path: &Path, file: &Path) -> Result<Inspection, AppError> {
    let meta = fs::metadata(file).map_err(|_| path_problem("not_found"))?;
    if !meta.is_file() {
        return Err(path_problem("not_file"));
    }
    let mut reader = BufReader::with_capacity(
        1 << 20,
        File::open(file).map_err(|_| path_problem("not_readable"))?,
    );
    let header = ExportHeader::read(&mut reader)
        .map_err(|e| corrupt(format!("this is not a readable export file ({e})")))?;
    let body_at = reader
        .stream_position()
        .map_err(|e| internal("read the export file", &e))?;
    let mut hasher = HashWriter(Sha256::new());
    io::copy(&mut reader, &mut hasher).map_err(|e| internal("read the export file", &e))?;
    if hex(&hasher.0.finalize()) != header.sha256.to_ascii_lowercase() {
        return Err(corrupt("the export file does not match its checksum"));
    }
    // The body must begin like a SQLite database once decompressed.
    reader
        .seek(SeekFrom::Start(body_at))
        .map_err(|e| internal("read the export file", &e))?;
    let mut magic = [0_u8; 16];
    zstd::stream::read::Decoder::new(reader)
        .and_then(|mut d| d.read_exact(&mut magic))
        .map_err(|_| corrupt("the export body is not a compressed database"))?;
    if &magic != SQLITE_MAGIC {
        return Err(corrupt("the export body is not a database"));
    }

    let live = open_read_only(db_path)?;
    let Some((branch_id, branch_name)) = live_branch(&live)? else {
        return Err(AppError::new(
            ErrorCode::RestoreWrongBranch,
            "this PC holds no branch to restore into",
        ));
    };
    if branch_id != header.branch_id {
        return Err(AppError::new(
            ErrorCode::RestoreWrongBranch,
            "this file belongs to another branch",
        ));
    }
    if header.schema_version > LATEST_SCHEMA_VERSION {
        return Err(AppError::new(
            ErrorCode::SchemaTooNew,
            format!(
                "the file's schema version {} is newer than this Hub's version {LATEST_SCHEMA_VERSION}",
                header.schema_version
            ),
        ));
    }
    Ok(Inspection {
        file_bytes: meta.len(),
        branch_name,
        rows_set_aside: rows_above(&live, &header.epoch, header.last_seq),
        has_tombstones: live
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM players WHERE erased_at IS NOT NULL)",
                [],
                |r| r.get(0),
            )
            .unwrap_or(false),
        header,
    })
}

/// The branch the live database holds or, on a replacement Hub waiting in "Restore this branch"
/// (Story 9.9) that has no branch rows yet, the branch it was paired to replace.
fn live_branch(live: &Connection) -> Result<Option<(String, String)>, AppError> {
    let held: Option<(String, String)> = live
        .query_row("SELECT branch_id, name FROM branch LIMIT 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()
        .map_err(db_error)?;
    if held.is_some() {
        return Ok(held);
    }
    live.query_row(
        "SELECT branch_id, COALESCE(restore_branch_name, '') FROM hub_identity
         WHERE id = 1 AND restore_pending = 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(db_error)
}

/// Change rows after `(epoch, seq)` in log order (epoch counter, epoch, seq).
fn rows_above(conn: &Connection, epoch: &str, seq: i64) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM changes WHERE (epoch_counter, epoch, seq) > (?1, ?2, ?3)",
        params![epoch_counter(epoch), epoch, seq],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

/// What the marker of a staged restore holds.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Marker {
    /// The Owner who confirmed (staff id).
    confirmed_by: String,
    /// The file's name as the Owner gave it.
    file_name: String,
    /// Set when the file was rebuilt from the cloud backup (Story 9.13).
    #[serde(default)]
    cloud: Option<CloudRestoreFacts>,
}

/// What a finished restore leaves for the Hub to record as `ops.restore`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreOutcome {
    /// The Owner who confirmed (staff id); the actor of the record when they are still in the
    /// restored database, else [`RESTORE_SYSTEM_ACTOR`] records it.
    pub confirmed_by: String,
    /// Fixed so that recording the fact twice (a crash in between) applies it once.
    pub intent_id: String,
    pub idempotency_key: String,
    pub restore: OpsRestore,
}

fn clear_dir(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

/// Stages `file` for the next start: checks it again ([`inspect_export`]), copies it into
/// `restore-pending/` and writes the marker last, so a marker always stands for a whole file. A
/// refused file stages nothing. `confirmed_by` is the Owner's staff id. Blocking (disk).
pub fn stage_export(
    db_path: &Path,
    file: &Path,
    confirmed_by: &str,
) -> Result<Inspection, AppError> {
    let inspection = inspect_export(db_path, file)?;
    let dir = data_dir(db_path).join(PENDING_DIR);
    // A file already in the pending folder would be deleted before it is copied.
    if let (Ok(src), Ok(pending)) = (file.canonicalize(), dir.canonicalize())
        && src.starts_with(&pending)
    {
        return Err(path_problem("not_readable"));
    }
    clear_dir(&dir);
    fs::create_dir_all(&dir).map_err(|e| internal("create the restore folder", &e))?;
    let staged = (|| -> Result<(), AppError> {
        let part = dir.join(format!("{PENDING_FILE}.part"));
        fs::copy(file, &part).map_err(|e| internal("copy the export file", &e))?;
        fs::OpenOptions::new()
            .write(true)
            .open(&part)
            .and_then(|f| f.sync_all())
            .map_err(|e| internal("copy the export file", &e))?;
        fs::rename(&part, dir.join(PENDING_FILE))
            .map_err(|e| internal("copy the export file", &e))?;
        let marker = Marker {
            confirmed_by: confirmed_by.to_owned(),
            file_name: file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            cloud: None,
        };
        write_marker(&dir, &marker)
    })();
    if let Err(e) = staged {
        clear_dir(&dir);
        return Err(e);
    }
    Ok(inspection)
}

fn write_marker(dir: &Path, marker: &Marker) -> Result<(), AppError> {
    let json = serde_json::to_vec(marker).map_err(|e| internal("write the marker", &e))?;
    let tmp = dir.join(format!("{PENDING_MARKER}.part"));
    fs::write(&tmp, json).map_err(|e| internal("write the marker", &e))?;
    fs::rename(&tmp, dir.join(PENDING_MARKER)).map_err(|e| internal("write the marker", &e))
}

/// The side table a rebuild from the cloud backup leaves in the database it hands over: the
/// paired devices and kitchen displays of the old Hub (`tbl` is the table, `id` the row). The
/// export strips their token hashes, which would make them look like devices added by hand;
/// [`apply_pending`] marks these [`PAIR_AGAIN_TOKEN`] instead and drops the table.
pub const PAIR_AGAIN_TABLE: &str = "restore_pair_again";

/// Stages the database `rebuilt` from the cloud backup (Story 9.13) for the next start, exactly as
/// a checked `.a360` file is staged: it is exported (stripped of the licence cache and token
/// hashes) into `restore-pending/`, checked against the branch this Hub waits to restore, and the
/// marker is written last. `rebuilt` must be closed. A refusal stages nothing. Blocking (disk).
pub fn stage_rebuilt(
    db_path: &Path,
    rebuilt: &Path,
    cloud: CloudRestoreFacts,
    now_ms: i64,
) -> Result<Inspection, AppError> {
    let dir = data_dir(db_path).join(PENDING_DIR);
    clear_dir(&dir);
    fs::create_dir_all(&dir).map_err(|e| internal("create the restore folder", &e))?;
    let staged = (|| -> Result<Inspection, AppError> {
        crate::export::write_export(rebuilt, &dir, PENDING_FILE, now_ms)?;
        let inspection = inspect_export(db_path, &dir.join(PENDING_FILE))?;
        write_marker(
            &dir,
            &Marker {
                confirmed_by: RESTORE_SYSTEM_ACTOR.to_owned(),
                file_name: "cloud-backup".to_owned(),
                cloud: Some(cloud),
            },
        )?;
        Ok(inspection)
    })();
    if staged.is_err() {
        clear_dir(&dir);
    }
    staged
}

/// Whether a restore is staged (its marker exists).
pub fn has_pending(db_path: &Path) -> bool {
    data_dir(db_path)
        .join(PENDING_DIR)
        .join(PENDING_MARKER)
        .is_file()
}

/// The facts of the live database a restore needs, read before anything moves.
struct Replaced {
    instance_id: Option<String>,
    tls_fingerprint: Option<String>,
    lease_counter: i64,
    max_seen: i64,
    /// `lease` epoch text, or empty.
    epoch: String,
    erased: Vec<(String, i64)>,
    identity: Option<(String, String, String, String, i64)>,
    licence: Option<(String, i64)>,
    device_tokens: Vec<(String, String)>,
    display_tokens: Vec<(String, String)>,
}

fn read_replaced(db_path: &Path) -> Result<Replaced, AppError> {
    let conn = open_read_only(db_path)?;
    let meta = |key: &str| -> Option<String> {
        conn.query_row("SELECT value FROM hub_meta WHERE key = ?1", [key], |r| {
            r.get::<_, Option<String>>(0)
        })
        .optional()
        .ok()
        .flatten()
        .flatten()
    };
    let lease = crate::read_lease(&conn).ok().flatten();
    let max_seen = crate::read_max_seen_counter(&conn).unwrap_or(0);
    let pairs = |sql: &str| -> Vec<(String, String)> {
        conn.prepare(sql)
            .and_then(|mut s| {
                s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<Result<Vec<_>, _>>()
            })
            .unwrap_or_default()
    };
    let erased = conn
        .prepare("SELECT player_id, erased_at FROM players WHERE erased_at IS NOT NULL")
        .and_then(|mut s| {
            s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(db_error)?;
    Ok(Replaced {
        instance_id: meta(crate::META_HUB_INSTANCE_ID),
        tls_fingerprint: meta(crate::META_TLS_FINGERPRINT),
        lease_counter: lease.as_ref().map_or(0, |l| l.counter),
        max_seen,
        epoch: lease
            .as_ref()
            .map(|l| format!("{}:{}", l.counter, l.hub_instance_id))
            .unwrap_or_default(),
        erased,
        identity: conn
            .query_row(
                "SELECT tenant_id, branch_id, hub_id, tier, paired_at FROM hub_identity WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()
            .ok()
            .flatten(),
        licence: conn
            .query_row(
                "SELECT jwt, received_at FROM licence_cache WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .ok()
            .flatten(),
        device_tokens: pairs("SELECT device_id, token_hash FROM devices WHERE token_hash <> ''"),
        display_tokens: pairs("SELECT id, token_hash FROM kitchen_displays WHERE token_hash <> ''"),
    })
}

/// `yyyymmddThhmmss` (UTC) of `ms`.
fn stamp(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// The ms of a `yyyymmddThhmmss` stamp (UTC), if it is one.
fn unstamp(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 15 || b[8] != b'T' {
        return None;
    }
    let num = |a: usize, z: usize| s.get(a..z)?.parse::<i64>().ok();
    let (y, m, d) = (num(0, 4)?, num(4, 6)?, num(6, 8)?);
    let (hh, mm, ss) = (num(9, 11)?, num(11, 13)?, num(13, 15)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 59 {
        return None;
    }
    let y = y - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((days * 86_400 + hh * 3600 + mm * 60 + ss) * 1000)
}

/// `<old epoch>-<stamp>.db`, the epoch with anything a file name cannot hold turned into `-`.
fn orphan_name(old_epoch: &str, at: i64) -> String {
    let epoch: String = if old_epoch.is_empty() {
        "none".to_owned()
    } else {
        old_epoch
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect()
    };
    format!("{epoch}-{}.db", stamp(at))
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Renames, retrying for a while: on Windows a file another process (or an antivirus scan) has
/// just closed can stay locked for a moment.
fn rename_retrying(from: &Path, to: &Path) -> io::Result<()> {
    let mut last = None;
    for _ in 0..RENAME_TRIES {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(e),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(RENAME_WAIT);
            }
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("rename failed")))
}

fn remove_scratch(work: &Path) {
    for suffix in ["", "-wal", "-shm", "-journal", ".lock"] {
        let _ = fs::remove_file(with_suffix(work, suffix));
    }
}

/// Folds the WAL of `path` into the file and removes it.
fn checkpoint(path: &Path) -> Result<(), AppError> {
    let conn = Connection::open(path).map_err(db_error)?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(db_error)?;
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .map_err(db_error)?;
    Ok(())
}

/// Writes the restored lease and the other changes made in the one write transaction.
struct Built {
    new_epoch: String,
    erasures_reapplied: i64,
}

fn build_restored(
    work: &Path,
    now_ms: i64,
    inspection: &Inspection,
    old: &Replaced,
    cloud: bool,
) -> Result<Built, AppError> {
    let mut store = Store::open(work, now_ms)?;
    if !store.holds_writer_lock() {
        return Err(AppError::new(
            ErrorCode::Internal,
            "the restored database could not be locked for writing",
        ));
    }
    let header = &inspection.header;
    store.write_tx(|tx| {
        // The file must be of this branch (the header is not covered by the body hash).
        let id: Option<String> = tx
            .query_row("SELECT branch_id FROM branch LIMIT 1", [], |r| r.get(0))
            .optional()
            .map_err(db_error)?;
        if id.as_deref() != Some(header.branch_id.as_str()) {
            return Err(AppError::new(
                ErrorCode::RestoreWrongBranch,
                "this file belongs to another branch",
            ));
        }

        // Every erasure of the replaced database holds in the restored one.
        let mut reapplied = 0_i64;
        for (player_id, erased_at) in &old.erased {
            let in_file: Option<bool> = tx
                .query_row(
                    "SELECT erased_at IS NOT NULL FROM players WHERE player_id = ?1",
                    [player_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_error)?;
            if in_file == Some(false) {
                crate::erase_player(tx, player_id, *erased_at)?;
                crate::delete_player_key(tx, player_id)?;
                reapplied += 1;
            }
        }

        // The new epoch is above everything either database has seen.
        let file_lease = crate::read_lease(tx)?;
        let counter = [
            epoch_counter(&header.epoch),
            old.lease_counter,
            old.max_seen,
            file_lease.as_ref().map_or(0, |l| l.counter),
            crate::read_max_seen_counter(tx)?,
        ]
        .into_iter()
        .max()
        .unwrap_or(0)
            + 1;
        let own = old
            .instance_id
            .clone()
            .or_else(|| {
                crate::get_meta(tx, crate::META_HUB_INSTANCE_ID)
                    .ok()
                    .flatten()
            })
            .ok_or_else(|| AppError::new(ErrorCode::Internal, "this Hub has no instance id"))?;
        crate::set_meta(tx, crate::META_HUB_INSTANCE_ID, &own)?;
        if let Some(fp) = &old.tls_fingerprint {
            crate::set_meta(tx, crate::META_TLS_FINGERPRINT, fp)?;
        }
        crate::write_lease(
            tx,
            &crate::LeaseRow {
                counter,
                hub_instance_id: own.clone(),
                provisional: true,
                renewed_at: now_ms,
                expires_at: now_ms,
            },
        )?;
        let fork = (!header.epoch.is_empty()).then(|| BatchFork {
            parent_epoch: header.epoch.clone(),
            parent_seq: header.last_seq,
        });
        crate::write_lease_fork(tx, fork.as_ref())?;
        crate::raise_max_seen_counter(tx, counter.max(old.max_seen))?;

        // A rebuild from the cloud backup names the devices that were paired with the old Hub:
        // their tokens were stripped, and they pair again (Story 9.13).
        if cloud {
            pair_again(tx)?;
        }

        // What belongs to this Hub and not to the branch is not in an export: its pairing, its
        // licence and the device tokens, so paired kiosks keep working.
        if let Some((tenant, branch, hub, tier, paired_at)) = &old.identity
            && branch == &header.branch_id
        {
            tx.execute(
                "INSERT INTO hub_identity (id, tenant_id, branch_id, hub_id, tier, paired_at)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (id) DO UPDATE SET tenant_id = excluded.tenant_id,
                   branch_id = excluded.branch_id, hub_id = excluded.hub_id,
                   tier = excluded.tier, paired_at = excluded.paired_at",
                params![tenant, branch, hub, tier, paired_at],
            )
            .map_err(db_error)?;
        }
        if let Some((jwt, received_at)) = &old.licence {
            crate::store_licence(tx, jwt, *received_at)?;
        }
        for (device_id, hash) in &old.device_tokens {
            tx.execute(
                "UPDATE devices SET token_hash = ?2 WHERE device_id = ?1 AND token_hash = ''",
                params![device_id, hash],
            )
            .map_err(db_error)?;
        }
        for (id, hash) in &old.display_tokens {
            tx.execute(
                "UPDATE kitchen_displays SET token_hash = ?2 WHERE id = ?1 AND token_hash = ''",
                params![id, hash],
            )
            .map_err(db_error)?;
        }
        Ok(Built {
            new_epoch: format!("{counter}:{own}"),
            erasures_reapplied: reapplied,
        })
    })
}

/// Marks the devices and kitchen displays listed in [`PAIR_AGAIN_TABLE`] [`PAIR_AGAIN_TOKEN`] and
/// drops the table. A file without the table (an ordinary export) changes nothing.
fn pair_again(tx: &crate::StoreTx<'_>) -> Result<(), AppError> {
    let listed: bool = tx
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [PAIR_AGAIN_TABLE],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if !listed {
        return Ok(());
    }
    tx.execute(
        &format!(
            "UPDATE devices SET token_hash = ?1 WHERE token_hash = ''
               AND device_id IN (SELECT id FROM {PAIR_AGAIN_TABLE} WHERE tbl = 'devices')"
        ),
        [PAIR_AGAIN_TOKEN],
    )
    .map_err(db_error)?;
    tx.execute(
        &format!(
            "UPDATE kitchen_displays SET token_hash = ?1 WHERE token_hash = ''
               AND id IN (SELECT id FROM {PAIR_AGAIN_TABLE} WHERE tbl = 'kitchen_displays')"
        ),
        [PAIR_AGAIN_TOKEN],
    )
    .map_err(db_error)?;
    tx.execute_batch(&format!("DROP TABLE {PAIR_AGAIN_TABLE};"))
        .map_err(db_error)?;
    Ok(())
}

/// Decompresses the body of `staged` (the file whose header is `inspection.header`) to `work`.
fn unpack(staged: &Path, work: &Path) -> Result<(), AppError> {
    let mut reader = BufReader::with_capacity(
        1 << 20,
        File::open(staged).map_err(|e| internal("open the staged file", &e))?,
    );
    ExportHeader::read(&mut reader).map_err(|e| corrupt(e.to_string()))?;
    let mut out = io::BufWriter::with_capacity(
        1 << 20,
        File::create(work).map_err(|e| internal("create the restored database", &e))?,
    );
    zstd::stream::copy_decode(&mut reader, &mut out)
        .map_err(|e| corrupt(format!("the export body could not be unpacked ({e})")))?;
    out.flush()
        .map_err(|e| internal("write the restored database", &e))?;
    out.into_inner()
        .map_err(|e| internal("write the restored database", e.error()))?
        .sync_all()
        .map_err(|e| internal("write the restored database", &e))
}

/// Moves the live database to `orphaned/` and the restored one into its place.
fn swap(db_path: &Path, work: &Path, orphan: &Path) -> Result<(), AppError> {
    checkpoint(db_path)?;
    if let Some(dir) = orphan.parent() {
        fs::create_dir_all(dir).map_err(|e| internal("create the orphaned folder", &e))?;
    }
    rename_retrying(db_path, orphan).map_err(|e| internal("move the old database aside", &e))?;
    for suffix in ["-wal", "-shm"] {
        let from = with_suffix(db_path, suffix);
        if from.exists() {
            let _ = rename_retrying(&from, &with_suffix(orphan, suffix));
        }
    }
    if let Err(e) = rename_retrying(work, db_path) {
        // Put the old database back; the restore did not happen.
        let _ = rename_retrying(orphan, db_path);
        for suffix in ["-wal", "-shm"] {
            let from = with_suffix(orphan, suffix);
            if from.exists() {
                let _ = rename_retrying(&from, &with_suffix(db_path, suffix));
            }
        }
        return Err(internal("move the restored database in", &e));
    }
    Ok(())
}

fn ulid_like(now_ms: i64, epoch: &str) -> String {
    // A fixed id per restore: the time and the new epoch, so the same restore is the same intent.
    let mut h = Sha256::new();
    h.update(now_ms.to_be_bytes());
    h.update(epoch.as_bytes());
    let mut id = hex(&h.finalize())[..26].to_ascii_uppercase();
    // A ULID's first character is at most `7` (its 48-bit time field must not overflow).
    let first = id.as_bytes()[0];
    let first = u8::try_from(char::from(first).to_digit(16).unwrap_or(0) & 7).unwrap_or(0);
    id.replace_range(..1, &first.to_string());
    id
}

/// Applies a staged restore, if there is one, before the Hub opens its database. `Ok(None)` when
/// nothing is staged. On `Ok(Some(..))` the restored database is in place and `outcome` is also
/// written to `restore-result.json` for [`read_outcome`]. On `Err` nothing was replaced (the live
/// database is where it was) and the pending files are gone, so the Hub starts normally.
/// Blocking (disk).
pub fn apply_pending(db_path: &Path, now_ms: i64) -> Result<Option<RestoreOutcome>, AppError> {
    let dir = data_dir(db_path);
    let pending = dir.join(PENDING_DIR);
    let Ok(marker_bytes) = fs::read(pending.join(PENDING_MARKER)) else {
        // No marker: a half-staged restore is not a restore.
        if pending.exists() {
            clear_dir(&pending);
        }
        return Ok(None);
    };
    let work = dir.join(WORK_FILE);
    let result = restore_from(db_path, &dir, &pending, &work, &marker_bytes, now_ms);
    remove_scratch(&work);
    clear_dir(&pending);
    result.map(Some)
}

fn restore_from(
    db_path: &Path,
    dir: &Path,
    pending: &Path,
    work: &Path,
    marker_bytes: &[u8],
    now_ms: i64,
) -> Result<RestoreOutcome, AppError> {
    let marker: Marker = serde_json::from_slice(marker_bytes)
        .map_err(|_| corrupt("the staged restore is unreadable"))?;
    let staged = pending.join(PENDING_FILE);
    let inspection = inspect_export(db_path, &staged)?;
    let old = read_replaced(db_path)?;

    remove_scratch(work);
    unpack(&staged, work)?;
    let built = build_restored(work, now_ms, &inspection, &old, marker.cloud.is_some())?;
    checkpoint(work)?;
    for suffix in ["-wal", "-shm", ".lock"] {
        let _ = fs::remove_file(with_suffix(work, suffix));
    }

    let orphan_file = orphan_name(&old.epoch, now_ms);
    swap(db_path, work, &dir.join(ORPHANED_DIR).join(&orphan_file))?;

    let outcome = RestoreOutcome {
        confirmed_by: marker.confirmed_by,
        intent_id: ulid_like(now_ms, &built.new_epoch),
        idempotency_key: ulid_like(now_ms ^ 0x5a5a, &built.new_epoch),
        restore: OpsRestore {
            file_name: plain_name(&marker.file_name),
            header: inspection.header,
            old_epoch: old.epoch,
            new_epoch: built.new_epoch,
            rows_set_aside: inspection.rows_set_aside,
            erasures_reapplied: built.erasures_reapplied,
            orphaned_file: orphan_file,
            restored_at: now_ms,
            by: None,
            cloud: marker.cloud,
        },
    };
    if let Ok(json) = serde_json::to_vec(&outcome) {
        let tmp = dir.join(format!("{RESULT_FILE}.part"));
        if fs::write(&tmp, json).is_ok() {
            let _ = fs::rename(&tmp, dir.join(RESULT_FILE));
        }
    }
    Ok(outcome)
}

/// A file name the audit record can hold: no path, no control characters, at most 100 characters.
fn plain_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !c.is_control() && *c != '/' && *c != '\\')
        .take(100)
        .collect();
    if cleaned.is_empty() {
        "restore.a360".to_owned()
    } else {
        cleaned
    }
}

/// The outcome of a finished restore the Hub has not recorded yet.
pub fn read_outcome(db_path: &Path) -> Option<RestoreOutcome> {
    let bytes = fs::read(data_dir(db_path).join(RESULT_FILE)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Forgets the outcome once `ops.restore` is recorded.
pub fn clear_outcome(db_path: &Path) {
    let _ = fs::remove_file(data_dir(db_path).join(RESULT_FILE));
}

/// Deletes databases in `orphaned/` that are older than [`ORPHAN_KEEP_DAYS`] days at `now_ms`
/// (the age is the stamp in the name, else the file's modified time). Returns how many files went.
/// Nothing else in the data directory is touched. Blocking (disk).
pub fn sweep_orphaned(db_path: &Path, now_ms: i64) -> usize {
    let dir = data_dir(db_path).join(ORPHANED_DIR);
    let Ok(entries) = fs::read_dir(&dir) else {
        return 0;
    };
    let limit = i64::try_from(ORPHAN_KEEP_DAYS).unwrap_or(90) * DAY_MS;
    let mut removed = 0;
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(base) = ["-wal", "-shm"]
            .iter()
            .find_map(|s| name.strip_suffix(s))
            .or(Some(name.as_str()))
            .and_then(|n| n.strip_suffix(".db"))
        else {
            continue;
        };
        let made = base
            .get(base.len().saturating_sub(15)..)
            .and_then(unstamp)
            .or_else(|| {
                entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .and_then(|d| i64::try_from(d.as_millis()).ok())
            });
        if made.is_some_and(|at| now_ms - at > limit) && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{export::write_export, set_meta};
    use arena_proto::EXPORT_FORMAT_VERSION;

    const T: i64 = 1_800_000_000_000;
    const BRANCH: &str = "01JBRANCH";

    /// A migrated database for the branch with a lease, an instance id and some players.
    fn live(dir: &Path, counter: i64, instance: &str) -> PathBuf {
        let path = dir.join("branch.db");
        let mut store = Store::open(&path, T).unwrap();
        store
            .write_tx(|tx| {
                tx.execute(
                    "INSERT INTO branch (branch_id, name, branch_code, currency, tz, day_start_min,
                                         fy_start_month, rev, created_at, updated_at)
                     VALUES (?1, 'Level Up', 'LVL01', 'INR', 'Asia/Kolkata', 240, 4, 1, 0, 0)",
                    [BRANCH],
                )
                .map_err(db_error)?;
                set_meta(tx, crate::META_HUB_INSTANCE_ID, instance)?;
                crate::write_lease(
                    tx,
                    &crate::LeaseRow {
                        counter,
                        hub_instance_id: instance.to_owned(),
                        provisional: false,
                        renewed_at: T,
                        expires_at: T + 1000,
                    },
                )?;
                for p in ["P1", "P2", "P3"] {
                    tx.execute(
                        "INSERT INTO players (player_id, name_enc, phone_enc, username, password_hash,
                                              phone_lookup, tags, rev, created_at, updated_at)
                         VALUES (?1, 'enc-name', 'enc-phone', 'user-' || lower(?1), 'hash', ?1, '[]', 1, 0, 0)",
                        [p],
                    )
                    .map_err(db_error)?;
                    tx.execute(
                        "INSERT INTO player_keys (player_id, dek_wrapped) VALUES (?1, 'wrapped')",
                        [p],
                    )
                    .map_err(db_error)?;
                }
                Ok(())
            })
            .unwrap();
        path
    }

    fn add_changes(db: &Path, epoch: &str, from: i64, to: i64) {
        let conn = Connection::open(db).unwrap();
        for seq in from..=to {
            conn.execute(
                "INSERT INTO changes (epoch, seq, tbl, row_id, op, row, intent_id,
                                      intent_kind, actor_kind, actor_id, at)
                 VALUES (?1, ?2, 't', 'r', 'insert', x'00', 'i', 'k', 'system', 'a', 0)",
                params![epoch, seq],
            )
            .unwrap();
        }
    }

    fn export(db: &Path, out: &Path, name: &str) -> PathBuf {
        let done = write_export(db, out, name, T - 5000).unwrap();
        done.path
    }

    fn count(db: &Path, sql: &str) -> i64 {
        Connection::open(db)
            .unwrap()
            .query_row(sql, [], |r| r.get(0))
            .unwrap()
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir)
            .map(|d| {
                d.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }

    /// A branch with 3 changes in epoch `2:HUB`, exported, then more trading and an erasure.
    struct Branch {
        _dir: tempfile::TempDir,
        out: tempfile::TempDir,
        db: PathBuf,
        file: PathBuf,
    }

    fn branch() -> Branch {
        let dir = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let db = live(dir.path(), 2, "HUB");
        add_changes(&db, "2:HUB", 1, 3);
        let file = export(&db, out.path(), "LVL01-2027-01-15-1330.a360");
        add_changes(&db, "2:HUB", 4, 8);
        Branch {
            _dir: dir,
            out,
            db,
            file,
        }
    }

    fn rewrite_header(file: &Path, edit: impl FnOnce(&mut ExportHeader)) {
        let bytes = fs::read(file).unwrap();
        let (mut header, at) = ExportHeader::decode(&bytes).unwrap();
        edit(&mut header);
        let mut out = header.encode();
        out.extend_from_slice(&bytes[at..]);
        fs::write(file, out).unwrap();
    }

    #[test]
    fn a_good_file_is_described_and_what_would_be_set_aside_is_counted() {
        let b = branch();
        let i = inspect_export(&b.db, &b.file).unwrap();
        assert_eq!(i.branch_name, "Level Up");
        assert_eq!(i.header.branch_id, BRANCH);
        assert_eq!((i.header.epoch.as_str(), i.header.last_seq), ("2:HUB", 3));
        assert_eq!(i.rows_set_aside, 5, "seq 4..=8 are newer than the file");
        assert!(!i.has_tombstones);
        assert_eq!(i.file_bytes, fs::metadata(&b.file).unwrap().len());
    }

    #[test]
    fn a_damaged_file_is_corrupt_and_nothing_is_staged() {
        let b = branch();
        let before = names(b._dir.path());
        let check = |path: &Path| {
            let e = stage_export(&b.db, path, "owner").unwrap_err();
            assert_eq!(e.code, ErrorCode::RestoreCorrupt, "{e:?}");
        };
        // Bit rot in the body.
        let bad = b.out.path().join("rot.a360");
        let mut bytes = fs::read(&b.file).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        fs::write(&bad, &bytes).unwrap();
        check(&bad);
        // Truncated.
        let short = b.out.path().join("short.a360");
        fs::write(&short, &fs::read(&b.file).unwrap()[..40]).unwrap();
        check(&short);
        // Not an export at all.
        let other = b.out.path().join("other.a360");
        fs::write(&other, b"SQLite format 3\0 and then some").unwrap();
        check(&other);
        // A header whose hash is wrong.
        let wrong = b.out.path().join("wrong.a360");
        fs::copy(&b.file, &wrong).unwrap();
        rewrite_header(&wrong, |h| h.sha256 = "00".repeat(32));
        check(&wrong);
        assert_eq!(names(b._dir.path()), before, "the data folder is untouched");
        assert!(!has_pending(&b.db));
    }

    #[test]
    fn corruption_is_reported_before_the_branch_and_the_schema() {
        let b = branch();
        let mixed = b.out.path().join("mixed.a360");
        fs::copy(&b.file, &mixed).unwrap();
        rewrite_header(&mixed, |h| {
            h.sha256 = "00".repeat(32);
            h.branch_id = "OTHER".into();
            h.schema_version = LATEST_SCHEMA_VERSION + 5;
        });
        assert_eq!(
            inspect_export(&b.db, &mixed).unwrap_err().code,
            ErrorCode::RestoreCorrupt
        );
    }

    #[test]
    fn another_branchs_file_and_a_newer_schema_are_refused() {
        let b = branch();
        // The header is not hashed, so a re-labelled file is "good" except for the branch.
        let other = b.out.path().join("other-branch.a360");
        fs::copy(&b.file, &other).unwrap();
        rewrite_header(&other, |h| h.branch_id = "01JOTHER".into());
        let e = inspect_export(&b.db, &other).unwrap_err();
        assert_eq!(e.code, ErrorCode::RestoreWrongBranch);
        assert_eq!(
            stage_export(&b.db, &other, "o").unwrap_err().code,
            ErrorCode::RestoreWrongBranch
        );

        let newer = b.out.path().join("newer.a360");
        fs::copy(&b.file, &newer).unwrap();
        rewrite_header(&newer, |h| h.schema_version = LATEST_SCHEMA_VERSION + 1);
        assert_eq!(
            inspect_export(&b.db, &newer).unwrap_err().code,
            ErrorCode::SchemaTooNew
        );
        // An older schema is accepted.
        let older = b.out.path().join("older.a360");
        fs::copy(&b.file, &older).unwrap();
        rewrite_header(&older, |h| h.schema_version = 40);
        assert!(inspect_export(&b.db, &older).is_ok());
        assert!(!has_pending(&b.db));
    }

    #[test]
    fn a_missing_path_is_a_path_problem_not_a_corrupt_file() {
        let b = branch();
        let e = inspect_export(&b.db, &b.out.path().join("nope.a360")).unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref(), e.message.as_str()),
            (ErrorCode::ValidationFailed, Some("path"), "not_found")
        );
        let e = inspect_export(&b.db, b.out.path()).unwrap_err();
        assert_eq!(e.message, "not_file");
    }

    #[test]
    fn staging_copies_the_file_and_writes_the_marker_last() {
        let b = branch();
        stage_export(&b.db, &b.file, "owner-1").unwrap();
        assert!(has_pending(&b.db));
        let pending = data_dir(&b.db).join(PENDING_DIR);
        assert_eq!(names(&pending), vec!["restore.a360", "restore.json"]);
        assert_eq!(
            fs::read(pending.join("restore.a360")).unwrap(),
            fs::read(&b.file).unwrap()
        );
        // The live database is untouched until the next start.
        assert_eq!(count(&b.db, "SELECT COUNT(*) FROM changes"), 8);
    }

    #[test]
    fn the_ids_of_a_restore_are_always_valid_ulids() {
        for now in 0..400 {
            let id = ulid_like(now, "2:HUBA");
            assert_eq!(id.len(), 26);
            assert!(id.as_bytes()[0] <= b'7', "{id}");
            assert!(
                id.bytes()
                    .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b)),
                "{id}"
            );
        }
    }

    #[test]
    fn the_staged_file_itself_cannot_be_staged_again() {
        let b = branch();
        stage_export(&b.db, &b.file, "owner-1").unwrap();
        let staged = data_dir(&b.db).join(PENDING_DIR).join("restore.a360");
        let e = stage_export(&b.db, &staged, "owner-1").unwrap_err();
        assert_eq!(e.code, ErrorCode::ValidationFailed);
        // The earlier staging survives the refusal.
        assert!(has_pending(&b.db));
        assert!(staged.is_file());
    }

    #[test]
    fn nothing_staged_applies_nothing() {
        let b = branch();
        assert_eq!(apply_pending(&b.db, T).unwrap(), None);
        // A folder without a marker is a half-staged restore and is cleared.
        let pending = data_dir(&b.db).join(PENDING_DIR);
        fs::create_dir_all(&pending).unwrap();
        fs::write(pending.join("restore.a360"), b"half").unwrap();
        assert_eq!(apply_pending(&b.db, T).unwrap(), None);
        assert!(!pending.exists());
    }

    #[test]
    fn the_swap_orphans_the_old_database_and_opens_a_new_provisional_epoch_forked_at_the_file() {
        let b = branch();
        // The replaced database moved on to epoch 4 (a promote it knew about) and saw 6 as well.
        {
            let conn = Connection::open(&b.db).unwrap();
            conn.execute("UPDATE lease SET counter = 4, max_seen_counter = 6", [])
                .unwrap();
        }
        stage_export(&b.db, &b.file, "owner-1").unwrap();
        let out = apply_pending(&b.db, T + 1000).unwrap().unwrap();

        // The new epoch is above the file (2), the replaced lease (4) and the highest seen (6).
        assert_eq!(out.restore.new_epoch, "7:HUB");
        assert_eq!(out.restore.old_epoch, "4:HUB");
        assert_eq!(out.restore.rows_set_aside, 5);
        assert_eq!(out.restore.header.last_seq, 3);
        assert_eq!(out.confirmed_by, "owner-1");
        assert_eq!(out.restore.file_name, "LVL01-2027-01-15-1330.a360");

        let orphaned = data_dir(&b.db).join(ORPHANED_DIR);
        assert_eq!(names(&orphaned), vec![out.restore.orphaned_file.clone()]);
        assert!(out.restore.orphaned_file.starts_with("4-HUB-2027"));
        assert!(out.restore.orphaned_file.ends_with(".db"));
        // The old database is whole: all its changes are there.
        assert_eq!(
            count(
                &orphaned.join(&out.restore.orphaned_file),
                "SELECT COUNT(*) FROM changes"
            ),
            8
        );

        // The restored database serves: the file's rows only, migrated, with the new lease.
        let store = Store::open(&b.db, T + 2000).unwrap();
        assert_eq!(store.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
        assert_eq!(store.changes().unwrap().len(), 3);
        let lease = store.lease().unwrap().unwrap();
        assert_eq!(
            (
                lease.counter,
                lease.hub_instance_id.as_str(),
                lease.provisional
            ),
            (7, "HUB", true)
        );
        let conn = Connection::open(&b.db).unwrap();
        assert_eq!(crate::read_max_seen_counter(&conn).unwrap(), 7);
        assert_eq!(
            crate::read_lease_fork(&conn).unwrap(),
            Some(BatchFork {
                parent_epoch: "2:HUB".into(),
                parent_seq: 3
            })
        );
        assert_eq!(
            crate::get_meta(&conn, crate::META_HUB_INSTANCE_ID)
                .unwrap()
                .as_deref(),
            Some("HUB")
        );

        // Pending files and scratch are gone; the outcome waits for the Hub to record it.
        assert!(!has_pending(&b.db));
        assert!(!data_dir(&b.db).join(PENDING_DIR).exists());
        assert!(!data_dir(&b.db).join(WORK_FILE).exists());
        assert_eq!(read_outcome(&b.db), Some(out));
        clear_outcome(&b.db);
        assert_eq!(read_outcome(&b.db), None);
    }

    #[test]
    fn a_player_erased_after_the_export_is_erased_in_the_restored_database() {
        let b = branch();
        {
            // P2 is erased after the file was made; P3 already was before, in both.
            let conn = Connection::open(&b.db).unwrap();
            crate::erase_player(&conn, "P2", T - 100).unwrap();
            crate::delete_player_key(&conn, "P2").unwrap();
            // A player created after the export is not in the file and is simply set aside.
            conn.execute(
                "INSERT INTO players (player_id, name_enc, phone_enc, username, password_hash,
                                      phone_lookup, tags, rev, created_at, updated_at)
                 VALUES ('P9', 'x', 'x', 'player9', 'h', '9', '[]', 1, 0, 0)",
                [],
            )
            .unwrap();
            crate::erase_player(&conn, "P9", T - 50).unwrap();
        }
        let i = inspect_export(&b.db, &b.file).unwrap();
        assert!(i.has_tombstones);
        stage_export(&b.db, &b.file, "owner-1").unwrap();
        let out = apply_pending(&b.db, T + 1000).unwrap().unwrap();
        assert_eq!(out.restore.erasures_reapplied, 1);

        let conn = Connection::open(&b.db).unwrap();
        let p2: (String, Option<i64>) = conn
            .query_row(
                "SELECT name_enc, erased_at FROM players WHERE player_id = 'P2'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(p2, (String::new(), Some(T - 100)));
        assert_eq!(
            count(
                &b.db,
                "SELECT COUNT(*) FROM player_keys WHERE player_id = 'P2'"
            ),
            0,
            "the key is shredded"
        );
        assert_eq!(
            count(
                &b.db,
                "SELECT COUNT(*) FROM players WHERE erased_at IS NULL"
            ),
            2,
            "P1 and P3 stand"
        );
        assert_eq!(
            count(&b.db, "SELECT COUNT(*) FROM player_keys"),
            2,
            "the other keys stand"
        );
    }

    #[test]
    fn pairing_licence_and_device_tokens_of_this_hub_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let db = live(dir.path(), 2, "HUB");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute(
                "INSERT INTO hub_identity (id, tenant_id, branch_id, hub_id, tier, paired_at)
                 VALUES (1, 'T1', ?1, 'HUBID', 'basic', 5)",
                [BRANCH],
            )
            .unwrap();
            crate::store_licence(&conn, "JWT", 9).unwrap();
        }
        add_changes(&db, "2:HUB", 1, 2);
        let file = export(&db, out.path(), "x.a360");
        stage_export(&db, &file, "o").unwrap();
        apply_pending(&db, T).unwrap().unwrap();
        let store = Store::open(&db, T).unwrap();
        assert_eq!(store.hub_identity().unwrap().unwrap().hub_id, "HUBID");
        assert_eq!(store.cached_licence().unwrap().unwrap().jwt, "JWT");
    }

    #[test]
    fn a_failure_before_the_swap_keeps_the_old_database_and_removes_the_pending_files() {
        let b = branch();
        stage_export(&b.db, &b.file, "owner-1").unwrap();
        // Spoil the staged file after it was staged.
        let staged = data_dir(&b.db).join(PENDING_DIR).join(PENDING_FILE);
        let mut bytes = fs::read(&staged).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        fs::write(&staged, bytes).unwrap();
        let e = apply_pending(&b.db, T).unwrap_err();
        assert_eq!(e.code, ErrorCode::RestoreCorrupt);
        assert!(!data_dir(&b.db).join(PENDING_DIR).exists());
        assert!(!data_dir(&b.db).join(ORPHANED_DIR).exists());
        assert!(!data_dir(&b.db).join(WORK_FILE).exists());
        assert_eq!(count(&b.db, "SELECT COUNT(*) FROM changes"), 8);
        assert_eq!(read_outcome(&b.db), None);
        // And the Hub starts: a second call has nothing to do.
        assert_eq!(apply_pending(&b.db, T).unwrap(), None);
    }

    #[test]
    fn a_swap_that_cannot_move_the_old_database_aside_leaves_it_in_place() {
        let b = branch();
        stage_export(&b.db, &b.file, "owner-1").unwrap();
        // `orphaned` is a file, so the old database cannot be moved into it.
        fs::write(data_dir(&b.db).join(ORPHANED_DIR), b"in the way").unwrap();
        let e = apply_pending(&b.db, T).unwrap_err();
        assert_eq!(e.code, ErrorCode::Internal);
        assert_eq!(count(&b.db, "SELECT COUNT(*) FROM changes"), 8);
        assert!(!data_dir(&b.db).join(WORK_FILE).exists());
        assert!(!data_dir(&b.db).join(PENDING_DIR).exists());
        assert_eq!(read_outcome(&b.db), None);
        assert!(Store::open(&b.db, T).unwrap().holds_writer_lock());
    }

    #[test]
    fn a_file_of_another_branch_in_the_pending_folder_is_refused_at_apply_too() {
        let b = branch();
        stage_export(&b.db, &b.file, "owner-1").unwrap();
        let staged = data_dir(&b.db).join(PENDING_DIR).join(PENDING_FILE);
        rewrite_header(&staged, |h| h.branch_id = "01JOTHER".into());
        let e = apply_pending(&b.db, T).unwrap_err();
        assert_eq!(e.code, ErrorCode::RestoreWrongBranch);
        assert_eq!(count(&b.db, "SELECT COUNT(*) FROM changes"), 8);
        assert!(!data_dir(&b.db).join(PENDING_DIR).exists());
    }

    #[test]
    fn an_older_schema_is_migrated_forward() {
        let dir = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let db = live(dir.path(), 1, "HUB");
        let file = export(&db, out.path(), "x.a360");
        // Rebuild the file's body from a database that stops at an older version.
        let old = out.path().join("old.db");
        {
            let conn = Connection::open(&old).unwrap();
            for m in crate::migrations::MIGRATIONS
                .iter()
                .filter(|m| m.version <= 60)
            {
                conn.execute_batch(m.sql).unwrap();
                record_version(&conn, m.version);
            }
            conn.execute(
                "INSERT INTO branch (branch_id, name, branch_code, currency, tz, day_start_min,
                                     fy_start_month, rev, created_at, updated_at)
                 VALUES (?1, 'Level Up', 'LVL01', 'INR', 'UTC', 0, 4, 1, 0, 0)",
                [BRANCH],
            )
            .unwrap();
        }
        let older = write_export(&old, out.path(), "older.a360", T).unwrap();
        assert_eq!(older.header.schema_version, 60);
        drop(file);
        stage_export(&db, &older.path, "o").unwrap();
        apply_pending(&db, T).unwrap().unwrap();
        let store = Store::open(&db, T).unwrap();
        assert_eq!(store.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
        assert_eq!(store.lease().unwrap().unwrap().counter, 2);
    }

    fn record_version(conn: &Connection, version: u32) {
        crate::record_schema_version(conn, version, 0).unwrap();
    }

    #[test]
    fn the_orphan_sweep_deletes_only_old_orphaned_databases() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("branch.db");
        fs::write(&db, b"live").unwrap();
        let orphaned = dir.path().join(ORPHANED_DIR);
        fs::create_dir_all(&orphaned).unwrap();
        let now = T;
        let old = orphan_name("3:HUB", now - 91 * DAY_MS);
        let young = orphan_name("3:HUB", now - 89 * DAY_MS);
        for n in [&old, &young] {
            fs::write(orphaned.join(n), b"db").unwrap();
        }
        fs::write(orphaned.join(format!("{old}-wal")), b"wal").unwrap();
        fs::write(orphaned.join("notes.txt"), b"keep").unwrap();
        assert_eq!(sweep_orphaned(&db, now), 2, "the old db and its wal");
        assert_eq!(names(&orphaned), vec![young, "notes.txt".to_owned()]);
        assert_eq!(fs::read(&db).unwrap(), b"live");
        assert_eq!(
            sweep_orphaned(&dir.path().join("none").join("branch.db"), now),
            0
        );
    }

    #[test]
    fn the_stamp_round_trips() {
        assert_eq!(stamp(0), "19700101T000000");
        assert_eq!(unstamp("19700101T000000"), Some(0));
        for ms in [T, T + 86_399_000, 951_782_400_000, 4_102_444_800_000] {
            assert_eq!(unstamp(&stamp(ms)), Some(ms / 1000 * 1000), "{ms}");
        }
        assert_eq!(unstamp("2027-01-15T133000"), None);
        assert_eq!(unstamp("20271301T000000"), None);
    }

    #[test]
    fn the_header_format_version_is_the_one_this_build_writes() {
        let b = branch();
        let bytes = fs::read(&b.file).unwrap();
        let (h, _) = ExportHeader::decode(&bytes).unwrap();
        assert_eq!(h.format_version, EXPORT_FORMAT_VERSION);
    }

    /// A replacement Hub: paired to the branch, waiting in "Restore this branch", no branch rows.
    fn replacement(dir: &Path, waiting: bool) -> PathBuf {
        let path = dir.join("branch.db");
        let mut store = Store::open(&path, T).unwrap();
        store
            .write_tx(|tx| {
                set_meta(tx, crate::META_HUB_INSTANCE_ID, "NEWHUB")?;
                crate::insert_hub_identity(
                    tx,
                    &crate::HubIdentity {
                        tenant_id: "T1".into(),
                        branch_id: BRANCH.into(),
                        hub_id: "H2".into(),
                        tier: arena_proto::Tier::Basic,
                        paired_at: T,
                    },
                )?;
                if waiting {
                    crate::mark_restore_pending(tx, "Level Up")?;
                }
                Ok(())
            })
            .unwrap();
        path
    }

    /// What the rebuild leaves: the branch, two devices (one paired, one by hand) and the list of
    /// devices that pair again.
    fn rebuilt(dir: &Path) -> PathBuf {
        let db = live(dir, 2, "OLDHUB");
        add_changes(&db, "2:OLDHUB", 1, 6);
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(&format!(
            "INSERT INTO devices (device_id, branch_id, class, label, token_hash, rev, created_at, updated_at)
               VALUES ('D1', '{BRANCH}', 'paired', 'PC 1', '$argon2id$x', 1, 0, 0),
                      ('D2', '{BRANCH}', 'paired', 'Console', '', 1, 0, 0);
             CREATE TABLE {PAIR_AGAIN_TABLE} (tbl TEXT NOT NULL, id TEXT NOT NULL, PRIMARY KEY (tbl, id));
             INSERT INTO {PAIR_AGAIN_TABLE} VALUES ('devices', 'D1');"
        ))
        .unwrap();
        db
    }

    fn facts() -> CloudRestoreFacts {
        CloudRestoreFacts {
            snapshot_date: Some("2027-01-14".into()),
            first: Some(arena_proto::RestorePosition {
                epoch: "2:OLDHUB".into(),
                seq: 1,
            }),
            last: Some(arena_proto::RestorePosition {
                epoch: "2:OLDHUB".into(),
                seq: 6,
            }),
            batches: 2,
            applied: 6,
            orphaned: 1,
            keys_restored: 3,
        }
    }

    #[test]
    fn a_rebuilt_database_replaces_a_waiting_hub_and_its_paired_devices_pair_again() {
        let hub = tempfile::tempdir().unwrap();
        let live_path = replacement(hub.path(), true);
        let src = tempfile::tempdir().unwrap();
        let db = rebuilt(src.path());

        let i = stage_rebuilt(&live_path, &db, facts(), T).unwrap();
        assert_eq!(i.branch_name, "Level Up");
        assert_eq!(
            (i.header.epoch.as_str(), i.header.last_seq),
            ("2:OLDHUB", 6)
        );
        assert_eq!(i.rows_set_aside, 0);
        assert!(has_pending(&live_path));

        let out = apply_pending(&live_path, T + 1000).unwrap().unwrap();
        assert_eq!(out.restore.cloud, Some(facts()));
        assert_eq!(out.confirmed_by, RESTORE_SYSTEM_ACTOR);
        assert_eq!(
            out.restore.new_epoch, "3:NEWHUB",
            "above the backup's epoch 2"
        );
        assert_eq!(out.restore.old_epoch, "", "the waiting Hub had no lease");
        assert!(!has_pending(&live_path));

        // This Hub's own identity is kept and no longer waits; the branch is there.
        assert_eq!(
            count(
                &live_path,
                "SELECT restore_pending FROM hub_identity WHERE hub_id = 'H2'"
            ),
            0
        );
        assert_eq!(count(&live_path, "SELECT COUNT(*) FROM branch"), 1);
        assert_eq!(count(&live_path, "SELECT COUNT(*) FROM player_keys"), 3);
        let store = Store::open(&live_path, T + 2000).unwrap();
        assert!(store.restore_pending().unwrap().is_none());
        let lease = store.lease().unwrap().unwrap();
        assert_eq!((lease.counter, lease.provisional), (3, true));
        let fork = crate::read_lease_fork(&Connection::open(&live_path).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(
            (fork.parent_epoch.as_str(), fork.parent_seq),
            ("2:OLDHUB", 6)
        );
        drop(store);

        // The paired device cannot authenticate and is told apart from the one added by hand.
        let conn = Connection::open(&live_path).unwrap();
        let hash = |id: &str| -> String {
            conn.query_row(
                "SELECT token_hash FROM devices WHERE device_id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(hash("D1"), PAIR_AGAIN_TOKEN);
        assert_eq!(hash("D2"), "");
        let left: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM sqlite_master WHERE name = '{PAIR_AGAIN_TABLE}'"),
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0, "the list is consumed");

        // The empty shell this Hub started with is kept, not deleted.
        assert_eq!(names(&data_dir(&live_path).join(ORPHANED_DIR)).len(), 1);
    }

    #[test]
    fn a_hub_that_is_not_waiting_or_a_file_of_another_branch_is_refused() {
        let src = tempfile::tempdir().unwrap();
        let db = rebuilt(src.path());

        // Paired but not waiting: it holds no branch, so there is nothing to restore into.
        let hub = tempfile::tempdir().unwrap();
        let live_path = replacement(hub.path(), false);
        let e = stage_rebuilt(&live_path, &db, facts(), T).unwrap_err();
        assert_eq!(e.code, ErrorCode::RestoreWrongBranch);
        assert!(!has_pending(&live_path));
        assert!(!data_dir(&live_path).join(PENDING_DIR).exists());

        // Waiting for another branch.
        let other = tempfile::tempdir().unwrap();
        let live_path = replacement(other.path(), true);
        Connection::open(&live_path)
            .unwrap()
            .execute("UPDATE hub_identity SET branch_id = '01JOTHER'", [])
            .unwrap();
        let e = stage_rebuilt(&live_path, &db, facts(), T).unwrap_err();
        assert_eq!(e.code, ErrorCode::RestoreWrongBranch);
        assert!(!has_pending(&live_path));
    }
}
