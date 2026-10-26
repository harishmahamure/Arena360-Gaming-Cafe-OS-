use super::stock::{intent, owner};
use super::*;
use arena_proto::{
    ClientClass, DeviceAssign, DeviceMaintenance, DeviceRegister, DeviceRevoke,
    DeviceTestSessionCompleted, DeviceTestSessionStart, GroupUpsert, KEY_TEST_SESSION,
    PAIR_SYSTEM_ACTOR, TestSessionOutcome, TestSessionState, TestSessionStatus,
};

fn start(d: &str) -> IntentPayload {
    IntentPayload::DeviceTestSessionStart(DeviceTestSessionStart {
        device_id: d.into(),
    })
}

fn done(d: &str, outcome: TestSessionOutcome) -> IntentPayload {
    IntentPayload::DeviceTestSessionCompleted(DeviceTestSessionCompleted {
        device_id: d.into(),
        outcome,
    })
}

fn actor(kind: ActorKind, id: &str) -> Actor {
    Actor {
        kind,
        id: id.into(),
    }
}

async fn run(f: &Fixture, a: Actor, p: IntentPayload, at: i64) -> Result<Applied, AppError> {
    let mut i = intent(a, p);
    i.issued_at = at;
    f.writer.submit_intent(i).await
}

async fn kiosk(f: &Fixture, grouped: bool) -> String {
    let d = DeviceRegister::new(
        ClientClass::Paired,
        "PC",
        crate::devices::hash_token_secret("s").unwrap(),
    );
    let id = f
        .writer
        .submit_intent(intent(
            actor(ActorKind::System, PAIR_SYSTEM_ACTOR),
            IntentPayload::DeviceRegister(d),
        ))
        .await
        .unwrap()
        .changes[0]
        .row_id
        .clone();
    if grouped {
        let g = f
            .writer
            .submit_intent(intent(
                owner(),
                IntentPayload::GroupUpsert(GroupUpsert {
                    id: None,
                    name: "PCs".into(),
                    group_kind: "pc".into(),
                    colour: "teal".into(),
                    sort_order: 1,
                }),
            ))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone();
        f.writer
            .submit_intent(intent(
                owner(),
                IntentPayload::DeviceAssign(DeviceAssign {
                    device_id: id.clone(),
                    group_id: Some(g),
                }),
            ))
            .await
            .unwrap();
    }
    id
}

fn status(f: &Fixture) -> TestSessionStatus {
    let v = reopen(f)
        .settings()
        .unwrap()
        .into_iter()
        .find(|s| s.key == KEY_TEST_SESSION)
        .unwrap()
        .value;
    TestSessionStatus::parse(&v).unwrap()
}

fn refusal(r: Result<Applied, AppError>) -> (ErrorCode, Option<String>) {
    let e = r.unwrap_err();
    (e.code, e.field)
}

fn invalid(field: &str) -> (ErrorCode, Option<String>) {
    (ErrorCode::ValidationFailed, Some(field.into()))
}

#[tokio::test]
async fn a_test_runs_to_passed_writing_only_the_setting() {
    let f = bootstrapped().await;
    let d = kiosk(&f, true).await;
    let a = run(&f, owner(), start(&d), T0).await.unwrap();
    assert_eq!(a.changes.len(), 1);
    assert_eq!(a.changes[0].tbl, "settings");
    let s = status(&f);
    assert_eq!(
        (s.state, s.device_id.as_deref(), s.at),
        (TestSessionState::Running, Some(d.as_str()), Some(T0))
    );
    let a = run(
        &f,
        actor(ActorKind::Device, &d),
        done(&d, TestSessionOutcome::Locked),
        T0 + 120_000,
    )
    .await
    .unwrap();
    assert_eq!(
        (a.changes.len(), a.changes[0].tbl.as_str()),
        (1, "settings")
    );
    assert_eq!(status(&f).state, TestSessionState::Passed);
    let snap = crate::snapshot::read_snapshot(&reopen(&f), 0).unwrap();
    assert_eq!(snap.setup.unwrap().test_session, TestSessionState::Passed);
}

#[tokio::test]
async fn the_hub_times_a_test_out_and_a_late_lock_is_ignored() {
    let f = bootstrapped().await;
    let d = kiosk(&f, true).await;
    run(&f, owner(), start(&d), T0).await.unwrap();
    run(
        &f,
        actor(ActorKind::System, "hub"),
        done(&d, TestSessionOutcome::TimedOut),
        T0 + 130_000,
    )
    .await
    .unwrap();
    assert_eq!(status(&f).state, TestSessionState::Failed);
    let late = run(
        &f,
        actor(ActorKind::Device, &d),
        done(&d, TestSessionOutcome::Locked),
        T0 + 131_000,
    )
    .await;
    assert_eq!(refusal(late), invalid("device_id"));
    assert_eq!(status(&f).state, TestSessionState::Failed);
}

#[tokio::test]
async fn a_start_is_refused_for_an_unfit_device_or_a_running_test() {
    let f = bootstrapped().await;
    assert_eq!(
        refusal(run(&f, owner(), start("01JNOSUCHDEVICE"), T0).await),
        invalid("device_id")
    );
    let loose = kiosk(&f, false).await;
    assert_eq!(
        refusal(run(&f, owner(), start(&loose), T0).await),
        invalid("device_id")
    );
    let d = kiosk(&f, true).await;
    run(
        &f,
        owner(),
        IntentPayload::DeviceMaintenance(DeviceMaintenance {
            device_id: d.clone(),
            on: true,
            note: None,
        }),
        T0,
    )
    .await
    .unwrap();
    assert_eq!(
        refusal(run(&f, owner(), start(&d), T0).await),
        invalid("device_id")
    );
    run(
        &f,
        owner(),
        IntentPayload::DeviceMaintenance(DeviceMaintenance {
            device_id: d.clone(),
            on: false,
            note: None,
        }),
        T0,
    )
    .await
    .unwrap();
    run(&f, owner(), start(&d), T0).await.unwrap();
    assert_eq!(
        refusal(run(&f, owner(), start(&d), T0 + 129_000).await),
        invalid("device_id")
    );
    run(&f, owner(), start(&d), T0 + 130_000).await.unwrap();
    assert_eq!(status(&f).at, Some(T0 + 130_000));
    let mut revoke = intent(
        owner(),
        IntentPayload::DeviceRevoke(DeviceRevoke {
            device_id: d.clone(),
        }),
    );
    revoke.reason = Some(arena_proto::ReasonCode::Other);
    f.writer.submit_intent(revoke).await.unwrap();
    f.clock.set(T0 + 400_000);
    assert_eq!(
        refusal(run(&f, owner(), start(&d), T0 + 400_000).await),
        invalid("device_id")
    );
}

#[tokio::test]
async fn only_the_right_actor_completes_a_test() {
    let f = bootstrapped().await;
    let a = kiosk(&f, true).await;
    run(&f, owner(), start(&a), T0).await.unwrap();
    let denied = (ErrorCode::PermissionDenied, None);
    let wrong = actor(ActorKind::Device, "01JOTHERDEVICE");
    for (who, outcome) in [
        (wrong, TestSessionOutcome::Locked),
        (actor(ActorKind::Device, &a), TestSessionOutcome::TimedOut),
        (actor(ActorKind::System, "hub"), TestSessionOutcome::Locked),
        (owner(), TestSessionOutcome::Locked),
    ] {
        assert_eq!(
            refusal(run(&f, who, done(&a, outcome), T0 + 1).await),
            denied
        );
    }
    assert_eq!(status(&f).state, TestSessionState::Running);
    assert_eq!(
        refusal(
            run(
                &f,
                actor(ActorKind::System, "hub"),
                done("01JOTHERDEVICE", TestSessionOutcome::TimedOut),
                T0 + 1
            )
            .await
        ),
        invalid("device_id")
    );
}
