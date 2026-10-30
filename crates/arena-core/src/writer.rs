//! The Writer actor: the only owner of the branch's write connection.
//!
//! Each submitted job runs in one `BEGIN IMMEDIATE` transaction on a dedicated thread. Once the
//! writer is closed (demotion or shutdown), every `submit` fails `NOT_PRIMARY`. A job that
//! panics is rolled back and fails `INTERNAL`; the thread keeps running. If the thread exits
//! for any other reason, the writer counts as closed and `submit` fails `INTERNAL`.
//!
//! Branch writes go through [`WriterHandle::submit_intent`], which checks the frame, runs
//! `apply()`, commits and then emits. The only other writes are maintenance: the lease renewal
//! ([`WriterHandle::renew_lease`]), the dedupe prune ([`WriterHandle::prune_applied_intents`]) and
//! the upload cursor ([`WriterHandle::record_upload`]),
//! neither of which writes change rows. The closure API (`submit`) exists for tests only
//! (`cfg(test)` or the `test-jobs` feature).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
#[cfg(any(test, feature = "test-jobs"))]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};

use arena_proto::{AppError, CommandWire, ErrorCode, Intent};
use arena_store::{
    AppliedCommand, LeaseRow, OwnerOverride, Store, UploadCursor, find_applied_command,
    insert_applied_command, insert_owner_override, mark_command_reported, max_applied_command_id,
    prune_applied_intents, prune_uploaded_changes, read_lease, read_upload_cursor,
    unreported_commands, write_lease, write_upload_cursor,
};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use crate::apply::{Applied, ApplyCtx, Outcome, emit, record_rejection, run, validate_frame};
use crate::clock::Clock;
use crate::commands::{command_intent, owner_wins};
use crate::status::{HubStatus, LeaseInfo};

type Job = Box<dyn FnOnce(&mut Store) + Send>;

enum Msg {
    Job(Job),
    Close(oneshot::Sender<()>),
}

const QUEUE_DEPTH: usize = 256;
const EVENTS_DEPTH: usize = 1024;

/// The outcome of one lease renewal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseRenewal {
    /// The row still held our epoch and was renewed.
    Renewed(LeaseRow),
    /// The row is missing or holds any other epoch (higher or not); nothing was written.
    Superseded(Option<LeaseRow>),
}

fn not_primary() -> AppError {
    AppError::new(ErrorCode::NotPrimary, "this hub is not the primary writer")
}

fn writer_gone() -> AppError {
    AppError::new(ErrorCode::Internal, "the writer thread stopped")
}

#[derive(Default)]
struct Flags {
    /// Set by `close()`.
    closed: AtomicBool,
    /// Set when the writer thread exits, however it exits.
    dead: AtomicBool,
    /// How many jobs were queued (tests: a `BAD_FRAME` queues none).
    #[cfg(any(test, feature = "test-jobs"))]
    jobs: AtomicU64,
}

/// Marks the writer dead when the thread ends (normal exit or unwinding).
struct DeadOnDrop(Arc<Flags>);

impl Drop for DeadOnDrop {
    fn drop(&mut self) {
        self.0.dead.store(true, Ordering::SeqCst);
    }
}

/// The `ApplyCtx` for a job, from the status watch as it is when the job runs.
fn ctx_from<'a>(clock: &'a dyn Clock, status: &watch::Receiver<HubStatus>) -> ApplyCtx<'a> {
    let (role, epoch, restore_pending) = {
        let s = status.borrow();
        (
            s.role,
            s.lease.as_ref().map(LeaseInfo::epoch),
            s.restore_pending.is_some(),
        )
    };
    ApplyCtx {
        restore_pending,
        ..ApplyCtx::new(clock, epoch, role)
    }
}

/// Spawns the writer actor.
pub struct Writer;

impl Writer {
    /// Moves `store` (the write connection) onto the writer thread and returns its handle.
    /// Each intent reads its role and epoch from `status` when it runs, and its time from
    /// `clock`.
    pub fn spawn(
        store: Store,
        clock: Arc<dyn Clock>,
        status: watch::Receiver<HubStatus>,
    ) -> Result<WriterHandle, AppError> {
        let (tx, mut rx) = mpsc::channel::<Msg>(QUEUE_DEPTH);
        let flags = Arc::new(Flags::default());
        let actor_flags = Arc::clone(&flags);
        std::thread::Builder::new()
            .name("arena-writer".into())
            .spawn(move || {
                let _dead = DeadOnDrop(Arc::clone(&actor_flags));
                let mut store = Some(store);
                while let Some(msg) = rx.blocking_recv() {
                    match msg {
                        Msg::Job(job) => {
                            // A job queued before close() is dropped unrun: its caller gets
                            // NOT_PRIMARY.
                            if actor_flags.closed.load(Ordering::SeqCst) {
                                continue;
                            }
                            if let Some(s) = store.as_mut() {
                                job(s);
                            }
                        }
                        Msg::Close(ack) => {
                            actor_flags.closed.store(true, Ordering::SeqCst);
                            drop(store.take());
                            rx.close();
                            let _ = ack.send(());
                        }
                    }
                }
            })
            .map_err(|e| AppError::new(ErrorCode::Internal, format!("spawn writer thread: {e}")))?;
        let (events, _) = broadcast::channel(EVENTS_DEPTH);
        Ok(WriterHandle {
            tx,
            flags,
            clock,
            status,
            events,
        })
    }
}

/// A cloneable handle for submitting write jobs.
#[derive(Debug, Clone)]
pub struct LicenceState {
    pub identity: Option<arena_store::HubIdentity>,
    pub jwt: Option<String>,
    pub trusted: Option<arena_store::TrustedClockRow>,
    pub status: Option<String>,
}

#[derive(Clone)]
pub struct WriterHandle {
    tx: mpsc::Sender<Msg>,
    flags: Arc<Flags>,
    clock: Arc<dyn Clock>,
    status: watch::Receiver<HubStatus>,
    events: broadcast::Sender<Applied>,
}

impl std::fmt::Debug for WriterHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriterHandle")
            .field("closed", &self.is_closed())
            .field("alive", &self.is_alive())
            .finish()
    }
}

impl WriterHandle {
    /// Runs `intent` through `apply()` in one `BEGIN IMMEDIATE` transaction, commits it and
    /// then emits the [`Applied`] to every [`subscribe`](Self::subscribe)r. On any error
    /// nothing is written and nothing is emitted, except that a deterministic rejection is
    /// recorded for dedupe in a separate short transaction after the rollback.
    ///
    /// A malformed idempotency key fails `BAD_FRAME` here, before any job is queued. A retried
    /// key returns its stored outcome without writing or emitting anything.
    pub async fn submit_intent(&self, intent: Intent) -> Result<Applied, AppError> {
        validate_frame(&intent)?;
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        let events = self.events.clone();
        let outcome = self
            .run_then(
                move |store| {
                    let ctx = ctx_from(clock.as_ref(), &status);
                    // commit: `write_tx_mut` commits on Ok and rolls back on Err.
                    let result = store.write_tx_mut(|tx| run(tx, &ctx, &intent));
                    if let Err(e) = &result {
                        // Best effort: if recording fails, a retry re-runs the intent, which is
                        // still safe (nothing was applied).
                        if let Err(rec) =
                            store.write_tx(|tx| record_rejection(tx, &ctx, &intent, e))
                        {
                            tracing::warn!(
                                intent_id = %intent.intent_id,
                                code = e.code.as_str(),
                                record_error = rec.code.as_str(),
                                "could not record a rejection for dedupe"
                            );
                        }
                    }
                    result
                },
                // emit: only after a successful commit of a fresh run, outside the panic guard
                // (a committed intent is never reported as failed), on the writer thread so
                // events keep commit order. A dedupe hit committed nothing, so emits nothing.
                move |outcome| {
                    if let Outcome::Fresh(applied) = outcome {
                        emit(&events, applied);
                    }
                },
            )
            .await?;
        match outcome {
            Outcome::Fresh(applied) => Ok(applied),
            Outcome::Replayed(stored) => stored,
        }
    }

    /// Housekeeping: deletes one batch of at most `limit` `applied_intents` dedupe records with
    /// `applied_at < cutoff_ms`, only while this writer is primary and still holds its epoch
    /// (`NOT_PRIMARY` otherwise). A maintenance write outside `apply()`: it writes no change
    /// rows. Returns how many records were deleted (fewer than `limit`: none remain).
    pub async fn prune_applied_intents(
        &self,
        cutoff_ms: i64,
        limit: usize,
    ) -> Result<usize, AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        self.run(move |store| {
            let ctx = ctx_from(clock.as_ref(), &status);
            store.write_tx(|tx| {
                crate::apply::check_epoch(tx, &ctx)?;
                prune_applied_intents(tx, cutoff_ms, limit)
            })
        })
        .await
    }

    /// Housekeeping (Story 9.15): deletes one batch of at most `limit` change rows the cloud
    /// already holds that are older than `cutoff_ms` (money and tax rows: older than
    /// `money_cutoff_ms`), only while this writer is primary and still holds its epoch
    /// (`NOT_PRIMARY` otherwise). A maintenance write outside `apply()`: it writes no change
    /// rows. Returns how many rows were deleted (fewer than `limit`: none remain).
    pub async fn prune_changes(
        &self,
        cutoff_ms: i64,
        money_cutoff_ms: i64,
        limit: usize,
    ) -> Result<usize, AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        self.run(move |store| {
            let ctx = ctx_from(clock.as_ref(), &status);
            store.write_tx(|tx| {
                crate::apply::check_epoch(tx, &ctx)?;
                prune_uploaded_changes(tx, cutoff_ms, money_cutoff_ms, limit)
            })
        })
        .await
    }

    /// Moves the change-log upload cursor to `epoch`/`last_seq` once the cloud has stored the
    /// batch ending there (Story 8.2). A maintenance write outside `apply()`: it writes no change
    /// rows, and only while this writer is primary and still holds its epoch (`NOT_PRIMARY`
    /// otherwise). The cursor never moves backwards: an older position is left as it is.
    pub async fn record_upload(&self, epoch: String, last_seq: i64) -> Result<(), AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        self.run(move |store| {
            let ctx = ctx_from(clock.as_ref(), &status);
            store.write_tx(|tx| {
                crate::apply::check_epoch(tx, &ctx)?;
                let counter = |e: &str| e.split_once(':').and_then(|(c, _)| c.parse::<i64>().ok());
                let Some(new_counter) = counter(&epoch).filter(|_| last_seq >= 0) else {
                    return Err(AppError::new(
                        ErrorCode::Internal,
                        format!("cannot record an upload at {epoch:?} seq {last_seq}"),
                    ));
                };
                if let Some(c) = read_upload_cursor(tx)?
                    && counter(&c.epoch).is_some_and(|old| {
                        (old, c.epoch.as_str(), c.last_seq_uploaded)
                            >= (new_counter, epoch.as_str(), last_seq)
                    })
                {
                    return Ok(());
                }
                write_upload_cursor(
                    tx,
                    &UploadCursor {
                        epoch,
                        last_seq_uploaded: last_seq,
                        uploaded_at: clock.now_ms(),
                    },
                )
            })
        })
        .await
    }

    /// Story 8.8: records that the daily snapshot of `row.date` was stored in the cloud, in
    /// `snapshot_log`. Like [`Self::record_upload`], a maintenance write outside `apply()` that
    /// writes no change rows and only while this writer is primary and holds its epoch
    /// (`NOT_PRIMARY` otherwise). The day's row is replaced when it is stored again.
    pub async fn record_snapshot(&self, row: arena_store::SnapshotLogRow) -> Result<(), AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        self.run(move |store| {
            let ctx = ctx_from(clock.as_ref(), &status);
            store.write_tx(|tx| {
                crate::apply::check_epoch(tx, &ctx)?;
                arena_store::write_snapshot_log(tx, &row)
            })
        })
        .await
    }

    /// Story 9.2: records that a key backup holding key-table `version` was stored in the cloud.
    /// A maintenance write outside `apply()` (no change rows), only while this writer is primary
    /// and holds its epoch (`NOT_PRIMARY` otherwise).
    pub async fn record_key_backup(&self, version: i64) -> Result<(), AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        self.run(move |store| {
            let ctx = ctx_from(clock.as_ref(), &status);
            store.write_tx(|tx| {
                crate::apply::check_epoch(tx, &ctx)?;
                arena_store::write_keys_backed_up(tx, version, clock.now_ms())
            })
        })
        .await
    }

    /// Story 9.2: records why the last key backup attempt failed (a short code, never player
    /// data). Same rules as [`Self::record_key_backup`].
    pub async fn record_key_backup_error(&self, error: String) -> Result<(), AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        self.run(move |store| {
            let ctx = ctx_from(clock.as_ref(), &status);
            store.write_tx(|tx| {
                crate::apply::check_epoch(tx, &ctx)?;
                arena_store::write_keys_error(tx, &error, clock.now_ms())
            })
        })
        .await
    }

    /// Story 9.16: records how the last upload cycle ended (`Some(code)` on a failure, `None`
    /// when it ended well). Same rules as [`Self::record_key_backup`].
    pub async fn record_upload_error(&self, error: Option<String>) -> Result<(), AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        self.run(move |store| {
            let ctx = ctx_from(clock.as_ref(), &status);
            store.write_tx(|tx| {
                crate::apply::check_epoch(tx, &ctx)?;
                arena_store::write_upload_error(tx, error.as_deref(), clock.now_ms())
            })
        })
        .await
    }

    /// Story 9.2: records that the cloud accepted this Hub's escrowed branch key. Same rules as
    /// [`Self::record_key_backup`].
    pub async fn record_kek_escrow(&self) -> Result<(), AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        self.run(move |store| {
            let ctx = ctx_from(clock.as_ref(), &status);
            store.write_tx(|tx| {
                crate::apply::check_epoch(tx, &ctx)?;
                arena_store::write_kek_escrowed(tx, clock.now_ms())
            })
        })
        .await
    }

    /// Story 9.11: marks the upgrade snapshot as pending, with why the last attempt failed
    /// (`error` is a short code). Same rules as [`Self::record_key_backup`].
    pub async fn record_first_snapshot_pending(
        &self,
        error: Option<String>,
    ) -> Result<(), AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        self.run(move |store| {
            let ctx = ctx_from(clock.as_ref(), &status);
            store.write_tx(|tx| {
                crate::apply::check_epoch(tx, &ctx)?;
                arena_store::write_first_snapshot_pending(tx, error.as_deref(), clock.now_ms())
            })
        })
        .await
    }

    /// Story 9.11: records that the upgrade snapshot was stored. In one transaction the snapshot
    /// goes into `snapshot_log`, the upload cursor moves to `epoch`/`last_seq` (the snapshot's
    /// last change row, so batching starts one after it), the base is kept for the heartbeat and
    /// the pending mark is cleared. Same rules as [`Self::record_key_backup`].
    pub async fn record_first_snapshot(
        &self,
        row: arena_store::SnapshotLogRow,
        epoch: String,
        last_seq: i64,
    ) -> Result<(), AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        self.run(move |store| {
            let ctx = ctx_from(clock.as_ref(), &status);
            store.write_tx(|tx| {
                crate::apply::check_epoch(tx, &ctx)?;
                if epoch
                    .split_once(':')
                    .and_then(|(c, _)| c.parse::<i64>().ok())
                    .is_none()
                    || last_seq < 0
                {
                    return Err(AppError::new(
                        ErrorCode::Internal,
                        format!("cannot start uploads at {epoch:?} seq {last_seq}"),
                    ));
                }
                arena_store::write_snapshot_log(tx, &row)?;
                arena_store::complete_first_snapshot(tx, &epoch, last_seq, clock.now_ms())
            })
        })
        .await
    }

    /// Story 9.11: the branch lapsed back to Free, so uploads stop: the cursor, the base and the
    /// pending mark are forgotten and the next upgrade starts with a fresh full snapshot. Answers
    /// whether anything was forgotten. Same rules as [`Self::record_key_backup`].
    pub async fn stop_uploads(&self) -> Result<bool, AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        self.run(move |store| {
            let ctx = ctx_from(clock.as_ref(), &status);
            store.write_tx(|tx| {
                crate::apply::check_epoch(tx, &ctx)?;
                arena_store::stop_uploads(tx)
            })
        })
        .await
    }

    /// Story 9.3: raises the highest epoch counter this Hub has seen (from a heartbeat answer, a
    /// claim reply or the LAN) and answers the value held afterwards. A lower counter changes
    /// nothing. Like [`Self::record_upload`] it is a maintenance write outside `apply()`, but it
    /// needs no epoch check: remembering a counter is harmless for a Hub that has just lost its
    /// epoch, and it is exactly the Hub that learns it has.
    pub async fn note_epoch_seen(&self, counter: i64) -> Result<i64, AppError> {
        self.run(move |store| store.write_tx(|tx| arena_store::raise_max_seen_counter(tx, counter)))
            .await
    }

    /// Story 9.3: the highest epoch counter this Hub has seen (0 before the lease row exists).
    pub async fn max_seen_counter(&self) -> Result<i64, AppError> {
        self.run(move |store| store.write_tx(|tx| arena_store::read_max_seen_counter(tx)))
            .await
    }

    /// Story 8.8: raises the branch-level alert `kind` as the system (`alert.raise`, so it shows
    /// in the Hub header list and writes an audit row). `STALE` when one is already open.
    pub async fn raise_branch_alert(
        &self,
        kind: arena_proto::AlertKind,
        actor_id: &str,
    ) -> Result<Applied, AppError> {
        let now = self.clock.now_ms();
        self.submit_intent(Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: arena_proto::IntentKind::AlertRaise,
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: now,
            base_rev: None,
            actor: arena_proto::Actor {
                kind: arena_proto::ActorKind::System,
                id: actor_id.to_owned(),
            },
            elevated_by: None,
            reason: None,
            payload: arena_proto::IntentPayload::AlertRaise(arena_proto::AlertRaise::new(
                kind, None,
            )),
        })
        .await
    }

    pub async fn record_pairing(
        &self,
        identity: arena_store::HubIdentity,
        licence: String,
        bootstrap: Option<arena_proto::BranchBootstrapCommand>,
    ) -> Result<bool, AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        let events = self.events.clone();
        let paired = self
            .run_then(
                move |store| {
                    let ctx = ctx_from(clock.as_ref(), &status);
                    store.write_tx_mut(|tx| {
                        crate::apply::check_epoch(tx, &ctx)?;
                        if !arena_store::insert_hub_identity(tx, &identity)? {
                            return Ok(None);
                        }
                        arena_store::store_licence(tx, &licence, identity.paired_at)?;
                        let Some(bootstrap) = bootstrap else {
                            return Ok(Some(None));
                        };
                        let intent =
                            bootstrap_intent(bootstrap, &identity.tenant_id, clock.now_ms());
                        validate_frame(&intent)?;
                        match run(tx, &ctx, &intent)? {
                            Outcome::Replayed(Err(e)) => Err(e),
                            out => Ok(Some(Some(out))),
                        }
                    })
                },
                move |out| {
                    if let Some(Some(Outcome::Fresh(applied))) = out {
                        emit(&events, applied);
                    }
                },
            )
            .await?;
        Ok(paired.is_some())
    }

    /// Pairs a Hub that replaces another of a branch with history (Story 9.9): stores the
    /// identity and licence and marks the Hub as waiting to be restored, in one transaction. No
    /// `branch.bootstrap` is applied and no branch row is written. The caller then sets
    /// `HubStatus::restore_pending`, after which every intent is refused `RESTORE_PENDING`.
    /// `false` when the Hub already has an identity.
    pub async fn record_replacement_pairing(
        &self,
        identity: arena_store::HubIdentity,
        licence: String,
        branch_name: &str,
    ) -> Result<bool, AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        let name = branch_name.to_owned();
        self.run(move |store| {
            let ctx = ctx_from(clock.as_ref(), &status);
            store.write_tx_mut(|tx| {
                crate::apply::check_epoch(tx, &ctx)?;
                if !arena_store::insert_hub_identity(tx, &identity)? {
                    return Ok(false);
                }
                arena_store::store_licence(tx, &licence, identity.paired_at)?;
                arena_store::mark_restore_pending(tx, &name)?;
                Ok(true)
            })
        })
        .await
    }

    /// Re-pair after the secrets' key was lost: replaces the hub id, tier and licence of the same
    /// branch in one transaction. No bootstrap is applied and nothing else is touched.
    /// `false` when the Hub has no identity or the code is for another branch.
    pub async fn record_repairing(
        &self,
        identity: arena_store::HubIdentity,
        licence: String,
    ) -> Result<bool, AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        self.run(move |store| {
            let ctx = ctx_from(clock.as_ref(), &status);
            store.write_tx_mut(|tx| {
                crate::apply::check_epoch(tx, &ctx)?;
                if !arena_store::replace_hub_identity(tx, &identity)? {
                    return Ok(false);
                }
                arena_store::store_licence(tx, &licence, identity.paired_at)?;
                // The branch key is minted again after this: the cloud holds the old one.
                arena_store::clear_kek_escrow(tx)?;
                Ok(true)
            })
        })
        .await
    }

    pub async fn record_heartbeat(
        &self,
        licence: Option<String>,
        confirm: Option<(i64, String)>,
        trusted: Option<arena_store::TrustedClockRow>,
    ) -> Result<(), AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        self.run(move |store| {
            let ctx = ctx_from(clock.as_ref(), &status);
            store.write_tx(|tx| {
                crate::apply::check_epoch(tx, &ctx)?;
                if let Some(licence) = &licence {
                    arena_store::store_licence(tx, licence, clock.now_ms())?;
                }
                if let Some(trusted) = &trusted {
                    arena_store::store_trusted_clock(tx, trusted)?;
                }
                if let (Some((counter, id)), Some(row)) = (confirm, read_lease(tx)?)
                    && row.counter == counter
                    && row.hub_instance_id == id
                    && row.provisional
                {
                    write_lease(
                        tx,
                        &LeaseRow {
                            provisional: false,
                            ..row
                        },
                    )?;
                }
                Ok(())
            })
        })
        .await
    }

    pub async fn reject_command(
        &self,
        cmd: CommandWire,
        error: AppError,
    ) -> Result<AppliedCommand, AppError> {
        self.record_command(cmd, Some(error)).await
    }

    /// Records the answer of a command that ran outside `apply()` (`backup.now`, or one refused
    /// before it): applied with no change rows (`applied_seq` null) when `error` is `None`,
    /// otherwise rejected with it. A command already recorded by id or key returns its row.
    pub async fn record_command(
        &self,
        cmd: CommandWire,
        error: Option<AppError>,
    ) -> Result<AppliedCommand, AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        self.run(move |store| {
            let ctx = ctx_from(clock.as_ref(), &status);
            store.write_tx(|tx| {
                crate::apply::check_epoch(tx, &ctx)?;
                if let Some(done) = find_applied_command(tx, cmd.id, &cmd.idempotency_key)? {
                    return Ok(done);
                }
                let row = AppliedCommand {
                    command_id: cmd.id,
                    idempotency_key: cmd.idempotency_key,
                    kind: cmd.kind,
                    applied: error.is_none(),
                    applied_seq: None,
                    error: error
                        .map(|e| {
                            serde_json::to_string(&e).map_err(|e| {
                                AppError::new(ErrorCode::Internal, format!("encode error: {e}"))
                            })
                        })
                        .transpose()?,
                    applied_at: clock.now_ms(),
                    reported_at: None,
                };
                insert_applied_command(tx, &row)?;
                Ok(row)
            })
        })
        .await
    }

    /// The record of a command already answered, by id or key.
    pub async fn recorded_command(
        &self,
        command_id: i64,
        idempotency_key: String,
    ) -> Result<Option<AppliedCommand>, AppError> {
        self.run(move |store| {
            store.write_tx(|tx| find_applied_command(tx, command_id, &idempotency_key))
        })
        .await
    }

    pub async fn apply_command(&self, cmd: CommandWire) -> Result<AppliedCommand, AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        let events = self.events.clone();
        let (row, _) = self
            .run_then(
                move |store| {
                    let ctx = ctx_from(clock.as_ref(), &status);
                    let now = clock.now_ms();
                    let record = |applied: Option<i64>, error: Option<String>| AppliedCommand {
                        command_id: cmd.id,
                        idempotency_key: cmd.idempotency_key.clone(),
                        kind: cmd.kind.clone(),
                        applied: error.is_none(),
                        applied_seq: applied,
                        error,
                        applied_at: now,
                        reported_at: None,
                    };
                    let tried = store.write_tx_mut(|tx| {
                        crate::apply::check_epoch(tx, &ctx)?;
                        if let Some(done) = find_applied_command(tx, cmd.id, &cmd.idempotency_key)?
                        {
                            return Ok((done, None));
                        }
                        if crate::commands::already_revoked(tx, &cmd)? {
                            let row = record(None, None);
                            insert_applied_command(tx, &row)?;
                            return Ok((row, None));
                        }
                        let mut intent = command_intent(&cmd, now)?;
                        let overwrote = owner_wins(tx, &mut intent)?;
                        let (applied, fresh) = match run(tx, &ctx, &intent)? {
                            Outcome::Fresh(a) => (a, true),
                            Outcome::Replayed(stored) => (stored?, false),
                        };
                        if fresh
                            && let Some(o) = overwrote
                            && let Some(c) = applied
                                .changes
                                .iter()
                                .find(|c| c.tbl == o.tbl && c.row_id == o.row_id)
                        {
                            insert_owner_override(
                                tx,
                                &OwnerOverride {
                                    epoch: c.epoch.clone(),
                                    seq: c.seq,
                                    tbl: o.tbl.to_owned(),
                                    row_id: o.row_id,
                                    intent_id: o.intent_id,
                                    actor_kind: o.actor_kind,
                                    actor_id: o.actor_id,
                                    at: o.at,
                                },
                            )?;
                        }
                        let row = record(applied.changes.last().map(|c| c.seq), None);
                        insert_applied_command(tx, &row)?;
                        Ok((row, fresh.then_some(applied)))
                    });
                    match tried {
                        Err(e)
                            if !matches!(e.code, ErrorCode::NotPrimary | ErrorCode::Internal) =>
                        {
                            let error = serde_json::to_string(&e).map_err(|err| {
                                AppError::new(ErrorCode::Internal, format!("encode error: {err}"))
                            })?;
                            let row = record(None, Some(error));
                            store.write_tx(|tx| {
                                crate::apply::check_epoch(tx, &ctx)?;
                                insert_applied_command(tx, &row).map(|_| ())
                            })?;
                            Ok((row, None))
                        }
                        other => other,
                    }
                },
                move |(_, applied)| {
                    if let Some(applied) = applied {
                        emit(&events, applied);
                    }
                },
            )
            .await?;
        Ok(row)
    }

    pub async fn command_cursor(&self) -> Result<i64, AppError> {
        self.run(|store| store.write_tx(|tx| max_applied_command_id(tx)))
            .await
    }

    pub async fn unreported_commands(&self, limit: usize) -> Result<Vec<AppliedCommand>, AppError> {
        self.run(move |store| store.write_tx(|tx| unreported_commands(tx, limit)))
            .await
    }

    pub async fn mark_command_reported(&self, command_id: i64) -> Result<(), AppError> {
        let clock = Arc::clone(&self.clock);
        self.run(move |store| {
            store.write_tx(|tx| mark_command_reported(tx, command_id, clock.now_ms()))
        })
        .await
    }

    pub fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    pub async fn licence_state(&self) -> Result<LicenceState, AppError> {
        self.run(|store| {
            Ok(LicenceState {
                identity: store.hub_identity()?,
                jwt: store.cached_licence()?.map(|l| l.jwt),
                trusted: store.trusted_clock()?,
                status: store
                    .settings()?
                    .into_iter()
                    .find(|s| s.key == arena_proto::KEY_LICENCE_STATUS)
                    .map(|s| s.value),
            })
        })
        .await
    }

    /// The branch's opening hours and time zone, as a paid Hub's heartbeat carries them
    /// (Story 8.35). Writes nothing.
    pub async fn branch_opening(&self) -> Result<Option<arena_proto::BranchOpening>, AppError> {
        let now = self.clock.now_ms();
        self.run(move |store| {
            let (settings, branch) =
                store.read_consistent(|s| Ok((s.settings()?, s.branch_locale()?)))?;
            Ok(crate::session_screen::branch_opening_of(
                branch, &settings, now,
            ))
        })
        .await
    }

    pub async fn snapshot(&self) -> Result<arena_proto::HubSnapshot, AppError> {
        let now = self.clock.now_ms();
        self.run(move |store| crate::snapshot::read_snapshot(store, now))
            .await
    }

    pub async fn apply_licence(
        &self,
        payload: arena_proto::LicenceApply,
    ) -> Result<Option<Applied>, AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        let events = self.events.clone();
        self.run_then(
            move |store| {
                let ctx = ctx_from(clock.as_ref(), &status);
                store.write_tx_mut(|tx| {
                    if arena_store::current_branch_id(tx)?.is_none()
                        || crate::apply::check_epoch(tx, &ctx).is_err()
                        || !crate::apply::licence_pending(tx, &payload)?
                    {
                        return Ok(None);
                    }
                    let intent = Intent {
                        intent_id: ulid::Ulid::generate().to_string(),
                        kind: arena_proto::IntentKind::LicenceApply,
                        idempotency_key: ulid::Ulid::generate().to_string(),
                        issued_at: clock.now_ms(),
                        base_rev: None,
                        actor: arena_proto::Actor {
                            kind: arena_proto::ActorKind::System,
                            id: "hub-licence".into(),
                        },
                        elevated_by: None,
                        reason: None,
                        payload: arena_proto::IntentPayload::LicenceApply(payload),
                    };
                    match run(tx, &ctx, &intent)? {
                        Outcome::Fresh(applied) => Ok(Some(applied)),
                        Outcome::Replayed(_) => Ok(None),
                    }
                })
            },
            move |out| {
                if let Some(applied) = out {
                    emit(&events, applied);
                }
            },
        )
        .await
    }

    /// The ad rows after the cursors, as the totals the next Free heartbeat carries (Story 4.14).
    /// Writes nothing; `NOT_PRIMARY` unless this writer still holds its epoch.
    pub async fn ad_pending(&self) -> Result<crate::AdPending, AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        self.run(move |store| {
            let ctx = ctx_from(clock.as_ref(), &status);
            store.write_tx(|tx| {
                crate::apply::check_epoch(tx, &ctx)?;
                crate::ad_metrics::pending(tx)
            })
        })
        .await
    }

    /// Moves the ad cursors to what the cloud acknowledged (one transaction; a cursor never moves
    /// past a row that exists, and rows it covers are dropped).
    pub async fn ad_ack(
        &self,
        impressions: Option<i64>,
        requests: Option<i64>,
    ) -> Result<(), AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        self.run(move |store| {
            let ctx = ctx_from(clock.as_ref(), &status);
            store.write_tx(|tx| {
                crate::apply::check_epoch(tx, &ctx)?;
                crate::ad_metrics::acknowledge(tx, impressions, requests)
            })
        })
        .await
    }

    /// Brings an upgraded branch up to the current defaults (and the manager verifier's history
    /// up to date with the staff rows): when the branch exists and lacks
    /// default settings, built-in roles or their permission rows, submits `branch.seed_defaults`
    /// (system actor) through `apply()` in the same transaction as the check, then emits. With
    /// nothing missing (or no branch) it writes nothing, submits no intent and answers `None`.
    pub async fn seed_defaults(&self) -> Result<Option<Applied>, AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        let events = self.events.clone();
        self.run_then(
            move |store| {
                let ctx = ctx_from(clock.as_ref(), &status);
                store.write_tx_mut(|tx| {
                    // The manager verifier's history catches up with the staff rows (an
                    // upgraded branch gets its first version here); kiosks hear it on hello.
                    if crate::apply::check_epoch(tx, &ctx).is_ok() {
                        crate::apply::verifier::refresh(tx, clock.now_ms())?;
                    }
                    match crate::apply::seed_defaults::missing(tx)? {
                        Some(m) if !m.is_empty() => {}
                        _ => return Ok(None),
                    }
                    let now = clock.now_ms();
                    let intent = Intent {
                        intent_id: ulid::Ulid::generate().to_string(),
                        kind: arena_proto::IntentKind::BranchSeedDefaults,
                        idempotency_key: ulid::Ulid::generate().to_string(),
                        issued_at: now,
                        base_rev: None,
                        actor: arena_proto::Actor {
                            kind: arena_proto::ActorKind::System,
                            id: "hub-seed-defaults".into(),
                        },
                        elevated_by: None,
                        reason: None,
                        payload: arena_proto::IntentPayload::BranchSeedDefaults(
                            arena_proto::BranchSeedDefaults::default(),
                        ),
                    };
                    match run(tx, &ctx, &intent)? {
                        Outcome::Fresh(applied) => Ok(Some(applied)),
                        Outcome::Replayed(_) => Ok(None),
                    }
                })
            },
            move |out| {
                if let Some(applied) = out {
                    emit(&events, applied);
                }
            },
        )
        .await
    }

    /// Pauses credit for players 15+ days overdue and reopens it when the branch rule is off or
    /// the debt is no longer overdue: checks and, only when something changes, submits
    /// `credit.auto_pause` (system actor) in the same transaction, then emits. `None` when
    /// nothing needed changing.
    pub async fn credit_auto_pause(
        &self,
        player_id: Option<String>,
    ) -> Result<Option<Applied>, AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        let events = self.events.clone();
        self.run_then(
            move |store| {
                let ctx = ctx_from(clock.as_ref(), &status);
                store.write_tx_mut(|tx| {
                    let now = clock.now_ms();
                    let payload = arena_proto::CreditAutoPause { player_id };
                    let due = crate::apply::credit_pause::load(tx, &payload, now)?;
                    if !crate::apply::credit_pause::is_due(&due) {
                        return Ok(None);
                    }
                    let intent = Intent {
                        intent_id: ulid::Ulid::generate().to_string(),
                        kind: arena_proto::IntentKind::CreditAutoPause,
                        idempotency_key: ulid::Ulid::generate().to_string(),
                        issued_at: now,
                        base_rev: None,
                        actor: arena_proto::Actor {
                            kind: arena_proto::ActorKind::System,
                            id: "hub-credit-auto-pause".into(),
                        },
                        elevated_by: None,
                        reason: None,
                        payload: arena_proto::IntentPayload::CreditAutoPause(payload),
                    };
                    match run(tx, &ctx, &intent)? {
                        Outcome::Fresh(applied) => Ok(Some(applied)),
                        Outcome::Replayed(_) => Ok(None),
                    }
                })
            },
            move |out| {
                if let Some(applied) = out {
                    emit(&events, applied);
                }
            },
        )
        .await
    }

    /// Story 5.7: raises the "Expiring soon" alert when a lot is expired or inside the warning
    /// window (once per business day) and clears it when none is: checks and, only when
    /// something changes, submits `stock.expiry_check` (system actor) in the same transaction,
    /// then emits. Reads only the Hub's own clock and business day. `None` when nothing needed
    /// changing.
    pub async fn stock_expiry_check(&self) -> Result<Option<Applied>, AppError> {
        let clock = Arc::clone(&self.clock);
        let status = self.status.clone();
        let events = self.events.clone();
        self.run_then(
            move |store| {
                let ctx = ctx_from(clock.as_ref(), &status);
                store.write_tx_mut(|tx| {
                    let now = clock.now_ms();
                    let due = crate::apply::stock::load_check(tx, now)?;
                    if !crate::apply::stock::check_due(&due) {
                        return Ok(None);
                    }
                    let intent = Intent {
                        intent_id: ulid::Ulid::generate().to_string(),
                        kind: arena_proto::IntentKind::StockExpiryCheck,
                        idempotency_key: ulid::Ulid::generate().to_string(),
                        issued_at: now,
                        base_rev: None,
                        actor: arena_proto::Actor {
                            kind: arena_proto::ActorKind::System,
                            id: "hub-stock-expiry".into(),
                        },
                        elevated_by: None,
                        reason: None,
                        payload: arena_proto::IntentPayload::StockExpiryCheck(
                            arena_proto::StockExpiryCheck {},
                        ),
                    };
                    match run(tx, &ctx, &intent)? {
                        Outcome::Fresh(applied) => Ok(Some(applied)),
                        Outcome::Replayed(_) => Ok(None),
                    }
                })
            },
            move |out| {
                if let Some(applied) = out {
                    emit(&events, applied);
                }
            },
        )
        .await
    }

    /// Story 8.4: records that the branch summary was PUT to the cloud, by submitting
    /// `summary.publish` (system actor) through `apply()`: it writes only the `rollup_log` row
    /// `latest` and appears in the audit. `NOT_PRIMARY` on a follower.
    pub async fn publish_summary(&self, built_at: i64, key: String) -> Result<Applied, AppError> {
        let now = self.clock.now_ms();
        self.submit_intent(Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: arena_proto::IntentKind::SummaryPublish,
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: now,
            base_rev: None,
            actor: arena_proto::Actor {
                kind: arena_proto::ActorKind::System,
                id: "hub-summary".into(),
            },
            elevated_by: None,
            reason: None,
            payload: arena_proto::IntentPayload::SummaryPublish(arena_proto::SummaryPublish {
                schema: arena_proto::SUMMARY_SCHEMA,
                built_at,
                key,
            }),
        })
        .await
    }

    /// Story 8.6: records that the daily rollup of `date` was PUT to the cloud, by submitting
    /// `rollup.publish` (system actor) through `apply()`: it upserts only that day's `rollup_log`
    /// row and appears in the audit. `NOT_PRIMARY` on a follower.
    pub async fn publish_rollup(
        &self,
        date: String,
        schema: u32,
        built_at: i64,
        key: String,
    ) -> Result<Applied, AppError> {
        let now = self.clock.now_ms();
        self.submit_intent(Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: arena_proto::IntentKind::RollupPublish,
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: now,
            base_rev: None,
            actor: arena_proto::Actor {
                kind: arena_proto::ActorKind::System,
                id: "hub-rollup".into(),
            },
            elevated_by: None,
            reason: None,
            payload: arena_proto::IntentPayload::RollupPublish(arena_proto::RollupPublish {
                date,
                schema,
                built_at,
                key,
            }),
        })
        .await
    }

    /// Story 9.14: records that the yearly GST archive of `publish.fy` was PUT to the cloud, by
    /// submitting `gst_archive.publish` (system actor) through `apply()`: it upserts only that
    /// year's `gst_archive_log` row and appears in the audit. `NOT_PRIMARY` on a follower.
    pub async fn publish_gst_archive(
        &self,
        publish: arena_proto::GstArchivePublish,
    ) -> Result<Applied, AppError> {
        let now = self.clock.now_ms();
        self.submit_intent(Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: arena_proto::IntentKind::GstArchivePublish,
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: now,
            base_rev: None,
            actor: arena_proto::Actor {
                kind: arena_proto::ActorKind::System,
                id: "hub-gst-archive".into(),
            },
            elevated_by: None,
            reason: None,
            payload: arena_proto::IntentPayload::GstArchivePublish(publish),
        })
        .await
    }

    /// How many jobs this writer has been handed (for tests: a `BAD_FRAME` queues none).
    #[cfg(any(test, feature = "test-jobs"))]
    pub fn jobs_submitted(&self) -> u64 {
        self.flags.jobs.load(Ordering::SeqCst)
    }

    /// Receives every committed intent, in commit order.
    ///
    /// The broadcast is bounded (1024 events). A subscriber that falls further behind gets
    /// `RecvError::Lagged` and has missed events: it must resync from the `changes` log rather
    /// than assume it saw everything.
    pub fn subscribe(&self) -> broadcast::Receiver<Applied> {
        self.events.subscribe()
    }

    /// Renews the lease held as `held`: inside one transaction, re-reads the row and, only if it
    /// still holds `held`'s epoch, extends it to `now + ttl_ms` (`now` read inside the job).
    /// A `ttl_ms` of 0 releases it: the row expires now, so another Hub may take it at once.
    pub async fn renew_lease(
        &self,
        held: LeaseRow,
        ttl_ms: i64,
        clock: Arc<dyn Clock>,
    ) -> Result<LeaseRenewal, AppError> {
        self.run(move |store| {
            store.write_tx(|tx| {
                let row = match read_lease(tx)? {
                    Some(row)
                        if row.counter == held.counter
                            && row.hub_instance_id == held.hub_instance_id =>
                    {
                        row
                    }
                    other => return Ok(LeaseRenewal::Superseded(other)),
                };
                let now = clock.now_ms();
                let renewed = LeaseRow {
                    renewed_at: now,
                    expires_at: now + ttl_ms,
                    ..row
                };
                write_lease(tx, &renewed)?;
                Ok(LeaseRenewal::Renewed(renewed))
            })
        })
        .await
    }

    /// Marks a queued outbox row `sending` before its bytes go out (Story 7.5), so a restart that
    /// finds it half-sent never sends it a second time. False when the row was not queued. The
    /// outbox is local to the Hub and outside the change log.
    pub async fn mark_outbox_sending(&self, id: String) -> Result<bool, AppError> {
        self.run(move |store| store.write_tx(|tx| arena_store::mark_outbox_sending(tx, &id)))
            .await
    }

    /// Runs `job` in one `BEGIN IMMEDIATE` transaction (committed on `Ok`, rolled back on
    /// `Err` or panic). Test-only: production writes go through `submit_intent`.
    #[cfg(any(test, feature = "test-jobs"))]
    pub async fn submit<R, F>(&self, job: F) -> Result<R, AppError>
    where
        F: FnOnce(&arena_store::Transaction<'_>) -> Result<R, AppError> + Send + 'static,
        R: Send + 'static,
    {
        self.run(move |store| store.write_tx(job)).await
    }

    /// Runs `job` on the writer thread. Fails `NOT_PRIMARY` once the writer is closed,
    /// `INTERNAL` if the job panicked or the writer thread is gone.
    async fn run<R, F>(&self, job: F) -> Result<R, AppError>
    where
        F: FnOnce(&mut Store) -> Result<R, AppError> + Send + 'static,
        R: Send + 'static,
    {
        self.run_then(job, |_| {}).await
    }

    /// Like [`run`](Self::run), then calls `after` on the writer thread with a successful
    /// result, outside the panic guard and before replying.
    async fn run_then<R, F, A>(&self, job: F, after: A) -> Result<R, AppError>
    where
        F: FnOnce(&mut Store) -> Result<R, AppError> + Send + 'static,
        A: FnOnce(&R) + Send + 'static,
        R: Send + 'static,
    {
        if self.closed_by_close() {
            return Err(not_primary());
        }
        if !self.is_alive() {
            return Err(writer_gone());
        }
        let (reply, result) = oneshot::channel();
        let job: Job = Box::new(move |store: &mut Store| {
            // Unwinding drops the transaction, which rolls it back.
            let out = catch_unwind(AssertUnwindSafe(|| job(store)))
                .unwrap_or_else(|_| Err(AppError::new(ErrorCode::Internal, "write job panicked")));
            if let Ok(r) = &out {
                after(r);
            }
            let _ = reply.send(out);
        });
        if self.tx.send(Msg::Job(job)).await.is_err() {
            return Err(self.channel_error());
        }
        #[cfg(any(test, feature = "test-jobs"))]
        self.flags.jobs.fetch_add(1, Ordering::SeqCst);
        match result.await {
            Ok(out) => out,
            Err(_) => Err(self.channel_error()),
        }
    }

    fn channel_error(&self) -> AppError {
        if self.closed_by_close() {
            not_primary()
        } else {
            writer_gone()
        }
    }

    fn closed_by_close(&self) -> bool {
        self.flags.closed.load(Ordering::SeqCst)
    }

    /// Whether the writer thread is still running.
    pub fn is_alive(&self) -> bool {
        !self.flags.dead.load(Ordering::SeqCst)
    }

    /// Whether the writer has been closed (by `close()` or because its thread exited).
    pub fn is_closed(&self) -> bool {
        self.closed_by_close() || !self.is_alive()
    }

    /// Closes the writer: no further job runs, and the write connection (and the process
    /// writer lock) are released before this returns.
    pub async fn close(&self) {
        self.flags.closed.store(true, Ordering::SeqCst);
        let (ack, done) = oneshot::channel();
        if self.tx.send(Msg::Close(ack)).await.is_ok() {
            let _ = done.await;
        }
    }
}

fn bootstrap_intent(c: arena_proto::BranchBootstrapCommand, tenant_id: &str, now: i64) -> Intent {
    let p = c.profile;
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: arena_proto::IntentKind::BranchBootstrap,
        idempotency_key: c.idempotency_key,
        issued_at: now,
        base_rev: None,
        actor: arena_proto::Actor {
            kind: arena_proto::ActorKind::Owner,
            id: tenant_id.to_owned(),
        },
        elevated_by: None,
        reason: None,
        payload: arena_proto::IntentPayload::BranchBootstrap(arena_proto::BranchBootstrap {
            name: p.name,
            branch_code: p.branch_code,
            currency: p.currency,
            tz: p.tz,
            day_start_min: p.day_start_min,
            fy_start_month: if p.country == "IN" { 4 } else { 1 },
            onboarding: Some(arena_proto::BootstrapOnboarding {
                country: p.country,
                gst_registered: p.gst_registered,
                gstin: p.gstin,
                tax_mode: p.tax_mode,
                owner_staff: c.owner_staff,
            }),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_store::{get_meta, set_meta};
    use std::path::PathBuf;
    use std::time::Duration;

    fn open() -> (tempfile::TempDir, PathBuf, Store) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let store = Store::open(&path, 0).unwrap();
        (dir, path, store)
    }

    fn spawn(store: Store) -> WriterHandle {
        let (_tx, rx) = crate::status_channel(HubStatus::opened(1, "H"));
        Writer::spawn(store, Arc::new(crate::ManualClock::new(0)), rx).unwrap()
    }

    #[tokio::test]
    async fn runs_jobs_and_commits() {
        let (_dir, _path, store) = open();
        let w = spawn(store);
        w.submit(|tx| set_meta(tx, "k", "v")).await.unwrap();
        let v = w.submit(|tx| get_meta(tx, "k")).await.unwrap();
        assert_eq!(v.as_deref(), Some("v"));
    }

    #[tokio::test]
    async fn failed_job_rolls_back() {
        let (_dir, _path, store) = open();
        let w = spawn(store);
        let err = w
            .submit(|tx| {
                set_meta(tx, "k", "v")?;
                Err::<(), _>(AppError::new(ErrorCode::Stale, "nope"))
            })
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Stale);
        assert_eq!(w.submit(|tx| get_meta(tx, "k")).await.unwrap(), None);
    }

    #[tokio::test]
    async fn panicking_job_rolls_back_and_writer_survives() {
        let (_dir, _path, store) = open();
        let w = spawn(store);
        let err = w
            .submit(|tx| -> Result<(), AppError> {
                set_meta(tx, "k", "v")?;
                panic!("boom");
            })
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Internal);
        assert!(w.is_alive() && !w.is_closed());
        assert_eq!(w.submit(|tx| get_meta(tx, "k")).await.unwrap(), None);
    }

    #[tokio::test]
    async fn submit_after_close_is_not_primary() {
        let (_dir, _path, store) = open();
        let w = spawn(store);
        let other = w.clone();
        w.close().await;
        assert!(other.is_closed());
        let err = other.submit(|tx| set_meta(tx, "k", "v")).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::NotPrimary);
        // Closing twice is harmless.
        w.close().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn job_queued_before_close_is_not_run() {
        let (_dir, path, store) = open();
        let w = spawn(store);
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let (started_tx, started_rx) = oneshot::channel::<()>();

        let blocker = tokio::spawn({
            let w = w.clone();
            async move {
                w.submit(move |_| {
                    let _ = started_tx.send(());
                    release_rx.recv().unwrap();
                    Ok(())
                })
                .await
            }
        });
        started_rx.await.unwrap();

        let queued = tokio::spawn({
            let w = w.clone();
            async move { w.submit(|tx| set_meta(tx, "queued", "yes")).await }
        });
        // Let the second job reach the queue behind the blocked one.
        tokio::time::sleep(Duration::from_millis(100)).await;

        let closer = tokio::spawn({
            let w = w.clone();
            async move { w.close().await }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        release_tx.send(()).unwrap();

        blocker.await.unwrap().unwrap();
        let err = queued.await.unwrap().unwrap_err();
        assert_eq!(err.code, ErrorCode::NotPrimary);
        closer.await.unwrap();
        let reread = Store::open(&path, 0).unwrap();
        assert_eq!(reread.meta("queued").unwrap(), None);
    }
}
