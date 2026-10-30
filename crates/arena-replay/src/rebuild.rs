//! Rebuilding a branch from its cloud backup (Story 9.13, AD-4, AD-11, AD-38).
//!
//! [`rebuild`] turns what Story 9.12 staged - the newest snapshot, the change-log batches after
//! it, the player key backup - into one database file: the snapshot is unpacked, every batch is
//! replayed in log order honouring fork points (rows of a forked epoch's parent above its fork are
//! kept out of state and listed in `orphaned_changes`), the replayed changes are written into
//! `changes` so the log is whole, the player key table is replaced by the key backup, and the
//! result is checked. Nothing here touches the live database: the caller stages the file and the
//! start-up swap of Story 9.7 makes it the branch, claiming the new epoch.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use arena_proto::{AppError, ErrorCode};
use arena_store::{
    BatchFork, DecodedBatch, PAIR_AGAIN_TABLE, PlayerKeyRow, Store, UploadCursor, db_error,
    decode_batch, has_branch_data, insert_change, write_upload_cursor,
};
use rusqlite::{Connection, OptionalExtension};

use crate::audit::{ensure_orphan_table, insert_orphan};
use crate::{EpochFork, Resume, replay_history, state_checksum};

/// Where a rebuild is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebuildStage {
    /// The batches are applied in order (progress counts batches).
    Replaying,
    /// The rebuilt database is checked and the keys settled (progress counts steps).
    Checking,
}

/// Why a rebuild ended without a database.
#[derive(Debug)]
pub enum RebuildStop {
    /// The caller asked to stop; the work file is gone.
    Cancelled,
    Failed(AppError),
}

impl From<AppError> for RebuildStop {
    fn from(e: AppError) -> Self {
        Self::Failed(e)
    }
}

/// What to rebuild from.
pub struct RebuildSource<'a> {
    /// The staged `snapshot.db.zst`, when the backup has one.
    pub snapshot: Option<&'a Path>,
    /// The staged batch files, in log order.
    pub batches: &'a [PathBuf],
    /// The player keys of the key backup, already opened with the branch key.
    pub keys: &'a [PlayerKeyRow],
}

/// What a rebuild did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rebuilt {
    /// The last change the database holds (the snapshot's when no batch added any).
    pub last: Option<Resume>,
    /// The first change applied from a batch.
    pub first_applied: Option<Resume>,
    /// Batches replayed.
    pub batches: u32,
    /// Changes applied from them.
    pub applied: i64,
    /// Changes kept out of state above a fork point.
    pub orphaned: i64,
    /// Player keys put back from the key backup.
    pub keys_restored: i64,
    /// Keys of the backup left out: an erased player, or one the database does not hold.
    pub keys_dropped: i64,
    /// Paired devices and kitchen displays that will need pairing again.
    pub pair_again: i64,
    /// [`state_checksum`] of the rebuilt rows.
    pub checksum: String,
}

fn bad(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::Internal, message.into())
}

fn io(what: &str, e: &std::io::Error) -> AppError {
    bad(format!("could not {what}: {e}"))
}

fn remove_db(path: &Path) {
    for suffix in ["", "-wal", "-shm", "-journal", ".lock"] {
        let mut s = path.as_os_str().to_owned();
        s.push(suffix);
        let _ = fs::remove_file(PathBuf::from(s));
    }
}

fn read_batch(path: &Path) -> Result<DecodedBatch, AppError> {
    let bytes = fs::read(path).map_err(|e| io("read a staged batch", &e))?;
    decode_batch(&bytes).map_err(|e| AppError::new(ErrorCode::RestoreCorrupt, e.message))
}

fn check_stop(cancel: &AtomicBool) -> Result<(), RebuildStop> {
    if cancel.load(Ordering::SeqCst) {
        Err(RebuildStop::Cancelled)
    } else {
        Ok(())
    }
}

/// Rebuilds the branch into the database file `work`, replacing any file there. `progress` is
/// told the stage and `done` of `total` after each batch and each check. `cancel` is looked at
/// between batches; a cancelled or failed rebuild leaves no `work` file. Blocking (disk).
pub fn rebuild(
    work: &Path,
    source: &RebuildSource<'_>,
    now_ms: i64,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(RebuildStage, u32, u32),
) -> Result<Rebuilt, RebuildStop> {
    let out = run(work, source, now_ms, cancel, progress);
    if out.is_err() {
        remove_db(work);
    }
    out
}

fn run(
    work: &Path,
    source: &RebuildSource<'_>,
    now_ms: i64,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(RebuildStage, u32, u32),
) -> Result<Rebuilt, RebuildStop> {
    remove_db(work);
    if let Some(snapshot) = source.snapshot {
        let mut input = File::open(snapshot).map_err(|e| io("open the staged snapshot", &e))?;
        let mut out = File::create(work).map_err(|e| io("create the rebuilt database", &e))?;
        zstd::stream::copy_decode(&mut input, &mut out)
            .map_err(|e| AppError::new(ErrorCode::RestoreCorrupt, format!("snapshot: {e}")))?;
    }

    // The forks come first: a parent epoch's rows above its fork come before the child's first
    // batch in log order, and must already be known to be orphans when they are met.
    let mut forks = Vec::new();
    for path in source.batches {
        check_stop(cancel)?;
        let header = read_batch(path)?.header;
        if let Some(BatchFork {
            parent_epoch,
            parent_seq,
        }) = header.fork
        {
            forks.push(EpochFork {
                epoch: header.epoch,
                parent_epoch,
                parent_seq,
            });
        }
    }

    let mut store = Store::open(work, now_ms)?;
    let mut resume = store.write_tx(|tx| {
        ensure_orphan_table(tx)?;
        tx.query_row(
            "SELECT epoch, seq FROM changes
             ORDER BY epoch_counter DESC, epoch DESC, seq DESC LIMIT 1",
            [],
            |r| {
                Ok(Resume {
                    epoch: r.get(0)?,
                    seq: r.get(1)?,
                })
            },
        )
        .optional()
        .map_err(db_error)
    })?;

    let total = u32::try_from(source.batches.len()).unwrap_or(u32::MAX);
    progress(RebuildStage::Replaying, 0, total);
    let (mut applied, mut orphaned) = (0_i64, 0_i64);
    let mut first_applied: Option<Resume> = None;
    for (i, path) in source.batches.iter().enumerate() {
        check_stop(cancel)?;
        let batch = read_batch(path)?;
        let done = store.write_tx_mut(|tx| {
            let done = replay_history(batch.rows, &forks, resume.as_ref(), tx)?;
            for c in &done.applied {
                insert_change(tx, c)?;
            }
            for c in &done.orphaned {
                insert_orphan(tx, c)?;
            }
            Ok(done)
        })?;
        applied += i64::try_from(done.applied.len()).unwrap_or(i64::MAX);
        orphaned += i64::try_from(done.orphaned.len()).unwrap_or(i64::MAX);
        if let Some(first) = done.applied.first()
            && first_applied.is_none()
        {
            first_applied = Some(Resume {
                epoch: first.epoch.clone(),
                seq: first.seq,
            });
        }
        if let Some(last) = done.applied.last() {
            resume = Some(Resume {
                epoch: last.epoch.clone(),
                seq: last.seq,
            });
        }
        progress(
            RebuildStage::Replaying,
            u32::try_from(i + 1).unwrap_or(u32::MAX),
            total,
        );
    }

    check_stop(cancel)?;
    const STEPS: u32 = 4;
    progress(RebuildStage::Checking, 0, STEPS);
    let checksum = state_checksum(&store)?;
    let (keys_restored, keys_dropped, pair_again) = store.write_tx(|tx| {
        if !has_branch_data(tx)? {
            return Err(bad("the backup holds no branch"));
        }
        // The snapshot carries no key table (Story 8.8): the key backup is the only source, and a
        // key it holds for an erased player, or for one the database does not know, stays out.
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS player_keys (
                 player_id   TEXT PRIMARY KEY REFERENCES players (player_id),
                 dek_wrapped TEXT NOT NULL
             );
             DELETE FROM player_keys;",
        )
        .map_err(db_error)?;
        let (mut kept, mut dropped) = (0_i64, 0_i64);
        for key in source.keys {
            let live: Option<bool> = tx
                .query_row(
                    "SELECT erased_at IS NULL FROM players WHERE player_id = ?1",
                    [&key.player_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_error)?;
            if live == Some(true) {
                tx.execute(
                    "INSERT INTO player_keys (player_id, dek_wrapped) VALUES (?1, ?2)",
                    rusqlite::params![key.player_id, key.dek_wrapped],
                )
                .map_err(db_error)?;
                kept += 1;
            } else {
                dropped += 1;
            }
        }
        // What is replayed is in the cloud already: uploading starts after the last change.
        if let Some(r) = &resume {
            write_upload_cursor(
                tx,
                &UploadCursor {
                    epoch: r.epoch.clone(),
                    last_seq_uploaded: r.seq,
                    uploaded_at: now_ms,
                },
            )?;
        }
        // The devices that were paired with the old Hub pair again (see PAIR_AGAIN_TABLE).
        tx.execute_batch(&format!(
            "DROP TABLE IF EXISTS {PAIR_AGAIN_TABLE};
             CREATE TABLE {PAIR_AGAIN_TABLE} (
                 tbl TEXT NOT NULL, id TEXT NOT NULL, PRIMARY KEY (tbl, id)
             );
             INSERT INTO {PAIR_AGAIN_TABLE} (tbl, id)
                 SELECT 'devices', device_id FROM devices
                 WHERE token_hash <> '' AND revoked_at IS NULL;
             INSERT INTO {PAIR_AGAIN_TABLE} (tbl, id)
                 SELECT 'kitchen_displays', id FROM kitchen_displays WHERE token_hash <> '';"
        ))
        .map_err(db_error)?;
        let again: i64 = tx
            .query_row(
                &format!("SELECT COUNT(*) FROM {PAIR_AGAIN_TABLE}"),
                [],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        Ok((kept, dropped, again))
    })?;
    progress(RebuildStage::Checking, 2, STEPS);
    drop(store);

    check_stop(cancel)?;
    let conn = Connection::open(work).map_err(db_error)?;
    let verdict: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(db_error)?;
    if verdict != "ok" {
        return Err(AppError::new(
            ErrorCode::RestoreCorrupt,
            "the rebuilt database failed its integrity check",
        )
        .into());
    }
    progress(RebuildStage::Checking, 3, STEPS);
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .map_err(db_error)?;
    drop(conn);
    for suffix in ["-wal", "-shm", ".lock"] {
        let mut s = work.as_os_str().to_owned();
        s.push(suffix);
        let _ = fs::remove_file(PathBuf::from(s));
    }
    progress(RebuildStage::Checking, STEPS, STEPS);

    Ok(Rebuilt {
        last: resume,
        first_applied,
        batches: total,
        applied,
        orphaned,
        keys_restored,
        keys_dropped,
        pair_again,
        checksum,
    })
}

#[cfg(test)]
#[path = "rebuild_tests.rs"]
mod tests;
