//! Queues the daily credit reminders (Story 8.31).
//!
//! A primary Hub asks the snapshot reader every few minutes which players are due a reminder
//! ([`arena_core::credit_reminder::due`]: nothing before 11:00 branch time) and applies each as a
//! `notify.requested` system intent. The intent writes one `notify_outbox` row per player per
//! business day, so a pass that runs twice, or a Hub that was off at 11:00 and starts later the
//! same day, queues nothing twice. The drain ([`crate::notify`]) sends the rows.

use std::sync::Arc;
use std::time::Duration;

use arena_core::{Clock, HubStatus, Role, SnapshotReader, WriterHandle};
use arena_proto::{CREDIT_REMINDER_INTERVAL_S, ErrorCode, IntentPayload, NotifyRequested};
use tokio::sync::watch;

use crate::notify_trigger::intent;

pub const CREDIT_REMINDER_TICK: Duration = Duration::from_secs(CREDIT_REMINDER_INTERVAL_S);

pub struct CreditReminderTask {
    writer: WriterHandle,
    snapshots: Arc<SnapshotReader>,
    clock: Arc<dyn Clock>,
    status: watch::Sender<HubStatus>,
    tick: Duration,
}

impl CreditReminderTask {
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
            tick: CREDIT_REMINDER_TICK,
        }
    }

    /// Tests: how often the task looks for reminders to queue.
    pub fn with_tick(mut self, tick: Duration) -> Self {
        self.tick = tick;
        self
    }

    async fn due(&self) -> Option<Vec<NotifyRequested>> {
        let reader = Arc::clone(&self.snapshots);
        let now = self.clock.now_ms();
        match tokio::task::spawn_blocking(move || reader.credit_reminders(now)).await {
            Ok(Ok(due)) => Some(due),
            Ok(Err(e)) => {
                tracing::warn!(code = e.code.as_str(), "credit reminders not read");
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, "credit reminders read panicked");
                None
            }
        }
    }

    /// One pass: queues every reminder due today that is not queued yet.
    pub async fn pass(&self) {
        if self.status.borrow().role != Role::Primary {
            return;
        }
        let Some(due) = self.due().await else {
            return;
        };
        for request in due {
            let payload = IntentPayload::NotifyRequested(request);
            match self
                .writer
                .submit_intent(intent(payload, self.clock.now_ms()))
                .await
            {
                Ok(_) => {}
                // Someone got there first, or the player, credit, consent or setting changed
                // since the read: nothing to queue.
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
                    tracing::warn!(code = e.code.as_str(), "credit reminder not queued");
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
