use std::sync::Arc;
use std::time::Duration;

use arena_core::{Applied, Clock, SnapshotReader, WriterHandle};
use arena_proto::{
    Actor, ActorKind, DeviceTestSessionCompleted, Intent, IntentKind, IntentPayload,
    KEY_TEST_SESSION, TestSessionOutcome, TestSessionState, TestSessionStatus,
};
use tokio::sync::broadcast::error::RecvError;

pub const TEST_SESSION_RETRY: Duration = Duration::from_secs(5);

pub struct TestSessionTask {
    writer: WriterHandle,
    reader: Arc<SnapshotReader>,
    clock: Arc<dyn Clock>,
}

fn touches(applied: &Applied) -> bool {
    applied
        .changes
        .iter()
        .any(|c| c.tbl == "settings" && c.row_id.ends_with(KEY_TEST_SESSION))
}

impl TestSessionTask {
    pub fn new(writer: WriterHandle, reader: Arc<SnapshotReader>, clock: Arc<dyn Clock>) -> Self {
        Self {
            writer,
            reader,
            clock,
        }
    }

    async fn due(&self) -> Option<(String, i64)> {
        let reader = Arc::clone(&self.reader);
        let rows = tokio::task::spawn_blocking(move || reader.settings())
            .await
            .ok()?
            .ok()?;
        let s = rows
            .iter()
            .find(|r| r.key == KEY_TEST_SESSION)
            .and_then(|r| TestSessionStatus::parse(&r.value))
            .filter(|s| s.state == TestSessionState::Running)?;
        Some((s.device_id?, s.at? + TestSessionStatus::WINDOW_MS))
    }

    async fn time_out(&self, device_id: String) {
        let now = self.clock.now_ms();
        let payload = IntentPayload::DeviceTestSessionCompleted(DeviceTestSessionCompleted {
            device_id,
            outcome: TestSessionOutcome::TimedOut,
        });
        let intent = Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: IntentKind::DeviceTestSessionCompleted,
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: now,
            base_rev: None,
            actor: Actor {
                kind: ActorKind::System,
                id: "hub".into(),
            },
            elevated_by: None,
            reason: None,
            payload,
        };
        if let Err(e) = self.writer.submit_intent(intent).await {
            tracing::warn!(code = e.code.as_str(), "test session timeout not recorded");
            tokio::time::sleep(TEST_SESSION_RETRY).await;
        }
    }

    pub async fn run(self) {
        let mut events = self.writer.subscribe();
        let mut due = self.due().await;
        loop {
            let wait = due
                .as_ref()
                .map(|(_, at)| Duration::from_millis((at - self.clock.now_ms()).max(0) as u64));
            tokio::select! {
                () = async {
                    match wait {
                        Some(w) => tokio::time::sleep(w).await,
                        None => std::future::pending().await,
                    }
                } => {
                    if let Some((id, _)) = due.take() {
                        self.time_out(id).await;
                    }
                    due = self.due().await;
                }
                ev = events.recv() => match ev {
                    Ok(a) if !touches(&a) => {}
                    Ok(_) | Err(RecvError::Lagged(_)) => due = self.due().await,
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
        BranchBootstrap, ClientClass, DeviceAssign, DeviceRegister, DeviceTestSessionStart,
        GroupUpsert, PAIR_SYSTEM_ACTOR,
    };
    use arena_store::{LeaseRow, Store, write_lease};

    const T0: i64 = 1_800_000_000_000;

    struct Fixture {
        _dir: tempfile::TempDir,
        path: std::path::PathBuf,
        clock: Arc<ManualClock>,
        writer: WriterHandle,
        device: String,
    }

    fn intent(actor: Actor, payload: IntentPayload, at: i64) -> Intent {
        Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: payload.kind(),
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: at,
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
        let owner = who(ActorKind::Owner, "o");
        let sys = who(ActorKind::System, "s");
        let submit = |a: Actor, p: IntentPayload| {
            let w = writer.clone();
            async move { w.submit_intent(intent(a, p, T0)).await.unwrap() }
        };
        submit(
            sys,
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
            owner.clone(),
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
            owner,
            IntentPayload::DeviceAssign(DeviceAssign {
                device_id: device.clone(),
                group_id: Some(group),
            }),
        )
        .await;
        Fixture {
            _dir: dir,
            path,
            clock,
            writer,
            device,
        }
    }

    async fn start(f: &Fixture) {
        f.writer
            .submit_intent(intent(
                who(ActorKind::Owner, "o"),
                IntentPayload::DeviceTestSessionStart(DeviceTestSessionStart {
                    device_id: f.device.clone(),
                }),
                f.clock.now_ms(),
            ))
            .await
            .unwrap();
    }

    fn state(f: &Fixture) -> TestSessionState {
        let reader = SnapshotReader::open(&f.path).unwrap();
        reader
            .settings()
            .unwrap()
            .into_iter()
            .find(|s| s.key == KEY_TEST_SESSION)
            .and_then(|s| TestSessionStatus::parse(&s.value))
            .unwrap()
            .state
    }

    fn task(f: &Fixture) -> TestSessionTask {
        TestSessionTask::new(
            f.writer.clone(),
            Arc::new(SnapshotReader::open(&f.path).unwrap()),
            f.clock.clone(),
        )
    }

    async fn settles(f: &Fixture, want: TestSessionState) -> bool {
        for _ in 0..500 {
            if state(f) == want {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
            tokio::task::yield_now().await;
        }
        state(f) == want
    }

    #[tokio::test]
    async fn a_test_found_running_past_its_window_fails_at_startup() {
        let f = fixture().await;
        start(&f).await;
        f.clock.advance(TestSessionStatus::WINDOW_MS);
        let job = tokio::spawn(task(&f).run());
        assert!(settles(&f, TestSessionState::Failed).await);
        job.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_running_test_fails_130_seconds_after_it_started_and_not_before() {
        let f = fixture().await;
        let job = tokio::spawn(task(&f).run());
        tokio::task::yield_now().await;
        start(&f).await;
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(2));
            tokio::task::yield_now().await;
        }
        tokio::time::advance(Duration::from_secs(129)).await;
        f.clock.advance(129_000);
        tokio::task::yield_now().await;
        assert_eq!(state(&f), TestSessionState::Running);
        tokio::time::advance(Duration::from_secs(2)).await;
        f.clock.advance(2_000);
        assert!(settles(&f, TestSessionState::Failed).await);
        job.abort();
    }

    #[tokio::test]
    async fn a_kiosk_that_locks_in_time_keeps_the_test_passed() {
        let f = fixture().await;
        start(&f).await;
        let job = tokio::spawn(task(&f).run());
        f.writer
            .submit_intent(intent(
                who(ActorKind::Device, &f.device),
                IntentPayload::DeviceTestSessionCompleted(DeviceTestSessionCompleted {
                    device_id: f.device.clone(),
                    outcome: TestSessionOutcome::Locked,
                }),
                f.clock.now_ms(),
            ))
            .await
            .unwrap();
        f.clock.advance(TestSessionStatus::WINDOW_MS * 2);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(state(&f), TestSessionState::Passed);
        job.abort();
    }
}
