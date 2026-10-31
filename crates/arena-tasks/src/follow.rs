//! The standby follows the main Hub (Story 9.19, AD-4).
//!
//! A [`Follower`] owns the standby's database and a [`ReplicationLink`] to the main Hub. With an
//! empty log it takes the main Hub's snapshot and loads it; then it opens the stream at its last
//! applied change and applies each batch in one transaction, verbatim (`arena_replay::
//! apply_replicated`). A dropped stream is retried with the jittered 2-30 s backoff and resumes
//! from the log's own tip, so nothing is skipped or applied twice. A position the main Hub does
//! not hold, or a log that stops continuing, empties the replicated state and takes a new
//! snapshot. A standby whose schema is below the main Hub's stops with [`FollowStatus::Behind`]
//! ("Update this PC to follow the main Hub") and does not retry.
//!
//! The link is a trait so the engine is tested without a network; the WebSocket and HTTPS link
//! that pins the main Hub's certificate is wired in with the standby runtime role.

use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_proto::{AppError, ErrorCode, ReplicateFrame, ReplicatedChange, replicate_backoff_ms};
use arena_store::{ChangeOp, ChangeRecord, Store};
use tokio::sync::watch;

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

    /// The next frame, or `None` when the stream closed.
    fn next(&mut self) -> impl Future<Output = Result<Option<ReplicateFrame>, LinkError>> + Send;
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
        (
            Self {
                store: Arc::new(Mutex::new(store)),
                link,
                scratch,
                config,
                status,
            },
            rx,
        )
    }

    /// The standby's database, for the status screen and promotion.
    pub fn store(&self) -> Arc<Mutex<Store>> {
        Arc::clone(&self.store)
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

    /// Follows until `stop` turns true. Returns `Behind` is reported through the status and the
    /// run ends; any other end is a stop request.
    pub async fn run(&mut self, mut stop: watch::Receiver<bool>) {
        let mut attempt = 0u32;
        loop {
            if *stop.borrow() {
                break;
            }
            let ended = tokio::select! {
                _ = stop.changed() => break,
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
        match self.link.open(position.clone(), schema).await {
            Ok(_) => {}
            Err(LinkError::Refused(ErrorCode::SchemaBehind, _)) => return Ended::Behind,
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
                            self.status
                                .send_replace(FollowStatus::Following { applied, live });
                        }
                        Err(e) if e.code == ErrorCode::ReplicateGap => return Ended::Resync,
                        Err(_) => return Ended::Retry,
                    }
                }
                ReplicateFrame::Ping | ReplicateFrame::Ready { .. } => {}
                ReplicateFrame::Refused { code, .. } => {
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
            Err(LinkError::Refused(ErrorCode::SchemaBehind, _)) => return Err(Ended::Behind),
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
            Err(e) if e.code == ErrorCode::SchemaBehind => Err(Ended::Behind),
            Err(_) => Err(Ended::Retry),
        }
    }
}
