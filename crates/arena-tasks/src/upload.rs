//! Ships the branch's change log to the cloud (Story 8.2).
//!
//! A paid primary Hub wakes every `upload_interval_s` of its licence (never under 30 s), or as
//! soon as 500 change rows are waiting, and uploads what the `upload_cursor` has not covered:
//! up to 5,000 rows of one epoch per batch, as a zstd file (see `arena_store::encode_batch`),
//! PUT to a link `POST /v1/presign` issued for exactly that `first`..`last`. A backlog goes back
//! to back, one batch after another. The cursor moves, through the Writer, only after the PUT
//! answered 2xx; a failed cycle leaves it, so the next one rebuilds the same batch under the
//! same key (a repeated PUT of the same bytes is harmless).
//!
//! After a day's rollup has published, the same cycle stores that day's snapshot (Story 8.8): a
//! zstd copy of the database without the player key table, PUT to `snapshots/<date>.db.zst`.
//!
//! A branch that goes from Free to paid uploads a full snapshot first (Story 9.11): until its
//! upload cursor exists, the cycle waits for the player key backup, stores a snapshot of today
//! under `snapshots/<today>.db.zst` and only then sets the cursor to the snapshot's last change
//! row, so batching starts one after it. Until then `backup_state` says "first snapshot pending"
//! with the last error and no batch goes up. A lapse back to Free forgets the cursor, so the next
//! upgrade starts again with a fresh snapshot.
//!
//! A Free Hub (or one whose tier is not known yet), a follower and an unpaired Hub make no
//! request at all. The task reads through the [`SnapshotReader`], never inside `apply()`.

use std::collections::{BTreeSet, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_core::{
    HubStatus, PairGate, PairingState, Role, SnapshotReader, UploadStatus, WriterHandle,
};
use arena_proto::{
    AlertKind, AppError, ErrorCode, GstArchivePublish, PRESIGN_KIND_GST_ARCHIVE,
    PRESIGN_KIND_ROLLUP, PRESIGN_KIND_SNAPSHOT, PRESIGN_TTL_S, PresignBatch, PresignRequest,
    ROLLUP_SCHEMA, ResendUnavailable, RollupV1, SUMMARY_OBJECT_KEY, SeqRange, Tier, VerifyKey,
    emitted_rollup_schema, gst_archive_object_key, licence::verify_claims, rollup_object_key,
    snapshot_object_key,
};
use arena_store::{
    BATCH_MAX_ROWS, ChangedRow, ROLLUP_LEDGER_TABLES, RangeRead, SnapshotLogRow, encode_batch,
};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use crate::cloud::{CloudClient, CloudFailure};
use crate::heartbeat::{REVOKED_BACKOFF, backoff, unit};
use crate::keystore::Keystore;
use crate::template::{NOT_ENTITLED_RETRY, TIER_RECHECK};

/// The shortest time between two scheduled uploads, whatever the licence says.
pub const MIN_UPLOAD_INTERVAL: Duration = Duration::from_secs(30);
/// Waiting change rows that wake the task before its interval is up.
pub const BACKLOG_TRIGGER_ROWS: i64 = 500;
/// How much sooner than its expiry a summary link carried from a batch presign stops being used.
const SUMMARY_LINK_MARGIN_S: u64 = 60;
/// The most daily rollups one cycle builds and publishes; a longer catch-up goes on next cycle.
const ROLLUPS_PER_CYCLE: usize = 10;
/// The wait before the next cycle of a catch-up that is not finished.
const ROLLUP_CATCH_UP_WAIT: Duration = Duration::from_secs(60);
/// The most old-schema days one Hub regenerates in a minute (Story 8.7).
const REGENERATIONS_PER_MINUTE: usize = 10;
const REGENERATION_WINDOW_MS: i64 = 60_000;
/// The daily snapshot needs this many times the database's size free on its disk (Story 8.8).
const SNAPSHOT_DISK_FACTOR: u64 = 2;
/// The most ledger changes remembered between two rollup plans; more means a rescan.
const MAX_HEARD_CHANGES: usize = 5_000;
/// How long to wait when the cloud limits the calls but names no time.
const RATE_LIMIT_WAIT: Duration = Duration::from_secs(60);
/// The longest Retry-After honoured.
const RATE_LIMIT_CEILING: Duration = Duration::from_secs(3600);
/// `backup_state.first_snapshot_error` while the player key backup has not been stored yet.
const FIRST_SNAPSHOT_KEYS_PENDING: &str = "KEY_BACKUP_PENDING";
/// `backup_state.first_snapshot_error` while the disk is too full for the copy.
const FIRST_SNAPSHOT_DISK: &str = "BACKUP_DISK";

/// What the task does after a cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Next {
    /// How long to sleep.
    pub wait: Duration,
    /// This Hub is uploading (paid, primary): waiting rows are counted.
    pub active: bool,
    /// A pile of waiting rows may cut the wait short (the last cycle did not fail).
    pub early: bool,
}

impl Next {
    fn idle(wait: Duration) -> Self {
        Self {
            wait,
            active: false,
            early: false,
        }
    }

    fn backing_off(wait: Duration) -> Self {
        Self {
            wait,
            active: true,
            early: false,
        }
    }
}

/// The run of the change log the cloud asked for again (Story 8.3) and the run the Hub could not
/// give. The heartbeat task fills it from its answers and reports from it; the uploader empties it.
#[derive(Default)]
pub struct ResendInbox {
    state: Mutex<ResendState>,
}

#[derive(Default)]
struct ResendState {
    pending: Option<(String, SeqRange)>,
    unavailable: Option<ResendUnavailable>,
}

impl ResendInbox {
    pub fn new() -> Arc<Self> {
        Arc::default()
    }

    fn with<R>(&self, f: impl FnOnce(&mut ResendState) -> R) -> R {
        f(&mut self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner))
    }

    /// The cloud asked for the range of the epoch. The same ask again (the answer to a heartbeat
    /// sent before the resend landed) keeps the progress already made.
    pub fn offer(&self, epoch: &str, range: SeqRange) {
        if !range.valid() {
            return;
        }
        self.with(|s| {
            let same = s
                .pending
                .as_ref()
                .is_some_and(|(e, r)| e == epoch && r.to == range.to && range.from <= r.from);
            if !same {
                s.pending = Some((epoch.to_owned(), range));
            }
        });
    }

    /// What is still to resend.
    pub fn pending(&self) -> Option<(String, SeqRange)> {
        self.with(|s| s.pending.clone())
    }

    /// Everything up to the given seq of the pending run is done.
    fn advance(&self, through: i64) {
        self.with(|s| {
            if let Some((_, r)) = s.pending.as_mut() {
                r.from = r.from.max(through.saturating_add(1));
                if r.from > r.to {
                    s.pending = None;
                }
            }
        });
    }

    fn drop_pending(&self) {
        self.with(|s| s.pending = None);
    }

    pub(crate) fn report_unavailable(&self, u: ResendUnavailable) {
        self.with(|s| {
            s.unavailable = Some(match s.unavailable.take() {
                Some(held) => held.merged(&u),
                None => u,
            });
        });
    }

    /// The run to tell the cloud about in the next heartbeat.
    pub fn unavailable(&self) -> Option<ResendUnavailable> {
        self.with(|s| s.unavailable.clone())
    }

    /// The heartbeat that carried the run was answered: it is not reported again.
    pub fn acknowledge(&self, sent: &ResendUnavailable) {
        self.with(|s| {
            if s.unavailable.as_ref() == Some(sent) {
                s.unavailable = None;
            }
        });
    }
}

/// The longest an owner's `backup.now` command waits for its upload cycle (Story 8.19).
pub const BACKUP_NOW_WAIT: Duration = Duration::from_secs(30);

pub(crate) type BackupReply = oneshot::Sender<Result<(), AppError>>;

/// Asks the upload task for an immediate cycle and waits for its result. Cloneable; the command
/// task holds one.
#[derive(Clone)]
pub struct BackupRequester {
    tx: mpsc::UnboundedSender<BackupReply>,
}

/// The upload task's end of the `backup.now` channel.
pub struct BackupResponder {
    pub(crate) rx: mpsc::UnboundedReceiver<BackupReply>,
}

pub fn backup_channel() -> (BackupRequester, BackupResponder) {
    let (tx, rx) = mpsc::unbounded_channel();
    (BackupRequester { tx }, BackupResponder { rx })
}

impl BackupRequester {
    /// Runs one upload and summary cycle at once and answers its outcome: `Ok` when its uploads
    /// succeeded (nothing waiting counts), else the transport error. Gives up after `wait` with
    /// `HUB_SLOW`, and answers `FEATURE_NOT_ENTITLED` when there is no upload task to ask.
    pub async fn run(&self, wait: Duration) -> Result<(), AppError> {
        let (reply, answer) = oneshot::channel();
        if self.tx.send(reply).is_err() {
            return Err(AppError::new(
                ErrorCode::FeatureNotEntitled,
                "this branch does not upload",
            ));
        }
        match tokio::time::timeout(wait, answer).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(AppError::new(
                ErrorCode::HubSlow,
                "the upload stopped before it answered",
            )),
            Err(_) => Err(AppError::new(
                ErrorCode::HubSlow,
                "the upload did not finish in time",
            )),
        }
    }
}

/// The result recorded for a cycle that did not run to its end until something says why.
fn unfinished() -> AppError {
    AppError::new(ErrorCode::Internal, "the upload did not finish")
}

/// A batch read and encoded, ready to presign and PUT.
struct Built {
    epoch: String,
    first: i64,
    last: i64,
    rows: usize,
    bytes: Vec<u8>,
}

fn build(snapshots: &SnapshotReader, limit: usize, schema: u32) -> Result<Option<Built>, AppError> {
    let Some(batch) = snapshots.pending_batch(limit)? else {
        return Ok(None);
    };
    let bytes = encode_batch(schema, &batch)?;
    Ok(Some(Built {
        rows: batch.rows.len(),
        epoch: batch.epoch,
        first: batch.first,
        last: batch.last,
        bytes,
    }))
}

/// Builds the rollup of a day in a schema; the snapshot reader's writers unless a test swaps it.
type RollupBuilder = fn(&SnapshotReader, &str, i64, u32) -> Result<Option<RollupV1>, AppError>;

pub struct UploadTask {
    cloud: CloudClient,
    keystore: Arc<dyn Keystore>,
    writer: Option<WriterHandle>,
    snapshots: Arc<SnapshotReader>,
    gate: PairGate,
    status: watch::Sender<HubStatus>,
    /// The licence verify keys, to read `upload_interval_s` from the stored licence.
    keys: Vec<VerifyKey<'static>>,
    batch_rows: usize,
    trigger_rows: i64,
    trigger_gap: Duration,
    fails: u32,
    resend: Arc<ResendInbox>,
    /// The summary link a batch presign brought along, and when it stops being usable.
    summary_link: Option<(String, tokio::time::Instant)>,
    /// When the summary was last published (this run).
    last_summary: Option<tokio::time::Instant>,
    /// Whether the cycle also publishes the branch summary (Story 8.4); tests of the change-log
    /// upload alone turn it off.
    summary: bool,
    /// Whether the cycle also publishes the daily rollups (Story 8.6).
    rollups: bool,
    /// Ledger changes heard since the last rollup plan (a late Undo re-opens a published day).
    heard: Vec<ChangedRow>,
    /// More changes were heard than are remembered: every recently published day is re-checked.
    rescan: bool,
    /// Days with a ledger change whose rollup was not (re)published yet.
    dirty: BTreeSet<String>,
    /// The newest rollup schema this Hub writes (N); the Hub keeps writers for N and N−1.
    newest_schema: u32,
    build_rollup: RollupBuilder,
    /// When (Hub clock, ms) old-schema days were last regenerated, within the last minute.
    regenerated: VecDeque<i64>,
    /// Old-schema days that could not be built this run; they do not hold up the days behind.
    unbuildable: BTreeSet<String>,
    /// Whether the cycle also stores the daily snapshot (Story 8.8).
    daily_snapshot: bool,
    /// Whether the cycle also publishes the yearly GST archives (Story 9.14).
    gst_archive: bool,
    /// The latest rollup day the archives were last worked out for in this run, once every
    /// archive due then went up; a day not yet checked, or an attempt that failed, is retried.
    gst_checked: Option<String>,
    /// Whether a branch that has never uploaded stores its full snapshot first (Story 9.11).
    first_snapshot: bool,
    /// The lapse back to Free was already looked at (Story 9.11); a paid cycle resets it.
    lapse_seen: bool,
    /// The free bytes on the disk holding a folder; tests swap it.
    free_space: fn(&Path) -> Option<u64>,
    /// The day the "not enough disk space" alert was already raised for in this run, so a disk
    /// that stays full does not raise one every cycle.
    disk_alerted: Option<String>,
    /// Where `backup.now` commands ask for an immediate cycle (Story 8.19).
    backup: Option<BackupResponder>,
    /// Why the last cycle did not finish; `None` when it ran to its end.
    outcome: Option<AppError>,
}

impl UploadTask {
    pub fn new(
        cloud: CloudClient,
        keystore: Arc<dyn Keystore>,
        writer: Option<WriterHandle>,
        snapshots: Arc<SnapshotReader>,
        gate: PairGate,
        status: watch::Sender<HubStatus>,
        keys: Vec<VerifyKey<'static>>,
    ) -> Self {
        Self {
            cloud,
            keystore,
            writer,
            snapshots,
            gate,
            status,
            keys,
            batch_rows: BATCH_MAX_ROWS,
            trigger_rows: BACKLOG_TRIGGER_ROWS,
            trigger_gap: MIN_UPLOAD_INTERVAL,
            fails: 0,
            resend: ResendInbox::new(),
            summary_link: None,
            last_summary: None,
            summary: true,
            rollups: true,
            heard: Vec::new(),
            // A restart forgets the changes heard before it: look at the recent days once.
            rescan: true,
            dirty: BTreeSet::new(),
            newest_schema: ROLLUP_SCHEMA,
            build_rollup: SnapshotReader::daily_rollup,
            regenerated: VecDeque::new(),
            unbuildable: BTreeSet::new(),
            daily_snapshot: true,
            gst_archive: true,
            gst_checked: None,
            first_snapshot: true,
            lapse_seen: false,
            free_space: crate::disk::free_bytes,
            disk_alerted: None,
            backup: None,
            outcome: None,
        }
    }

    /// Where `backup.now` commands ask for an immediate cycle (Story 8.19).
    pub fn with_backup(mut self, responder: BackupResponder) -> Self {
        self.backup = Some(responder);
        self
    }

    /// Where the heartbeat leaves the runs the cloud asked for again (Story 8.3).
    pub fn with_resend(mut self, inbox: Arc<ResendInbox>) -> Self {
        self.resend = inbox;
        self
    }

    /// Tests: the cycle uploads the change log only.
    pub fn without_summary(mut self) -> Self {
        self.summary = false;
        self.rollups = false;
        self.daily_snapshot = false;
        self.gst_archive = false;
        self.first_snapshot = false;
        self
    }

    /// Tests: the cycle does not publish the yearly GST archives.
    pub fn without_gst_archive(mut self) -> Self {
        self.gst_archive = false;
        self
    }

    /// Tests: the cycle does not store the daily snapshot.
    pub fn without_snapshot(mut self) -> Self {
        self.daily_snapshot = false;
        self.first_snapshot = false;
        self
    }

    /// Tests: how the free room on a disk is read.
    pub fn with_free_space(mut self, free_space: fn(&Path) -> Option<u64>) -> Self {
        self.free_space = free_space;
        self
    }

    /// Remembers the ledger rows an applied intent changed, for the rollup plan.
    fn hear(&mut self, applied: &arena_core::Applied) {
        for c in applied
            .changes
            .iter()
            .filter(|c| ROLLUP_LEDGER_TABLES.contains(&c.tbl.as_str()))
        {
            if self.heard.len() >= MAX_HEARD_CHANGES {
                self.rescan = true;
                return;
            }
            self.heard.push(ChangedRow {
                epoch: c.epoch.clone(),
                seq: c.seq,
                tbl: c.tbl.clone(),
                row_id: c.row_id.clone(),
            });
        }
    }

    /// Tests: the rows per batch.
    pub fn with_batch_rows(mut self, rows: usize) -> Self {
        self.batch_rows = rows.clamp(1, BATCH_MAX_ROWS);
        self
    }

    /// Tests: how many waiting rows wake the task early, and the least time after a cycle they
    /// may do so.
    pub fn with_trigger(mut self, rows: i64, gap: Duration) -> Self {
        self.trigger_rows = rows;
        self.trigger_gap = gap;
        self
    }

    pub async fn run(mut self) {
        let mut paired = self.gate.subscribe();
        if paired.wait_for(|p| *p).await.is_err() {
            return;
        }
        let Some(writer) = self.writer.clone() else {
            return;
        };
        let mut events = writer.subscribe();
        let mut backup = self.backup.take().map(|b| b.rx);
        let mut waiting: Vec<BackupReply> = Vec::new();
        loop {
            if let Some(rx) = backup.as_mut() {
                while let Ok(reply) = rx.try_recv() {
                    waiting.push(reply);
                }
            }
            if !waiting.is_empty() {
                self.last_summary = None;
            }
            let next = self.cycle().await;
            let result = self.outcome.take();
            for reply in waiting.drain(..) {
                let _ = reply.send(result.clone().map_or(Ok(()), Err));
            }
            let ended = tokio::time::Instant::now();
            let deadline = ended + next.wait;
            let mut pending = self.status.borrow().upload.pending_rows;
            let mut triggered = next.early && pending >= self.trigger_rows;
            // A shift closing may be the last of the business day: its rollup is due at once.
            let mut shift_closed = false;
            loop {
                let wake = if triggered || shift_closed {
                    deadline.min(ended + self.trigger_gap)
                } else {
                    deadline
                };
                tokio::select! {
                    () = tokio::time::sleep_until(wake) => break,
                    reply = async {
                        match backup.as_mut() {
                            Some(rx) => rx.recv().await,
                            None => std::future::pending().await,
                        }
                    } => match reply {
                        Some(reply) => {
                            waiting.push(reply);
                            break;
                        }
                        None => backup = None,
                    },
                    event = events.recv() => {
                        match event {
                            Ok(applied) => {
                                pending += applied.changes.len() as i64;
                                if self.rollups && next.active {
                                    self.hear(&applied);
                                    shift_closed |= applied.changes.iter().any(|c| c.tbl == "shifts");
                                } else if self.rollups {
                                    // Not listening now: look at the recent days on resuming.
                                    self.rescan = true;
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => {
                                // Dropped events may hold a late ledger change.
                                self.rescan |= self.rollups;
                                if let Some(p) = self.progress().await {
                                    pending = p.pending_rows;
                                }
                            }
                            Err(broadcast::error::RecvError::Closed) => return,
                        }
                        if next.active {
                            self.status.send_if_modified(|s| {
                                let changed = s.upload.pending_rows != pending;
                                s.upload.pending_rows = pending;
                                changed
                            });
                            triggered |= next.early && pending >= self.trigger_rows;
                        }
                    }
                }
            }
        }
    }

    /// One wake: uploads every waiting batch, one after another, until nothing is left or one
    /// fails. Answers what to do next.
    pub async fn cycle(&mut self) -> Next {
        self.outcome = Some(unfinished());
        let (role, lease, pairing, tier, schema) = {
            let s = self.status.borrow();
            (s.role, s.lease.clone(), s.pairing, s.tier, s.schema_version)
        };
        let (Some(writer), Some(lease), Role::Primary) = (self.writer.clone(), lease, role) else {
            self.outcome = Some(AppError::new(
                ErrorCode::NotPrimary,
                "this hub is not the primary",
            ));
            return Next::idle(TIER_RECHECK);
        };
        if pairing == PairingState::Revoked {
            self.outcome = Some(AppError::new(
                ErrorCode::TokenRevoked,
                "this hub is no longer paired",
            ));
            return Next::idle(REVOKED_BACKOFF);
        }
        let Some(tier) = tier.filter(|t| *t != Tier::Free) else {
            if tier == Some(Tier::Free) {
                self.stop_uploads(&writer).await;
            }
            self.outcome = Some(AppError::new(
                ErrorCode::FeatureNotEntitled,
                "this branch does not upload",
            ));
            return Next::idle(TIER_RECHECK);
        };
        self.lapse_seen = false;
        let keystore = Arc::clone(&self.keystore);
        let token = match tokio::task::spawn_blocking(move || keystore.get()).await {
            Ok(Ok(Some(t))) => String::from_utf8_lossy(&t).into_owned(),
            other => {
                tracing::warn!(ok = other.is_ok(), "upload: no hub token");
                self.outcome = Some(AppError::new(
                    ErrorCode::TokenInvalid,
                    "this hub has no token to upload with",
                ));
                return Next::idle(TIER_RECHECK);
            }
        };
        let interval = self.interval(&writer, tier).await;
        let summary_due =
            self.summary && self.summary_due(self.summary_interval(&writer, tier).await);
        let epoch = lease.epoch().to_string();
        if self.first_snapshot
            && let Err(next) = self
                .first_snapshot_pass(&writer, &token, &epoch, interval)
                .await
        {
            self.refresh_progress().await;
            return next;
        }
        if let Some(next) = self.resend_pass(&token, &epoch, interval, schema).await {
            self.refresh_progress().await;
            return next;
        }
        loop {
            let snapshots = Arc::clone(&self.snapshots);
            let rows = self.batch_rows;
            let built = match tokio::task::spawn_blocking(move || build(&snapshots, rows, schema))
                .await
            {
                Ok(Ok(Some(b))) => b,
                Ok(Ok(None)) => break,
                Ok(Err(e)) => {
                    tracing::warn!(code = e.code.as_str(), error = %e, "upload: could not read a batch");
                    self.refresh_progress().await;
                    return self.failed(interval);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "upload: read task failed");
                    return self.failed(interval);
                }
            };
            if let Some(next) = self
                .send(&writer, &token, built, interval, summary_due)
                .await
            {
                self.refresh_progress().await;
                return next;
            }
            self.refresh_progress().await;
        }
        if summary_due
            && let Some(next) = self
                .publish_summary(&writer, &token, &epoch, interval)
                .await
        {
            self.refresh_progress().await;
            return next;
        }
        let mut wait = interval;
        if self.rollups {
            let target = self.rollup_schema(&writer, tier).await;
            match self
                .publish_rollups(&writer, &token, &epoch, interval, target)
                .await
            {
                Ok(sooner) => wait = sooner.map_or(interval, |s| s.min(interval)),
                Err(next) => {
                    self.refresh_progress().await;
                    return next;
                }
            }
        }
        // The snapshot follows the day's rollup, never ahead of it.
        if self.daily_snapshot
            && let Err(next) = self
                .publish_snapshot(&writer, &token, &epoch, interval)
                .await
        {
            self.refresh_progress().await;
            return next;
        }
        // The yearly GST archive follows the first close after the financial year ends.
        if self.gst_archive
            && let Err(next) = self
                .publish_gst_archives(&writer, &token, &epoch, interval)
                .await
        {
            self.refresh_progress().await;
            return next;
        }
        self.refresh_progress().await;
        self.outcome = None;
        Next {
            wait,
            active: true,
            early: true,
        }
    }

    /// A lapse back to Free (Story 9.11): uploads stop at once and the cursor is forgotten, so
    /// re-upgrading starts again with a fresh full snapshot. Objects already stored stay in the
    /// cloud. Looked at once per lapse.
    async fn stop_uploads(&mut self, writer: &WriterHandle) {
        if std::mem::replace(&mut self.lapse_seen, true) {
            return;
        }
        match writer.stop_uploads().await {
            Ok(true) => tracing::info!("upload: the branch is on Free, uploads stopped"),
            Ok(false) => {}
            // Not the primary any more; the next lease holder looks at it.
            Err(e) if e.code == ErrorCode::NotPrimary => {}
            Err(e) => {
                tracing::warn!(code = e.code.as_str(), "upload: could not stop the uploads");
                self.lapse_seen = false;
            }
        }
    }

    /// The first snapshot of a branch that has never uploaded (Story 9.11): waits for the player
    /// key backup, stores a full snapshot of today under `snapshots/<today>.db.zst`, records it
    /// in `snapshot_log` and moves the cursor to its last change row, so the batches that follow
    /// start one after it. `Ok` when the cursor exists and the cycle goes on; `Err` is what to do
    /// instead, with `backup_state` saying "first snapshot pending" and the last error.
    async fn first_snapshot_pass(
        &mut self,
        writer: &WriterHandle,
        token: &str,
        lease_epoch: &str,
        interval: Duration,
    ) -> Result<(), Next> {
        let snapshots = Arc::clone(&self.snapshots);
        let now = writer.now_ms();
        let start = match tokio::task::spawn_blocking(move || snapshots.upload_start(now)).await {
            Ok(Ok(start)) => start,
            Ok(Err(e)) => {
                tracing::warn!(code = e.code.as_str(), error = %e, "first snapshot: could not plan");
                return Err(self.failed(interval));
            }
            Err(e) => {
                tracing::warn!(error = %e, "first snapshot: plan task failed");
                return Err(self.failed(interval));
            }
        };
        if start.started {
            return Ok(());
        }
        let date = start.today;
        let known = start.state.first_snapshot_error.clone();
        if !start.state.first_snapshot_pending {
            self.note_first_snapshot(writer, None, None).await;
        }
        // The key backup goes up first: a snapshot without it could not be opened after a loss.
        if start.state.keys_dirty() {
            self.note_first_snapshot(writer, Some(FIRST_SNAPSHOT_KEYS_PENDING), known.as_deref())
                .await;
            return Err(Next::backing_off(TIER_RECHECK));
        }
        let snapshots = Arc::clone(&self.snapshots);
        let scratch = tokio::task::spawn_blocking(move || snapshots.snapshot_scratch()).await;
        let Ok(Ok((dir, db_bytes))) = scratch else {
            tracing::warn!("first snapshot: could not find the database folder");
            self.note_first_snapshot(writer, Some(ErrorCode::Internal.as_str()), known.as_deref())
                .await;
            return Err(self.failed(interval));
        };
        let free = (self.free_space)(&dir);
        let needed = db_bytes.saturating_mul(SNAPSHOT_DISK_FACTOR);
        if free.is_some_and(|f| f < needed) {
            tracing::warn!(date, free, needed, "first snapshot: not enough disk space");
            self.alert_disk_low(writer, &date).await;
            self.note_first_snapshot(writer, Some(FIRST_SNAPSHOT_DISK), known.as_deref())
                .await;
            return Err(Next::backing_off(interval));
        }
        let snapshots = Arc::clone(&self.snapshots);
        let build_date = date.clone();
        let built = tokio::task::spawn_blocking(move || {
            let file = snapshots.snapshot_file(&dir, &build_date)?;
            let bytes = file.read()?;
            Ok::<_, AppError>((bytes, file.epoch.clone(), file.last_seq))
        })
        .await;
        let (bytes, snapshot_epoch, last_seq) = match built {
            Ok(Ok(built)) => built,
            other => {
                let code = match &other {
                    Ok(Err(e)) => e.code,
                    _ => ErrorCode::Internal,
                };
                tracing::warn!(
                    date,
                    code = code.as_str(),
                    "first snapshot: could not build"
                );
                self.note_first_snapshot(writer, Some(code.as_str()), known.as_deref())
                    .await;
                return Err(self.failed(interval));
            }
        };
        let req = PresignRequest {
            epoch: lease_epoch.to_owned(),
            kind: Some(PRESIGN_KIND_SNAPSHOT.to_owned()),
            date: Some(date.clone()),
            ..PresignRequest::default()
        };
        let links = match self.cloud.presign(token, &req).await {
            Ok(l) => l,
            Err(f) => {
                return Err(self
                    .first_snapshot_refused(writer, f, known, interval)
                    .await);
            }
        };
        let Some(url) = links.url else {
            tracing::warn!(date, "first snapshot: the cloud issued no snapshot link");
            self.note_first_snapshot(writer, Some(ErrorCode::Internal.as_str()), known.as_deref())
                .await;
            return Err(self.failed(interval));
        };
        let size = bytes.len();
        if let Err(f) = self.cloud.put_object(&url, bytes).await {
            return Err(self
                .first_snapshot_refused(writer, f, known, interval)
                .await);
        }
        self.fails = 0;
        // A branch whose log holds no row yet starts at the lease's epoch, seq 0.
        let (cursor_epoch, cursor_seq) = if snapshot_epoch.is_empty() {
            (lease_epoch.to_owned(), 0)
        } else {
            (snapshot_epoch.clone(), last_seq)
        };
        let row = SnapshotLogRow {
            key: snapshot_object_key(&date),
            date: date.clone(),
            epoch: snapshot_epoch,
            last_seq,
            bytes: i64::try_from(size).unwrap_or(i64::MAX),
            uploaded_at: writer.now_ms(),
        };
        match writer
            .record_first_snapshot(row, cursor_epoch, cursor_seq)
            .await
        {
            Ok(()) => {
                tracing::info!(
                    date,
                    last_seq = cursor_seq,
                    bytes = size,
                    "first snapshot stored"
                );
                Ok(())
            }
            Err(e) if e.code == ErrorCode::NotPrimary => {
                tracing::info!("first snapshot: this hub is no longer the primary");
                Err(Next::idle(TIER_RECHECK))
            }
            Err(e) => {
                // The cloud has the file; the next cycle sends it again under the same key.
                tracing::warn!(
                    date,
                    code = e.code.as_str(),
                    "first snapshot: could not record the upload"
                );
                Err(self.failed(interval))
            }
        }
    }

    /// Marks the first snapshot pending in `backup_state`. An `error` equal to the one already
    /// held (`known`) is not written again.
    async fn note_first_snapshot(
        &self,
        writer: &WriterHandle,
        error: Option<&str>,
        known: Option<&str>,
    ) {
        if error.is_some() && error == known {
            return;
        }
        if let Err(e) = writer
            .record_first_snapshot_pending(error.map(str::to_owned))
            .await
        {
            tracing::warn!(
                code = e.code.as_str(),
                "first snapshot: could not record the state"
            );
        }
    }

    /// The cloud refused or could not be reached while the first snapshot was going up.
    async fn first_snapshot_refused(
        &mut self,
        writer: &WriterHandle,
        f: CloudFailure,
        known: Option<String>,
        interval: Duration,
    ) -> Next {
        let code = match &f {
            CloudFailure::Refused { error, .. } => error.code,
            CloudFailure::Unreachable(_) => ErrorCode::CloudUnreachable,
        };
        self.note_first_snapshot(writer, Some(code.as_str()), known.as_deref())
            .await;
        self.refused(f, interval)
    }

    /// Sends again the run the cloud asked for (Story 8.3), batch by batch under keys for exactly
    /// its seqs, and never touches the cursor. Rows that were pruned are left for the next
    /// heartbeat to report. `None` when nothing is left to do; otherwise what to do instead.
    async fn resend_pass(
        &mut self,
        token: &str,
        lease_epoch: &str,
        interval: Duration,
        schema: u32,
    ) -> Option<Next> {
        while let Some((epoch, range)) = self.resend.pending() {
            if epoch != lease_epoch {
                tracing::info!(%epoch, "upload: dropping a resend of another epoch");
                self.resend.drop_pending();
                return None;
            }
            let snapshots = Arc::clone(&self.snapshots);
            let rows = self.batch_rows;
            let read_epoch = epoch.clone();
            let read = tokio::task::spawn_blocking(move || {
                snapshots.range_batch(&read_epoch, range.from, range.to, rows)
            })
            .await;
            match read {
                Ok(Ok(RangeRead::Unavailable { from, to })) => {
                    tracing::warn!(%epoch, from, to, "upload: rows to resend are no longer held");
                    self.resend
                        .report_unavailable(ResendUnavailable { epoch, from, to });
                    self.resend.advance(to);
                }
                Ok(Ok(RangeRead::Batch(batch))) => {
                    let bytes = match encode_batch(schema, &batch) {
                        Ok(b) => b,
                        Err(e) => {
                            tracing::warn!(
                                code = e.code.as_str(),
                                "upload: could not encode a resend"
                            );
                            return Some(self.failed(interval));
                        }
                    };
                    let built = Built {
                        rows: batch.rows.len(),
                        epoch: batch.epoch,
                        first: batch.first,
                        last: batch.last,
                        bytes,
                    };
                    let (first, last, count) = (built.first, built.last, built.rows);
                    if let Err(next) = self.put(token, built, interval, false).await {
                        return Some(next);
                    }
                    tracing::info!(%epoch, first, last, rows = count, "change log batch resent");
                    self.fails = 0;
                    self.resend.advance(last);
                }
                Ok(Err(e)) => {
                    tracing::warn!(code = e.code.as_str(), error = %e, "upload: could not read a resend");
                    return Some(self.failed(interval));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "upload: resend read task failed");
                    return Some(self.failed(interval));
                }
            }
        }
        None
    }

    /// Presigns and PUTs one batch; the error is what to do instead of going on. With
    /// `with_summary` the same presign also asks for the summary link, kept for this cycle's
    /// summary publish (Story 8.4).
    async fn put(
        &mut self,
        token: &str,
        batch: Built,
        interval: Duration,
        with_summary: bool,
    ) -> Result<(), Next> {
        let req = PresignRequest {
            epoch: batch.epoch.clone(),
            batch: Some(PresignBatch {
                first: batch.first,
                last: batch.last,
            }),
            summary: with_summary,
            ..PresignRequest::default()
        };
        let links = match self.cloud.presign(token, &req).await {
            Ok(links) => links,
            Err(f) => return Err(self.refused(f, interval)),
        };
        if with_summary && let Some(url) = links.summary_url {
            let ttl = Duration::from_secs(PRESIGN_TTL_S.saturating_sub(SUMMARY_LINK_MARGIN_S));
            self.summary_link = Some((url, tokio::time::Instant::now() + ttl));
        }
        let Some(url) = links.batch_url else {
            tracing::warn!("upload: the cloud issued no batch link");
            return Err(self.failed(interval));
        };
        if let Err(f) = self.cloud.put_object(&url, batch.bytes).await {
            return Err(self.refused(f, interval));
        }
        Ok(())
    }

    /// Presigns, PUTs and records one batch. `None` when the cursor moved; otherwise what to do
    /// instead.
    async fn send(
        &mut self,
        writer: &WriterHandle,
        token: &str,
        batch: Built,
        interval: Duration,
        with_summary: bool,
    ) -> Option<Next> {
        let (epoch, first, last, rows) = (batch.epoch.clone(), batch.first, batch.last, batch.rows);
        if let Err(next) = self.put(token, batch, interval, with_summary).await {
            return Some(next);
        }
        match writer.record_upload(epoch.clone(), last).await {
            Ok(()) => {
                tracing::info!(
                    epoch = %epoch,
                    first,
                    last,
                    rows,
                    "change log batch uploaded"
                );
                self.fails = 0;
                None
            }
            Err(e) if e.code == ErrorCode::NotPrimary => {
                tracing::info!("upload: this hub is no longer the primary");
                Some(Next::idle(TIER_RECHECK))
            }
            Err(e) => {
                // The cloud has the batch; the next cycle sends it again under the same key.
                tracing::warn!(code = e.code.as_str(), "upload: could not move the cursor");
                Some(self.failed(interval))
            }
        }
    }

    /// Whether the summary is due: never published in this run, or a summary interval ago.
    fn summary_due(&self, every: Duration) -> bool {
        self.last_summary
            .is_none_or(|at| at.elapsed() + Duration::from_secs(1) >= every)
    }

    /// Builds the branch summary, PUTs it to `state/latest.json` (with the link a batch presign
    /// brought, else a presign of its own) and records the publish with `summary.publish`
    /// (Story 8.4). A summary that cannot be built or published is not queued: the next due cycle
    /// builds a fresh one. `None` when the cycle goes on; otherwise what to do instead.
    async fn publish_summary(
        &mut self,
        writer: &WriterHandle,
        token: &str,
        epoch: &str,
        interval: Duration,
    ) -> Option<Next> {
        let as_of = writer.now_ms();
        let snapshots = Arc::clone(&self.snapshots);
        let summary =
            match tokio::task::spawn_blocking(move || snapshots.branch_summary(as_of)).await {
                Ok(Ok(Some(s))) => s,
                Ok(Ok(None)) => return None,
                Ok(Err(e)) => {
                    tracing::warn!(code = e.code.as_str(), error = %e, "summary: could not build");
                    return None;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "summary: build task failed");
                    return None;
                }
            };
        let bytes = match serde_json::to_vec(&summary) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(error = %e, "summary: could not encode");
                return None;
            }
        };
        let url = match self.summary_link.take() {
            Some((url, until)) if tokio::time::Instant::now() < until => url,
            _ => {
                let req = PresignRequest {
                    epoch: epoch.to_owned(),
                    summary: true,
                    ..PresignRequest::default()
                };
                let links = match self.cloud.presign(token, &req).await {
                    Ok(l) => l,
                    Err(f) => return Some(self.refused(f, interval)),
                };
                match links.summary_url {
                    Some(url) => url,
                    None => {
                        tracing::info!(
                            fenced = links.summary_fenced.unwrap_or(false),
                            "summary: the cloud issued no summary link"
                        );
                        return None;
                    }
                }
            }
        };
        if let Err(f) = self.cloud.put_object(&url, bytes).await {
            return Some(self.refused(f, interval));
        }
        self.last_summary = Some(tokio::time::Instant::now());
        self.fails = 0;
        match writer
            .publish_summary(summary.as_of, SUMMARY_OBJECT_KEY.to_owned())
            .await
        {
            Ok(_) => {
                tracing::info!(as_of = summary.as_of, "branch summary published");
                None
            }
            Err(e) if e.code == ErrorCode::NotPrimary => {
                tracing::info!("summary: this hub is no longer the primary");
                Some(Next::idle(TIER_RECHECK))
            }
            Err(e) => {
                tracing::warn!(
                    code = e.code.as_str(),
                    "summary: could not record the publish"
                );
                None
            }
        }
    }

    /// Builds, PUTs and records the daily rollups that are due (Story 8.6): every business day
    /// since the branch went paid with no `rollup_log` row, oldest first, a day whose closing time
    /// plus 30 minutes has passed, the day whose last shift just closed, and a published day a
    /// late ledger change re-opened (its file is overwritten). At most [`ROLLUPS_PER_CYCLE`] a
    /// cycle. A day that cannot be built is skipped; a refused or failed PUT stops the pass and the
    /// day is planned again next cycle. `Ok` carries how soon the next wake should be, if sooner
    /// than the interval; `Err` what to do instead.
    async fn publish_rollups(
        &mut self,
        writer: &WriterHandle,
        token: &str,
        epoch: &str,
        interval: Duration,
        target: u32,
    ) -> Result<Option<Duration>, Next> {
        let now = writer.now_ms();
        let heard = std::mem::take(&mut self.heard);
        let rescan = std::mem::take(&mut self.rescan);
        let carried = self.dirty.clone();
        let snapshots = Arc::clone(&self.snapshots);
        let planned = {
            let heard = heard.clone();
            tokio::task::spawn_blocking(move || {
                snapshots.rollup_plan(now, &heard, &carried, rescan)
            })
            .await
        };
        let plan = match planned {
            Ok(Ok(Some(plan))) => plan,
            Ok(Ok(None)) => return Ok(None),
            Ok(Err(e)) => {
                tracing::warn!(code = e.code.as_str(), error = %e, "rollup: could not plan");
                self.heard = heard;
                self.rescan |= rescan;
                return Ok(None);
            }
            Err(e) => {
                tracing::warn!(error = %e, "rollup: plan task failed");
                self.heard = heard;
                self.rescan |= rescan;
                return Ok(None);
            }
        };
        self.dirty = plan.dirty.clone();
        // A day that cannot be built costs nothing, so it never starves the days behind it.
        let mut sent = 0;
        let mut more = false;
        for day in &plan.due {
            if sent >= ROLLUPS_PER_CYCLE {
                more = true;
                break;
            }
            sent += usize::from(
                self.publish_rollup(writer, token, epoch, interval, day, target)
                    .await?,
            );
        }
        if more {
            return Ok(Some(ROLLUP_CATCH_UP_WAIT));
        }
        // The next day to fall due by the clock: wake for it rather than a whole interval later.
        let by_clock = plan.next_at.map(|at| {
            Duration::from_millis(u64::try_from(at.saturating_sub(now)).unwrap_or(0))
                .max(Duration::from_secs(1))
        });
        let regeneration = self
            .regenerate_old_days(writer, token, epoch, interval, target)
            .await?;
        Ok(match (by_clock, regeneration) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        })
    }

    /// One daily rollup: build, presign `kind: rollup`, PUT, then `rollup.publish`. `Ok(false)`
    /// when the day could not be built (skipped), `Ok(true)` when it went to the cloud; otherwise
    /// what to do instead.
    async fn publish_rollup(
        &mut self,
        writer: &WriterHandle,
        token: &str,
        epoch: &str,
        interval: Duration,
        day: &str,
        schema: u32,
    ) -> Result<bool, Next> {
        let built_at = writer.now_ms();
        let snapshots = Arc::clone(&self.snapshots);
        let build_day = day.to_owned();
        let build = self.build_rollup;
        let doc = match tokio::task::spawn_blocking(move || {
            build(&snapshots, &build_day, built_at, schema)
        })
        .await
        {
            Ok(Ok(Some(doc))) => doc,
            Ok(Ok(None)) => {
                tracing::warn!(day, "rollup: nothing to build for the day");
                return Ok(false);
            }
            Ok(Err(e)) => {
                tracing::warn!(day, code = e.code.as_str(), error = %e, "rollup: could not build");
                return Ok(false);
            }
            Err(e) => {
                tracing::warn!(day, error = %e, "rollup: build task failed");
                return Ok(false);
            }
        };
        let bytes = match serde_json::to_vec(&doc) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(day, error = %e, "rollup: could not encode");
                return Ok(false);
            }
        };
        let req = PresignRequest {
            epoch: epoch.to_owned(),
            kind: Some(PRESIGN_KIND_ROLLUP.to_owned()),
            date: Some(day.to_owned()),
            ..PresignRequest::default()
        };
        let links = match self.cloud.presign(token, &req).await {
            Ok(l) => l,
            Err(f) => return Err(self.refused(f, interval)),
        };
        let Some(url) = links.url else {
            tracing::warn!(day, "rollup: the cloud issued no rollup link");
            return Err(self.failed(interval));
        };
        if let Err(f) = self.cloud.put_object(&url, bytes).await {
            return Err(self.refused(f, interval));
        }
        self.fails = 0;
        match writer
            .publish_rollup(day.to_owned(), schema, built_at, rollup_object_key(day))
            .await
        {
            Ok(_) => {
                self.dirty.remove(day);
                tracing::info!(day, "daily rollup published");
                Ok(true)
            }
            Err(e) if e.code == ErrorCode::NotPrimary => {
                tracing::info!("rollup: this hub is no longer the primary");
                Err(Next::idle(TIER_RECHECK))
            }
            Err(e) => {
                tracing::warn!(
                    day,
                    code = e.code.as_str(),
                    "rollup: could not record the publish"
                );
                Ok(true)
            }
        }
    }

    /// Rebuilds published days whose schema is below `target`, oldest first, at most
    /// [`REGENERATIONS_PER_MINUTE`] a minute (Story 8.7). The `rollup_log` is the resume point:
    /// a restart starts again from the oldest day still below, and a rebuilt day is not rebuilt
    /// again. It runs after the cycle's batches, summary and due days, never inside `apply()`.
    /// `Ok` carries when the next lot can go, if days are left over.
    async fn regenerate_old_days(
        &mut self,
        writer: &WriterHandle,
        token: &str,
        epoch: &str,
        interval: Duration,
        target: u32,
    ) -> Result<Option<Duration>, Next> {
        let now = writer.now_ms();
        while self
            .regenerated
            .front()
            .is_some_and(|at| now.saturating_sub(*at) >= REGENERATION_WINDOW_MS)
        {
            self.regenerated.pop_front();
        }
        let allowed = REGENERATIONS_PER_MINUTE.saturating_sub(self.regenerated.len());
        let snapshots = Arc::clone(&self.snapshots);
        // Days that cannot be built are passed over, so they never starve the ones behind.
        let look = allowed + self.unbuildable.len() + 1;
        let old = match tokio::task::spawn_blocking(move || {
            snapshots.rollup_days_below(target, look)
        })
        .await
        {
            Ok(Ok(days)) => days,
            Ok(Err(e)) => {
                tracing::warn!(code = e.code.as_str(), error = %e, "rollup: could not list old days");
                return Ok(None);
            }
            Err(e) => {
                tracing::warn!(error = %e, "rollup: list task failed");
                return Ok(None);
            }
        };
        let old: Vec<String> = old
            .into_iter()
            .filter(|d| !self.unbuildable.contains(d))
            .collect();
        let mut tried = 0;
        for day in old.iter().take(allowed) {
            tried += 1;
            if self
                .publish_rollup(writer, token, epoch, interval, day, target)
                .await?
            {
                self.regenerated.push_back(writer.now_ms());
            } else {
                self.unbuildable.insert(day.clone());
            }
        }
        if old.len() <= tried {
            return Ok(None);
        }
        // More waits for the minute to free a place.
        let wake = self
            .regenerated
            .front()
            .map_or(Duration::from_secs(1), |at| {
                let free = at.saturating_add(REGENERATION_WINDOW_MS);
                Duration::from_millis(
                    u64::try_from(free.saturating_sub(writer.now_ms())).unwrap_or(0),
                )
                .max(Duration::from_secs(1))
            });
        Ok(Some(wake))
    }

    /// Stores the daily snapshot of the latest published business day (Story 8.8): `VACUUM INTO`
    /// a scratch file beside the database, player keys dropped, zstd, the scratch file deleted,
    /// then PUT to `snapshots/<date>.db.zst` via presign `kind: snapshot` and recorded in
    /// `snapshot_log`. Only the latest day is stored: days missed while offline are skipped, and a
    /// failed PUT leaves no row, so the next cycle takes the snapshot again. Skipped, with an
    /// alert, when the disk has less than twice the database free. Off the writer's path: the copy
    /// is read on a connection of its own. `Ok` when the cycle goes on; `Err` what to do instead.
    async fn publish_snapshot(
        &mut self,
        writer: &WriterHandle,
        token: &str,
        epoch: &str,
        interval: Duration,
    ) -> Result<(), Next> {
        let snapshots = Arc::clone(&self.snapshots);
        let due = {
            let snapshots = Arc::clone(&snapshots);
            tokio::task::spawn_blocking(move || {
                let Some(date) = snapshots.snapshot_due()? else {
                    return Ok(None);
                };
                let (dir, db_bytes) = snapshots.snapshot_scratch()?;
                Ok::<_, AppError>(Some((date, dir, db_bytes)))
            })
            .await
        };
        let (date, dir, db_bytes) = match due {
            Ok(Ok(Some(due))) => due,
            Ok(Ok(None)) => return Ok(()),
            Ok(Err(e)) => {
                tracing::warn!(code = e.code.as_str(), error = %e, "snapshot: could not plan");
                return Ok(());
            }
            Err(e) => {
                tracing::warn!(error = %e, "snapshot: plan task failed");
                return Ok(());
            }
        };
        let free = (self.free_space)(&dir);
        let needed = db_bytes.saturating_mul(SNAPSHOT_DISK_FACTOR);
        if free.is_some_and(|f| f < needed) {
            tracing::warn!(
                date,
                free,
                needed,
                "snapshot: not enough disk space, skipped"
            );
            self.alert_disk_low(writer, &date).await;
            return Ok(());
        }
        let build_date = date.clone();
        let built = tokio::task::spawn_blocking(move || {
            let file = snapshots.snapshot_file(&dir, &build_date)?;
            let bytes = file.read()?;
            // The scratch file goes as soon as its bytes are held.
            Ok::<_, AppError>((bytes, file.epoch.clone(), file.last_seq))
        })
        .await;
        let (bytes, snapshot_epoch, last_seq) = match built {
            Ok(Ok(built)) => built,
            Ok(Err(e)) => {
                tracing::warn!(date, code = e.code.as_str(), error = %e, "snapshot: could not build");
                return Ok(());
            }
            Err(e) => {
                tracing::warn!(date, error = %e, "snapshot: build task failed");
                return Ok(());
            }
        };
        let req = PresignRequest {
            epoch: epoch.to_owned(),
            kind: Some(PRESIGN_KIND_SNAPSHOT.to_owned()),
            date: Some(date.clone()),
            ..PresignRequest::default()
        };
        let links = match self.cloud.presign(token, &req).await {
            Ok(l) => l,
            Err(f) => return Err(self.refused(f, interval)),
        };
        let Some(url) = links.url else {
            tracing::warn!(date, "snapshot: the cloud issued no snapshot link");
            return Err(self.failed(interval));
        };
        let size = bytes.len();
        if let Err(f) = self.cloud.put_object(&url, bytes).await {
            return Err(self.refused(f, interval));
        }
        self.fails = 0;
        let row = SnapshotLogRow {
            key: snapshot_object_key(&date),
            date: date.clone(),
            epoch: snapshot_epoch,
            last_seq,
            bytes: i64::try_from(size).unwrap_or(i64::MAX),
            uploaded_at: writer.now_ms(),
        };
        match writer.record_snapshot(row).await {
            Ok(()) => {
                tracing::info!(date, last_seq, bytes = size, "daily snapshot stored");
                Ok(())
            }
            Err(e) if e.code == ErrorCode::NotPrimary => {
                tracing::info!("snapshot: this hub is no longer the primary");
                Err(Next::idle(TIER_RECHECK))
            }
            Err(e) => {
                // The cloud has the file; the next cycle sends it again under the same key.
                tracing::warn!(
                    date,
                    code = e.code.as_str(),
                    "snapshot: could not record the upload"
                );
                Ok(())
            }
        }
    }

    /// Uploads the yearly GST archives that are due (Story 9.14): each financial year earlier than
    /// the year of the latest published rollup day whose invoices or credit notes were never
    /// archived or have changed since. Looked at once per newly published rollup day in this run;
    /// a day with an attempt that failed is looked at again next cycle. A year that cannot be
    /// built is skipped. `Ok` when the cycle goes on; `Err` what to do instead.
    async fn publish_gst_archives(
        &mut self,
        writer: &WriterHandle,
        token: &str,
        epoch: &str,
        interval: Duration,
    ) -> Result<(), Next> {
        let snapshots = Arc::clone(&self.snapshots);
        let checked = self.gst_checked.clone();
        let due = match tokio::task::spawn_blocking(move || {
            snapshots.gst_archive_due(checked.as_deref())
        })
        .await
        {
            Ok(Ok(due)) => due,
            Ok(Err(e)) => {
                tracing::warn!(code = e.code.as_str(), error = %e, "gst archive: could not plan");
                return Ok(());
            }
            Err(e) => {
                tracing::warn!(error = %e, "gst archive: plan task failed");
                return Ok(());
            }
        };
        let Some(day) = due.day else {
            return Ok(());
        };
        let mut complete = true;
        for fy in &due.years {
            complete &= self
                .publish_gst_archive(writer, token, epoch, interval, fy)
                .await?;
        }
        if complete {
            self.gst_checked = Some(day);
        }
        Ok(())
    }

    /// One yearly GST archive: build, presign `kind: gst_archive`, PUT, then
    /// `gst_archive.publish`. `Ok(false)` when it could not be built or recorded (tried again
    /// next cycle), `Ok(true)` when it is in the cloud and recorded; otherwise what to do
    /// instead.
    async fn publish_gst_archive(
        &mut self,
        writer: &WriterHandle,
        token: &str,
        epoch: &str,
        interval: Duration,
        fy: &str,
    ) -> Result<bool, Next> {
        let built_at = writer.now_ms();
        let snapshots = Arc::clone(&self.snapshots);
        let build_fy = fy.to_owned();
        let file = match tokio::task::spawn_blocking(move || {
            snapshots.gst_archive_file(&build_fy, built_at)
        })
        .await
        {
            Ok(Ok(Some(file))) => file,
            Ok(Ok(None)) => return Ok(false),
            Ok(Err(e)) => {
                tracing::warn!(fy, code = e.code.as_str(), error = %e, "gst archive: could not build");
                return Ok(false);
            }
            Err(e) => {
                tracing::warn!(fy, error = %e, "gst archive: build task failed");
                return Ok(false);
            }
        };
        if file.unreadable_orphans > 0 {
            tracing::warn!(
                fy,
                files = file.unreadable_orphans,
                "gst archive: set-aside databases could not be read and were skipped"
            );
        }
        let req = PresignRequest {
            epoch: epoch.to_owned(),
            kind: Some(PRESIGN_KIND_GST_ARCHIVE.to_owned()),
            fy: Some(fy.to_owned()),
            ..PresignRequest::default()
        };
        let links = match self.cloud.presign(token, &req).await {
            Ok(l) => l,
            Err(f) => return Err(self.refused(f, interval)),
        };
        let Some(url) = links.url else {
            tracing::warn!(fy, "gst archive: the cloud issued no archive link");
            return Err(self.failed(interval));
        };
        let size = i64::try_from(file.bytes.len()).unwrap_or(i64::MAX);
        if let Err(f) = self.cloud.put_object(&url, file.bytes).await {
            return Err(self.refused(f, interval));
        }
        self.fails = 0;
        let (invoices, credit_notes, orphaned) =
            (file.invoices, file.credit_notes, file.orphaned.len());
        let publish = GstArchivePublish {
            fy: fy.to_owned(),
            key: gst_archive_object_key(fy),
            sha256: file.sha256,
            bytes: size,
            invoices,
            credit_notes,
            orphaned: file.orphaned,
            built_at,
        };
        match writer.publish_gst_archive(publish).await {
            Ok(_) => {
                tracing::info!(
                    fy,
                    invoices,
                    credit_notes,
                    orphaned,
                    bytes = size,
                    "gst archive published"
                );
                Ok(true)
            }
            Err(e) if e.code == ErrorCode::NotPrimary => {
                tracing::info!("gst archive: this hub is no longer the primary");
                Err(Next::idle(TIER_RECHECK))
            }
            Err(e) => {
                // The cloud has the file; the next cycle sends it again under the same key,
                // after a backoff so a record that keeps failing does not re-upload at full rate.
                tracing::warn!(
                    fy,
                    code = e.code.as_str(),
                    "gst archive: could not record the upload"
                );
                Err(self.failed(interval))
            }
        }
    }

    /// Raises the "not enough disk space for backup" alert (it shows in the Hub's header list and
    /// writes an audit row), once for a day's snapshot per run.
    async fn alert_disk_low(&mut self, writer: &WriterHandle, date: &str) {
        if self.disk_alerted.as_deref() == Some(date) {
            return;
        }
        self.disk_alerted = Some(date.to_owned());
        match writer
            .raise_branch_alert(AlertKind::BackupDisk, "hub-snapshot")
            .await
        {
            Ok(_) => tracing::info!(date, "snapshot: the not-enough-disk alert is raised"),
            // Already open: staff have not acknowledged the last one.
            Err(e) if e.code == ErrorCode::Stale => {}
            Err(e) => {
                tracing::warn!(
                    code = e.code.as_str(),
                    "snapshot: could not raise the alert"
                );
                self.disk_alerted = None;
            }
        }
    }

    /// The schema to emit: the licence's pinned `rollup_schema` (the tier's default when it
    /// cannot be read), held within the writers this release keeps (Story 8.7).
    async fn rollup_schema(&self, writer: &WriterHandle, tier: Tier) -> u32 {
        let from_licence = match writer.licence_state().await {
            Ok(state) => state
                .jwt
                .and_then(|jwt| verify_claims(&jwt, &self.keys).ok())
                .map(|c| c.rollup_schema),
            Err(_) => None,
        };
        emitted_rollup_schema(
            from_licence.unwrap_or(tier.defaults().rollup_schema),
            self.newest_schema,
        )
    }

    /// How the cloud's refusal changes the schedule.
    fn refused(&mut self, f: CloudFailure, interval: Duration) -> Next {
        self.outcome = Some(match &f {
            CloudFailure::Refused { error, .. } => error.clone(),
            CloudFailure::Unreachable(_) => AppError::new(
                ErrorCode::CloudUnreachable,
                "the cloud could not be reached",
            ),
        });
        match f {
            CloudFailure::Refused { error, .. }
                if matches!(
                    error.code,
                    ErrorCode::TokenInvalid | ErrorCode::TokenRevoked
                ) =>
            {
                tracing::warn!("upload: this hub is no longer paired");
                self.status
                    .send_modify(|s| s.pairing = PairingState::Revoked);
                Next::backing_off(REVOKED_BACKOFF)
            }
            CloudFailure::Refused { error, .. } if error.code == ErrorCode::FeatureNotEntitled => {
                tracing::info!("upload: the cloud has this branch on Free");
                Next::backing_off(NOT_ENTITLED_RETRY)
            }
            CloudFailure::Refused {
                status,
                error,
                retry_after_s,
            } if status == 429 || error.code == ErrorCode::RateLimited => {
                let wait = retry_after_s
                    .map_or(RATE_LIMIT_WAIT, |s| Duration::from_secs(u64::from(s)))
                    .clamp(Duration::from_secs(1), RATE_LIMIT_CEILING);
                tracing::info!(
                    wait_s = wait.as_secs(),
                    "upload: the cloud limits the calls"
                );
                Next::backing_off(wait)
            }
            f => {
                tracing::warn!(failure = ?f, "upload failed");
                self.failed(interval)
            }
        }
    }

    fn failed(&mut self, interval: Duration) -> Next {
        self.fails += 1;
        Next::backing_off(backoff(self.fails, interval, unit()))
    }

    /// `summary_interval_s` from the stored, verified licence (the tier's default when it cannot
    /// be read), never under [`MIN_UPLOAD_INTERVAL`].
    async fn summary_interval(&self, writer: &WriterHandle, tier: Tier) -> Duration {
        let from_licence = match writer.licence_state().await {
            Ok(state) => state
                .jwt
                .and_then(|jwt| verify_claims(&jwt, &self.keys).ok())
                .map(|c| c.summary_interval_s),
            Err(_) => None,
        };
        clamp_interval(from_licence.unwrap_or(tier.defaults().summary_interval_s))
    }

    /// `upload_interval_s` from the stored, verified licence (the tier's default when it cannot
    /// be read), never under [`MIN_UPLOAD_INTERVAL`].
    pub async fn interval(&self, writer: &WriterHandle, tier: Tier) -> Duration {
        let from_licence = match writer.licence_state().await {
            Ok(state) => state
                .jwt
                .and_then(|jwt| verify_claims(&jwt, &self.keys).ok())
                .map(|c| c.upload_interval_s),
            Err(_) => None,
        };
        clamp_interval(from_licence.unwrap_or(tier.defaults().upload_interval_s))
    }

    async fn progress(&self) -> Option<UploadStatus> {
        let snapshots = Arc::clone(&self.snapshots);
        match tokio::task::spawn_blocking(move || snapshots.upload_progress()).await {
            Ok(Ok(p)) => Some(p),
            Ok(Err(e)) => {
                tracing::warn!(
                    code = e.code.as_str(),
                    "upload: could not count the waiting rows"
                );
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, "upload: count task failed");
                None
            }
        }
    }

    async fn refresh_progress(&self) {
        if let Some(p) = self.progress().await {
            self.status.send_if_modified(|s| {
                let changed = s.upload != p;
                s.upload = p;
                changed
            });
        }
    }
}

/// An interval in seconds as a wait, at least [`MIN_UPLOAD_INTERVAL`].
pub fn clamp_interval(secs: u32) -> Duration {
    Duration::from_secs(u64::from(secs)).max(MIN_UPLOAD_INTERVAL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::CloudEndpoint;
    use crate::lease::LeaseTask;
    use arena_core::{Clock, ManualClock, status_channel};
    use arena_proto::licence::{DEV_LICENCE_VERIFY_KEYS, LicenceClaims, dev_sign};
    use arena_store::{
        BatchFork, ChangeOp, ChangeRecord, Store, Table, decode_batch, insert_change,
        next_change_seq, write_lease_fork,
    };
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const T0: i64 = 1_800_000_000_000;
    const OWN: &str = "01JAAAAAAAAAAAAAAAAAAAAAAA";
    /// The epoch the fixture's lease gives its writer.
    const EPOCH: &str = "1:01JAAAAAAAAAAAAAAAAAAAAAAA";

    struct Mem(Mutex<Option<Vec<u8>>>);

    impl Keystore for Mem {
        fn put(&self, secret: &[u8]) -> std::io::Result<()> {
            *self.0.lock().unwrap() = Some(secret.to_vec());
            Ok(())
        }
        fn get(&self) -> std::io::Result<Option<Vec<u8>>> {
            Ok(self.0.lock().unwrap().clone())
        }
    }

    /// (request line, body bytes).
    type Seen = Arc<Mutex<Vec<(String, Vec<u8>)>>>;
    type Handler = Box<dyn FnMut(&str, &[u8]) -> (u16, String) + Send>;

    /// A one-request-per-connection HTTP server. `make` gets the port, to build links back to
    /// the server itself.
    async fn serve(make: impl FnOnce(u16) -> Handler) -> (u16, Seen) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut handler = make(port);
        let seen: Seen = Arc::default();
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut raw = Vec::new();
                let mut buf = [0u8; 65536];
                let (head_end, length) = loop {
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    raw.extend_from_slice(&buf[..n]);
                    if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&raw[..at]).into_owned();
                        let length = head
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        break (at + 4, length);
                    }
                    if n == 0 {
                        break (raw.len(), 0);
                    }
                };
                while raw.len() < head_end + length {
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&buf[..n]);
                }
                let line = String::from_utf8_lossy(&raw)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_owned();
                let body = raw.get(head_end..).unwrap_or(&[]).to_vec();
                log.lock().unwrap().push((line.clone(), body.clone()));
                let (status, reply) = handler(&line, &body);
                let head = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    reply.len()
                );
                let _ = sock.write_all(format!("{head}{reply}").as_bytes()).await;
            }
        });
        (port, seen)
    }

    /// A cloud that issues a link per batch and takes every PUT.
    fn cloud(port: u16) -> Handler {
        Box::new(move |line, body| {
            if line.starts_with("POST /v1/presign") {
                let req: serde_json::Value = serde_json::from_slice(body).unwrap();
                let b = &req["batch"];
                let url = format!(
                    "http://127.0.0.1:{port}/put/{}/{}-{}",
                    req["epoch"].as_str().unwrap(),
                    b["first"],
                    b["last"]
                );
                (200, serde_json::json!({ "batch_url": url }).to_string())
            } else {
                (200, "{}".into())
            }
        })
    }

    fn refused(status: u16, code: &str) -> (u16, String) {
        (status, format!(r#"{{"code":"{code}","message":"no"}}"#))
    }

    fn presigns(seen: &Seen) -> Vec<serde_json::Value> {
        seen.lock()
            .unwrap()
            .iter()
            .filter(|(l, _)| l.starts_with("POST /v1/presign"))
            .map(|(_, b)| serde_json::from_slice(b).unwrap())
            .collect()
    }

    /// (key, body) of every PUT.
    fn puts(seen: &Seen) -> Vec<(String, Vec<u8>)> {
        seen.lock()
            .unwrap()
            .iter()
            .filter_map(|(l, b)| {
                l.strip_prefix("PUT ")
                    .map(|rest| (rest.split(' ').next().unwrap().to_owned(), b.clone()))
            })
            .collect()
    }

    struct Fx {
        _dir: tempfile::TempDir,
        path: std::path::PathBuf,
        status: watch::Sender<HubStatus>,
        task: UploadTask,
        writer: WriterHandle,
        clock: Arc<ManualClock>,
    }

    async fn fx(port: u16, tier: Tier) -> Fx {
        fx_with(port, tier, false).await
    }

    /// `bootstrap`: the branch comes from `branch.bootstrap` through `apply()`, so its change
    /// rows replay; otherwise it is inserted directly and the log starts empty.
    async fn fx_with(port: u16, tier: Tier, bootstrap: bool) -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let store = Store::open(&path, T0).unwrap();
        let (status, _rx) = status_channel(HubStatus::opened(73, OWN));
        let manual = Arc::new(ManualClock::new(T0));
        let clock: Arc<dyn Clock> = manual.clone();
        let lease = LeaseTask::start(store, OWN.into(), Arc::clone(&clock), status.clone())
            .await
            .unwrap();
        let writer = lease.writer().unwrap();
        std::mem::forget(lease);
        let claims = LicenceClaims::for_tier("T", "B", tier, T0 / 1000);
        writer
            .record_pairing(
                arena_store::HubIdentity {
                    tenant_id: "T".into(),
                    branch_id: "B".into(),
                    hub_id: "H".into(),
                    tier,
                    paired_at: T0,
                },
                dev_sign(&claims).unwrap(),
                None,
            )
            .await
            .unwrap();
        if bootstrap {
            writer
                .submit_intent(intent(
                    arena_proto::IntentPayload::BranchBootstrap(arena_proto::BranchBootstrap {
                        name: "Arena".into(),
                        branch_code: "ARN".into(),
                        currency: "INR".into(),
                        tz: "Asia/Kolkata".into(),
                        day_start_min: 360,
                        fy_start_month: 4,
                        onboarding: None,
                    }),
                    arena_proto::ActorKind::System,
                    None,
                ))
                .await
                .unwrap();
        } else {
            writer
                .submit(|tx| {
                    arena_store::insert_branch(
                        tx,
                        &arena_store::NewBranch {
                            branch_id: "B",
                            name: "Arena",
                            branch_code: "ARN",
                            currency: "INR",
                            tz: "Asia/Kolkata",
                            day_start_min: 0,
                            fy_start_month: 4,
                        },
                    )
                })
                .await
                .unwrap();
        }
        status.send_modify(|s| s.tier = Some(tier));
        let cloud = CloudClient::new(
            &CloudEndpoint::parse(&format!("http://127.0.0.1:{port}"), true).unwrap(),
        )
        .unwrap();
        let keystore: Arc<dyn Keystore> = Arc::new(Mem(Mutex::new(Some(b"hub_x.y".to_vec()))));
        let task = UploadTask::new(
            cloud,
            keystore,
            Some(writer.clone()),
            Arc::new(SnapshotReader::open(&path).unwrap()),
            PairGate::new(true),
            status.clone(),
            DEV_LICENCE_VERIFY_KEYS.to_vec(),
        )
        .without_summary();
        Fx {
            _dir: dir,
            path,
            status,
            task,
            writer,
            clock: manual,
        }
    }

    fn intent(
        payload: arena_proto::IntentPayload,
        kind: arena_proto::ActorKind,
        base_rev: Option<i64>,
    ) -> arena_proto::Intent {
        arena_proto::Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: payload.kind(),
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: T0,
            base_rev,
            actor: arena_proto::Actor {
                kind,
                id: "test".into(),
            },
            elevated_by: None,
            reason: None,
            payload,
        }
    }

    /// Appends `n` change rows to `epoch`, from the next free `seq`, directly.
    async fn add_rows(writer: &WriterHandle, epoch: &str, n: i64) {
        let epoch = epoch.to_owned();
        writer
            .submit(move |tx| {
                let first = next_change_seq(tx, &epoch)?;
                for seq in first..first + n {
                    insert_change(
                        tx,
                        &ChangeRecord {
                            epoch: epoch.clone(),
                            seq,
                            tbl: "settings".into(),
                            row_id: format!("k{seq}"),
                            op: ChangeOp::Insert,
                            row: vec![0x80],
                            intent_id: "I".into(),
                            intent_kind: "settings.set".into(),
                            actor_kind: "system".into(),
                            actor_id: "test".into(),
                            elevated_by_kind: None,
                            elevated_by_id: None,
                            elevated_by_mode: None,
                            elevated_by_verifier_version: None,
                            at: T0,
                            business_day: None,
                            fy: None,
                            reason: None,
                        },
                    )?;
                }
                Ok(())
            })
            .await
            .unwrap();
    }

    fn progress(f: &Fx) -> UploadStatus {
        f.status.borrow().upload.clone()
    }

    fn cursor(f: &Fx) -> Option<arena_store::UploadCursor> {
        Store::open_reader(&f.path)
            .unwrap()
            .read_with(arena_store::read_upload_cursor)
            .unwrap()
    }

    #[tokio::test]
    async fn a_paid_hub_uploads_its_rows_and_only_then_moves_the_cursor() {
        let (port, seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 3).await;

        let next = f.task.cycle().await;
        assert_eq!(
            next,
            Next {
                wait: Duration::from_secs(300),
                active: true,
                early: true
            }
        );
        let asked = presigns(&seen);
        assert_eq!(asked.len(), 1, "{asked:?}");
        assert_eq!(asked[0]["epoch"], EPOCH);
        assert_eq!(
            asked[0]["batch"],
            serde_json::json!({"first": 1, "last": 3})
        );
        let sent = puts(&seen);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, format!("/put/{EPOCH}/1-3"));
        let batch = decode_batch(&sent[0].1).unwrap();
        assert_eq!(batch.header.schema, 73);
        assert_eq!((batch.header.first, batch.header.last), (1, 3));
        assert_eq!(batch.rows.len(), 3);

        let c = cursor(&f).unwrap();
        assert_eq!(
            (c.epoch.as_str(), c.last_seq_uploaded, c.uploaded_at),
            (EPOCH, 3, T0)
        );
        assert_eq!(
            progress(&f),
            UploadStatus {
                last_upload_at: Some(T0),
                pending_rows: 0,
                last_seq_uploaded: 3,
                fork_seq: None,
                base_seq: None,
                last_summary_at: None,
                last_rollup_date: None,
            }
        );

        // Nothing new: no further request.
        f.task.cycle().await;
        assert_eq!(presigns(&seen).len(), 1);
        assert_eq!(puts(&seen).len(), 1);
    }

    /// A cloud that issues a batch link and/or a summary link as asked. `up` false: the bucket
    /// refuses every PUT (the WAN is down).
    fn cloud_with_summary(
        up: Arc<std::sync::atomic::AtomicBool>,
        fenced: bool,
    ) -> impl FnOnce(u16) -> Handler {
        move |port| {
            Box::new(move |line, body| {
                if line.starts_with("POST /v1/presign") {
                    let req: serde_json::Value = serde_json::from_slice(body).unwrap();
                    let mut links = serde_json::Map::new();
                    if let Some(b) = req.get("batch").filter(|b| !b.is_null()) {
                        links.insert(
                            "batch_url".into(),
                            format!(
                                "http://127.0.0.1:{port}/put/{}/{}-{}",
                                req["epoch"].as_str().unwrap(),
                                b["first"],
                                b["last"]
                            )
                            .into(),
                        );
                    }
                    if req["summary"] == true {
                        if fenced {
                            links.insert("summary_fenced".into(), true.into());
                        } else {
                            links.insert(
                                "summary_url".into(),
                                format!("http://127.0.0.1:{port}/put/summary").into(),
                            );
                        }
                    }
                    (200, serde_json::Value::Object(links).to_string())
                } else if line.starts_with("PUT /put/summary")
                    && !up.load(std::sync::atomic::Ordering::SeqCst)
                {
                    (503, "{}".into())
                } else {
                    (200, "{}".into())
                }
            })
        }
    }

    fn summary_puts(seen: &Seen) -> Vec<arena_proto::BranchSummary> {
        puts(seen)
            .into_iter()
            .filter(|(k, _)| k == "/put/summary")
            .map(|(_, b)| serde_json::from_slice(&b).unwrap())
            .collect()
    }

    fn latest(f: &Fx) -> Option<arena_store::RollupLogRow> {
        Store::open_reader(&f.path)
            .unwrap()
            .rollup_log("latest")
            .unwrap()
    }

    #[tokio::test]
    async fn the_summary_rides_the_batch_presign_and_is_recorded_after_its_put() {
        let up = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let (port, seen) = serve(cloud_with_summary(Arc::clone(&up), false)).await;
        let mut f = fx(port, Tier::Basic).await;
        f.task.summary = true;
        add_rows(&f.writer, EPOCH, 3).await;

        let next = f.task.cycle().await;
        assert!(next.active && next.early);
        let asked = presigns(&seen);
        assert_eq!(asked.len(), 1, "one presign serves both: {asked:?}");
        assert_eq!(asked[0]["summary"], true);
        assert_eq!(
            asked[0]["batch"],
            serde_json::json!({"first": 1, "last": 3})
        );
        let summaries = summary_puts(&seen);
        assert_eq!(summaries.len(), 1);
        let s = &summaries[0];
        assert_eq!(
            (s.schema, s.branch_id.as_str(), s.as_of),
            (arena_proto::SUMMARY_SCHEMA, "B", T0)
        );
        let row = latest(&f).expect("summary.publish wrote the rollup_log row");
        assert_eq!(
            (row.schema, row.built_at, row.key.as_str()),
            (1, T0, "state/latest.json")
        );
        assert_eq!(progress(&f).last_summary_at, Some(T0));

        // Not due again within the interval; the publish itself is a row the next cycle ships.
        f.task.cycle().await;
        assert_eq!(summary_puts(&seen).len(), 1);
        assert_eq!(puts(&seen).len(), 3, "batch, summary, then the publish row");
    }

    #[tokio::test]
    async fn a_summary_is_published_with_no_rows_waiting_and_with_its_own_presign() {
        let up = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let (port, seen) = serve(cloud_with_summary(up, false)).await;
        let mut f = fx(port, Tier::Premium).await;
        f.task.summary = true;
        f.task.cycle().await;
        let asked = presigns(&seen);
        assert_eq!(asked.len(), 1);
        assert!(asked[0].get("batch").is_none(), "{asked:?}");
        assert_eq!(asked[0]["summary"], true);
        assert_eq!(summary_puts(&seen).len(), 1);
        assert!(latest(&f).is_some());
    }

    #[tokio::test]
    async fn a_free_hub_builds_no_summary() {
        let up = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let (port, seen) = serve(cloud_with_summary(up, false)).await;
        let mut f = fx(port, Tier::Free).await;
        f.task.summary = true;
        f.task.cycle().await;
        assert!(seen.lock().unwrap().is_empty());
        assert!(latest(&f).is_none());
    }

    #[tokio::test]
    async fn a_failed_summary_put_is_not_queued_and_the_next_cycle_builds_a_fresh_one() {
        let up = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (port, seen) = serve(cloud_with_summary(Arc::clone(&up), false)).await;
        let mut f = fx(port, Tier::Basic).await;
        f.task.summary = true;

        let next = f.task.cycle().await;
        assert!(!next.early, "a failed cycle backs off");
        assert!(latest(&f).is_none(), "nothing is recorded for a lost PUT");
        assert_eq!(progress(&f).last_summary_at, None);

        up.store(true, std::sync::atomic::Ordering::SeqCst);
        f.task.cycle().await;
        let row = latest(&f).expect("the next cycle publishes");
        assert_eq!(row.built_at, T0);
        assert_eq!(
            summary_puts(&seen).len(),
            2,
            "the first PUT reached the server and failed"
        );
    }

    #[tokio::test]
    async fn a_fenced_epoch_gets_no_summary_link_and_publishes_nothing() {
        let up = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let (port, seen) = serve(cloud_with_summary(up, true)).await;
        let mut f = fx(port, Tier::Basic).await;
        f.task.summary = true;
        f.task.cycle().await;
        assert!(summary_puts(&seen).is_empty());
        assert!(latest(&f).is_none());
    }

    #[tokio::test]
    async fn a_batch_read_back_matches_its_header_and_replays_into_an_empty_database() {
        let (port, seen) = serve(cloud).await;
        let mut f = fx_with(port, Tier::Premium, true).await;
        f.writer
            .submit_intent(intent(
                arena_proto::IntentPayload::SettingsSet(arena_proto::SettingsSet {
                    key: "hub.locale".into(),
                    value: "en-NZ".into(),
                }),
                arena_proto::ActorKind::Owner,
                Some(1),
            ))
            .await
            .unwrap();
        f.task.cycle().await;

        let sent = puts(&seen);
        assert_eq!(sent.len(), 1);
        let batch = decode_batch(&sent[0].1).expect("sha256 and row count match the header");
        assert_eq!(
            batch.rows.len() as i64,
            batch.header.last - batch.header.first + 1
        );
        assert_eq!(batch.header.epoch, EPOCH);

        let source = Store::open_reader(&f.path).unwrap();
        assert_eq!(batch.rows, source.changes().unwrap());

        let dir = tempfile::tempdir().unwrap();
        let mut dest = Store::open(dir.path().join("replica.db"), T0).unwrap();
        let n = dest
            .write_tx_mut(|tx| arena_replay::replay(batch.rows.clone(), tx))
            .unwrap();
        assert_eq!(n, batch.rows.len());
        for table in Table::ALL {
            assert_eq!(
                dest.rows(table).unwrap(),
                source.rows(table).unwrap(),
                "{}",
                table.name()
            );
        }
        assert!(!dest.rows(Table::Branch).unwrap().is_empty());
        assert!(!dest.rows(Table::Settings).unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_backlog_ships_back_to_back_in_contiguous_keys() {
        let (port, seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 12_000).await;

        f.task.cycle().await;
        let keys: Vec<String> = puts(&seen).into_iter().map(|(k, _)| k).collect();
        assert_eq!(
            keys,
            [
                format!("/put/{EPOCH}/1-5000"),
                format!("/put/{EPOCH}/5001-10000"),
                format!("/put/{EPOCH}/10001-12000"),
            ]
        );
        let sizes: Vec<usize> = puts(&seen)
            .iter()
            .map(|(_, b)| decode_batch(b).unwrap().rows.len())
            .collect();
        assert_eq!(sizes, [5000, 5000, 2000]);
        assert_eq!(cursor(&f).unwrap().last_seq_uploaded, 12_000);
        assert_eq!(progress(&f).pending_rows, 0);
    }

    #[tokio::test]
    async fn the_first_batch_of_a_forked_epoch_carries_the_fork_and_starts_after_the_parent() {
        let (port, seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Basic).await;
        // The parent's rows 1..=5 are in the cloud already; this epoch continues at 6.
        f.writer.record_upload("0:OLD".into(), 5).await.unwrap();
        f.writer
            .submit(|tx| {
                write_lease_fork(
                    tx,
                    Some(&BatchFork {
                        parent_epoch: "0:OLD".into(),
                        parent_seq: 5,
                    }),
                )
            })
            .await
            .unwrap();
        f.writer
            .submit(|tx| {
                for seq in 6..=8 {
                    insert_change(
                        tx,
                        &ChangeRecord {
                            epoch: EPOCH.into(),
                            seq,
                            tbl: "settings".into(),
                            row_id: format!("k{seq}"),
                            op: ChangeOp::Insert,
                            row: vec![0x80],
                            intent_id: "I".into(),
                            intent_kind: "settings.set".into(),
                            actor_kind: "system".into(),
                            actor_id: "test".into(),
                            elevated_by_kind: None,
                            elevated_by_id: None,
                            elevated_by_mode: None,
                            elevated_by_verifier_version: None,
                            at: T0,
                            business_day: None,
                            fy: None,
                            reason: None,
                        },
                    )?;
                }
                Ok(())
            })
            .await
            .unwrap();

        f.task.cycle().await;
        let asked = presigns(&seen);
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0]["epoch"], EPOCH);
        assert_eq!(
            asked[0]["batch"],
            serde_json::json!({"first": 6, "last": 8})
        );
        let sent = puts(&seen);
        assert_eq!(sent[0].0, format!("/put/{EPOCH}/6-8"));
        let batch = decode_batch(&sent[0].1).unwrap();
        assert_eq!(
            batch.header.fork,
            Some(BatchFork {
                parent_epoch: "0:OLD".into(),
                parent_seq: 5
            })
        );
        assert_eq!((batch.header.first, batch.header.last), (6, 8));

        // Later rows of the epoch carry no fork.
        add_rows(&f.writer, EPOCH, 2).await;
        f.task.cycle().await;
        let batch = decode_batch(&puts(&seen)[1].1).unwrap();
        assert_eq!(batch.header.fork, None);
        assert_eq!((batch.header.first, batch.header.last), (9, 10));
    }

    #[tokio::test]
    async fn a_failed_put_leaves_the_cursor_and_the_next_cycle_sends_the_same_key() {
        let mut puts_seen = 0;
        let (port, seen) = serve(move |port| {
            let mut ok = cloud(port);
            Box::new(move |line, body| {
                if line.starts_with("PUT ") {
                    puts_seen += 1;
                    if puts_seen == 1 {
                        return refused(503, "INTERNAL");
                    }
                }
                ok(line, body)
            })
        })
        .await;
        let mut f = fx(port, Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 3).await;

        let next = f.task.cycle().await;
        assert!(!next.early && next.active);
        assert!(
            next.wait >= Duration::from_secs(1) && next.wait <= Duration::from_secs(300),
            "{next:?}"
        );
        assert_eq!(cursor(&f), None, "the cursor stays");
        assert_eq!(
            progress(&f),
            UploadStatus {
                last_upload_at: None,
                pending_rows: 3,
                last_seq_uploaded: 0,
                fork_seq: None,
                base_seq: None,
                last_summary_at: None,
                last_rollup_date: None,
            }
        );

        let next = f.task.cycle().await;
        assert!(next.early);
        let sent = puts(&seen);
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].0, sent[1].0, "the same key");
        assert_eq!(sent[0].1, sent[1].1, "the same bytes");
        assert_eq!(cursor(&f).unwrap().last_seq_uploaded, 3);
        assert_eq!(progress(&f).pending_rows, 0);
    }

    #[tokio::test]
    async fn an_unreachable_cloud_is_a_failure_that_leaves_the_rows_waiting() {
        // Nothing listens on this port.
        let spare = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = spare.local_addr().unwrap().port();
        drop(spare);
        let mut f = fx(port, Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 2).await;
        let next = f.task.cycle().await;
        assert!(!next.early && next.active);
        assert_eq!(cursor(&f), None);
        assert_eq!(progress(&f).pending_rows, 2);
    }

    #[tokio::test]
    async fn a_free_hub_or_one_without_a_tier_makes_no_request() {
        let (port, seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Free).await;
        add_rows(&f.writer, EPOCH, 3).await;
        assert_eq!(f.task.cycle().await, Next::idle(TIER_RECHECK));
        f.status.send_modify(|s| s.tier = None);
        assert_eq!(f.task.cycle().await, Next::idle(TIER_RECHECK));
        assert!(seen.lock().unwrap().is_empty());
        assert_eq!(cursor(&f), None);
        assert_eq!(progress(&f), UploadStatus::default());

        // An upgrade starts the upload.
        f.status.send_modify(|s| s.tier = Some(Tier::Basic));
        f.task.cycle().await;
        assert_eq!(puts(&seen).len(), 1);
    }

    #[tokio::test]
    async fn a_follower_makes_no_request() {
        let (port, seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 3).await;
        f.status.send_modify(|s| s.role = Role::Follower);
        assert_eq!(f.task.cycle().await, Next::idle(TIER_RECHECK));
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_clouds_refusals_are_told_apart() {
        for (status, code, wait, revoked) in [
            (401, "TOKEN_REVOKED", REVOKED_BACKOFF, true),
            (401, "TOKEN_INVALID", REVOKED_BACKOFF, true),
            (403, "FEATURE_NOT_ENTITLED", NOT_ENTITLED_RETRY, false),
        ] {
            let (port, seen) = serve(move |_| Box::new(move |_, _| refused(status, code))).await;
            let mut f = fx(port, Tier::Basic).await;
            add_rows(&f.writer, EPOCH, 3).await;
            assert_eq!(f.task.cycle().await.wait, wait, "{code}");
            assert_eq!(presigns(&seen).len(), 1, "{code}");
            assert!(puts(&seen).is_empty(), "{code}");
            assert_eq!(cursor(&f), None, "{code}");
            let now_revoked = f.status.borrow().pairing == PairingState::Revoked;
            assert_eq!(now_revoked, revoked, "{code}");
            if revoked {
                // A revoked Hub stops asking.
                assert_eq!(f.task.cycle().await, Next::idle(REVOKED_BACKOFF));
                assert_eq!(presigns(&seen).len(), 1);
            }
        }
    }

    #[tokio::test]
    async fn a_rate_limited_presign_without_a_retry_after_waits_a_minute() {
        let (port, seen) = serve(|_| {
            Box::new(|_, _| {
                (
                    429,
                    r#"{"code":"RATE_LIMITED","message":"slow down"}"#.into(),
                )
            })
        })
        .await;
        let mut f = fx(port, Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 3).await;
        let next = f.task.cycle().await;
        assert_eq!(next.wait, RATE_LIMIT_WAIT);
        assert!(!next.early);
        assert!(puts(&seen).is_empty());
        assert_eq!(cursor(&f), None);
        assert_ne!(f.status.borrow().pairing, PairingState::Revoked);
    }

    #[tokio::test]
    async fn a_429_with_a_retry_after_header_is_honoured() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 8192];
                let _ = sock.read(&mut buf).await;
                let body = r#"{"code":"RATE_LIMITED","message":"slow down"}"#;
                let _ = sock
                    .write_all(
                        format!(
                            "HTTP/1.1 429 X\r\nretry-after: 120\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .await;
            }
        });
        let mut f = fx(port, Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 3).await;
        assert_eq!(f.task.cycle().await.wait, Duration::from_secs(120));
    }

    #[tokio::test]
    async fn the_interval_comes_from_the_verified_licence_and_never_goes_under_30_seconds() {
        let (port, _seen) = serve(cloud).await;
        let f = fx(port, Tier::Basic).await;
        assert_eq!(
            f.task.interval(&f.writer, Tier::Basic).await,
            Duration::from_secs(300)
        );
        let mut claims = LicenceClaims::for_tier("T", "B", Tier::Basic, T0 / 1000);
        claims.upload_interval_s = 45;
        f.writer
            .record_heartbeat(Some(dev_sign(&claims).unwrap()), None, None)
            .await
            .unwrap();
        assert_eq!(
            f.task.interval(&f.writer, Tier::Basic).await,
            Duration::from_secs(45)
        );
        claims.upload_interval_s = 5;
        f.writer
            .record_heartbeat(Some(dev_sign(&claims).unwrap()), None, None)
            .await
            .unwrap();
        assert_eq!(
            f.task.interval(&f.writer, Tier::Premium).await,
            MIN_UPLOAD_INTERVAL
        );

        // A licence that cannot be verified falls back to the tier's default.
        let blind = UploadTask::new(
            f.task.cloud.clone(),
            Arc::clone(&f.task.keystore),
            Some(f.writer.clone()),
            Arc::clone(&f.task.snapshots),
            PairGate::new(true),
            f.status.clone(),
            Vec::new(),
        );
        assert_eq!(
            blind.interval(&f.writer, Tier::Premium).await,
            Duration::from_secs(300)
        );
        assert_eq!(clamp_interval(0), MIN_UPLOAD_INTERVAL);
        assert_eq!(clamp_interval(900), Duration::from_secs(900));
    }

    #[tokio::test]
    async fn the_cursor_never_moves_backwards() {
        let (port, _seen) = serve(cloud).await;
        let f = fx(port, Tier::Basic).await;
        f.writer.record_upload("2:B".into(), 10).await.unwrap();
        f.writer.record_upload("2:B".into(), 4).await.unwrap();
        f.writer.record_upload("1:Z".into(), 99).await.unwrap();
        let c = cursor(&f).unwrap();
        assert_eq!((c.epoch.as_str(), c.last_seq_uploaded), ("2:B", 10));
        f.writer.record_upload("10:A".into(), 1).await.unwrap();
        assert_eq!(cursor(&f).unwrap().epoch, "10:A");
        assert!(f.writer.record_upload("nonsense".into(), 1).await.is_err());
    }

    /// A `group.upsert` through `apply()`, so the change is broadcast like any other.
    async fn add_group(writer: &WriterHandle, name: &str) {
        static NEXT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        writer
            .apply_command(arena_proto::CommandWire {
                id,
                idempotency_key: ulid::Ulid::generate().to_string(),
                kind: "group.upsert".into(),
                schema_version: arena_proto::COMMAND_SCHEMA_VERSION,
                payload: serde_json::json!({
                    "name": name, "group_kind": "pc", "colour": "teal", "sort_order": id
                }),
                issued_by: "owner-1".into(),
                elevated_by: None,
            })
            .await
            .unwrap();
    }

    fn change_rows(path: &std::path::Path) -> i64 {
        Store::open_reader(path).unwrap().changes().unwrap().len() as i64
    }

    #[tokio::test]
    async fn waiting_rows_are_counted_and_enough_of_them_wake_the_task_early() {
        let (port, seen) = serve(cloud).await;
        let f = fx(port, Tier::Basic).await;
        let status = f.status.clone();
        let writer = f.writer.clone();
        let path = f.path.clone();
        let job = tokio::spawn(f.task.with_trigger(8, Duration::from_millis(100)).run());
        let wait_for = async |n: usize| {
            for _ in 0..200 {
                if puts(&seen).len() >= n {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            panic!("expected {n} uploads, saw {:?}", puts(&seen));
        };
        // The first look after start: nothing waiting, nothing sent.
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(puts(&seen).is_empty());

        // Under the trigger the rows wait for the interval (300 s), and /healthz counts them.
        add_group(&writer, "One").await;
        let mut rows = change_rows(&path);
        assert!(rows > 0 && rows < 8, "{rows}");
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(puts(&seen).is_empty());
        assert_eq!(status.borrow().upload.pending_rows, rows);

        // Enough rows: the batch goes without waiting out the interval.
        let mut i = 0;
        while rows < 8 {
            i += 1;
            add_group(&writer, &format!("More {i}")).await;
            rows = change_rows(&path);
        }
        wait_for(1).await;
        let batch = decode_batch(&puts(&seen)[0].1).unwrap();
        assert!(batch.rows.len() as i64 >= 8);
        let mut done = false;
        for _ in 0..100 {
            if status.borrow().upload.pending_rows == 0 {
                done = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(done, "{:?}", status.borrow().upload);
        assert_eq!(status.borrow().upload.last_upload_at, Some(T0));
        job.abort();
    }

    fn with_inbox(f: &mut Fx, batch_rows: usize) -> Arc<ResendInbox> {
        let inbox = ResendInbox::new();
        let task = std::mem::replace(
            &mut f.task,
            UploadTask::new(
                CloudClient::new(&CloudEndpoint::parse("http://127.0.0.1:9", true).unwrap())
                    .unwrap(),
                Arc::new(Mem(Mutex::new(None))),
                None,
                Arc::new(SnapshotReader::open(&f.path).unwrap()),
                PairGate::new(false),
                f.status.clone(),
                Vec::new(),
            ),
        );
        f.task = task
            .with_resend(Arc::clone(&inbox))
            .with_batch_rows(batch_rows);
        inbox
    }

    #[tokio::test]
    async fn a_resend_ships_exactly_the_asked_seqs_first_and_leaves_the_cursor_alone() {
        let (port, seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Basic).await;
        let inbox = with_inbox(&mut f, 3);
        add_rows(&f.writer, EPOCH, 12).await;
        f.task.cycle().await;
        let before = cursor(&f).unwrap();
        assert_eq!(before.last_seq_uploaded, 12);
        let sent_before = puts(&seen).len();

        inbox.offer(EPOCH, arena_proto::SeqRange { from: 4, to: 8 });
        add_rows(&f.writer, EPOCH, 2).await;
        f.task.cycle().await;

        let keys: Vec<String> = puts(&seen)
            .iter()
            .skip(sent_before)
            .map(|(k, _)| k.clone())
            .collect();
        assert_eq!(
            keys,
            [
                format!("/put/{EPOCH}/4-6"),
                format!("/put/{EPOCH}/7-8"),
                format!("/put/{EPOCH}/13-14"),
            ],
            "the resend first, then the new rows"
        );
        let resent = decode_batch(&puts(&seen)[sent_before].1).unwrap();
        assert_eq!((resent.header.first, resent.header.last), (4, 6));
        assert_eq!(resent.rows.len(), 3);
        assert_eq!(inbox.pending(), None);
        assert_eq!(cursor(&f).unwrap().last_seq_uploaded, 14);
        assert_eq!(cursor(&f).unwrap().uploaded_at, before.uploaded_at);
    }

    #[tokio::test]
    async fn pruned_rows_are_reported_not_resent() {
        let (port, seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Basic).await;
        let inbox = with_inbox(&mut f, 5);
        add_rows(&f.writer, EPOCH, 3).await;
        f.task.cycle().await;
        let sent_before = puts(&seen).len();

        inbox.offer(EPOCH, arena_proto::SeqRange { from: 5, to: 9 });
        f.task.cycle().await;

        assert_eq!(puts(&seen).len(), sent_before, "nothing to send");
        assert_eq!(
            inbox.unavailable(),
            Some(arena_proto::ResendUnavailable {
                epoch: EPOCH.into(),
                from: 5,
                to: 9
            })
        );
        assert_eq!(inbox.pending(), None, "not asked for again by the Hub");
    }

    #[tokio::test]
    async fn rows_missing_in_the_middle_are_sent_where_held_and_reported_where_not() {
        let (port, seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Basic).await;
        let inbox = with_inbox(&mut f, 5);
        add_rows(&f.writer, EPOCH, 3).await;
        f.task.cycle().await;
        let sent_before = puts(&seen).len();

        // Rows 1..=3 are held; 4..=6 are not.
        inbox.offer(EPOCH, arena_proto::SeqRange { from: 2, to: 6 });
        f.task.cycle().await;

        let keys: Vec<String> = puts(&seen)
            .iter()
            .skip(sent_before)
            .map(|(k, _)| k.clone())
            .collect();
        assert_eq!(keys, [format!("/put/{EPOCH}/2-3")]);
        assert_eq!(inbox.unavailable().map(|u| (u.from, u.to)), Some((4, 6)));
        assert_eq!(inbox.pending(), None);
    }

    #[tokio::test]
    async fn a_resend_of_another_epoch_is_dropped() {
        let (port, seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Basic).await;
        let inbox = with_inbox(&mut f, 5);
        inbox.offer("9:OTHER", arena_proto::SeqRange { from: 1, to: 4 });
        f.task.cycle().await;
        assert_eq!(inbox.pending(), None);
        assert!(presigns(&seen).is_empty());
        assert_eq!(inbox.unavailable(), None);
    }

    #[tokio::test]
    async fn a_failed_resend_put_keeps_the_run_for_the_next_cycle() {
        let mut puts_seen = 0;
        let (port, seen) = serve(move |port| {
            let mut ok = cloud(port);
            Box::new(move |line, body| {
                if line.starts_with("PUT ") {
                    puts_seen += 1;
                    if puts_seen == 3 {
                        return refused(503, "INTERNAL");
                    }
                }
                ok(line, body)
            })
        })
        .await;
        let mut f = fx(port, Tier::Basic).await;
        let inbox = with_inbox(&mut f, 3);
        add_rows(&f.writer, EPOCH, 6).await;
        f.task.cycle().await;
        assert_eq!(puts(&seen).len(), 2, "6 rows in two batches of 3");

        inbox.offer(EPOCH, arena_proto::SeqRange { from: 1, to: 6 });
        let next = f.task.cycle().await;
        assert!(!next.early && next.active, "{next:?}");
        assert_eq!(
            inbox.pending().map(|(_, r)| (r.from, r.to)),
            Some((1, 6)),
            "the first batch of the resend failed"
        );

        f.task.cycle().await;
        assert_eq!(inbox.pending(), None);
        let keys: Vec<String> = puts(&seen).iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(keys.last().unwrap(), &format!("/put/{EPOCH}/4-6"));
        assert_eq!(cursor(&f).unwrap().last_seq_uploaded, 6);
    }

    #[test]
    fn the_same_ask_again_keeps_the_progress_already_made() {
        let inbox = ResendInbox::new();
        inbox.offer("1:A", arena_proto::SeqRange { from: 1, to: 10 });
        inbox.advance(4);
        inbox.offer("1:A", arena_proto::SeqRange { from: 1, to: 10 });
        assert_eq!(inbox.pending().map(|(_, r)| r.from), Some(5));
        inbox.offer("1:A", arena_proto::SeqRange { from: 20, to: 30 });
        assert_eq!(inbox.pending().map(|(_, r)| r.from), Some(20));
        inbox.offer("1:A", arena_proto::SeqRange { from: 9, to: 3 });
        assert_eq!(inbox.pending().map(|(_, r)| r.from), Some(20), "invalid");
        inbox.advance(30);
        assert_eq!(inbox.pending(), None);
    }

    // ---- the daily rollup (Story 8.6) ----

    const DAY: i64 = 24 * 60 * 60 * 1000;

    /// A cloud that issues batch and rollup links; `up` false makes the bucket refuse every
    /// rollup PUT.
    fn cloud_with_rollups(up: Arc<std::sync::atomic::AtomicBool>) -> impl FnOnce(u16) -> Handler {
        cloud_with_snapshots(up, Arc::new(std::sync::atomic::AtomicBool::new(true)))
    }

    /// As [`cloud_with_rollups`], and it issues snapshot links; snapshot PUTs fail while
    /// `snapshots_up` is false.
    fn cloud_with_snapshots(
        up: Arc<std::sync::atomic::AtomicBool>,
        snapshots_up: Arc<std::sync::atomic::AtomicBool>,
    ) -> impl FnOnce(u16) -> Handler {
        move |port| {
            Box::new(move |line, body| {
                if line.starts_with("POST /v1/presign") {
                    let req: serde_json::Value = serde_json::from_slice(body).unwrap();
                    let mut links = serde_json::Map::new();
                    if let Some(b) = req.get("batch").filter(|b| !b.is_null()) {
                        links.insert(
                            "batch_url".into(),
                            format!(
                                "http://127.0.0.1:{port}/put/{}/{}-{}",
                                req["epoch"].as_str().unwrap(),
                                b["first"],
                                b["last"]
                            )
                            .into(),
                        );
                    }
                    if req["kind"] == "snapshot" {
                        links.insert(
                            "url".into(),
                            format!(
                                "http://127.0.0.1:{port}/put/snapshots/{}.db.zst",
                                req["date"].as_str().unwrap()
                            )
                            .into(),
                        );
                    }
                    if req["kind"] == "rollup" {
                        links.insert(
                            "url".into(),
                            format!(
                                "http://127.0.0.1:{port}/put/rollups/{}.json",
                                req["date"].as_str().unwrap()
                            )
                            .into(),
                        );
                    }
                    (200, serde_json::Value::Object(links).to_string())
                } else if (line.starts_with("PUT /put/rollups/")
                    && !up.load(std::sync::atomic::Ordering::SeqCst))
                    || (line.starts_with("PUT /put/snapshots/")
                        && !snapshots_up.load(std::sync::atomic::Ordering::SeqCst))
                {
                    (503, "{}".into())
                } else {
                    (200, "{}".into())
                }
            })
        }
    }

    fn rollup_asks(seen: &Seen) -> Vec<String> {
        presigns(seen)
            .into_iter()
            .filter(|p| p["kind"] == "rollup")
            .map(|p| p["date"].as_str().unwrap().to_owned())
            .collect()
    }

    fn rollup_puts(seen: &Seen) -> Vec<(String, arena_proto::RollupV1)> {
        puts(seen)
            .into_iter()
            .filter(|(k, _)| k.starts_with("/put/rollups/"))
            .map(|(k, b)| (k, serde_json::from_slice(&b).unwrap()))
            .collect()
    }

    fn rollup_row(f: &Fx, day: &str) -> Option<arena_store::RollupLogRow> {
        Store::open_reader(&f.path)
            .unwrap()
            .rollup_log(day)
            .unwrap()
    }

    /// Makes the branch paid `days` days before the fixture's clock.
    async fn paid_since(f: &Fx, days: i64) {
        f.writer
            .submit(move |tx| {
                arena_store::write_upload_cursor(
                    tx,
                    &arena_store::UploadCursor {
                        epoch: EPOCH.into(),
                        last_seq_uploaded: 0,
                        uploaded_at: T0 - days * DAY,
                    },
                )
            })
            .await
            .unwrap();
    }

    /// The fixture runs no lease task: after the clock moves, extend the lease it holds.
    async fn keep_lease(f: &Fx) {
        let now = f.clock.now_ms();
        f.writer
            .submit(move |tx| {
                tx.execute(
                    "UPDATE lease SET renewed_at = ?1, expires_at = ?2 WHERE id = 1",
                    [now, now + 600_000],
                )
                .map_err(|e| arena_proto::AppError::new(ErrorCode::Internal, e.to_string()))
            })
            .await
            .unwrap();
    }

    async fn rollup_fx(tier: Tier, up: bool) -> (Fx, Seen, Arc<std::sync::atomic::AtomicBool>) {
        let up = Arc::new(std::sync::atomic::AtomicBool::new(up));
        let (port, seen) = serve(cloud_with_rollups(Arc::clone(&up))).await;
        let mut f = fx(port, tier).await;
        f.task.rollups = true;
        (f, seen, up)
    }

    #[tokio::test]
    async fn every_missed_day_since_going_paid_is_rolled_up_oldest_first() {
        let (mut f, seen, _) = rollup_fx(Tier::Basic, true).await;
        // T0 is 2027-01-15 13:30 in Asia/Kolkata; paid since the 12th.
        paid_since(&f, 3).await;

        let next = f.task.cycle().await;
        assert!(next.active);
        assert_eq!(
            rollup_asks(&seen),
            ["2027-01-12", "2027-01-13", "2027-01-14"]
        );
        let sent = rollup_puts(&seen);
        assert_eq!(sent.len(), 3);
        for (key, doc) in &sent {
            assert_eq!(key, &format!("/put/rollups/{}.json", doc.business_day));
            assert_eq!((doc.schema, doc.branch_id.as_str()), (1, "B"));
        }
        for day in ["2027-01-12", "2027-01-13", "2027-01-14"] {
            let row = rollup_row(&f, day).unwrap_or_else(|| panic!("no rollup_log row for {day}"));
            assert_eq!(
                (row.schema, row.built_at, row.key),
                (1, T0, format!("rollups/{day}.json"))
            );
        }
        assert!(rollup_row(&f, "2027-01-15").is_none(), "today is not over");
        assert_eq!(progress(&f).last_rollup_date.as_deref(), Some("2027-01-14"));

        // Each publish is a system intent in the audit: one change row per day.
        let audit: i64 = Store::open_reader(&f.path)
            .unwrap()
            .read_with(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM changes WHERE tbl = 'rollup_log'
                       AND intent_kind = 'rollup.publish' AND actor_kind = 'system'",
                    [],
                    |r| r.get(0),
                )
                .unwrap())
            })
            .unwrap();
        assert_eq!(audit, 3);

        // Nothing more is due.
        f.task.cycle().await;
        assert_eq!(rollup_asks(&seen).len(), 3);
    }

    #[tokio::test]
    async fn a_day_is_rolled_up_thirty_minutes_after_the_roll_not_before() {
        let (mut f, seen, _) = rollup_fx(Tier::Premium, true).await;
        paid_since(&f, 0).await;
        // 2027-01-16 00:29 IST: the roll is at midnight (day start 00:00), the rollup 30 min later.
        f.clock.set(T0 + 10 * 60 * 60_000 + 59 * 60_000);
        keep_lease(&f).await;
        let next = f.task.cycle().await;
        assert!(rollup_asks(&seen).is_empty());
        assert_eq!(
            next.wait,
            Duration::from_secs(60),
            "wakes for the rollup, not a whole interval later"
        );

        f.clock.advance(60_000);
        keep_lease(&f).await;
        f.task.cycle().await;
        assert_eq!(rollup_asks(&seen), ["2027-01-15"]);
        assert!(rollup_row(&f, "2027-01-15").is_some());
    }

    #[tokio::test]
    async fn a_failed_put_leaves_no_row_and_the_day_is_planned_again() {
        let (mut f, seen, up) = rollup_fx(Tier::Basic, false).await;
        paid_since(&f, 1).await;

        let next = f.task.cycle().await;
        assert!(next.active && !next.early, "{next:?}");
        assert_eq!(rollup_asks(&seen), ["2027-01-14"]);
        assert!(rollup_row(&f, "2027-01-14").is_none());

        up.store(true, std::sync::atomic::Ordering::SeqCst);
        f.task.cycle().await;
        assert_eq!(rollup_asks(&seen), ["2027-01-14", "2027-01-14"]);
        assert!(rollup_row(&f, "2027-01-14").is_some());
    }

    fn orders_change(seq: i64, tbl: &str) -> arena_core::Applied {
        arena_core::Applied {
            intent_id: "I".into(),
            changes: vec![arena_core::ChangeMeta {
                epoch: EPOCH.into(),
                seq,
                tbl: tbl.into(),
                row_id: "O1".into(),
                op: ChangeOp::Update,
            }],
        }
    }

    #[tokio::test]
    async fn a_late_change_to_a_rolled_day_regenerates_and_overwrites_its_file() {
        let (mut f, seen, _) = rollup_fx(Tier::Basic, true).await;
        paid_since(&f, 1).await;
        f.task.cycle().await;
        assert_eq!(rollup_asks(&seen), ["2027-01-14"]);
        let first = rollup_row(&f, "2027-01-14").unwrap();

        // A late Undo: a change to an order, stamped to the 14th, after its rollup went out.
        f.clock.advance(5 * 60_000);
        keep_lease(&f).await;
        let seq = f
            .writer
            .submit(|tx| {
                let seq = next_change_seq(tx, EPOCH)?;
                insert_change(
                    tx,
                    &ChangeRecord {
                        epoch: EPOCH.into(),
                        seq,
                        tbl: "orders".into(),
                        row_id: "O1".into(),
                        op: ChangeOp::Update,
                        row: vec![0x80],
                        intent_id: "I".into(),
                        intent_kind: "order.undo".into(),
                        actor_kind: "staff".into(),
                        actor_id: "s".into(),
                        elevated_by_kind: None,
                        elevated_by_id: None,
                        elevated_by_mode: None,
                        elevated_by_verifier_version: None,
                        at: T0,
                        business_day: Some("2027-01-14".into()),
                        fy: None,
                        reason: None,
                    },
                )?;
                Ok(seq)
            })
            .await
            .unwrap();
        f.task.hear(&orders_change(seq, "orders"));
        // Configuration changes are not heard at all.
        f.task.hear(&orders_change(1, "settings"));
        assert_eq!(f.task.heard.len(), 1);

        f.task.cycle().await;
        assert_eq!(rollup_asks(&seen), ["2027-01-14", "2027-01-14"]);
        let second = rollup_row(&f, "2027-01-14").unwrap();
        assert!(second.built_at > first.built_at);
        assert_eq!(second.key, first.key, "the same file is overwritten");

        // Handled: the next cycle asks for nothing.
        f.task.cycle().await;
        assert_eq!(rollup_asks(&seen).len(), 2);
    }

    #[tokio::test]
    async fn a_free_hub_and_a_hub_that_has_not_uploaded_roll_nothing_up() {
        let (mut f, seen, _) = rollup_fx(Tier::Free, true).await;
        paid_since(&f, 3).await;
        f.task.cycle().await;
        assert!(seen.lock().unwrap().is_empty());

        // Paid, but nothing was ever uploaded: not paid as far as the Hub can tell.
        let (mut g, seen, _) = rollup_fx(Tier::Basic, true).await;
        g.task.cycle().await;
        assert!(rollup_asks(&seen).is_empty());
    }

    #[tokio::test]
    async fn a_long_catch_up_is_spread_over_cycles() {
        let (mut f, seen, _) = rollup_fx(Tier::Basic, true).await;
        paid_since(&f, 14).await;
        let next = f.task.cycle().await;
        assert_eq!(rollup_asks(&seen).len(), ROLLUPS_PER_CYCLE);
        assert_eq!(next.wait, ROLLUP_CATCH_UP_WAIT);
        assert_eq!(rollup_asks(&seen)[0], "2027-01-01", "oldest first");
        f.task.cycle().await;
        assert_eq!(rollup_asks(&seen).len(), 14);
    }

    /// A Hub release whose newest writer is v2: the v1 document stamped as v2 (the real v2 shape
    /// arrives with Epic 13).
    fn v2_writer(
        r: &SnapshotReader,
        day: &str,
        now: i64,
        schema: u32,
    ) -> Result<Option<arena_proto::RollupV1>, AppError> {
        r.daily_rollup(day, now, 1).map(|d| {
            d.map(|mut d| {
                d.schema = schema;
                d
            })
        })
    }

    async fn pin_rollup_schema(f: &Fx, tier: Tier, pin: u32) {
        let mut claims = LicenceClaims::for_tier("T", "B", tier, T0 / 1000);
        claims.rollup_schema = pin;
        f.writer
            .record_heartbeat(Some(dev_sign(&claims).unwrap()), None, None)
            .await
            .unwrap();
    }

    fn logged_schemas(f: &Fx) -> Vec<u32> {
        Store::open_reader(&f.path)
            .unwrap()
            .read_with(|c| {
                let mut stmt = c
                    .prepare("SELECT schema FROM rollup_log WHERE date <> 'latest' ORDER BY date")
                    .unwrap();
                Ok(stmt
                    .query_map([], |r| r.get(0))
                    .unwrap()
                    .collect::<Result<Vec<u32>, _>>()
                    .unwrap())
            })
            .unwrap()
    }

    #[tokio::test]
    async fn old_days_are_regenerated_oldest_first_ten_a_minute_and_only_once() {
        let (mut f, seen, _) = rollup_fx(Tier::Basic, true).await;
        paid_since(&f, 14).await;
        f.task.cycle().await;
        f.task.cycle().await;
        assert_eq!(rollup_asks(&seen).len(), 14);
        assert_eq!(logged_schemas(&f), vec![1; 14]);

        // The Hub is updated to a v2 writer and the licence raises the pin to 2.
        f.task.newest_schema = 2;
        f.task.build_rollup = v2_writer;
        pin_rollup_schema(&f, Tier::Basic, 2).await;
        let before = rollup_asks(&seen).len();
        let next = f.task.cycle().await;
        let asks = rollup_asks(&seen);
        assert_eq!(asks.len() - before, REGENERATIONS_PER_MINUTE);
        assert_eq!(asks[before], "2027-01-01", "oldest first");
        assert_eq!(asks[asks.len() - 1], "2027-01-10");
        let schemas = logged_schemas(&f);
        assert_eq!(&schemas[..10], &[2; 10]);
        assert_eq!(&schemas[10..], &[1; 4]);
        let sent = rollup_puts(&seen);
        assert_eq!(sent.last().unwrap().1.schema, 2);
        assert!(
            next.wait <= Duration::from_secs(60) && next.wait >= Duration::from_secs(1),
            "wakes when the minute frees a place: {:?}",
            next.wait
        );

        // The rest of the minute: nothing more goes out.
        f.clock.advance(30_000);
        keep_lease(&f).await;
        f.task.cycle().await;
        assert_eq!(rollup_asks(&seen).len(), before + 10);

        // A minute after the first lot, the remaining four go and nothing is rebuilt twice.
        f.clock.advance(31_000);
        keep_lease(&f).await;
        let next = f.task.cycle().await;
        let asks = rollup_asks(&seen);
        assert_eq!(asks.len(), before + 14);
        assert_eq!(
            &asks[before + 10..],
            ["2027-01-11", "2027-01-12", "2027-01-13", "2027-01-14"]
        );
        assert_eq!(logged_schemas(&f), vec![2; 14]);
        assert_eq!(next.wait, Duration::from_secs(300));
        f.task.cycle().await;
        assert_eq!(rollup_asks(&seen).len(), before + 14);
    }

    #[tokio::test]
    async fn a_restart_resumes_from_the_oldest_day_still_below() {
        let (mut f, seen, _) = rollup_fx(Tier::Basic, true).await;
        paid_since(&f, 12).await;
        f.task.cycle().await;
        f.task.cycle().await;
        f.task.newest_schema = 2;
        f.task.build_rollup = v2_writer;
        pin_rollup_schema(&f, Tier::Basic, 2).await;
        f.task.cycle().await;
        assert_eq!(logged_schemas(&f)[..10], [2; 10]);

        // The Hub restarts: a fresh task knows only the log.
        let mut again = UploadTask::new(
            f.task.cloud.clone(),
            Arc::clone(&f.task.keystore),
            Some(f.writer.clone()),
            Arc::clone(&f.task.snapshots),
            PairGate::new(true),
            f.status.clone(),
            DEV_LICENCE_VERIFY_KEYS.to_vec(),
        );
        again.newest_schema = 2;
        again.build_rollup = v2_writer;
        let before = rollup_asks(&seen).len();
        f.clock.advance(61_000);
        keep_lease(&f).await;
        again.cycle().await;
        let asks = rollup_asks(&seen);
        assert_eq!(&asks[before..], ["2027-01-13", "2027-01-14"]);
        assert_eq!(logged_schemas(&f), vec![2; 12]);
    }

    #[tokio::test]
    async fn the_hub_emits_the_pinned_schema_not_its_newest() {
        let (mut f, seen, _) = rollup_fx(Tier::Basic, true).await;
        paid_since(&f, 2).await;
        // The Hub can write v2, but the licence still pins v1: new days are v1 and nothing is old.
        f.task.newest_schema = 2;
        f.task.build_rollup = v2_writer;
        pin_rollup_schema(&f, Tier::Basic, 1).await;
        f.task.cycle().await;
        assert_eq!(rollup_asks(&seen).len(), 2);
        assert!(rollup_puts(&seen).iter().all(|(_, d)| d.schema == 1));
        f.task.cycle().await;
        assert_eq!(rollup_asks(&seen).len(), 2, "nothing is rebuilt");
        assert_eq!(logged_schemas(&f), [1, 1]);

        // A pin past what the Hub can write is held to its newest writer.
        pin_rollup_schema(&f, Tier::Basic, 9).await;
        assert_eq!(f.task.rollup_schema(&f.writer, Tier::Basic).await, 2);
        // A licence that cannot be verified falls back to the tier's default pin.
        f.task.keys = Vec::new();
        assert_eq!(f.task.rollup_schema(&f.writer, Tier::Basic).await, 1);
    }

    #[tokio::test]
    async fn a_day_that_cannot_be_rebuilt_does_not_hold_up_the_days_behind_it() {
        let (mut f, seen, _) = rollup_fx(Tier::Basic, true).await;
        paid_since(&f, 3).await;
        f.task.cycle().await;
        f.task.newest_schema = 2;
        f.task.build_rollup = |r, day, now, schema| {
            if day == "2027-01-12" {
                Ok(None)
            } else {
                v2_writer(r, day, now, schema)
            }
        };
        pin_rollup_schema(&f, Tier::Basic, 2).await;
        f.task.cycle().await;
        assert_eq!(logged_schemas(&f), [1, 2, 2]);
        let before = rollup_asks(&seen).len();
        f.task.cycle().await;
        assert_eq!(
            rollup_asks(&seen).len(),
            before,
            "the stuck day is not retried every cycle"
        );
    }

    // ---- the daily snapshot (Story 8.8) ----

    async fn snapshot_fx(tier: Tier) -> (Fx, Seen, Arc<std::sync::atomic::AtomicBool>) {
        let snapshots_up = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let (port, seen) = serve(cloud_with_snapshots(
            Arc::new(std::sync::atomic::AtomicBool::new(true)),
            Arc::clone(&snapshots_up),
        ))
        .await;
        let mut f = fx(port, tier).await;
        f.task.rollups = true;
        f.task.daily_snapshot = true;
        f.task.free_space = |_| Some(u64::MAX);
        (f, seen, snapshots_up)
    }

    fn snapshot_asks(seen: &Seen) -> Vec<String> {
        presigns(seen)
            .into_iter()
            .filter(|p| p["kind"] == "snapshot")
            .map(|p| p["date"].as_str().unwrap().to_owned())
            .collect()
    }

    fn snapshot_puts(seen: &Seen) -> Vec<(String, Vec<u8>)> {
        puts(seen)
            .into_iter()
            .filter(|(k, _)| k.starts_with("/put/snapshots/"))
            .collect()
    }

    fn snapshot_row(f: &Fx, day: &str) -> Option<arena_store::SnapshotLogRow> {
        Store::open_reader(&f.path)
            .unwrap()
            .read_with(|c| arena_store::find_snapshot_log(c, day))
            .unwrap()
    }

    /// The files a snapshot leaves beside the database.
    fn scratch_files(f: &Fx) -> Vec<String> {
        std::fs::read_dir(f.path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("snapshot-"))
            .collect()
    }

    fn count(f: &Fx, sql: &'static str) -> i64 {
        Store::open_reader(&f.path)
            .unwrap()
            .read_with(|c| Ok(c.query_row(sql, [], |r| r.get(0)).unwrap()))
            .unwrap()
    }

    /// Opens a downloaded snapshot as a database.
    fn open_snapshot(dir: &std::path::Path, bytes: &[u8]) -> arena_store::DbConn {
        let plain = zstd::decode_all(bytes).expect("a zstd file");
        let path = dir.join("opened.db");
        std::fs::write(&path, plain).unwrap();
        arena_store::DbConn::open(path).unwrap()
    }

    #[tokio::test]
    async fn the_snapshot_follows_the_days_rollup_and_is_recorded_after_its_put() {
        let (mut f, seen, _) = snapshot_fx(Tier::Basic).await;
        paid_since(&f, 1).await;

        f.task.cycle().await;
        assert_eq!(rollup_asks(&seen), ["2027-01-14"]);
        assert_eq!(snapshot_asks(&seen), ["2027-01-14"]);
        let sent = snapshot_puts(&seen);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, "/put/snapshots/2027-01-14.db.zst");

        let out = tempfile::tempdir().unwrap();
        let copy = open_snapshot(out.path(), &sent[0].1);
        let (epoch, last): (String, i64) = copy
            .query_row(
                "SELECT epoch, MAX(seq) FROM changes
                 WHERE epoch_counter = (SELECT MAX(epoch_counter) FROM changes)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let row = snapshot_row(&f, "2027-01-14").expect("recorded after the PUT");
        assert_eq!(
            (row.epoch.as_str(), row.last_seq, row.key.as_str()),
            (epoch.as_str(), last, "snapshots/2027-01-14.db.zst")
        );
        assert_eq!(row.epoch, EPOCH);
        assert_eq!(row.bytes, i64::try_from(sent[0].1.len()).unwrap());
        assert_eq!(row.uploaded_at, T0);
        // The copy holds the day's rollup publish, so it is taken after the rollup.
        let published: i64 = copy
            .query_row(
                "SELECT COUNT(*) FROM rollup_log WHERE date = '2027-01-14'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(published, 1);
        assert!(scratch_files(&f).is_empty(), "{:?}", scratch_files(&f));

        // Taken once: the next cycle asks for nothing.
        f.task.cycle().await;
        assert_eq!(snapshot_asks(&seen).len(), 1);
    }

    #[tokio::test]
    async fn the_snapshot_holds_player_data_only_as_ciphertext_and_no_key_table() {
        let (mut f, seen, _) = snapshot_fx(Tier::Basic).await;
        paid_since(&f, 1).await;
        f.writer
            .submit(|tx| {
                tx.execute_batch(
                    "INSERT INTO players (player_id, username, password_hash, phone_lookup,
                                          name_enc, phone_enc, tags, rev, created_at, updated_at)
                     VALUES ('P1', 'asha', 'argon-hash', 'lookup-hash', 'v1.CIPHERTEXT-NAME',
                             'v1.CIPHERTEXT-PHONE', '[]', 0, 0, 0);
                     INSERT INTO player_keys (player_id, dek_wrapped)
                     VALUES ('P1', 'WRAPPED-DEK-MUST-NOT-LEAVE');",
                )
                .map_err(|e| arena_proto::AppError::new(ErrorCode::Internal, e.to_string()))
            })
            .await
            .unwrap();

        f.task.cycle().await;
        let sent = snapshot_puts(&seen);
        assert_eq!(sent.len(), 1);
        let plain = zstd::decode_all(&sent[0].1[..]).unwrap();
        let text = String::from_utf8_lossy(&plain);
        assert!(!text.contains("WRAPPED-DEK-MUST-NOT-LEAVE"), "no key bytes");

        let out = tempfile::tempdir().unwrap();
        let copy = open_snapshot(out.path(), &sent[0].1);
        let tables: i64 = copy
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'player_keys'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 0, "the player key table is not in the file");
        let (name, phone): (String, String) = copy
            .query_row(
                "SELECT name_enc, phone_enc FROM players WHERE player_id = 'P1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            (name.as_str(), phone.as_str()),
            ("v1.CIPHERTEXT-NAME", "v1.CIPHERTEXT-PHONE")
        );
        // The live database keeps its keys.
        assert_eq!(count(&f, "SELECT COUNT(*) FROM player_keys"), 1);
    }

    #[tokio::test]
    async fn after_a_long_outage_only_the_latest_missed_day_is_stored() {
        let (mut f, seen, _) = snapshot_fx(Tier::Basic).await;
        paid_since(&f, 3).await;
        f.task.cycle().await;
        assert_eq!(
            rollup_asks(&seen),
            ["2027-01-12", "2027-01-13", "2027-01-14"]
        );
        assert_eq!(snapshot_asks(&seen), ["2027-01-14"]);
        assert!(snapshot_row(&f, "2027-01-12").is_none());
        assert!(snapshot_row(&f, "2027-01-13").is_none());
        assert!(snapshot_row(&f, "2027-01-14").is_some());
    }

    #[tokio::test]
    async fn a_failed_snapshot_put_leaves_no_row_and_the_next_cycle_takes_it_again() {
        let (mut f, seen, snapshots_up) = snapshot_fx(Tier::Basic).await;
        snapshots_up.store(false, std::sync::atomic::Ordering::SeqCst);
        paid_since(&f, 1).await;

        let next = f.task.cycle().await;
        assert!(next.active && !next.early, "{next:?}");
        assert_eq!(snapshot_asks(&seen), ["2027-01-14"]);
        assert!(snapshot_row(&f, "2027-01-14").is_none());
        assert!(
            rollup_row(&f, "2027-01-14").is_some(),
            "the rollup is not held back"
        );
        assert!(scratch_files(&f).is_empty(), "{:?}", scratch_files(&f));

        snapshots_up.store(true, std::sync::atomic::Ordering::SeqCst);
        f.task.cycle().await;
        assert_eq!(snapshot_asks(&seen), ["2027-01-14", "2027-01-14"]);
        assert!(snapshot_row(&f, "2027-01-14").is_some());
    }

    #[tokio::test]
    async fn with_too_little_disk_the_snapshot_is_skipped_with_one_alert_and_an_audit_row() {
        let (mut f, seen, _) = snapshot_fx(Tier::Basic).await;
        paid_since(&f, 1).await;
        // Under twice the database free.
        f.task.free_space = |_| Some(1);

        f.task.cycle().await;
        assert!(snapshot_asks(&seen).is_empty(), "no link asked for");
        assert!(snapshot_row(&f, "2027-01-14").is_none());
        assert_eq!(
            count(
                &f,
                "SELECT COUNT(*) FROM alerts WHERE kind = 'backup_disk' AND acked_at IS NULL"
            ),
            1,
            "the alert is open"
        );
        let audit = "SELECT COUNT(*) FROM changes WHERE tbl = 'alerts'
                       AND intent_kind = 'alert.raise' AND actor_kind = 'system'";
        assert_eq!(count(&f, audit), 1, "and it is in the audit");
        assert!(scratch_files(&f).is_empty());

        // A disk that stays full raises nothing more.
        f.task.cycle().await;
        f.task.cycle().await;
        assert_eq!(count(&f, audit), 1);
        assert!(snapshot_asks(&seen).is_empty());

        // Room again: the snapshot goes, the alert stays for staff to acknowledge.
        f.task.free_space = |_| Some(u64::MAX);
        f.task.cycle().await;
        assert_eq!(snapshot_asks(&seen), ["2027-01-14"]);
        assert!(snapshot_row(&f, "2027-01-14").is_some());
    }

    #[tokio::test]
    async fn the_disk_rule_is_twice_the_database() {
        let (mut f, seen, _) = snapshot_fx(Tier::Basic).await;
        paid_since(&f, 1).await;
        let db = arena_store::database_bytes(&f.path);
        assert!(db > 0);
        // Just under twice what the database occupies is not enough; far over it is.
        f.task.free_space = |_| Some(1);
        f.task.cycle().await;
        assert!(snapshot_asks(&seen).is_empty());
        f.task.free_space = |_| Some(1 << 40);
        f.task.cycle().await;
        assert_eq!(snapshot_asks(&seen).len(), 1);
    }

    #[tokio::test]
    async fn a_free_hub_stores_no_snapshot() {
        let (mut f, seen, _) = snapshot_fx(Tier::Free).await;
        paid_since(&f, 1).await;
        f.task.cycle().await;
        assert!(presigns(&seen).is_empty());
        assert!(snapshot_row(&f, "2027-01-14").is_none());
    }

    #[tokio::test]
    async fn trading_goes_on_while_a_snapshot_is_taken() {
        let (f, _, _) = snapshot_fx(Tier::Basic).await;
        // A database big enough for the copy to take a moment.
        add_rows(&f.writer, EPOCH, 30_000).await;
        let reader = Arc::clone(&f.task.snapshots);
        let (dir, _) = reader.snapshot_scratch().unwrap();
        let copying = tokio::task::spawn_blocking(move || {
            let mut sizes = Vec::new();
            for _ in 0..3 {
                let file = reader.snapshot_file(&dir, "2027-01-14").unwrap();
                sizes.push(file.bytes);
            }
            sizes
        });
        let mut slowest = Duration::ZERO;
        let mut writes = 0;
        while !copying.is_finished() {
            let started = std::time::Instant::now();
            add_rows(&f.writer, EPOCH, 1).await;
            slowest = slowest.max(started.elapsed());
            writes += 1;
            tokio::task::yield_now().await;
        }
        let sizes = copying.await.unwrap();
        assert!(sizes.iter().all(|s| *s > 0));
        assert!(writes > 0);
        assert!(
            slowest < Duration::from_secs(2),
            "a write waited {slowest:?} behind the copy"
        );
    }

    #[tokio::test]
    async fn a_cycles_outcome_tells_why_it_did_not_finish() {
        let (port, _seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 2).await;
        f.task.cycle().await;
        assert_eq!(f.task.outcome, None);

        let (port, _seen) = serve(|_| Box::new(|_, _| refused(403, "FEATURE_NOT_ENTITLED"))).await;
        let mut f = fx(port, Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 2).await;
        f.task.cycle().await;
        assert_eq!(
            f.task.outcome.as_ref().map(|e| e.code),
            Some(ErrorCode::FeatureNotEntitled)
        );

        let spare = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = spare.local_addr().unwrap().port();
        drop(spare);
        let mut f = fx(port, Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 2).await;
        f.task.cycle().await;
        assert_eq!(
            f.task.outcome.as_ref().map(|e| e.code),
            Some(ErrorCode::CloudUnreachable)
        );

        let (port, _seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Free).await;
        f.task.cycle().await;
        assert_eq!(
            f.task.outcome.as_ref().map(|e| e.code),
            Some(ErrorCode::FeatureNotEntitled)
        );
    }

    #[tokio::test]
    async fn backup_now_runs_a_cycle_at_once_and_answers_its_outcome() {
        let (port, seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Basic).await;
        let (requester, responder) = backup_channel();
        f.task = f.task.with_backup(responder);
        add_rows(&f.writer, EPOCH, 2).await;
        let writer = f.writer.clone();
        let job = tokio::spawn(f.task.run());
        while puts(&seen).is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(puts(&seen).len(), 1);

        add_rows(&writer, EPOCH, 1).await;
        assert_eq!(
            puts(&seen).len(),
            1,
            "a few rows alone wait for the interval"
        );
        requester.run(Duration::from_secs(10)).await.unwrap();
        assert_eq!(puts(&seen).len(), 2, "the poke uploaded them at once");
        requester.run(Duration::from_secs(10)).await.unwrap();
        assert_eq!(puts(&seen).len(), 2, "nothing waiting counts as success");
        job.abort();
    }

    #[tokio::test]
    async fn backup_now_reports_the_transport_error_and_a_free_branch_is_refused() {
        let spare = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = spare.local_addr().unwrap().port();
        drop(spare);
        let mut f = fx(port, Tier::Basic).await;
        let (requester, responder) = backup_channel();
        f.task = f.task.with_backup(responder);
        add_rows(&f.writer, EPOCH, 2).await;
        let job = tokio::spawn(f.task.run());
        let err = requester.run(Duration::from_secs(10)).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::CloudUnreachable);
        job.abort();

        let (port, seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Free).await;
        let (requester, responder) = backup_channel();
        f.task = f.task.with_backup(responder);
        let job = tokio::spawn(f.task.run());
        let err = requester.run(Duration::from_secs(10)).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::FeatureNotEntitled);
        assert!(seen.lock().unwrap().is_empty());
        job.abort();
    }

    #[tokio::test]
    async fn backup_now_gives_up_after_its_wait_and_without_an_uploader() {
        let (requester, mut responder) = backup_channel();
        let err = requester.run(Duration::from_millis(30)).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::HubSlow);
        assert!(responder.rx.try_recv().is_ok(), "the ask was queued");
        drop(responder);
        let err = requester.run(Duration::from_secs(1)).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::FeatureNotEntitled);
    }

    // ---- Story 9.11: the upgrade snapshot ----

    /// A branch that has never uploaded: the cursor is absent and the first snapshot is on.
    async fn upgrade_fx(tier: Tier) -> (Fx, Seen, Arc<std::sync::atomic::AtomicBool>) {
        let (mut f, seen, snapshots_up) = snapshot_fx(tier).await;
        f.task.rollups = false;
        f.task.daily_snapshot = false;
        f.task.first_snapshot = true;
        (f, seen, snapshots_up)
    }

    /// The key backup has stored the key table as it is now.
    async fn keys_backed_up(f: &Fx) {
        let version = Store::open_reader(&f.path)
            .unwrap()
            .read_with(arena_store::read_backup_state)
            .unwrap()
            .keys_version;
        f.writer.record_key_backup(version).await.unwrap();
    }

    fn backup_state(f: &Fx) -> arena_store::BackupState {
        Store::open_reader(&f.path)
            .unwrap()
            .read_with(arena_store::read_backup_state)
            .unwrap()
    }

    /// The kinds of the requests the cloud saw, in order: `snapshot` PUT or `batch` PUT.
    fn put_order(seen: &Seen) -> Vec<&'static str> {
        puts(seen)
            .into_iter()
            .map(|(k, _)| {
                if k.starts_with("/put/snapshots/") {
                    "snapshot"
                } else {
                    "batch"
                }
            })
            .collect()
    }

    #[tokio::test]
    async fn an_upgrade_stores_a_full_snapshot_before_any_batch_and_batches_from_the_next_row() {
        let (mut f, seen, _) = upgrade_fx(Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 5).await;
        keys_backed_up(&f).await;
        assert!(cursor(&f).is_none(), "a Free branch uploaded nothing");

        f.task.cycle().await;
        assert_eq!(snapshot_asks(&seen), ["2027-01-15"]);
        let sent = snapshot_puts(&seen);
        assert_eq!(sent[0].0, "/put/snapshots/2027-01-15.db.zst");
        let row = snapshot_row(&f, "2027-01-15").expect("recorded after the PUT");
        assert_eq!(row.key, "snapshots/2027-01-15.db.zst");
        assert_eq!(row.epoch, EPOCH);
        let base = row.last_seq;
        assert!(base >= 5);
        assert!(scratch_files(&f).is_empty());

        // Batching starts after the snapshot: nothing is shipped for the history it holds.
        let c = cursor(&f).expect("the cursor is set by the snapshot");
        assert_eq!((c.epoch.as_str(), c.last_seq_uploaded), (EPOCH, base));
        assert_eq!(put_order(&seen), ["snapshot"]);
        let state = backup_state(&f);
        assert!(!state.first_snapshot_pending);
        assert_eq!(state.first_snapshot_error, None);
        assert_eq!(
            (state.base_epoch.as_deref(), state.base_seq),
            (Some(EPOCH), Some(base))
        );
        // The heartbeat reports the base, so the cloud asks for no history before it.
        let p = progress(&f);
        assert_eq!((p.last_seq_uploaded, p.base_seq), (base, Some(base)));
        assert_eq!(p.pending_rows, 0);

        add_rows(&f.writer, EPOCH, 3).await;
        f.task.cycle().await;
        assert_eq!(put_order(&seen), ["snapshot", "batch"]);
        let batch = &puts(&seen)[1];
        assert_eq!(batch.0, format!("/put/{EPOCH}/{}-{}", base + 1, base + 3));
        assert_eq!(snapshot_asks(&seen).len(), 1, "taken once");
    }

    #[tokio::test]
    async fn the_snapshot_and_the_cursor_are_one_step_a_failed_put_leaves_neither() {
        let (mut f, seen, snapshots_up) = upgrade_fx(Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 5).await;
        keys_backed_up(&f).await;
        snapshots_up.store(false, std::sync::atomic::Ordering::SeqCst);

        f.task.cycle().await;
        assert!(snapshot_row(&f, "2027-01-15").is_none());
        assert!(cursor(&f).is_none());
        assert_eq!(
            put_order(&seen),
            ["snapshot"],
            "no batch while the snapshot is missing"
        );
        let state = backup_state(&f);
        assert!(state.first_snapshot_pending);
        assert_eq!(
            state.first_snapshot_error.as_deref(),
            Some("CLOUD_UNREACHABLE")
        );
        assert!(state.first_snapshot_error_at.is_some());
        assert!(scratch_files(&f).is_empty());
        assert_eq!(progress(&f).pending_rows, 5, "the rows wait");

        // The cloud is back: the same snapshot goes up, then the batches follow.
        snapshots_up.store(true, std::sync::atomic::Ordering::SeqCst);
        f.task.fails = 0;
        f.task.cycle().await;
        assert!(snapshot_row(&f, "2027-01-15").is_some());
        let state = backup_state(&f);
        assert!(!state.first_snapshot_pending);
        assert_eq!(state.first_snapshot_error, None);
        assert_eq!(snapshot_asks(&seen), ["2027-01-15", "2027-01-15"]);
        assert!(cursor(&f).is_some());
    }

    #[tokio::test]
    async fn the_snapshot_waits_for_the_player_key_backup() {
        let (mut f, seen, _) = upgrade_fx(Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 2).await;

        let next = f.task.cycle().await;
        assert_eq!(next.wait, TIER_RECHECK);
        assert!(
            presigns(&seen).is_empty(),
            "nothing goes up before the keys"
        );
        let state = backup_state(&f);
        assert!(state.first_snapshot_pending);
        assert_eq!(
            state.first_snapshot_error.as_deref(),
            Some("KEY_BACKUP_PENDING")
        );

        keys_backed_up(&f).await;
        f.task.cycle().await;
        assert_eq!(snapshot_asks(&seen), ["2027-01-15"]);
        assert!(cursor(&f).is_some());
    }

    #[tokio::test]
    async fn the_snapshot_is_skipped_with_an_alert_when_the_disk_is_too_full_and_no_batch_goes_up()
    {
        let (mut f, seen, _) = upgrade_fx(Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 2).await;
        keys_backed_up(&f).await;
        f.task.free_space = |_| Some(0);

        f.task.cycle().await;
        assert!(presigns(&seen).is_empty());
        assert_eq!(
            backup_state(&f).first_snapshot_error.as_deref(),
            Some("BACKUP_DISK")
        );
        assert_eq!(
            count(&f, "SELECT COUNT(*) FROM alerts WHERE kind = 'backup_disk'"),
            1
        );
    }

    #[tokio::test]
    async fn a_branch_that_already_uploads_takes_no_upgrade_snapshot() {
        let (mut f, seen, _) = upgrade_fx(Tier::Basic).await;
        paid_since(&f, 0).await;
        add_rows(&f.writer, EPOCH, 3).await;
        f.task.cycle().await;
        assert!(snapshot_asks(&seen).is_empty());
        assert_eq!(put_order(&seen), ["batch"]);
    }

    #[tokio::test]
    async fn a_lapse_stops_uploads_at_once_and_the_next_upgrade_starts_with_a_fresh_snapshot() {
        let (mut f, seen, _) = upgrade_fx(Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 4).await;
        keys_backed_up(&f).await;
        f.task.cycle().await;
        assert!(cursor(&f).is_some());
        assert!(backup_state(&f).base_seq.is_some());
        let before = presigns(&seen).len();

        f.status.send_modify(|s| s.tier = Some(Tier::Free));
        add_rows(&f.writer, EPOCH, 2).await;
        f.task.cycle().await;
        assert_eq!(presigns(&seen).len(), before, "nothing goes up on Free");
        assert!(cursor(&f).is_none());
        let state = backup_state(&f);
        assert_eq!((state.base_epoch, state.base_seq), (None, None));
        assert!(!state.first_snapshot_pending);
        assert!(
            snapshot_row(&f, "2027-01-15").is_some(),
            "what is stored stays recorded"
        );

        f.status.send_modify(|s| s.tier = Some(Tier::Basic));
        f.task.cycle().await;
        assert_eq!(snapshot_asks(&seen), ["2027-01-15", "2027-01-15"]);
        let row = snapshot_row(&f, "2027-01-15").unwrap();
        assert!(
            row.last_seq >= 6,
            "the fresh snapshot holds the rows added on Free"
        );
        assert_eq!(cursor(&f).unwrap().last_seq_uploaded, row.last_seq);
        assert_eq!(
            put_order(&seen),
            ["snapshot", "snapshot"],
            "the rows from Free are in the snapshot, not in batches"
        );
    }

    // ---- Story 9.14: the yearly GST archive ----

    type Flag = Arc<std::sync::atomic::AtomicBool>;

    fn flag(on: bool) -> Flag {
        Arc::new(std::sync::atomic::AtomicBool::new(on))
    }

    fn is_on(f: &Flag) -> bool {
        f.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// As [`cloud_with_snapshots`], and it issues GST archive links; archive PUTs fail while
    /// `gst_up` is false and archive presigns are refused `FENCED` while `fenced` is true.
    fn cloud_with_gst(gst_up: Flag, fenced: Flag) -> impl FnOnce(u16) -> Handler {
        move |port| {
            let mut inner = cloud_with_snapshots(flag(true), flag(true))(port);
            Box::new(move |line, body| {
                if line.starts_with("POST /v1/presign") {
                    let req: serde_json::Value = serde_json::from_slice(body).unwrap();
                    if req["kind"] == "gst_archive" {
                        if is_on(&fenced) {
                            return refused(409, "FENCED");
                        }
                        let url = format!(
                            "http://127.0.0.1:{port}/put/gst-archive/{}.zst",
                            req["fy"].as_str().unwrap()
                        );
                        return (200, serde_json::json!({ "url": url }).to_string());
                    }
                } else if line.starts_with("PUT /put/gst-archive/") && !is_on(&gst_up) {
                    return (503, "{}".into());
                }
                inner(line, body)
            })
        }
    }

    async fn gst_fx(tier: Tier) -> (Fx, Seen, Flag, Flag) {
        let (gst_up, fenced) = (flag(true), flag(false));
        let (port, seen) = serve(cloud_with_gst(Flag::clone(&gst_up), Flag::clone(&fenced))).await;
        let mut f = fx(port, tier).await;
        f.task.rollups = true;
        f.task.daily_snapshot = true;
        f.task.gst_archive = true;
        f.task.free_space = |_| Some(u64::MAX);
        (f, seen, gst_up, fenced)
    }

    fn gst_asks(seen: &Seen) -> Vec<String> {
        presigns(seen)
            .into_iter()
            .filter(|p| p["kind"] == "gst_archive")
            .map(|p| p["fy"].as_str().unwrap().to_owned())
            .collect()
    }

    fn gst_puts(seen: &Seen) -> Vec<(String, Vec<u8>)> {
        puts(seen)
            .into_iter()
            .filter(|(k, _)| k.starts_with("/put/gst-archive/"))
            .collect()
    }

    fn gst_row(f: &Fx, fy: &str) -> Option<arena_store::GstArchiveLogRow> {
        Store::open_reader(&f.path)
            .unwrap()
            .read_with(|c| arena_store::find_gst_archive_log(c, fy))
            .unwrap()
    }

    /// A settled, numbered invoice of `fy`, written directly (the change log is not what is tested).
    async fn invoice_of(f: &Fx, id: &str, fy: &str, total: i64, updated: i64) {
        let (id, fy) = (id.to_owned(), fy.to_owned());
        f.writer
            .submit(move |tx| {
                tx.execute_batch(&format!(
                    "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy,
                                         rev, created_at, updated_at, settled_at, kind, invoice_no,
                                         tax_minor, tax_breakup)
                     VALUES ('{id}', 'settled', {total}, {total}, '2026-06-01', '{fy}', 0, 1,
                             {updated}, 1, 'pos_sale', 'ARN/{fy}/E1/{id}', 0, NULL)"
                ))
                .map_err(arena_store::db_error)
            })
            .await
            .unwrap();
    }

    /// A credit note of `fy` against order `order`.
    async fn credit_note_of(f: &Fx, id: &str, order: &str, fy: &str, updated: i64) {
        let (id, order, fy) = (id.to_owned(), order.to_owned(), fy.to_owned());
        f.writer
            .submit(move |tx| {
                tx.execute_batch(&format!(
                    "INSERT INTO refunds (id, order_id, credit_note_no, reason, amount_minor,
                                          tax_minor, tax_breakup, lines, staff_id, approved_by,
                                          business_day, fy, rev, created_at, updated_at)
                     VALUES ('{id}', '{order}', 'CN/{id}', 'mistake', 50, 0, NULL, '[]', 'S', 'S',
                             '2026-06-02', '{fy}', 0, {updated}, {updated})"
                ))
                .map_err(arena_store::db_error)
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_closed_year_is_archived_once_and_recorded_after_its_put() {
        let (mut f, seen, _, _) = gst_fx(Tier::Basic).await;
        paid_since(&f, 1).await;
        invoice_of(&f, "A", "FY2025-26", 1_000, 5).await;
        invoice_of(&f, "B", "FY2025-26", 2_000, 5).await;
        credit_note_of(&f, "R", "A", "FY2025-26", 6).await;
        // The year in progress is not archived.
        invoice_of(&f, "C", "FY2026-27", 3_000, 5).await;

        f.task.cycle().await;
        assert_eq!(rollup_asks(&seen), ["2027-01-14"]);
        assert_eq!(gst_asks(&seen), ["FY2025-26"]);
        let sent = gst_puts(&seen);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, "/put/gst-archive/FY2025-26.zst");

        let archive = arena_core::gst_archive::decode_archive(&sent[0].1).unwrap();
        assert_eq!(
            (
                archive.fy.as_str(),
                archive.invoices.len(),
                archive.credit_notes.len()
            ),
            ("FY2025-26", 2, 1)
        );
        let row = gst_row(&f, "FY2025-26").expect("recorded after the PUT");
        assert_eq!(row.key, "gst-archive/FY2025-26.zst");
        assert_eq!((row.invoices, row.credit_notes), (2, 1));
        assert_eq!(row.bytes, i64::try_from(sent[0].1.len()).unwrap());
        assert_eq!(row.sha256.len(), 64);
        assert_eq!(row.built_at, T0);
        assert!(gst_row(&f, "FY2026-27").is_none());
        // The record is a system intent, so it is in the audit.
        let recorded = count(
            &f,
            "SELECT COUNT(*) FROM changes
             WHERE intent_kind = 'gst_archive.publish' AND actor_kind = 'system'",
        );
        assert_eq!(recorded, 1);

        // Archived and unchanged: the next cycle asks for nothing.
        f.task.cycle().await;
        assert_eq!(gst_asks(&seen).len(), 1);
    }

    #[tokio::test]
    async fn a_failed_put_records_nothing_and_the_next_cycle_tries_again() {
        let (mut f, seen, gst_up, _) = gst_fx(Tier::Basic).await;
        paid_since(&f, 1).await;
        invoice_of(&f, "A", "FY2025-26", 1_000, 5).await;
        gst_up.store(false, std::sync::atomic::Ordering::SeqCst);

        let next = f.task.cycle().await;
        assert!(next.active && !next.early, "a failure backs off");
        assert!(
            gst_row(&f, "FY2025-26").is_none(),
            "no row for a failed PUT"
        );
        assert_eq!(gst_asks(&seen), ["FY2025-26"]);

        gst_up.store(true, std::sync::atomic::Ordering::SeqCst);
        f.task.fails = 0;
        f.task.cycle().await;
        assert_eq!(gst_asks(&seen), ["FY2025-26", "FY2025-26"]);
        assert!(gst_row(&f, "FY2025-26").is_some());
    }

    #[tokio::test]
    async fn a_late_credit_note_makes_the_next_close_rebuild_and_overwrite_the_same_key() {
        let (mut f, seen, _, _) = gst_fx(Tier::Basic).await;
        paid_since(&f, 1).await;
        invoice_of(&f, "A", "FY2025-26", 1_000, 5).await;
        f.task.cycle().await;
        assert_eq!(gst_puts(&seen).len(), 1);

        // A credit note with the old year's stamp arrives, and the next day closes.
        f.clock.advance(DAY);
        keep_lease(&f).await;
        credit_note_of(&f, "R", "A", "FY2025-26", f.clock.now_ms()).await;
        f.clock.advance(60_000);
        keep_lease(&f).await;
        f.task.cycle().await;
        let sent = gst_puts(&seen);
        assert_eq!(sent.len(), 2, "rebuilt once");
        assert_eq!(sent[0].0, sent[1].0, "under the same key");
        let rebuilt = arena_core::gst_archive::decode_archive(&sent[1].1).unwrap();
        assert_eq!(rebuilt.credit_notes.len(), 1);
        let row = gst_row(&f, "FY2025-26").unwrap();
        assert_eq!((row.invoices, row.credit_notes), (1, 1));

        // Once is enough: the next close finds it current.
        f.clock.advance(DAY);
        keep_lease(&f).await;
        f.task.cycle().await;
        assert_eq!(gst_puts(&seen).len(), 2);
    }

    #[tokio::test]
    async fn a_free_hub_uploads_no_archive_and_a_fenced_epoch_records_nothing() {
        let (mut f, seen, _, _) = gst_fx(Tier::Free).await;
        paid_since(&f, 1).await;
        invoice_of(&f, "A", "FY2025-26", 1_000, 5).await;
        f.task.cycle().await;
        assert!(presigns(&seen).is_empty(), "Free makes no request");
        assert!(gst_row(&f, "FY2025-26").is_none());

        let (mut f, seen, _, fenced) = gst_fx(Tier::Basic).await;
        paid_since(&f, 1).await;
        invoice_of(&f, "A", "FY2025-26", 1_000, 5).await;
        fenced.store(true, std::sync::atomic::Ordering::SeqCst);
        f.task.cycle().await;
        assert_eq!(gst_asks(&seen), ["FY2025-26"]);
        assert!(gst_puts(&seen).is_empty());
        assert!(gst_row(&f, "FY2025-26").is_none());
    }
}
