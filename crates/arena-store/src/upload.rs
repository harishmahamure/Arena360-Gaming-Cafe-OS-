//! The Hub's change-log upload (Story 8.2): which rows are still to ship, the cursor that says
//! how far the cloud has them, and the batch file.
//!
//! A batch is `zstd(header ++ rows)`: `header` is a MessagePack map `{epoch, fork?, first, last,
//! sha256, schema}` and `rows` a MessagePack array of maps keyed by the `changes` column names.
//! `sha256` is over the `rows` bytes. A batch holds rows of one epoch only, `first`..`last` with
//! no gap, so `rows.len() == last - first + 1`.
//!
//! The cursor (`upload_cursor`) is local bookkeeping outside the change log, like the ad cursors:
//! it is written only after the cloud answered 2xx for the batch, and never by `apply()`.

use std::io::Read;

use arena_proto::{AppError, ErrorCode};
use rusqlite::{Connection, OptionalExtension, params};
use serde::de::{self, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::branch::{CHANGE_COLUMNS, ChangeOp, ChangeRecord, change_from};
use crate::db_error;

/// The most rows one batch holds.
pub const BATCH_MAX_ROWS: usize = 5_000;
/// The most bytes a batch may inflate to when read back.
const MAX_INFLATED_BYTES: u64 = 512 * 1024 * 1024;
const ZSTD_LEVEL: i32 = 3;

fn bad(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::Internal, message)
}

/// The last change row the cloud has stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadCursor {
    pub epoch: String,
    pub last_seq_uploaded: i64,
    pub uploaded_at: i64,
}

/// Where an epoch was forked from: the parent epoch and the last `seq` it contributed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchFork {
    pub parent_epoch: String,
    pub parent_seq: i64,
}

/// The rows of one batch, read from the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingBatch {
    pub epoch: String,
    pub first: i64,
    pub last: i64,
    pub fork: Option<BatchFork>,
    pub rows: Vec<ChangeRecord>,
}

/// The header of a batch file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchHeader {
    pub epoch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork: Option<BatchFork>,
    pub first: i64,
    pub last: i64,
    /// Lower-case hex SHA-256 of the rows array bytes.
    pub sha256: String,
    /// The Hub's schema version when the batch was written.
    pub schema: u32,
}

/// A batch file read back and checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedBatch {
    pub header: BatchHeader,
    pub rows: Vec<ChangeRecord>,
}

// ---- cursor and fork ----

/// The upload cursor, if a batch has ever been accepted.
pub fn read_upload_cursor(conn: &Connection) -> Result<Option<UploadCursor>, AppError> {
    conn.query_row(
        "SELECT epoch, last_seq_uploaded, uploaded_at FROM upload_cursor WHERE id = 1",
        [],
        |r| {
            Ok(UploadCursor {
                epoch: r.get(0)?,
                last_seq_uploaded: r.get(1)?,
                uploaded_at: r.get(2)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

/// Sets the upload cursor (inserts it the first time).
pub fn write_upload_cursor(conn: &Connection, c: &UploadCursor) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO upload_cursor (id, epoch, last_seq_uploaded, uploaded_at, first_uploaded_at)
         VALUES (1, ?1, ?2, ?3, ?3)
         ON CONFLICT (id) DO UPDATE SET
           epoch = excluded.epoch,
           last_seq_uploaded = excluded.last_seq_uploaded,
           uploaded_at = excluded.uploaded_at",
        params![c.epoch, c.last_seq_uploaded, c.uploaded_at],
    )
    .map_err(db_error)?;
    Ok(())
}

/// When the first batch was accepted (Hub clock, ms): when the branch went paid as far as the Hub
/// can tell (Story 8.6). `None` before any upload.
pub fn read_paid_since(conn: &Connection) -> Result<Option<i64>, AppError> {
    conn.query_row(
        "SELECT COALESCE(first_uploaded_at, uploaded_at) FROM upload_cursor WHERE id = 1",
        [],
        |r| r.get(0),
    )
    .optional()
    .map_err(db_error)
}

/// The lease's fork parent, when both columns are set.
pub fn read_lease_fork(conn: &Connection) -> Result<Option<BatchFork>, AppError> {
    let row: Option<(Option<String>, Option<i64>)> = conn
        .query_row(
            "SELECT fork_parent_epoch, fork_parent_seq FROM lease WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db_error)?;
    Ok(match row {
        Some((Some(parent_epoch), Some(parent_seq))) => Some(BatchFork {
            parent_epoch,
            parent_seq,
        }),
        _ => None,
    })
}

/// Sets or clears the lease's fork parent (Epic 9 sets it at a real epoch change; tests set it
/// by fixture). Needs the lease row.
pub fn write_lease_fork(conn: &Connection, fork: Option<&BatchFork>) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE lease SET fork_parent_epoch = ?1, fork_parent_seq = ?2 WHERE id = 1",
            params![
                fork.map(|f| f.parent_epoch.as_str()),
                fork.map(|f| f.parent_seq)
            ],
        )
        .map_err(db_error)?;
    if n == 0 {
        return Err(bad("there is no lease row to set a fork on"));
    }
    Ok(())
}

// ---- what is left to upload ----

fn lease_epoch(conn: &Connection) -> Result<Option<String>, AppError> {
    conn.query_row(
        "SELECT counter, hub_instance_id FROM lease WHERE id = 1",
        [],
        |r| {
            Ok(format!(
                "{}:{}",
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?
            ))
        },
    )
    .optional()
    .map_err(db_error)
}

fn epoch_counter(epoch: &str) -> Result<i64, AppError> {
    epoch
        .split_once(':')
        .and_then(|(c, _)| c.parse::<i64>().ok())
        .ok_or_else(|| bad(format!("malformed epoch {epoch:?}")))
}

/// The first epoch after `after` in log order (numeric counter, then text); the first of all
/// when `after` is `None`.
fn epoch_after(conn: &Connection, after: Option<&str>) -> Result<Option<String>, AppError> {
    match after {
        None => conn
            .query_row(
                "SELECT epoch FROM changes ORDER BY epoch_counter, epoch LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional(),
        Some(epoch) => conn
            .query_row(
                "SELECT epoch FROM changes WHERE (epoch_counter, epoch) > (?1, ?2)
                 ORDER BY epoch_counter, epoch LIMIT 1",
                params![epoch_counter(epoch)?, epoch],
                |r| r.get(0),
            )
            .optional(),
    }
    .map_err(db_error)
}

/// The fork a batch of `epoch` carries: the lease's, when `epoch` is the lease's own and the
/// cursor is not yet in it.
fn fork_for(
    conn: &Connection,
    epoch: &str,
    cursor: Option<&UploadCursor>,
) -> Result<Option<BatchFork>, AppError> {
    if cursor.is_some_and(|c| c.epoch == epoch) {
        return Ok(None);
    }
    if lease_epoch(conn)?.as_deref() != Some(epoch) {
        return Ok(None);
    }
    read_lease_fork(conn)
}

struct Target {
    epoch: String,
    first: i64,
    fork: Option<BatchFork>,
}

/// Where the next batch starts: the rest of the cursor's epoch, else the first row of the next
/// epoch that has rows to ship (a forked epoch starts after its parent's `seq`).
fn next_target(
    conn: &Connection,
    cursor: Option<&UploadCursor>,
) -> Result<Option<Target>, AppError> {
    let mut after = None;
    if let Some(c) = cursor {
        let more: bool = conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM changes WHERE epoch = ?1 AND seq > ?2)",
                params![c.epoch, c.last_seq_uploaded],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        if more {
            return Ok(Some(Target {
                epoch: c.epoch.clone(),
                first: c.last_seq_uploaded + 1,
                fork: None,
            }));
        }
        after = Some(c.epoch.clone());
    }
    loop {
        let Some(epoch) = epoch_after(conn, after.as_deref())? else {
            return Ok(None);
        };
        let fork = fork_for(conn, &epoch, cursor)?;
        let first = match &fork {
            Some(f) => f.parent_seq + 1,
            None => conn
                .query_row(
                    "SELECT MIN(seq) FROM changes WHERE epoch = ?1",
                    [&epoch],
                    |r| r.get::<_, i64>(0),
                )
                .map_err(db_error)?,
        };
        let any: bool = conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM changes WHERE epoch = ?1 AND seq >= ?2)",
                params![epoch, first],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        if any {
            return Ok(Some(Target { epoch, first, fork }));
        }
        after = Some(epoch);
    }
}

/// The next batch to upload: up to `limit` rows of one epoch, in `seq` order with no gap, after
/// the cursor. `None` when everything is uploaded. A hole in the log is `INTERNAL` (nothing is
/// shipped across it).
pub fn read_pending_batch(
    conn: &Connection,
    limit: usize,
) -> Result<Option<PendingBatch>, AppError> {
    let cursor = read_upload_cursor(conn)?;
    let Some(target) = next_target(conn, cursor.as_ref())? else {
        return Ok(None);
    };
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {CHANGE_COLUMNS} FROM changes WHERE epoch = ?1 AND seq >= ?2
             ORDER BY seq LIMIT ?3"
        ))
        .map_err(db_error)?;
    let limit = i64::try_from(limit.clamp(1, BATCH_MAX_ROWS)).unwrap_or(i64::MAX);
    let found = stmt
        .query_map(params![target.epoch, target.first, limit], change_from)
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    let mut rows: Vec<ChangeRecord> = Vec::with_capacity(found.len());
    for row in found {
        let expected = rows.last().map_or(target.first, |r| r.seq + 1);
        if row.seq != expected {
            if rows.is_empty() {
                return Err(bad(format!(
                    "the change log of {} has a gap: expected seq {expected}, found {}",
                    target.epoch, row.seq
                )));
            }
            break;
        }
        rows.push(row);
    }
    let last = rows.last().map_or(target.first, |r| r.seq);
    Ok(Some(PendingBatch {
        epoch: target.epoch,
        first: target.first,
        last,
        fork: target.fork,
        rows,
    }))
}

/// How far the upload has got in the lease's own epoch, as the heartbeat reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EpochProgress {
    /// The cursor's `seq` when it is in the lease's epoch, else the fork parent's (or 0).
    pub last_seq_uploaded: i64,
    /// The parent `seq` the lease's epoch was forked from.
    pub fork_seq: Option<i64>,
    /// The last `seq` of the lease's epoch that the upgrade snapshot holds instead of batches
    /// (Story 9.11), when the snapshot ended in the lease's epoch.
    pub base_seq: Option<i64>,
}

/// The upload position in the lease's epoch (Story 8.3).
pub fn read_epoch_progress(conn: &Connection) -> Result<EpochProgress, AppError> {
    let fork = read_lease_fork(conn)?.map(|f| f.parent_seq);
    let lease = lease_epoch(conn)?;
    let cursor = read_upload_cursor(conn)?;
    let last_seq_uploaded = match (&lease, cursor) {
        (Some(l), Some(c)) if &c.epoch == l => c.last_seq_uploaded,
        _ => fork.unwrap_or(0),
    };
    let state = crate::read_backup_state(conn)?;
    let base_seq = match (&lease, state.base_epoch) {
        (Some(l), Some(base)) if *l == base => state.base_seq,
        _ => None,
    };
    Ok(EpochProgress {
        last_seq_uploaded,
        fork_seq: fork,
        base_seq,
    })
}

/// What a resend of `epoch` `from..=to` finds in the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RangeRead {
    /// The rows from `from` on, with no gap, at most `limit` of them and none past `to`.
    Batch(PendingBatch),
    /// Nothing at `from`: the rows `from..=to` are gone (pruned).
    Unavailable { from: i64, to: i64 },
}

/// Reads the next batch of a resend: rows of `epoch` from `from`, in `seq` order with no gap,
/// at most `limit` and none after `to`. When there is no row at `from`, the run up to the next
/// row (or `to`) is [`RangeRead::Unavailable`]. The first batch of a forked epoch carries its
/// fork, like the normal upload.
pub fn read_range_batch(
    conn: &Connection,
    epoch: &str,
    from: i64,
    to: i64,
    limit: usize,
) -> Result<RangeRead, AppError> {
    let limit = i64::try_from(limit.clamp(1, BATCH_MAX_ROWS)).unwrap_or(i64::MAX);
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {CHANGE_COLUMNS} FROM changes WHERE epoch = ?1 AND seq >= ?2 AND seq <= ?3
             ORDER BY seq LIMIT ?4"
        ))
        .map_err(db_error)?;
    let found = stmt
        .query_map(params![epoch, from, to, limit], change_from)
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    let Some(head) = found.first() else {
        return Ok(RangeRead::Unavailable { from, to });
    };
    if head.seq != from {
        return Ok(RangeRead::Unavailable {
            from,
            to: head.seq - 1,
        });
    }
    let mut rows: Vec<ChangeRecord> = Vec::with_capacity(found.len());
    for row in found {
        if row.seq != rows.last().map_or(from, |r| r.seq + 1) {
            break;
        }
        rows.push(row);
    }
    let last = rows.last().map_or(from, |r| r.seq);
    let fork = match read_lease_fork(conn)? {
        Some(f) if f.parent_seq + 1 == from && lease_epoch(conn)?.as_deref() == Some(epoch) => {
            Some(f)
        }
        _ => None,
    };
    Ok(RangeRead::Batch(PendingBatch {
        epoch: epoch.to_owned(),
        first: from,
        last,
        fork,
        rows,
    }))
}

/// How many change rows are still to upload.
pub fn count_pending_rows(conn: &Connection) -> Result<i64, AppError> {
    let cursor = read_upload_cursor(conn)?;
    let lease = lease_epoch(conn)?;
    let fork = match (&lease, read_lease_fork(conn)?) {
        (Some(l), Some(f)) if cursor.as_ref().is_none_or(|c| &c.epoch != l) => {
            Some((l.clone(), f.parent_seq))
        }
        _ => None,
    };
    // Rows of a forked epoch up to its parent's seq belong to the parent.
    let (fork_epoch, fork_seq) = fork.unwrap_or_else(|| (String::new(), -1));
    let mut total: i64 = 0;
    if let Some(c) = &cursor {
        total += conn
            .query_row(
                "SELECT COUNT(*) FROM changes WHERE epoch = ?1 AND seq > ?2",
                params![c.epoch, c.last_seq_uploaded],
                |r| r.get::<_, i64>(0),
            )
            .map_err(db_error)?;
        total += conn
            .query_row(
                "SELECT COUNT(*) FROM changes WHERE (epoch_counter, epoch) > (?1, ?2)
                   AND NOT (epoch = ?3 AND seq <= ?4)",
                params![epoch_counter(&c.epoch)?, c.epoch, fork_epoch, fork_seq],
                |r| r.get::<_, i64>(0),
            )
            .map_err(db_error)?;
    } else {
        total += conn
            .query_row(
                "SELECT COUNT(*) FROM changes WHERE NOT (epoch = ?1 AND seq <= ?2)",
                params![fork_epoch, fork_seq],
                |r| r.get::<_, i64>(0),
            )
            .map_err(db_error)?;
    }
    Ok(total)
}

// ---- the batch file ----

/// A byte string that is MessagePack `bin`, not an array of numbers.
struct Bin(Vec<u8>);

impl Serialize for Bin {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for Bin {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Bin;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a byte string")
            }
            fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Bin, E> {
                Ok(Bin(v.to_vec()))
            }
            fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Bin, E> {
                Ok(Bin(v))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Bin, A::Error> {
                let mut out = Vec::new();
                while let Some(b) = seq.next_element::<u8>()? {
                    out.push(b);
                }
                Ok(Bin(out))
            }
        }
        d.deserialize_byte_buf(V)
    }
}

/// One `changes` row as the batch file holds it: every column, by name.
#[derive(Serialize, Deserialize)]
struct BatchRow {
    epoch: String,
    seq: i64,
    tbl: String,
    row_id: String,
    op: String,
    row: Bin,
    intent_id: String,
    intent_kind: String,
    actor_kind: String,
    actor_id: String,
    elevated_by_kind: Option<String>,
    elevated_by_id: Option<String>,
    elevated_by_mode: Option<String>,
    elevated_by_verifier_version: Option<i64>,
    at: i64,
    business_day: Option<String>,
    fy: Option<String>,
    reason: Option<String>,
}

impl From<&ChangeRecord> for BatchRow {
    fn from(c: &ChangeRecord) -> Self {
        Self {
            epoch: c.epoch.clone(),
            seq: c.seq,
            tbl: c.tbl.clone(),
            row_id: c.row_id.clone(),
            op: c.op.as_str().to_owned(),
            row: Bin(c.row.clone()),
            intent_id: c.intent_id.clone(),
            intent_kind: c.intent_kind.clone(),
            actor_kind: c.actor_kind.clone(),
            actor_id: c.actor_id.clone(),
            elevated_by_kind: c.elevated_by_kind.clone(),
            elevated_by_id: c.elevated_by_id.clone(),
            elevated_by_mode: c.elevated_by_mode.clone(),
            elevated_by_verifier_version: c.elevated_by_verifier_version,
            at: c.at,
            business_day: c.business_day.clone(),
            fy: c.fy.clone(),
            reason: c.reason.clone(),
        }
    }
}

impl TryFrom<BatchRow> for ChangeRecord {
    type Error = AppError;

    fn try_from(r: BatchRow) -> Result<Self, AppError> {
        let op = ChangeOp::parse(&r.op)
            .ok_or_else(|| bad(format!("batch row {}:{} has op {:?}", r.epoch, r.seq, r.op)))?;
        Ok(Self {
            epoch: r.epoch,
            seq: r.seq,
            tbl: r.tbl,
            row_id: r.row_id,
            op,
            row: r.row.0,
            intent_id: r.intent_id,
            intent_kind: r.intent_kind,
            actor_kind: r.actor_kind,
            actor_id: r.actor_id,
            elevated_by_kind: r.elevated_by_kind,
            elevated_by_id: r.elevated_by_id,
            elevated_by_mode: r.elevated_by_mode,
            elevated_by_verifier_version: r.elevated_by_verifier_version,
            at: r.at,
            business_day: r.business_day,
            fy: r.fy,
            reason: r.reason,
        })
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn expected_count(first: i64, last: i64) -> Option<usize> {
    let n = last.checked_sub(first)?.checked_add(1)?;
    usize::try_from(n).ok().filter(|n| *n > 0)
}

/// Builds the batch file for `batch`. `schema` is the Hub's schema version. The batch must hold
/// `first..=last` of one epoch with no gap.
pub fn encode_batch(schema: u32, batch: &PendingBatch) -> Result<Vec<u8>, AppError> {
    if expected_count(batch.first, batch.last) != Some(batch.rows.len()) {
        return Err(bad(format!(
            "batch {}..{} cannot hold {} rows",
            batch.first,
            batch.last,
            batch.rows.len()
        )));
    }
    for (i, r) in batch.rows.iter().enumerate() {
        if r.epoch != batch.epoch || r.seq != batch.first + i as i64 {
            return Err(bad(format!(
                "batch row {}:{} is out of place in {} {}..{}",
                r.epoch, r.seq, batch.epoch, batch.first, batch.last
            )));
        }
    }
    let rows: Vec<BatchRow> = batch.rows.iter().map(BatchRow::from).collect();
    let rows_bytes =
        rmp_serde::to_vec_named(&rows).map_err(|e| bad(format!("encode rows: {e}")))?;
    let header = BatchHeader {
        epoch: batch.epoch.clone(),
        fork: batch.fork.clone(),
        first: batch.first,
        last: batch.last,
        sha256: hex(&Sha256::digest(&rows_bytes)),
        schema,
    };
    let mut plain =
        rmp_serde::to_vec_named(&header).map_err(|e| bad(format!("encode header: {e}")))?;
    plain.extend_from_slice(&rows_bytes);
    zstd::bulk::compress(&plain, ZSTD_LEVEL).map_err(|e| bad(format!("compress: {e}")))
}

/// Reads a batch file back, checking its SHA-256, row count and sequence numbers against its
/// header.
pub fn decode_batch(bytes: &[u8]) -> Result<DecodedBatch, AppError> {
    let mut plain = Vec::new();
    zstd::stream::read::Decoder::new(bytes)
        .and_then(|d| d.take(MAX_INFLATED_BYTES + 1).read_to_end(&mut plain))
        .map_err(|e| bad(format!("not a zstd batch: {e}")))?;
    if plain.len() as u64 > MAX_INFLATED_BYTES {
        return Err(bad("the batch inflates past its size limit"));
    }
    let mut rest: &[u8] = &plain;
    let header: BatchHeader =
        rmp_serde::from_read(&mut rest).map_err(|e| bad(format!("bad batch header: {e}")))?;
    if hex(&Sha256::digest(rest)) != header.sha256 {
        return Err(bad("the batch rows do not match the header sha256"));
    }
    let rows: Vec<BatchRow> =
        rmp_serde::from_read(&mut rest).map_err(|e| bad(format!("bad batch rows: {e}")))?;
    if !rest.is_empty() {
        return Err(bad("the batch has bytes after its rows"));
    }
    if expected_count(header.first, header.last) != Some(rows.len()) {
        return Err(bad(format!(
            "the batch header says {}..{} but it holds {} rows",
            header.first,
            header.last,
            rows.len()
        )));
    }
    let mut out = Vec::with_capacity(rows.len());
    for (i, r) in rows.into_iter().enumerate() {
        if r.epoch != header.epoch || r.seq != header.first + i as i64 {
            return Err(bad(format!(
                "batch row {}:{} is out of place in {} {}..{}",
                r.epoch, r.seq, header.epoch, header.first, header.last
            )));
        }
        out.push(ChangeRecord::try_from(r)?);
    }
    Ok(DecodedBatch { header, rows: out })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LeaseRow, Store, insert_change, write_lease};

    const T0: i64 = 1_800_000_000_000;

    fn change(epoch: &str, seq: i64) -> ChangeRecord {
        ChangeRecord {
            epoch: epoch.into(),
            seq,
            tbl: "settings".into(),
            row_id: format!("k{seq}"),
            op: ChangeOp::Insert,
            row: vec![0x80, seq as u8, 0xc4, 0x01],
            intent_id: format!("I{seq}"),
            intent_kind: "settings.set".into(),
            actor_kind: "staff".into(),
            actor_id: "S1".into(),
            elevated_by_kind: (seq % 2 == 0).then(|| "pin".into()),
            elevated_by_id: (seq % 2 == 0).then(|| "S2".into()),
            elevated_by_mode: (seq % 2 == 0).then(|| "hub".into()),
            elevated_by_verifier_version: (seq % 2 == 0).then_some(3),
            at: T0 + seq,
            business_day: Some("2026-10-29".into()),
            fy: Some("FY2026-27".into()),
            reason: (seq % 3 == 0).then(|| "stolen".into()),
        }
    }

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("branch.db"), T0).unwrap();
        (dir, store)
    }

    fn put(store: &mut Store, epoch: &str, seqs: impl IntoIterator<Item = i64>) {
        store
            .write_tx(|tx| {
                for s in seqs {
                    insert_change(tx, &change(epoch, s))?;
                }
                Ok(())
            })
            .unwrap();
    }

    fn lease(store: &mut Store, counter: i64, id: &str) {
        store
            .write_tx(|tx| {
                write_lease(
                    tx,
                    &LeaseRow {
                        counter,
                        hub_instance_id: id.into(),
                        provisional: false,
                        renewed_at: T0,
                        expires_at: T0 + 15_000,
                    },
                )
            })
            .unwrap();
    }

    fn read(store: &Store, limit: usize) -> Option<PendingBatch> {
        store.read_with(|c| read_pending_batch(c, limit)).unwrap()
    }

    fn pending(store: &Store) -> i64 {
        store.read_with(count_pending_rows).unwrap()
    }

    fn cursor(store: &mut Store, epoch: &str, last: i64) {
        store
            .write_tx(|tx| {
                write_upload_cursor(
                    tx,
                    &UploadCursor {
                        epoch: epoch.into(),
                        last_seq_uploaded: last,
                        uploaded_at: T0,
                    },
                )
            })
            .unwrap();
    }

    #[test]
    fn a_batch_round_trips_through_zstd_with_every_column() {
        let batch = PendingBatch {
            epoch: "3:HUB".into(),
            first: 1,
            last: 6,
            fork: None,
            rows: (1..=6).map(|s| change("3:HUB", s)).collect(),
        };
        let bytes = encode_batch(73, &batch).unwrap();
        assert_eq!(bytes[..4], [0x28, 0xb5, 0x2f, 0xfd], "a zstd frame");
        let back = decode_batch(&bytes).unwrap();
        assert_eq!(back.rows, batch.rows);
        assert_eq!(
            back.header,
            BatchHeader {
                epoch: "3:HUB".into(),
                fork: None,
                first: 1,
                last: 6,
                sha256: back.header.sha256.clone(),
                schema: 73,
            }
        );
        assert_eq!(back.header.sha256.len(), 64);

        let forked = PendingBatch {
            fork: Some(BatchFork {
                parent_epoch: "2:OLD".into(),
                parent_seq: 40,
            }),
            first: 41,
            last: 42,
            rows: vec![change("3:HUB", 41), change("3:HUB", 42)],
            ..batch
        };
        let back = decode_batch(&encode_batch(73, &forked).unwrap()).unwrap();
        assert_eq!(back.header.fork, forked.fork);
        assert_eq!((back.header.first, back.header.last), (41, 42));
    }

    #[test]
    fn the_header_is_a_map_and_the_row_a_bin() {
        let batch = PendingBatch {
            epoch: "1:A".into(),
            first: 5,
            last: 5,
            fork: None,
            rows: vec![change("1:A", 5)],
        };
        let plain = zstd::decode_all(&encode_batch(9, &batch).unwrap()[..]).unwrap();
        let mut rest: &[u8] = &plain;
        let header: serde_json::Value = rmp_serde::from_read(&mut rest).unwrap();
        assert_eq!(header["epoch"], "1:A");
        assert_eq!(header["schema"], 9);
        assert!(header.get("fork").is_none());
        // The rows: an array of one map of 18 entries (map16), keyed by column name, whose
        // `row` is a bin holding the stored MessagePack as is.
        assert_eq!(rest[..4], [0x91, 0xde, 0x00, 18]);
        for key in [
            "epoch",
            "tbl",
            "elevated_by_verifier_version",
            "business_day",
            "reason",
        ] {
            assert!(
                rest.windows(key.len()).any(|w| w == key.as_bytes()),
                "{key} is a key"
            );
        }
        assert!(rest.windows(6).any(|w| w == [0xc4, 4, 0x80, 5, 0xc4, 1]));
    }

    #[test]
    fn a_tampered_or_inconsistent_batch_is_refused() {
        let batch = PendingBatch {
            epoch: "1:A".into(),
            first: 1,
            last: 3,
            fork: None,
            rows: (1..=3).map(|s| change("1:A", s)).collect(),
        };
        let bytes = encode_batch(73, &batch).unwrap();
        let plain = zstd::decode_all(&bytes[..]).unwrap();
        let recompress = |p: &[u8]| zstd::encode_all(p, 1).unwrap();

        // A flipped byte in the rows.
        let mut flipped = plain.clone();
        *flipped.last_mut().unwrap() ^= 0xff;
        let err = decode_batch(&recompress(&flipped)).unwrap_err();
        assert!(err.message.contains("sha256"), "{err}");

        // Trailing bytes.
        let mut trailing = plain.clone();
        trailing.push(0xc0);
        assert!(decode_batch(&recompress(&trailing)).is_err());

        // Not zstd at all.
        assert!(decode_batch(b"not a batch").is_err());
        assert!(decode_batch(&[]).is_err());

        // A header that claims another range than the rows hold.
        let mut other = batch.clone();
        other.last = 4;
        assert!(
            encode_batch(73, &other).is_err(),
            "count must be last-first+1"
        );
        let mut gap = batch.clone();
        gap.rows[2] = change("1:A", 9);
        assert!(encode_batch(73, &gap).is_err(), "seqs must be contiguous");
        let mut mixed = batch.clone();
        mixed.rows[1] = change("2:B", 2);
        assert!(encode_batch(73, &mixed).is_err(), "one epoch only");
        let mut empty = batch;
        empty.rows.clear();
        assert!(encode_batch(73, &empty).is_err());
    }

    #[test]
    fn the_migration_adds_the_cursor_and_the_fork_columns() {
        let (_d, mut store) = store();
        assert_eq!(store.read_with(read_upload_cursor).unwrap(), None);
        assert_eq!(store.read_with(read_lease_fork).unwrap(), None);
        // No lease row yet: a fork cannot be set.
        assert!(
            store
                .write_tx(|tx| write_lease_fork(
                    tx,
                    Some(&BatchFork {
                        parent_epoch: "1:A".into(),
                        parent_seq: 1
                    })
                ))
                .is_err()
        );
        lease(&mut store, 2, "B");
        let fork = BatchFork {
            parent_epoch: "1:A".into(),
            parent_seq: 7,
        };
        store
            .write_tx(|tx| write_lease_fork(tx, Some(&fork)))
            .unwrap();
        assert_eq!(
            store.read_with(read_lease_fork).unwrap(),
            Some(fork.clone())
        );
        // Renewing the lease leaves the fork alone.
        lease(&mut store, 2, "B");
        assert_eq!(store.read_with(read_lease_fork).unwrap(), Some(fork));
        store.write_tx(|tx| write_lease_fork(tx, None)).unwrap();
        assert_eq!(store.read_with(read_lease_fork).unwrap(), None);

        cursor(&mut store, "2:B", 10);
        cursor(&mut store, "2:B", 15);
        let c = store.read_with(read_upload_cursor).unwrap().unwrap();
        assert_eq!((c.epoch.as_str(), c.last_seq_uploaded), ("2:B", 15));
        assert!(
            store
                .write_tx(|tx| write_upload_cursor(
                    tx,
                    &UploadCursor {
                        epoch: "2:B".into(),
                        last_seq_uploaded: -1,
                        uploaded_at: 0
                    }
                ))
                .is_err(),
            "the cursor is never negative"
        );
    }

    #[test]
    fn pending_batches_follow_the_cursor_in_chunks_without_gap_or_overlap() {
        let (_d, mut store) = store();
        assert_eq!(read(&store, 10), None);
        assert_eq!(pending(&store), 0);
        put(&mut store, "1:A", 1..=12);
        assert_eq!(pending(&store), 12);

        let b = read(&store, 5).unwrap();
        assert_eq!((b.first, b.last, b.rows.len()), (1, 5, 5));
        assert_eq!(b.fork, None);
        cursor(&mut store, "1:A", 5);
        let b = read(&store, 5).unwrap();
        assert_eq!((b.first, b.last), (6, 10));
        assert_eq!(pending(&store), 7);
        cursor(&mut store, "1:A", 10);
        let b = read(&store, 5).unwrap();
        assert_eq!((b.first, b.last, b.rows.len()), (11, 12, 2));
        cursor(&mut store, "1:A", 12);
        assert_eq!(read(&store, 5), None);
        assert_eq!(pending(&store), 0);
    }

    #[test]
    fn a_batch_never_crosses_an_epoch_and_epochs_order_numerically() {
        let (_d, mut store) = store();
        put(&mut store, "9:Z", 1..=3);
        put(&mut store, "10:A", 1..=2);
        put(&mut store, "10:B", 1..=1);
        assert_eq!(pending(&store), 6);

        let b = read(&store, 5000).unwrap();
        assert_eq!((b.epoch.as_str(), b.first, b.last), ("9:Z", 1, 3));
        cursor(&mut store, "9:Z", 3);
        assert_eq!(pending(&store), 3);
        let b = read(&store, 5000).unwrap();
        assert_eq!((b.epoch.as_str(), b.first, b.last), ("10:A", 1, 2));
        cursor(&mut store, "10:A", 2);
        let b = read(&store, 5000).unwrap();
        assert_eq!((b.epoch.as_str(), b.first, b.last), ("10:B", 1, 1));
        cursor(&mut store, "10:B", 1);
        assert_eq!(read(&store, 5000), None);
        assert_eq!(pending(&store), 0);
    }

    #[test]
    fn the_first_batch_of_a_forked_epoch_starts_after_the_parents_seq() {
        let (_d, mut store) = store();
        put(&mut store, "1:A", 1..=5);
        put(&mut store, "2:B", 6..=9);
        lease(&mut store, 2, "B");
        store
            .write_tx(|tx| {
                write_lease_fork(
                    tx,
                    Some(&BatchFork {
                        parent_epoch: "1:A".into(),
                        parent_seq: 5,
                    }),
                )
            })
            .unwrap();
        cursor(&mut store, "1:A", 5);
        let b = read(&store, 5000).unwrap();
        assert_eq!((b.epoch.as_str(), b.first, b.last), ("2:B", 6, 9));
        assert_eq!(
            b.fork,
            Some(BatchFork {
                parent_epoch: "1:A".into(),
                parent_seq: 5
            })
        );
        assert_eq!(pending(&store), 4);

        // Once the cursor is in the epoch, the next batch carries no fork.
        cursor(&mut store, "2:B", 7);
        let b = read(&store, 5000).unwrap();
        assert_eq!((b.first, b.last, b.fork), (8, 9, None));
    }

    #[test]
    fn a_forked_epochs_rows_up_to_the_parent_seq_are_not_pending() {
        let (_d, mut store) = store();
        put(&mut store, "1:A", 1..=3);
        put(&mut store, "2:B", 1..=6);
        lease(&mut store, 2, "B");
        store
            .write_tx(|tx| {
                write_lease_fork(
                    tx,
                    Some(&BatchFork {
                        parent_epoch: "1:A".into(),
                        parent_seq: 4,
                    }),
                )
            })
            .unwrap();
        // The parent's rows first, then 2:B from seq 5.
        assert_eq!(pending(&store), 3 + 2);
        cursor(&mut store, "1:A", 3);
        assert_eq!(pending(&store), 2);
        let b = read(&store, 5000).unwrap();
        assert_eq!((b.epoch.as_str(), b.first, b.last), ("2:B", 5, 6));
    }

    #[test]
    fn a_fork_only_applies_to_the_leases_own_epoch() {
        let (_d, mut store) = store();
        put(&mut store, "1:A", 1..=2);
        put(&mut store, "2:B", 3..=4);
        lease(&mut store, 3, "C");
        store
            .write_tx(|tx| {
                write_lease_fork(
                    tx,
                    Some(&BatchFork {
                        parent_epoch: "2:B".into(),
                        parent_seq: 4,
                    }),
                )
            })
            .unwrap();
        // Nothing is in 3:C; the older epochs ship from their first rows.
        let b = read(&store, 5000).unwrap();
        assert_eq!(
            (b.epoch.as_str(), b.first, b.last, b.fork),
            ("1:A", 1, 2, None)
        );
        cursor(&mut store, "1:A", 2);
        let b = read(&store, 5000).unwrap();
        assert_eq!(
            (b.epoch.as_str(), b.first, b.last, b.fork),
            ("2:B", 3, 4, None)
        );
    }

    #[test]
    fn a_hole_in_the_log_is_an_error_not_a_batch() {
        let (_d, mut store) = store();
        put(&mut store, "1:A", [1, 2, 4, 5]);
        // The first batch stops before the hole.
        let b = read(&store, 5000).unwrap();
        assert_eq!((b.first, b.last, b.rows.len()), (1, 2, 2));
        cursor(&mut store, "1:A", 2);
        let err = store
            .read_with(|c| read_pending_batch(c, 5000))
            .unwrap_err();
        assert!(err.message.contains("gap"), "{err}");
    }

    fn range(store: &Store, epoch: &str, from: i64, to: i64, limit: usize) -> RangeRead {
        store
            .read_with(|c| read_range_batch(c, epoch, from, to, limit))
            .unwrap()
    }

    #[test]
    fn a_resend_reads_the_asked_rows_up_to_the_limit_and_the_end() {
        let (_d, mut store) = store();
        put(&mut store, "1:A", 1..=12);
        let RangeRead::Batch(b) = range(&store, "1:A", 4, 9, 5000) else {
            panic!("rows are held");
        };
        assert_eq!((b.first, b.last, b.rows.len(), b.fork), (4, 9, 6, None));
        let RangeRead::Batch(b) = range(&store, "1:A", 4, 12, 3) else {
            panic!("rows are held");
        };
        assert_eq!((b.first, b.last, b.rows.len()), (4, 6, 3));
        let RangeRead::Batch(b) = range(&store, "1:A", 11, 99, 5000) else {
            panic!("rows are held");
        };
        assert_eq!((b.first, b.last), (11, 12), "never past what the log holds");
    }

    #[test]
    fn pruned_rows_are_unavailable_up_to_the_next_row_or_the_end() {
        let (_d, mut store) = store();
        put(&mut store, "1:A", 6..=9);
        assert_eq!(
            range(&store, "1:A", 2, 8, 5000),
            RangeRead::Unavailable { from: 2, to: 5 }
        );
        assert_eq!(
            range(&store, "1:A", 10, 14, 5000),
            RangeRead::Unavailable { from: 10, to: 14 }
        );
        assert_eq!(
            range(&store, "2:B", 1, 3, 5000),
            RangeRead::Unavailable { from: 1, to: 3 },
            "another epoch has nothing"
        );
    }

    #[test]
    fn a_hole_inside_the_log_ends_the_resend_batch() {
        let (_d, mut store) = store();
        put(&mut store, "1:A", [1, 2, 3, 5, 6]);
        let RangeRead::Batch(b) = range(&store, "1:A", 1, 6, 5000) else {
            panic!("rows are held");
        };
        assert_eq!((b.first, b.last), (1, 3));
        assert_eq!(
            range(&store, "1:A", 4, 6, 5000),
            RangeRead::Unavailable { from: 4, to: 4 }
        );
    }

    #[test]
    fn the_first_resend_batch_of_a_forked_epoch_carries_the_fork() {
        let (_d, mut store) = store();
        put(&mut store, "2:B", 6..=9);
        lease(&mut store, 2, "B");
        let fork = BatchFork {
            parent_epoch: "1:A".into(),
            parent_seq: 5,
        };
        store
            .write_tx(|tx| write_lease_fork(tx, Some(&fork)))
            .unwrap();
        let RangeRead::Batch(b) = range(&store, "2:B", 6, 7, 5000) else {
            panic!("rows are held");
        };
        assert_eq!(b.fork, Some(fork));
        let RangeRead::Batch(b) = range(&store, "2:B", 7, 9, 5000) else {
            panic!("rows are held");
        };
        assert_eq!(b.fork, None);
    }

    #[test]
    fn the_heartbeat_position_is_the_cursor_in_the_leases_epoch_else_the_forks_parent() {
        let (_d, mut store) = store();
        lease(&mut store, 2, "B");
        let progress = |store: &Store| store.read_with(read_epoch_progress).unwrap();
        assert_eq!(progress(&store), EpochProgress::default());

        cursor(&mut store, "1:A", 40);
        assert_eq!(progress(&store).last_seq_uploaded, 0, "an older epoch");

        store
            .write_tx(|tx| {
                write_lease_fork(
                    tx,
                    Some(&BatchFork {
                        parent_epoch: "1:A".into(),
                        parent_seq: 40,
                    }),
                )
            })
            .unwrap();
        assert_eq!(
            progress(&store),
            EpochProgress {
                last_seq_uploaded: 40,
                fork_seq: Some(40),
                base_seq: None,
            }
        );

        cursor(&mut store, "2:B", 55);
        assert_eq!(
            progress(&store),
            EpochProgress {
                last_seq_uploaded: 55,
                fork_seq: Some(40),
                base_seq: None,
            }
        );
    }

    #[test]
    fn the_upgrade_snapshots_last_seq_is_reported_only_in_the_leases_epoch() {
        let (_d, mut store) = store();
        lease(&mut store, 2, "B");
        let progress = |store: &Store| store.read_with(read_epoch_progress).unwrap();
        store
            .write_tx(|tx| crate::complete_first_snapshot(tx, "2:B", 30, 1))
            .unwrap();
        let p = progress(&store);
        assert_eq!((p.last_seq_uploaded, p.base_seq), (30, Some(30)));
        // A snapshot that ended in an older epoch says nothing about this one.
        store
            .write_tx(|tx| crate::complete_first_snapshot(tx, "1:A", 30, 1))
            .unwrap();
        assert_eq!(progress(&store).base_seq, None);
    }
}
