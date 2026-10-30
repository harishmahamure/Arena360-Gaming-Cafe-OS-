//! The notification drain (Story 8.29): sends what `notify_outbox` holds once the Undo window has
//! passed, through the cloud, exactly once per row.
//!
//! Every 10 s a paid, primary, paired Hub asks the snapshot reader for its plan: each due row is
//! sent, left waiting (a receipt without its invoice number), skipped for good (the sale was
//! undone, the player withdrew consent), or expired (7 days old). The task applies the result as
//! a system intent: `notify.sent` (with the channel) or `notify.failed` (with the code) after a
//! post, `notify.skip`, `notify.expire`. `notify.sent` registers "notification sent" as an
//! irreversible effect, so the sale can no longer be undone.
//!
//! Rows go out in `not_before` order. While the WAN is down a pass posts nothing and the rows stay
//! `queued`. A post is keyed by the row's ULID, so a lost reply is answered by the cloud's send
//! ledger with the original result and no second message goes out; a row the cloud answers
//! `queued` (an outcome in doubt) is asked about again a minute later.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use arena_core::notify_drain::{DrainAction, DrainItem};
use arena_core::players::Kek;
use arena_core::{Clock, HubStatus, PairGate, PairingState, Role, SnapshotReader, WriterHandle};
use arena_proto::{
    Actor, ActorKind, ErrorCode, Intent, IntentKind, IntentPayload, NOTIFY_DRAIN_INTERVAL_S,
    NotifyExpired, NotifyFailed, NotifyRequest, NotifyResponse, NotifySent, NotifySkipped,
    NotifyState, Tier,
};
use tokio::sync::watch;

use crate::cloud::{CloudClient, CloudFailure};
use crate::heartbeat::REVOKED_BACKOFF;
use crate::keystore::Keystore;

pub const NOTIFY_TICK: Duration = Duration::from_secs(NOTIFY_DRAIN_INTERVAL_S);
/// How long a row the cloud answered `queued` (outcome in doubt) is left before it is asked again.
pub const IN_DOUBT_RETRY_MS: i64 = 60_000;
/// How long the drain stays quiet after the cloud refuses it for a reason a retry will not mend.
const REFUSED_BACKOFF: Duration = Duration::from_secs(60);

/// What the Hub does with the cloud's answer to one post.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Reply {
    Sent(NotifyResponse),
    Failed(String),
    /// The cloud has not settled the row (`queued`): ask again later.
    InDoubt,
    /// The cloud could not be asked right now: stop this pass, rows stay queued.
    Stop(Duration),
}

fn reply(result: Result<NotifyResponse, CloudFailure>) -> Reply {
    match result {
        Ok(r) => match r.state {
            NotifyState::Sent => Reply::Sent(r),
            NotifyState::Failed => Reply::Failed(
                r.error_code
                    .clone()
                    .filter(|c| !c.trim().is_empty())
                    .unwrap_or_else(|| "send_failed".to_owned()),
            ),
            NotifyState::Queued => Reply::InDoubt,
        },
        Err(CloudFailure::Unreachable(_)) => Reply::Stop(NOTIFY_TICK),
        Err(CloudFailure::Refused {
            status,
            error,
            retry_after_s,
        }) => match error.code {
            // The request itself is wrong: no retry would change that, the row failed.
            ErrorCode::ValidationFailed | ErrorCode::IdempotencyConflict => {
                Reply::Failed(error.code.as_str().to_owned())
            }
            ErrorCode::TokenRevoked => Reply::Stop(REVOKED_BACKOFF),
            _ if status == 429 || status >= 500 => Reply::Stop(
                retry_after_s
                    .map_or(REFUSED_BACKOFF, |s| Duration::from_secs(u64::from(s)))
                    .max(NOTIFY_TICK),
            ),
            _ => Reply::Stop(REFUSED_BACKOFF),
        },
    }
}

fn intent(row_id: &str, kind: IntentKind, payload: IntentPayload, now: i64) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind,
        // The row id (a ULID) makes a retried result the same intent.
        idempotency_key: row_id.to_owned(),
        issued_at: now,
        base_rev: None,
        actor: Actor {
            kind: ActorKind::System,
            id: "notify".into(),
        },
        elevated_by: None,
        reason: None,
        payload,
    }
}

pub struct NotifyDrainTask {
    cloud: CloudClient,
    keystore: Arc<dyn Keystore>,
    writer: WriterHandle,
    snapshots: Arc<SnapshotReader>,
    clock: Arc<dyn Clock>,
    gate: PairGate,
    status: watch::Sender<HubStatus>,
    kek: Option<Arc<dyn Kek>>,
    tick: Duration,
    /// Rows the cloud answered `queued`, and when to ask about them again.
    in_doubt: HashMap<String, i64>,
}

impl NotifyDrainTask {
    pub fn new(
        cloud: CloudClient,
        keystore: Arc<dyn Keystore>,
        writer: WriterHandle,
        snapshots: Arc<SnapshotReader>,
        clock: Arc<dyn Clock>,
        gate: PairGate,
        status: watch::Sender<HubStatus>,
    ) -> Self {
        Self {
            cloud,
            keystore,
            writer,
            snapshots,
            clock,
            gate,
            status,
            kek: None,
            tick: NOTIFY_TICK,
            in_doubt: HashMap::new(),
        }
    }

    /// The key the players' phone numbers are opened with. Without one nothing is sent.
    pub fn with_player_kek(mut self, kek: Arc<dyn Kek>) -> Self {
        self.kek = Some(kek);
        self
    }

    /// Tests: how often the drain looks for due rows.
    pub fn with_tick(mut self, tick: Duration) -> Self {
        self.tick = tick;
        self
    }

    async fn token(&self) -> Option<String> {
        let keystore = Arc::clone(&self.keystore);
        match tokio::task::spawn_blocking(move || keystore.get()).await {
            Ok(Ok(Some(t))) => Some(String::from_utf8_lossy(&t).into_owned()),
            other => {
                tracing::warn!(ok = other.is_ok(), "notify: no hub token");
                None
            }
        }
    }

    async fn plan(&self) -> Option<Vec<DrainItem>> {
        let reader = Arc::clone(&self.snapshots);
        let kek = Arc::clone(self.kek.as_ref()?);
        let now = self.clock.now_ms();
        match tokio::task::spawn_blocking(move || reader.notify_plan(&kek.kek()?, now)).await {
            Ok(Ok(items)) => Some(items),
            Ok(Err(e)) => {
                tracing::warn!(code = e.code.as_str(), "notify: the due rows were not read");
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, "notify: the due rows read panicked");
                None
            }
        }
    }

    async fn apply(&self, id: &str, kind: IntentKind, payload: IntentPayload) {
        let now = self.clock.now_ms();
        match self
            .writer
            .submit_intent(intent(id, kind, payload, now))
            .await
        {
            Ok(_) => {}
            // The row was settled meanwhile (an Undo skipped it): nothing to record.
            Err(e) if e.code == ErrorCode::Stale || e.code == ErrorCode::NotFound => {}
            Err(e) => {
                tracing::warn!(
                    code = e.code.as_str(),
                    outbox_id = id,
                    "notify: result not recorded"
                );
            }
        }
    }

    async fn post(&self, token: &str, req: &NotifyRequest) -> Reply {
        reply(self.cloud.notify(token, req).await)
    }

    /// One pass over the due rows. Returns how long to stay quiet when the cloud asked for it.
    pub async fn drain(&mut self) -> Option<Duration> {
        let (role, pairing, tier) = {
            let s = self.status.borrow();
            (s.role, s.pairing, s.tier)
        };
        if role != Role::Primary || tier.is_none_or(|t| t == Tier::Free) {
            return None;
        }
        if pairing == PairingState::Revoked {
            return Some(REVOKED_BACKOFF);
        }
        let items = self.plan().await?;
        let mut token: Option<String> = None;
        let mut wan_down = false;
        for item in items {
            let id = item.id;
            match item.action {
                DrainAction::Wait => {}
                DrainAction::Skip(reason) => {
                    self.apply(
                        &id,
                        IntentKind::NotifySkip,
                        IntentPayload::NotifySkip(NotifySkipped {
                            outbox_id: id.clone(),
                            reason,
                        }),
                    )
                    .await;
                }
                DrainAction::Expire => {
                    self.apply(
                        &id,
                        IntentKind::NotifyExpire,
                        IntentPayload::NotifyExpire(NotifyExpired {
                            outbox_id: id.clone(),
                        }),
                    )
                    .await;
                }
                DrainAction::Send(req) => {
                    if wan_down
                        || self
                            .in_doubt
                            .get(&id)
                            .is_some_and(|at| *at > self.clock.now_ms())
                    {
                        continue;
                    }
                    if token.is_none() {
                        token = Some(self.token().await?);
                    }
                    let token = token.as_deref()?;
                    match self.post(token, &req).await {
                        Reply::Sent(r) => {
                            self.in_doubt.remove(&id);
                            self.apply(
                                &id,
                                IntentKind::NotifySent,
                                IntentPayload::NotifySent(NotifySent {
                                    outbox_id: id.clone(),
                                    channel: r.channel,
                                }),
                            )
                            .await;
                        }
                        Reply::Failed(code) => {
                            self.in_doubt.remove(&id);
                            self.apply(
                                &id,
                                IntentKind::NotifyFailed,
                                IntentPayload::NotifyFailed(NotifyFailed {
                                    outbox_id: id.clone(),
                                    code,
                                }),
                            )
                            .await;
                        }
                        Reply::InDoubt => {
                            self.in_doubt
                                .insert(id, self.clock.now_ms() + IN_DOUBT_RETRY_MS);
                        }
                        Reply::Stop(wait) => {
                            wan_down = true;
                            if wait > self.tick {
                                return Some(wait);
                            }
                        }
                    }
                }
            }
        }
        None
    }

    pub async fn run(mut self) {
        let mut paired = self.gate.subscribe();
        if paired.wait_for(|p| *p).await.is_err() {
            return;
        }
        let mut tick = tokio::time::interval(self.tick);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Some(quiet) = self.drain().await {
                tokio::time::sleep(quiet).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_proto::{AppError, NotifyChannel};

    fn answer(state: NotifyState, error_code: Option<&str>) -> NotifyResponse {
        NotifyResponse {
            outbox_id: "N".into(),
            state,
            channel: NotifyChannel::Whatsapp,
            attempts: 1,
            provider_message_id: None,
            error_code: error_code.map(str::to_owned),
        }
    }

    fn refused(status: u16, code: ErrorCode, retry_after_s: Option<u32>) -> CloudFailure {
        CloudFailure::Refused {
            status,
            error: AppError::new(code, "no"),
            retry_after_s,
        }
    }

    #[test]
    fn the_cloud_s_answer_decides_what_the_hub_records() {
        assert!(matches!(
            reply(Ok(answer(NotifyState::Sent, None))),
            Reply::Sent(_)
        ));
        assert_eq!(
            reply(Ok(answer(NotifyState::Failed, Some("REJECTED")))),
            Reply::Failed("REJECTED".into())
        );
        assert_eq!(
            reply(Ok(answer(NotifyState::Failed, None))),
            Reply::Failed("send_failed".into())
        );
        assert_eq!(
            reply(Ok(answer(NotifyState::Queued, None))),
            Reply::InDoubt,
            "an outcome in doubt is asked about later, never recorded"
        );
    }

    #[test]
    fn a_wan_that_is_down_or_a_cloud_that_is_busy_leaves_the_rows_queued() {
        assert_eq!(
            reply(Err(CloudFailure::Unreachable("offline".into()))),
            Reply::Stop(NOTIFY_TICK)
        );
        assert_eq!(
            reply(Err(refused(429, ErrorCode::RateLimited, Some(120)))),
            Reply::Stop(Duration::from_secs(120))
        );
        assert_eq!(
            reply(Err(refused(503, ErrorCode::Internal, None))),
            Reply::Stop(REFUSED_BACKOFF)
        );
        assert_eq!(
            reply(Err(refused(403, ErrorCode::TokenRevoked, None))),
            Reply::Stop(REVOKED_BACKOFF)
        );
    }

    #[test]
    fn a_request_the_cloud_will_never_accept_fails_the_row() {
        assert_eq!(
            reply(Err(refused(400, ErrorCode::ValidationFailed, None))),
            Reply::Failed("VALIDATION_FAILED".into())
        );
    }
}
