//! The standby follows the main Hub (Story 9.19, AD-4).
//!
//! A [`Follower`] owns the standby's database and a [`ReplicationLink`] to the main Hub. With an
//! empty log it takes the main Hub's snapshot and loads it; then it opens the stream at its last
//! applied change and applies each batch in one transaction, verbatim (`arena_replay::
//! apply_replicated`). A dropped stream is retried with the jittered 2-30 s backoff and resumes
//! from the log's own tip, so nothing is skipped or applied twice. A position the main Hub does
//! not hold, or a log that stops continuing, empties the replicated state and takes a new
//! snapshot. A standby whose licence lacks `standby` stops following and waits ([`FollowStatus::NeedsPlan`], Story
//! 10.13) until the plan has it again. A standby whose schema is below the main Hub's stops with [`FollowStatus::Behind`]
//! ("Update this PC to follow the main Hub") and does not retry.
//!
//! The link is a trait so the engine is tested without a network; the WebSocket and HTTPS link
//! that pins the main Hub's certificate is wired in with the standby runtime role.

use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_core::{Clock, HubStatus, SystemClock};
use arena_proto::{
    AppError, ErrorCode, ReplicateFrame, ReplicatedChange, StandbyReport, replicate_backoff_ms,
};
use arena_store::{ChangeOp, ChangeRecord, Store};
use tokio::sync::watch;

use crate::standby_progress::{SharedProgress, StandbyProgress};

/// Why a link call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    /// The main Hub refused with this code.
    Refused(ErrorCode, String),
    /// The network failed: connect, TLS, read or write.
    Io(String),
}

/// The position of the standby's log, `(epoch, seq)`.
pub type Position = Option<(String, i64)>;

/// The standby's line to the main Hub.
pub trait ReplicationLink: Send {
    /// Downloads the main Hub's snapshot into `into` and returns the position it states.
    fn snapshot(
        &mut self,
        into: PathBuf,
    ) -> impl Future<Output = Result<Position, LinkError>> + Send;

    /// Opens the stream after `from` (a new connection each call) and returns where the main
    /// Hub's log ends.
    fn open(
        &mut self,
        from: Position,
        schema_version: u32,
    ) -> impl Future<Output = Result<Position, LinkError>> + Send;

    /// The main Hub's schema version as its `ready` frame stated it, for the last `open`
    /// (Story 9.21). The default knows nothing.
    fn main_schema(&self) -> Option<u32> {
        None
    }

    /// The next frame, or `None` when the stream closed.
    fn next(&mut self) -> impl Future<Output = Result<Option<ReplicateFrame>, LinkError>> + Send;

    /// Tells the main Hub how far this standby has got (Story 9.20). Best effort: a failure is
    /// the next frame's problem, so there is nothing to return. The default sends nothing.
    fn report(&mut self, _report: StandbyReport) -> impl Future<Output = ()> + Send {
        async {}
    }
}

/// What the standby is doing, for its status screen (Story 9.20).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FollowStatus {
    Connecting,
    /// Loading the main Hub's snapshot.
    Bootstrapping,
    /// Applying the stream; `live` once it has caught up with the main Hub.
    Following {
        applied: Position,
        live: bool,
    },
    /// The standby's own licence lacks `standby`: not following until the plan has it (Story 10.13).
    NeedsPlan,
    /// Below the main Hub's schema: update this PC.
    Behind,
    Stopped,
}

/// Tunables; the defaults are the product's.
#[derive(Clone)]
pub struct FollowerConfig {
    /// The delay before reconnect attempt `n` (0 = the first retry).
    pub backoff: Arc<dyn Fn(u32) -> Duration + Send + Sync>,
}

impl Default for FollowerConfig {
    fn default() -> Self {
        Self {
            backoff: Arc::new(|attempt| {
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.subsec_nanos());
                let jitter = f64::from(nanos % 1000) / 1000.0;
                Duration::from_millis(replicate_backoff_ms(attempt, jitter))
            }),
        }
    }
}

/// The follower engine.
pub struct Follower<L: ReplicationLink> {
    store: Arc<Mutex<Store>>,
    link: L,
    scratch: PathBuf,
    config: FollowerConfig,
    status: watch::Sender<FollowStatus>,
    progress: SharedProgress,
    clock: Arc<dyn Clock>,
    /// The Hub status, for the `standby` flag of the standby's own licence (Story 10.13).
    plan: Option<watch::Receiver<HubStatus>>,
}

fn plan_is(plan: &mut Option<watch::Receiver<HubStatus>>, needs: bool) -> bool {
    plan.as_mut()
        .is_some_and(|rx| rx.borrow().standby_needs_plan == needs)
}

/// Resolves when the status says `standby_needs_plan == needs`; never with no gate.
async fn wait_for_plan(plan: &mut Option<watch::Receiver<HubStatus>>, needs: bool) {
    if let Some(rx) = plan
        && rx.wait_for(|s| s.standby_needs_plan == needs).await.is_ok()
    {
        return;
    }
    std::future::pending::<()>().await;
}

fn change_from_wire(c: ReplicatedChange) -> Result<ChangeRecord, AppError> {
    let op = ChangeOp::parse(&c.op).ok_or_else(|| {
        AppError::new(
            ErrorCode::ReplicateGap,
            format!("unknown change op {:?}", c.op),
        )
    })?;
    Ok(ChangeRecord {
        epoch: c.epoch,
        seq: c.seq,
        tbl: c.tbl,
        row_id: c.row_id,
        op,
        row: c.row,
        intent_id: c.intent_id,
        intent_kind: c.intent_kind,
        actor_kind: c.actor_kind,
        actor_id: c.actor_id,
        elevated_by_kind: c.elevated_by_kind,
        elevated_by_id: c.elevated_by_id,
        elevated_by_mode: c.elevated_by_mode,
        elevated_by_verifier_version: c.elevated_by_verifier_version,
        at: c.at,
        business_day: c.business_day,
        fy: c.fy,
        reason: c.reason,
    })
}

/// What ended one connection.
enum Ended {
    /// The stream closed or failed: retry after the backoff.
    Retry,
    /// The log no longer continues: take a new snapshot.
    Resync,
    /// Below the main Hub's schema.
    Behind,
}

impl<L: ReplicationLink> Follower<L> {
    /// A follower over `store` (the standby's database) and `link`. `scratch` is where the
    /// snapshot is downloaded before it is loaded.
    pub fn new(
        store: Store,
        link: L,
        scratch: PathBuf,
        config: FollowerConfig,
    ) -> (Self, watch::Receiver<FollowStatus>) {
        let (status, rx) = watch::channel(FollowStatus::Connecting);
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let progress = Arc::new(Mutex::new(StandbyProgress::new(
            clock.now_ms(),
            store.schema_version().unwrap_or(0),
        )));
        (
            Self {
                store: Arc::new(Mutex::new(store)),
                link,
                scratch,
                config,
                status,
                progress,
                clock,
                plan: None,
            },
            rx,
        )
    }

    /// The standby's database, for the status screen and promotion.
    pub fn store(&self) -> Arc<Mutex<Store>> {
        Arc::clone(&self.store)
    }

    /// Uses `clock` for the progress timestamps (tests; the default is the system clock).
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Follows only while the Hub status says the standby's licence has the `standby` flag
    /// (Story 10.13). Without it the follower always follows.
    #[must_use]
    pub fn with_plan_gate(mut self, status: watch::Receiver<HubStatus>) -> Self {
        self.plan = Some(status);
        self
    }

    /// The standby's progress, for the status publisher and the standby screen (Story 9.20).
    pub fn progress(&self) -> SharedProgress {
        Arc::clone(&self.progress)
    }

    fn note(&self, f: impl FnOnce(&mut StandbyProgress, i64)) {
        let now = self.clock.now_ms();
        f(
            &mut self
                .progress
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            now,
        );
    }

    fn current_report(&self) -> StandbyReport {
        self.progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .report(self.clock.now_ms())
    }

    async fn blocking<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Store) -> Result<R, AppError> + Send + 'static,
    ) -> Result<R, AppError> {
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || {
            let mut guard = store
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            f(&mut guard)
        })
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, format!("store task failed: {e}")))?
    }

    /// The main Hub refused this standby as behind: its schema is at least one above ours, which
    /// is all a promote needs to know to refuse (Story 9.21).
    async fn note_main_ahead(&self, own: u32) {
        let _ = self
            .blocking(move |s| s.record_main_schema(own.saturating_add(1), true))
            .await;
    }

    async fn note_main_ahead_now(&self) {
        if let Ok(own) = self.blocking(|s| s.schema_version()).await {
            self.note_main_ahead(own).await;
        }
    }

    /// Follows until `stop` turns true. Returns `Behind` is reported through the status and the
    /// run ends; any other end is a stop request.
    pub async fn run(&mut self, mut stop: watch::Receiver<bool>) {
        let mut attempt = 0u32;
        let mut plan = self.plan.clone();
        loop {
            if *stop.borrow() {
                break;
            }
            if plan_is(&mut plan, true) {
                self.status.send_replace(FollowStatus::NeedsPlan);
                attempt = 0;
                tokio::select! {
                    _ = stop.changed() => break,
                    () = wait_for_plan(&mut plan, false) => {}
                }
                continue;
            }
            let ended = tokio::select! {
                _ = stop.changed() => break,
                () = wait_for_plan(&mut plan, true) => continue,
                ended = self.connect_once(&mut attempt) => ended,
            };
            match ended {
                Ended::Behind => {
                    self.status.send_replace(FollowStatus::Behind);
                    return;
                }
                Ended::Resync => {
                    if let Err(e) = self.blocking(|s| s.clear_replicated_state()).await {
                        tracing::error!(code = e.code.as_str(), "standby: could not reset");
                    }
                    attempt = 0;
                    continue;
                }
                Ended::Retry => {}
            }
            let delay = (self.config.backoff)(attempt);
            attempt = attempt.saturating_add(1);
            self.status.send_replace(FollowStatus::Connecting);
            tokio::select! {
                _ = stop.changed() => break,
                () = tokio::time::sleep(delay) => {}
            }
        }
        self.status.send_replace(FollowStatus::Stopped);
    }

    async fn connect_once(&mut self, attempt: &mut u32) -> Ended {
        let schema = match self.blocking(|s| s.schema_version()).await {
            Ok(v) => v,
            Err(_) => return Ended::Retry,
        };
        let mut position = match self.blocking(|s| s.replication_tip()).await {
            Ok(p) => p,
            Err(_) => return Ended::Retry,
        };
        if position.is_none() {
            self.status.send_replace(FollowStatus::Bootstrapping);
            let loaded = self.bootstrap().await;
            match loaded {
                Ok(p) => position = p,
                Err(ended) => return ended,
            }
        }
        self.note(|p, now| p.start_at(position.clone(), now));
        match self.link.open(position.clone(), schema).await {
            Ok(tip) => {
                self.note(|p, now| p.ready(tip, now));
                if let Some(main) = self.link.main_schema() {
                    let _ = self
                        .blocking(move |s| s.record_main_schema(main, false))
                        .await;
                }
            }
            Err(LinkError::Refused(ErrorCode::SchemaBehind, _)) => {
                self.note_main_ahead(schema).await;
                return Ended::Behind;
            }
            Err(LinkError::Refused(ErrorCode::ReplicateGap, _)) => return Ended::Resync,
            Err(_) => return Ended::Retry,
        }
        self.status.send_replace(FollowStatus::Following {
            applied: position,
            live: false,
        });
        loop {
            let frame = match self.link.next().await {
                Ok(Some(f)) => f,
                Ok(None) | Err(_) => return Ended::Retry,
            };
            match frame {
                ReplicateFrame::Batch {
                    changes,
                    keys,
                    live,
                } => {
                    let rows = changes.len();
                    let mut records = Vec::with_capacity(changes.len());
                    for c in changes {
                        match change_from_wire(c) {
                            Ok(r) => records.push(r),
                            Err(_) => return Ended::Resync,
                        }
                    }
                    let keys: Vec<(String, Option<String>)> = keys
                        .into_iter()
                        .map(|k| (k.player_id, k.dek_wrapped))
                        .collect();
                    let applied = self
                        .blocking(move |s| {
                            s.write_tx_mut(|tx| arena_replay::apply_replicated(records, &keys, tx))
                        })
                        .await;
                    match applied {
                        Ok(applied) => {
                            *attempt = 0;
                            self.note(|p, now| p.batch(applied.clone(), rows, live, now));
                            self.status
                                .send_replace(FollowStatus::Following { applied, live });
                            let report = self.current_report();
                            self.link.report(report).await;
                        }
                        Err(e) if e.code == ErrorCode::ReplicateGap => return Ended::Resync,
                        Err(_) => return Ended::Retry,
                    }
                }
                ReplicateFrame::Ping => {
                    self.note(|p, now| p.heard(now));
                    let report = self.current_report();
                    self.link.report(report).await;
                }
                ReplicateFrame::Ready { .. } | ReplicateFrame::Report { .. } => {}
                ReplicateFrame::Refused { code, .. } => {
                    if code == ErrorCode::SchemaBehind {
                        self.note_main_ahead(schema).await;
                    }
                    return match code {
                        ErrorCode::SchemaBehind => Ended::Behind,
                        ErrorCode::ReplicateGap => Ended::Resync,
                        _ => Ended::Retry,
                    };
                }
                ReplicateFrame::Open { .. } => return Ended::Retry,
            }
        }
    }

    /// Downloads and loads the snapshot. `Err` carries what to do next.
    async fn bootstrap(&mut self) -> Result<Position, Ended> {
        let scratch = self.scratch.clone();
        let stated = match self.link.snapshot(scratch.clone()).await {
            Ok(p) => p,
            Err(LinkError::Refused(ErrorCode::SchemaBehind, _)) => {
                self.note_main_ahead_now().await;
                return Err(Ended::Behind);
            }
            Err(_) => return Err(Ended::Retry),
        };
        let file = scratch.clone();
        let loaded = self.blocking(move |s| s.load_standby_snapshot(&file)).await;
        let _ = tokio::fs::remove_file(&scratch).await;
        match loaded {
            Ok(tip) => {
                debug_assert_eq!(tip, stated);
                Ok(tip)
            }
            Err(e) if e.code == ErrorCode::SchemaBehind => {
                self.note_main_ahead_now().await;
                Err(Ended::Behind)
            }
            Err(_) => Err(Ended::Retry),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_core::status_channel;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A link that never reaches the main Hub and counts how often the follower tried.
    struct Dead(Arc<AtomicUsize>);

    impl ReplicationLink for Dead {
        async fn snapshot(&mut self, _into: PathBuf) -> Result<Position, LinkError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(LinkError::Io("down".into()))
        }

        async fn open(&mut self, _from: Position, _schema: u32) -> Result<Position, LinkError> {
            Err(LinkError::Io("down".into()))
        }

        async fn next(&mut self) -> Result<Option<ReplicateFrame>, LinkError> {
            Ok(None)
        }
    }

    fn quick() -> FollowerConfig {
        FollowerConfig {
            backoff: Arc::new(|_| Duration::from_millis(5)),
        }
    }

    #[tokio::test]
    async fn a_standby_without_the_standby_flag_waits_and_follows_again_when_the_plan_returns() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("standby.db"), 1).unwrap();
        let tries = Arc::new(AtomicUsize::new(0));
        let (hub, hub_rx) = status_channel(HubStatus::opened(90, "01JSELF"));
        hub.send_modify(|s| s.standby_needs_plan = true);
        let (follower, mut status) = Follower::new(
            store,
            Dead(Arc::clone(&tries)),
            dir.path().join("scratch"),
            quick(),
        );
        let (stop, stopped) = watch::channel(false);
        let run = tokio::spawn(async move {
            let mut follower = follower.with_plan_gate(hub_rx);
            follower.run(stopped).await;
        });

        status
            .wait_for(|s| *s == FollowStatus::NeedsPlan)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(tries.load(Ordering::SeqCst), 0, "it never reached out");

        hub.send_modify(|s| s.standby_needs_plan = false);
        for _ in 0..200 {
            if tries.load(Ordering::SeqCst) > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(tries.load(Ordering::SeqCst) > 0, "it follows again");

        // And a drop in the middle of following stops it again.
        hub.send_modify(|s| s.standby_needs_plan = true);
        status
            .wait_for(|s| *s == FollowStatus::NeedsPlan)
            .await
            .unwrap();
        stop.send_replace(true);
        run.await.unwrap();
    }
}
