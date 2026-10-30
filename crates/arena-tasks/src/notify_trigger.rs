//! Queues the messages a running session asks for (Story 8.30): a low wallet balance, a PC
//! session five minutes from its end and a console session five minutes from its end.
//!
//! Every 10 s a primary Hub asks the snapshot reader which messages are due
//! ([`arena_core::notify_trigger::triggers`]) and applies each as a system intent:
//! `notify.requested` writes one `notify_outbox` row for the end of the Undo window,
//! `notify.rearm` closes a low-balance crossing whose wallet rose above the level again. The
//! intents check the branch tier, the message switch, the player's consent and "already queued"
//! again, so a pass that runs twice, or races an Extend, writes nothing twice. The drain
//! ([`crate::notify`]) sends the rows.

use std::sync::Arc;
use std::time::Duration;

use arena_core::notify_trigger::Trigger;
use arena_core::{Clock, HubStatus, Role, SnapshotReader, WriterHandle};
use arena_proto::{
    Actor, ActorKind, ErrorCode, Intent, IntentPayload, NOTIFY_TRIGGER_INTERVAL_S, NotifyRearm,
};
use tokio::sync::watch;

pub const NOTIFY_TRIGGER_TICK: Duration = Duration::from_secs(NOTIFY_TRIGGER_INTERVAL_S);

pub(crate) fn intent(payload: IntentPayload, now: i64) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: payload.kind(),
        idempotency_key: ulid::Ulid::generate().to_string(),
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

pub struct NotifyTriggerTask {
    writer: WriterHandle,
    snapshots: Arc<SnapshotReader>,
    clock: Arc<dyn Clock>,
    status: watch::Sender<HubStatus>,
    tick: Duration,
}

impl NotifyTriggerTask {
    pub fn new(
        writer: WriterHandle,
        snapshots: Arc<SnapshotReader>,
        clock: Arc<dyn Clock>,
        status: watch::Sender<HubStatus>,
    ) -> Self {
        Self {
            writer,
            snapshots,
            clock,
            status,
            tick: NOTIFY_TRIGGER_TICK,
        }
    }

    /// Tests: how often the task looks for messages to queue.
    pub fn with_tick(mut self, tick: Duration) -> Self {
        self.tick = tick;
        self
    }

    async fn due(&self) -> Option<Vec<Trigger>> {
        let reader = Arc::clone(&self.snapshots);
        let now = self.clock.now_ms();
        match tokio::task::spawn_blocking(move || reader.notify_triggers(now)).await {
            Ok(Ok(t)) => Some(t),
            Ok(Err(e)) => {
                tracing::warn!(code = e.code.as_str(), "notify: triggers not read");
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, "notify: triggers read panicked");
                None
            }
        }
    }

    /// One pass: records every message a running session is due to send.
    pub async fn pass(&self) {
        if self.status.borrow().role != Role::Primary {
            return;
        }
        let Some(due) = self.due().await else {
            return;
        };
        for trigger in due {
            let payload = match trigger {
                Trigger::Request(r) => IntentPayload::NotifyRequested(r),
                Trigger::Rearm(outbox_id) => IntentPayload::NotifyRearm(NotifyRearm { outbox_id }),
            };
            let kind = payload.kind();
            match self
                .writer
                .submit_intent(intent(payload, self.clock.now_ms()))
                .await
            {
                Ok(_) => {}
                // Someone else got there first (an earlier pass, an Undo), or the player, session
                // or setting changed since the read: nothing to queue.
                Err(e)
                    if matches!(
                        e.code,
                        ErrorCode::Stale
                            | ErrorCode::NotFound
                            | ErrorCode::ValidationFailed
                            | ErrorCode::ConsentRequired
                            | ErrorCode::FeatureNotEntitled
                    ) => {}
                Err(e) => {
                    tracing::warn!(
                        code = e.code.as_str(),
                        kind = kind.as_str(),
                        "notify: message not queued"
                    );
                }
            }
        }
    }

    pub async fn run(self) {
        let mut tick = tokio::time::interval(self.tick);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            self.pass().await;
        }
    }
}
