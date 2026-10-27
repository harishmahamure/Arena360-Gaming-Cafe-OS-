use std::sync::Arc;
use std::time::Duration;

use arena_core::{
    Applied, Clock, SnapshotReader, TimedSession, TimerAction, TimerEngine, TimerEvent,
    WriterHandle,
};
use arena_proto::{Actor, ActorKind, ErrorCode, Intent, IntentKind, IntentPayload, SessionExpire};
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;

pub const TIMER_TICK: Duration = Duration::from_secs(1);
pub const TIMER_RESYNC_TICKS: u32 = 30;
pub const TIMER_EVENTS_DEPTH: usize = 1024;

pub struct SessionTimerTask {
    writer: WriterHandle,
    reader: Arc<SnapshotReader>,
    clock: Arc<dyn Clock>,
    events: broadcast::Sender<TimerEvent>,
}

fn touches(applied: &Applied) -> bool {
    applied.changes.iter().any(|c| c.tbl == "sessions")
}

fn expire_intent(session_id: String, now: i64) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: IntentKind::SessionExpire,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: now,
        base_rev: None,
        actor: Actor {
            kind: ActorKind::System,
            id: "hub".into(),
        },
        elevated_by: None,
        reason: None,
        payload: IntentPayload::SessionExpire(SessionExpire { session_id }),
    }
}

impl SessionTimerTask {
    pub fn new(
        writer: WriterHandle,
        reader: Arc<SnapshotReader>,
        clock: Arc<dyn Clock>,
        events: broadcast::Sender<TimerEvent>,
    ) -> Self {
        Self {
            writer,
            reader,
            clock,
            events,
        }
    }

    async fn running(&self) -> Option<Vec<TimedSession>> {
        let (reader, now) = (Arc::clone(&self.reader), self.clock.now_ms());
        match tokio::task::spawn_blocking(move || reader.timed_sessions(now)).await {
            Ok(Ok(rows)) => Some(rows),
            Ok(Err(e)) => {
                tracing::warn!(code = e.code.as_str(), "timed sessions not read");
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, "timed sessions read panicked");
                None
            }
        }
    }

    async fn sync(&self, engine: &mut TimerEngine) {
        if let Some(rows) = self.running().await {
            engine.sync(rows, self.clock.now_ms());
        }
    }

    async fn expire(&self, engine: &mut TimerEngine, s: TimedSession) {
        let intent = expire_intent(s.session_id.clone(), self.clock.now_ms());
        match self.writer.submit_intent(intent).await {
            Ok(applied) => {
                let wrote = |tbl: &str| applied.changes.iter().any(|c| c.tbl == tbl);
                if applied.changes.iter().any(|c| c.tbl != "sessions") {
                    let _ = self.events.send(TimerEvent::Expired {
                        session_id: s.session_id,
                        device_id: s.device_id,
                        held: !wrote("devices_meta") && !wrote("orders"),
                    });
                }
            }
            Err(e) if e.code == ErrorCode::Stale || e.code == ErrorCode::NotFound => {}
            Err(e)
                if matches!(
                    e.code,
                    ErrorCode::ValidationFailed | ErrorCode::PermissionDenied
                ) =>
            {
                tracing::error!(
                    code = e.code.as_str(),
                    "session expiry refused; not retried"
                );
            }
            Err(e) => {
                tracing::warn!(code = e.code.as_str(), "session expiry not recorded");
                engine.requeue(&s, self.clock.now_ms());
            }
        }
    }

    pub async fn run(self) {
        let mut applied = self.writer.subscribe();
        let mut engine = TimerEngine::new();
        self.sync(&mut engine).await;
        let mut ticks = 0u32;
        let mut tick = tokio::time::interval(TIMER_TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    ticks += 1;
                    if ticks.is_multiple_of(TIMER_RESYNC_TICKS) {
                        self.sync(&mut engine).await;
                    }
                    for action in engine.tick(self.clock.now_ms()) {
                        match action {
                            TimerAction::Warn { session_id, device_id, minutes_left } => {
                                let _ = self.events.send(TimerEvent::Warning {
                                    session_id,
                                    device_id,
                                    minutes_left,
                                });
                            }
                            TimerAction::Expire(s) => self.expire(&mut engine, s).await,
                        }
                    }
                }
                ev = applied.recv() => match ev {
                    Ok(a) if !touches(&a) => {}
                    Ok(_) | Err(RecvError::Lagged(_)) => self.sync(&mut engine).await,
                    Err(RecvError::Closed) => return,
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_core::{LeaseInfo, ManualClock, Role, Writer, status_channel};
    use arena_proto::{
        BranchBootstrap, ClientClass, DeviceAssign, DeviceRegister, GroupUpsert, PAIR_SYSTEM_ACTOR,
    };
    use arena_store::{LeaseRow, Store, write_lease};

    const T0: i64 = 1_800_000_000_000;

    struct Fixture {
        _dir: tempfile::TempDir,
        path: std::path::PathBuf,
        clock: Arc<ManualClock>,
        writer: WriterHandle,
        device: String,
        group: String,
    }

    fn intent(actor: Actor, payload: IntentPayload) -> Intent {
        Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: payload.kind(),
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: T0,
            base_rev: None,
            actor,
            elevated_by: None,
            reason: None,
            payload,
        }
    }

    fn who(kind: ActorKind, id: &str) -> Actor {
        Actor {
            kind,
            id: id.into(),
        }
    }

    async fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let mut store = Store::open(&path, T0).unwrap();
        let lease = LeaseRow {
            counter: 1,
            hub_instance_id: "H".into(),
            provisional: true,
            renewed_at: T0,
            expires_at: i64::MAX,
        };
        store.write_tx(|tx| write_lease(tx, &lease)).unwrap();
        let mut initial = arena_core::HubStatus::opened(store.schema_version().unwrap(), "H");
        initial.role = Role::Primary;
        initial.lease = Some(LeaseInfo::from(&lease));
        let (_status, rx) = status_channel(initial);
        let clock = Arc::new(ManualClock::new(T0));
        let writer = Writer::spawn(store, clock.clone(), rx).unwrap();
        let submit = |a: Actor, p: IntentPayload| {
            let w = writer.clone();
            async move { w.submit_intent(intent(a, p)).await.unwrap() }
        };
        submit(
            who(ActorKind::System, "s"),
            IntentPayload::BranchBootstrap(BranchBootstrap {
                name: "B".into(),
                branch_code: "B01".into(),
                currency: "INR".into(),
                tz: "UTC".into(),
                day_start_min: 0,
                fy_start_month: 1,
                onboarding: None,
            }),
        )
        .await;
        let device = submit(
            who(ActorKind::System, PAIR_SYSTEM_ACTOR),
            IntentPayload::DeviceRegister(DeviceRegister::new(
                ClientClass::Paired,
                "PC",
                arena_core::devices::hash_token_secret("s").unwrap(),
            )),
        )
        .await
        .changes[0]
            .row_id
            .clone();
        let group = submit(
            who(ActorKind::Owner, "o"),
            IntentPayload::GroupUpsert(GroupUpsert {
                id: None,
                name: "PCs".into(),
                group_kind: "pc".into(),
                colour: "teal".into(),
                sort_order: 1,
            }),
        )
        .await
        .changes[0]
            .row_id
            .clone();
        submit(
            who(ActorKind::Owner, "o"),
            IntentPayload::DeviceAssign(DeviceAssign {
                device_id: device.clone(),
                group_id: Some(group.clone()),
            }),
        )
        .await;
        Fixture {
            _dir: dir,
            path,
            clock,
            writer,
            device,
            group,
        }
    }

    async fn live_session(f: &Fixture, ends_at: i64) {
        timed_session(f, ends_at, 0, "clean_up").await;
    }

    async fn timed_session(f: &Fixture, ends_at: i64, grace_min: u32, mode: &str) {
        let sql = format!(
            "INSERT INTO sessions (id, device_id, group_id, mode, state, started_at, ends_at,
                 rounding_min, minimum_min, grace_min, time_up_mode, epoch, business_day, fy,
                 guest_no, rev, created_at, updated_at)
             VALUES ('S1', '{}', '{}', 'pack', 'active', {T0}, {ends_at}, 15, 30, {grace_min}, '{mode}',
                     'e', 'd', 'fy', 1, 0, 0, 0)",
            f.device, f.group
        );
        f.writer
            .submit(move |tx| {
                tx.execute_batch(&sql).unwrap();
                Ok(())
            })
            .await
            .unwrap();
    }

    fn session_state(f: &Fixture) -> Option<String> {
        SnapshotReader::open(&f.path)
            .unwrap()
            .timed_sessions(T0)
            .unwrap()
            .first()
            .map(|_| "active".to_owned())
    }

    fn task(f: &Fixture, tx: broadcast::Sender<TimerEvent>) -> SessionTimerTask {
        SessionTimerTask::new(
            f.writer.clone(),
            Arc::new(SnapshotReader::open(&f.path).unwrap()),
            f.clock.clone(),
            tx,
        )
    }

    #[test]
    fn the_expiry_is_a_system_intent_without_a_base_rev() {
        let i = expire_intent("S".into(), T0);
        assert_eq!(i.kind, IntentKind::SessionExpire);
        assert_eq!(i.actor.kind, ActorKind::System);
        assert!(i.base_rev.is_none());
    }

    #[tokio::test]
    async fn a_session_found_past_its_end_is_ended_at_once_and_announced() {
        let f = fixture().await;
        live_session(&f, T0 - 5_000).await;
        let (tx, mut rx) = broadcast::channel(8);
        let job = tokio::spawn(task(&f, tx).run());
        let ev = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(ev, TimerEvent::Expired { ref session_id, .. } if session_id == "S1"));
        assert_eq!(session_state(&f), None);
        job.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_running_session_gets_its_warnings_then_ends_on_time() {
        let f = fixture().await;
        live_session(&f, T0 + 11 * 60_000).await;
        let (tx, mut rx) = broadcast::channel(8);
        let job = tokio::spawn(task(&f, tx).run());
        tokio::task::yield_now().await;
        let mut seen = Vec::new();
        for _ in 0..(12 * 60) {
            tokio::time::advance(Duration::from_secs(1)).await;
            f.clock.advance(1_000);
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
            while let Ok(ev) = rx.try_recv() {
                seen.push(ev);
            }
            if matches!(seen.last(), Some(TimerEvent::Expired { .. })) {
                break;
            }
        }
        let marks: Vec<u8> = seen
            .iter()
            .filter_map(|e| match e {
                TimerEvent::Warning { minutes_left, .. } => Some(*minutes_left),
                TimerEvent::Expired { .. } => None,
            })
            .collect();
        assert_eq!(marks, [10, 5, 1]);
        assert!(matches!(seen.last(), Some(TimerEvent::Expired { .. })));
        assert_eq!(session_state(&f), None);
        job.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn staff_handles_waits_out_the_grace_then_announces_a_held_expiry() {
        let f = fixture().await;
        timed_session(&f, T0 + 5_000, 1, "staff_handles").await;
        let (tx, mut rx) = broadcast::channel(8);
        let job = tokio::spawn(task(&f, tx).run());
        tokio::task::yield_now().await;
        let mut seen = Vec::new();
        for _ in 0..90 {
            tokio::time::advance(Duration::from_secs(1)).await;
            f.clock.advance(1_000);
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
            while let Ok(ev) = rx.try_recv() {
                seen.push(ev);
            }
            if !seen.is_empty() {
                break;
            }
        }
        assert!(
            matches!(seen.as_slice(), [TimerEvent::Expired { held: true, .. }]),
            "{seen:?}"
        );
        assert!(f.clock.now_ms() >= T0 + 65_000);
        job.abort();
    }
}
