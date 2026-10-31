//! Promoting the standby to main (Story 9.21, AD-11, AD-24).
//!
//! A [`Promoter`] runs the steps of `lease.promote` on the standby Hub, in this order, so that
//! nothing is changed before it is known to be allowed:
//!
//! 1. this build's schema must not be below the main Hub's, else `SCHEMA_BEHIND`;
//! 2. the main Hub's lease must have run out (no frame for 15 s), else `LEASE_FRESH`;
//! 3. the follower stops, so no late row of the old epoch lands after the fork point;
//! 4. the new epoch is claimed from the cloud with `fork = {old epoch, last replicated seq}`,
//!    waiting 3 s; no answer makes the epoch provisional (the first heartbeat settles it), a
//!    `FENCED` answer names the fence and is tried once more;
//! 5. the database part, in one transaction ([`Store::promote_to_main`]): every erasure applied
//!    again, the lease taken, the fork recorded, the kitchen displays marked to be paired again.
//!
//! The caller then opens the writer on the promoted database, publishes the status
//! ([`Promotion::publish`]; the lease change is what `epoch.changed` announces), starts the lease
//! and heartbeat tasks and records `lease.promote` with [`Promotion::record`]. Promotion is
//! manual only; nothing here runs on its own.
//!
//! Force promote (Story 9.22): [`Promoter::promote_forced`] runs the same steps without the
//! freshness check, and with a provisional epoch when the cloud refuses the claim. A refusal that
//! force could get past (`LEASE_FRESH`, the cloud's no) carries a [`ForceOffer`] with the lag;
//! `SCHEMA_BEHIND` and `FENCED` never do. The reasons force went past are on the [`Promotion`].

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_core::{Clock, HubStatus, LeaseInfo, Role};
use arena_proto::{
    AppError, ErrorCode, FORCE_REASON_PLAIN, FenceClaimReason, FenceClaimRequest, FenceFork,
    ForceOffer, LEASE_FRESH_MESSAGE, LeasePromote, PROMOTE_CLAIM_WAIT_MS, STANDBY_NEEDS_PLAN,
    main_lease_fresh,
};
use arena_store::{PromoteInput, PromoteOutcome, Store};
use tokio::sync::watch;

use crate::cloud::{CloudClient, CloudFailure};
use crate::fence::Claim;
use crate::lease::LEASE_TTL_MS;
use crate::standby_progress::SharedProgress;

/// How a standby asks the cloud for its new epoch.
pub trait EpochClaimer: Send + Sync {
    fn claim(
        &self,
        req: &FenceClaimRequest,
    ) -> impl Future<Output = Result<Claim, CloudFailure>> + Send;
}

/// The real claimer: `POST /v1/fence/claim` with the standby's own Hub token.
pub struct CloudClaimer {
    cloud: CloudClient,
    token: String,
}

impl CloudClaimer {
    pub fn new(cloud: CloudClient, token: String) -> Self {
        Self { cloud, token }
    }
}

impl EpochClaimer for CloudClaimer {
    async fn claim(&self, req: &FenceClaimRequest) -> Result<Claim, CloudFailure> {
        match self.cloud.fence_claim(&self.token, req).await {
            Ok(won) => Ok(Claim::Won { epoch: won.epoch }),
            Err(CloudFailure::Refused { error, .. }) if error.code == ErrorCode::Fenced => {
                Ok(Claim::Fenced {
                    current: arena_proto::fenced_epoch(&error.message).map(str::to_owned),
                })
            }
            Err(other) => Err(other),
        }
    }
}

/// A promote that did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromoteError {
    pub error: AppError,
    /// The follower was stopped before the failure; the caller starts it again.
    pub follower_stopped: bool,
    /// A forced promote could get past this refusal (Story 9.22): the warning and the lag.
    pub force_offer: Option<ForceOffer>,
}

impl PromoteError {
    fn before_stop(error: AppError) -> Self {
        Self {
            error,
            follower_stopped: false,
            force_offer: None,
        }
    }

    fn after_stop(error: AppError) -> Self {
        Self {
            error,
            follower_stopped: true,
            force_offer: None,
        }
    }

    fn offering(mut self, lag_rows: i64) -> Self {
        self.force_offer = Some(ForceOffer::new(lag_rows));
        self
    }

    /// What the route sends back: the error and the offer.
    pub fn failure(&self) -> arena_proto::PromoteFailure {
        arena_proto::PromoteFailure {
            error: self.error.clone(),
            force_offer: self.force_offer.clone(),
        }
    }
}

/// A finished promote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Promotion {
    pub outcome: PromoteOutcome,
    /// Change rows of the main Hub the standby had not copied.
    pub lag_rows: i64,
    /// When the database part finished (Hub clock, ms).
    pub promoted_at: i64,
    /// Set by a forced promote: what force went past (`LEASE_FRESH`, the cloud's refusal, or
    /// `FORCED` when nothing stood in the way).
    pub force_reason: Option<String>,
}

impl Promotion {
    /// The `lease.promote` facts for the audit record; the Hub fills in the manager's name when it
    /// applies the intent.
    pub fn record(&self) -> LeasePromote {
        LeasePromote {
            old_epoch: self.outcome.old_epoch.clone(),
            fork_seq: self.outcome.fork_seq,
            new_epoch: self.outcome.new_epoch.clone(),
            provisional: self.outcome.provisional,
            lag_rows: self.lag_rows,
            force: self.force_reason.is_some(),
            force_reason: self.force_reason.clone(),
            erasures_reapplied: self.outcome.erasures_reapplied,
            kitchen_displays_to_repair: self.outcome.kitchen_displays_to_repair,
            promoted_at: self.promoted_at,
            by: None,
        }
    }

    /// Makes the Hub status say this Hub is the primary writer of the new epoch and no longer a
    /// follower. The lease change is what `epoch.changed` announces on `GET /ui/events`.
    pub fn publish(&self, instance_id: &str, status: &watch::Sender<HubStatus>) {
        let lease = LeaseInfo {
            counter: self.outcome.counter,
            hub_instance_id: instance_id.to_owned(),
            provisional: self.outcome.provisional,
            expires_at: self.promoted_at + LEASE_TTL_MS,
        };
        status.send_modify(|s| {
            s.role = Role::Primary;
            s.lease = Some(lease);
            s.following = None;
        });
    }
}

/// Runs a promote on the standby.
pub struct Promoter<C: EpochClaimer> {
    store: Arc<Mutex<Store>>,
    progress: SharedProgress,
    clock: Arc<dyn Clock>,
    claimer: C,
    stop_follower: watch::Sender<bool>,
    claim_wait: Duration,
    /// The Hub status, for the `standby` flag of the standby's own licence (Story 10.13).
    plan: Option<watch::Receiver<HubStatus>>,
}

impl<C: EpochClaimer> Promoter<C> {
    /// `store` and `progress` are the follower's own (`Follower::store`, `Follower::progress`),
    /// and `stop_follower` is the sender of the stop channel its `run` listens on.
    pub fn new(
        store: Arc<Mutex<Store>>,
        progress: SharedProgress,
        clock: Arc<dyn Clock>,
        claimer: C,
        stop_follower: watch::Sender<bool>,
    ) -> Self {
        Self {
            store,
            progress,
            clock,
            claimer,
            stop_follower,
            claim_wait: Duration::from_millis(PROMOTE_CLAIM_WAIT_MS),
            plan: None,
        }
    }

    /// Overrides the 3 s wait for the cloud (tests).
    pub fn with_claim_wait(mut self, wait: Duration) -> Self {
        self.claim_wait = wait;
        self
    }

    /// Refuses a promote, forced or not, while the standby's own licence lacks `standby`
    /// (Story 10.13). Without a gate nothing is refused.
    #[must_use]
    pub fn with_plan_gate(mut self, status: watch::Receiver<HubStatus>) -> Self {
        self.plan = Some(status);
        self
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

    /// The claim: `Some(epoch)` when the cloud issued one, `None` when it did not answer in time
    /// (a provisional epoch), an error when it answered no.
    async fn claim(&self, mut req: FenceClaimRequest) -> Result<Option<String>, AppError> {
        // One wait for both tries: the cloud has 3 s in all. Once more with the fence the cloud
        // named; a second `FENCED` means a newer Hub won.
        let deadline = tokio::time::Instant::now() + self.claim_wait;
        for attempt in 0..2 {
            let answer = tokio::time::timeout_at(deadline, self.claimer.claim(&req)).await;
            match answer {
                Err(_) | Ok(Err(CloudFailure::Unreachable(_))) => return Ok(None),
                Ok(Err(CloudFailure::Refused { status, .. })) if status >= 500 => {
                    return Ok(None);
                }
                Ok(Err(CloudFailure::Refused { error, .. })) => return Err(error),
                Ok(Ok(Claim::Won { epoch })) => return Ok(Some(epoch)),
                Ok(Ok(Claim::Fenced { current })) => match current {
                    Some(fence) if attempt == 0 && fence != req.expected => req.expected = fence,
                    _ => {
                        return Err(AppError::new(
                            ErrorCode::Fenced,
                            "A newer Hub has taken over this branch.",
                        ));
                    }
                },
            }
        }
        Err(AppError::new(
            ErrorCode::Fenced,
            "A newer Hub has taken over this branch.",
        ))
    }

    /// Promotes this standby to main. See the module notes for the order.
    pub async fn promote(&self) -> Result<Promotion, PromoteError> {
        self.run(false).await
    }

    /// Promotes although the main Hub's lease is fresh or the cloud refuses the claim (Story 9.22).
    /// The caller has the manager's PIN and the data-loss acknowledgement. A standby behind the
    /// main Hub's schema, or one a newer Hub has fenced out, is still refused.
    pub async fn promote_forced(&self) -> Result<Promotion, PromoteError> {
        self.run(true).await
    }

    async fn run(&self, force: bool) -> Result<Promotion, PromoteError> {
        if self
            .plan
            .as_ref()
            .is_some_and(|rx| rx.borrow().standby_needs_plan)
        {
            return Err(PromoteError::before_stop(AppError::new(
                ErrorCode::FeatureNotEntitled,
                STANDBY_NEEDS_PLAN,
            )));
        }
        let now = self.clock.now_ms();
        let (silent, report) = {
            let progress = self
                .progress
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (progress.silent_ms(now), progress.report(now))
        };
        // The schema check comes first: force is never offered to a standby that is behind.
        let basis = self
            .blocking(|s| s.promote_basis())
            .await
            .map_err(PromoteError::before_stop)?;
        if basis.main_schema.is_some_and(|m| basis.schema_version < m) {
            return Err(PromoteError::before_stop(AppError::new(
                ErrorCode::SchemaBehind,
                "Update this PC before it can take over from the main hub.",
            )));
        }
        // What force went past, for the record and the owner's alert.
        let mut went_past: Vec<&'static str> = Vec::new();
        if main_lease_fresh(Some(silent)) {
            if !force {
                return Err(PromoteError::before_stop(AppError::new(
                    ErrorCode::LeaseFresh,
                    LEASE_FRESH_MESSAGE,
                ))
                .offering(report.lag_rows));
            }
            went_past.push("LEASE_FRESH");
        }

        // From here the log of the old epoch must not move. The basis is read again once the stop
        // is sent (it waits for a batch the follower is applying), so the fork the cloud is given
        // is the one the database records.
        self.stop_follower.send_replace(true);
        let basis = self
            .blocking(|s| s.promote_basis())
            .await
            .map_err(PromoteError::after_stop)?;
        let claimed = if basis.old_epoch.is_empty() {
            // Nothing was ever replicated, so there is no fence to move from.
            None
        } else {
            let claim = self
                .claim(FenceClaimRequest {
                    expected: basis.old_epoch.clone(),
                    instance_id: basis.instance_id.clone(),
                    reason: FenceClaimReason::Promote,
                    fork: FenceFork {
                        parent_epoch: basis.old_epoch.clone(),
                        parent_seq: basis.fork_seq,
                    },
                })
                .await;
            match claim {
                Ok(claimed) => claimed,
                // The cloud said no for a reason other than a newer Hub: force takes the lease
                // on a provisional epoch and the first heartbeat settles it.
                Err(e) if force && e.code != ErrorCode::Fenced => {
                    tracing::warn!(
                        code = e.code.as_str(),
                        "forced promote past a refused claim"
                    );
                    went_past.push(e.code.as_str());
                    None
                }
                Err(e) if e.code != ErrorCode::Fenced => {
                    return Err(PromoteError::after_stop(e).offering(report.lag_rows));
                }
                Err(e) => return Err(PromoteError::after_stop(e)),
            }
        };

        let promoted_at = self.clock.now_ms();
        let outcome = self
            .blocking(move |s| {
                s.promote_to_main(&PromoteInput {
                    now_ms: promoted_at,
                    lease_ttl_ms: LEASE_TTL_MS,
                    claimed_epoch: claimed.as_deref(),
                })
            })
            .await
            .map_err(PromoteError::after_stop)?;
        let force_reason = force.then(|| {
            if went_past.is_empty() {
                FORCE_REASON_PLAIN.to_owned()
            } else {
                went_past.join(",")
            }
        });
        Ok(Promotion {
            outcome,
            lag_rows: report.lag_rows,
            promoted_at,
            force_reason,
        })
    }
}
