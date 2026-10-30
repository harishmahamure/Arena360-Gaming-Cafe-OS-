//! The audit builder's replay of one month (Story 8.36, AD-37): a snapshot (or nothing) plus the
//! change-log batches up to the end of the month, replayed batch by batch into a temporary
//! SQLite file that is packed as `audit-cache/<yyyy-mm>.db.zst`.
//!
//! The finished database holds the branch rows as of the month end, every replayed change in its
//! `changes` table (the audit search reads it) and, in `orphaned_changes`, the rows a fork point
//! left out of state (AD-4). A month is cut by `business_day`: replay stops at the first change
//! that belongs to a later month.

use std::fs::File;
use std::path::{Path, PathBuf};

use arena_proto::{AppError, ErrorCode};
use arena_store::{ChangeRecord, DbConn, DecodedBatch, Store, db_error, insert_change};
use rusqlite::{OptionalExtension, params};

use crate::{EpochFork, Resume, replay_history, state_checksum};

const ZSTD_LEVEL: i32 = 3;
const WORK_DB: &str = "audit.db";
const OUT_DB: &str = "audit-out.db";

fn io(what: &str, e: &std::io::Error) -> AppError {
    AppError::new(ErrorCode::Internal, format!("could not {what}: {e}"))
}

/// A month rebuilt and packed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltMonth {
    /// The zstd-compressed database.
    pub bytes: Vec<u8>,
    /// The epoch of the last change in the database (empty when it holds none).
    pub epoch: String,
    /// That change's `seq`.
    pub seq: i64,
    /// [`state_checksum`] of the rebuilt rows.
    pub checksum: String,
    /// Changes applied from batches (not counting those inside the snapshot).
    pub applied: usize,
    /// Changes kept out of state above a fork point.
    pub orphaned: usize,
}

/// Replays batches into a temporary database one at a time.
pub struct MonthBuilder {
    store: Store,
    dir: PathBuf,
    month: String,
    forks: Vec<EpochFork>,
    resume: Option<Resume>,
    complete: bool,
    applied: usize,
    orphaned: usize,
}

impl MonthBuilder {
    /// Starts a build for `month` (`yyyy-mm`) in the scratch directory `dir`: from the zstd
    /// `snapshot` when there is one, else from an empty database. `forks` are the fork points
    /// of every epoch that will be replayed.
    pub fn open(
        dir: &Path,
        month: &str,
        snapshot: Option<&[u8]>,
        forks: Vec<EpochFork>,
        now_ms: i64,
    ) -> Result<Self, AppError> {
        let path = dir.join(WORK_DB);
        if let Some(bytes) = snapshot {
            let mut out = File::create(&path).map_err(|e| io("create the snapshot copy", &e))?;
            zstd::stream::copy_decode(bytes, &mut out)
                .map_err(|e| io("unpack the snapshot", &e))?;
        }
        let mut store = Store::open(&path, now_ms)?;
        let resume = store.write_tx(|tx| {
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS orphaned_changes AS
                 SELECT epoch, seq, tbl, row_id, op, row, intent_id, intent_kind, actor_kind,
                        actor_id, elevated_by_kind, elevated_by_id, elevated_by_mode,
                        elevated_by_verifier_version, at, business_day, fy, reason
                 FROM changes WHERE 0",
            )
            .map_err(db_error)?;
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
        Ok(Self {
            store,
            dir: dir.to_path_buf(),
            month: month.to_owned(),
            forks,
            resume,
            complete: false,
            applied: 0,
            orphaned: 0,
        })
    }

    /// Sets the fork points of the epochs that will be replayed (known once their first batches
    /// are read, after the snapshot's position is).
    pub fn set_forks(&mut self, forks: Vec<EpochFork>) {
        self.forks = forks;
    }

    /// The last change the database holds: the snapshot's, then each applied batch's.
    pub fn position(&self) -> Option<&Resume> {
        self.resume.as_ref()
    }

    /// Whether a change belongs to a month after this one.
    fn after_month(&self, c: &ChangeRecord) -> bool {
        c.business_day
            .as_deref()
            .and_then(|d| d.get(..7))
            .is_some_and(|m| m > self.month.as_str())
    }

    /// Replays `batch` and answers whether more batches are wanted: `false` once a change of a
    /// later month was met (the rest of that batch is left out). A gap, a bad row or a delete of
    /// a missing row fails and leaves the database as it was before this batch.
    pub fn apply(&mut self, batch: &DecodedBatch) -> Result<bool, AppError> {
        if self.complete {
            return Ok(false);
        }
        let mut rows = Vec::with_capacity(batch.rows.len());
        for c in &batch.rows {
            let held = match &self.resume {
                Some(r) => r.covers(c)?,
                None => false,
            };
            let orphan = self
                .forks
                .iter()
                .any(|f| f.parent_epoch == c.epoch && c.seq > f.parent_seq);
            if !held && !orphan && self.after_month(c) {
                self.complete = true;
                break;
            }
            rows.push(c.clone());
        }
        if rows.is_empty() {
            return Ok(!self.complete);
        }
        let forks = &self.forks;
        let resume = self.resume.as_ref();
        let done = self.store.write_tx_mut(|tx| {
            let done = replay_history(rows, forks, resume, tx)?;
            for c in &done.applied {
                insert_change(tx, c)?;
            }
            for c in &done.orphaned {
                insert_orphan(tx, c)?;
            }
            Ok(done)
        })?;
        self.applied += done.applied.len();
        self.orphaned += done.orphaned.len();
        if let Some(last) = done.applied.last() {
            self.resume = Some(Resume {
                epoch: last.epoch.clone(),
                seq: last.seq,
            });
        }
        Ok(!self.complete)
    }

    /// Checksums the rebuilt rows and packs the database. The scratch files stay in the
    /// directory for the caller to delete.
    pub fn finish(self) -> Result<BuiltMonth, AppError> {
        let checksum = state_checksum(&self.store)?;
        let out = self.dir.join(OUT_DB);
        {
            let conn = DbConn::open(self.store.path()).map_err(db_error)?;
            conn.execute("VACUUM INTO ?1", [out.to_string_lossy().as_ref()])
                .map_err(db_error)?;
        }
        let mut input = File::open(&out).map_err(|e| io("open the audit copy", &e))?;
        let mut bytes = Vec::new();
        zstd::stream::copy_encode(&mut input, &mut bytes, ZSTD_LEVEL)
            .map_err(|e| io("pack the audit database", &e))?;
        let (epoch, seq) = self.resume.map(|r| (r.epoch, r.seq)).unwrap_or_default();
        Ok(BuiltMonth {
            bytes,
            epoch,
            seq,
            checksum,
            applied: self.applied,
            orphaned: self.orphaned,
        })
    }
}

fn insert_orphan(conn: &DbConn, c: &ChangeRecord) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO orphaned_changes (epoch, seq, tbl, row_id, op, row, intent_id, intent_kind,
                                       actor_kind, actor_id, elevated_by_kind, elevated_by_id,
                                       elevated_by_mode, elevated_by_verifier_version, at,
                                       business_day, fy, reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            c.epoch,
            c.seq,
            c.tbl,
            c.row_id,
            c.op.as_str(),
            c.row,
            c.intent_id,
            c.intent_kind,
            c.actor_kind,
            c.actor_id,
            c.elevated_by_kind,
            c.elevated_by_id,
            c.elevated_by_mode,
            c.elevated_by_verifier_version,
            c.at,
            c.business_day,
            c.fy,
            c.reason
        ],
    )
    .map_err(db_error)?;
    Ok(())
}
