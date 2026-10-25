//! The Writer actor: the only owner of the branch's write connection.
//!
//! Each submitted job runs in one `BEGIN IMMEDIATE` transaction on a dedicated thread. Once the
//! writer is closed (demotion or shutdown), every `submit` fails `NOT_PRIMARY`. A job that
//! panics is rolled back and fails `INTERNAL`; the thread keeps running. If the thread exits
//! for any other reason, the writer counts as closed and `submit` fails `INTERNAL`.
//!
//! Branch writes go through [`WriterHandle::submit_intent`], which checks the frame, runs
//! `apply()`, commits and then emits. The only other writes are maintenance: the lease renewal
//! ([`WriterHandle::renew_lease`]) and the dedupe prune ([`WriterHandle::prune_applied_intents`]),
//! neither of which writes change rows. The closure API (`submit`) exists for tests only
//! (`cfg(test)` or the `test-jobs` feature).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
#[cfg(any(test, feature = "test-jobs"))]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};

use arena_proto::{AppError, ErrorCode, Intent};
use arena_store::{LeaseRow, Store, prune_applied_intents, read_lease, write_lease};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use crate::apply::{Applied, ApplyCtx, Outcome, emit, record_rejection, run, validate_frame};
use crate::clock::Clock;
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
    let (role, epoch) = {
        let s = status.borrow();
        (s.role, s.lease.as_ref().map(LeaseInfo::epoch))
    };
    ApplyCtx::new(clock, epoch, role)
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
