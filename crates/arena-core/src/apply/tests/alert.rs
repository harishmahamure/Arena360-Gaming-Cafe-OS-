use super::stock::{intent, owner, staff};
use super::*;
use crate::rbac::{ROLE_COUNTER, ROLE_KITCHEN};
use arena_proto::{AlertAck, AlertKind, AlertRaise, DeviceKind};

fn text(row: &Row, col: &str) -> String {
    match &row[col] {
        RowValue::Text(t) => t.clone(),
        other => panic!("{col}: {other:?}"),
    }
}

fn system() -> Actor {
    Actor {
        kind: ActorKind::System,
        id: "hub".into(),
    }
}

async fn device(f: &Fixture) -> String {
    super::session::pc(f, DeviceKind::Pc, "pc", Some(12_000)).await
}

async fn raise(
    f: &Fixture,
    actor: Actor,
    kind: AlertKind,
    device: Option<&str>,
) -> Result<Applied, AppError> {
    f.writer
        .submit_intent(intent(
            actor,
            IntentPayload::AlertRaise(AlertRaise::new(kind, device.map(str::to_owned))),
        ))
        .await
}

async fn ack(f: &Fixture, actor: Actor, id: &str) -> Result<Applied, AppError> {
    f.writer
        .submit_intent(intent(
            actor,
            IntentPayload::AlertAck(AlertAck {
                alert_id: id.into(),
            }),
        ))
        .await
}

fn alerts(f: &Fixture) -> Vec<Row> {
    reopen(f).rows(Table::Alerts).unwrap()
}

fn log_len(f: &Fixture) -> usize {
    reopen(f).changes().unwrap().len()
}

#[tokio::test]
async fn raising_opens_an_alert_that_the_snapshot_lists() {
    let f = bootstrapped().await;
    let d = device(&f).await;
    f.clock.set(T0 + 5_000);
    let a = raise(&f, system(), AlertKind::Fixture, Some(&d))
        .await
        .unwrap();
    assert_eq!(
        a.changes
            .iter()
            .map(|c| (c.tbl.as_str(), c.op))
            .collect::<Vec<_>>(),
        [("alerts", ChangeOp::Insert)]
    );
    let row = &alerts(&f)[0];
    assert_eq!(text(row, "kind"), "fixture");
    assert_eq!(text(row, "device_id"), d);
    assert_eq!(row["raised_at"], RowValue::Integer(T0 + 5_000));
    assert_eq!(row["acked_at"], RowValue::Null);
    let snap = crate::snapshot::read_snapshot(&reopen(&f), T0 + 6_000).unwrap();
    assert_eq!(snap.alerts.len(), 1);
    assert_eq!(snap.alerts[0].id, a.changes[0].row_id);
    assert_eq!(snap.alerts[0].kind, AlertKind::Fixture);
    assert_eq!(snap.alerts[0].device_id.as_deref(), Some(d.as_str()));
    assert_eq!(snap.alerts[0].raised_at, T0 + 5_000);
}

#[tokio::test]
async fn raising_the_same_open_alert_again_writes_nothing() {
    let f = bootstrapped().await;
    let d = device(&f).await;
    raise(&f, system(), AlertKind::Fixture, Some(&d))
        .await
        .unwrap();
    let before = log_len(&f);
    let e = raise(&f, system(), AlertKind::Fixture, Some(&d))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Stale);
    assert_eq!(log_len(&f), before);
    raise(&f, system(), AlertKind::TimeUp, Some(&d))
        .await
        .unwrap();
    assert_eq!(alerts(&f).len(), 2);
}

#[tokio::test]
async fn a_second_open_branch_alert_of_a_kind_is_stale() {
    let f = bootstrapped().await;
    raise(&f, system(), AlertKind::Fixture, None).await.unwrap();
    let before = log_len(&f);
    let e = raise(&f, system(), AlertKind::Fixture, None)
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Stale);
    assert_eq!(log_len(&f), before);
    let snap = crate::snapshot::read_snapshot(&reopen(&f), T0).unwrap();
    assert_eq!(snap.alerts[0].device_id, None);
}

#[tokio::test]
async fn raising_after_an_acknowledgement_opens_a_new_alert() {
    let f = bootstrapped().await;
    let d = device(&f).await;
    let first = raise(&f, system(), AlertKind::Fixture, Some(&d))
        .await
        .unwrap();
    ack(&f, owner(), &first.changes[0].row_id).await.unwrap();
    let second = raise(&f, system(), AlertKind::Fixture, Some(&d))
        .await
        .unwrap();
    assert_ne!(first.changes[0].row_id, second.changes[0].row_id);
    assert_eq!(alerts(&f).len(), 2);
    let snap = crate::snapshot::read_snapshot(&reopen(&f), T0).unwrap();
    assert_eq!(snap.alerts.len(), 1);
    assert_eq!(snap.alerts[0].id, second.changes[0].row_id);
}

#[tokio::test]
async fn an_unknown_or_revoked_device_is_not_found() {
    let f = bootstrapped().await;
    let d = device(&f).await;
    let e = raise(&f, system(), AlertKind::Fixture, Some("NOPE"))
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::NotFound, Some("device_id"))
    );
    let mut revoke = intent(
        owner(),
        IntentPayload::DeviceRevoke(arena_proto::DeviceRevoke {
            device_id: d.clone(),
        }),
    );
    revoke.reason = Some(ReasonCode::Lost);
    f.writer.submit_intent(revoke).await.unwrap();
    let after_revoke = log_len(&f);
    let e = raise(&f, system(), AlertKind::Fixture, Some(&d))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
    assert_eq!(log_len(&f), after_revoke);
    assert!(alerts(&f).is_empty());
}

#[tokio::test]
async fn an_unknown_kind_is_a_validation_failure() {
    let f = bootstrapped().await;
    let d = device(&f).await;
    let before = log_len(&f);
    let e = f
        .writer
        .submit_intent(intent(
            system(),
            IntentPayload::AlertRaise(AlertRaise {
                alert_kind: "bogus".into(),
                device_id: Some(d),
            }),
        ))
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::ValidationFailed, Some("alert_kind"))
    );
    assert_eq!(log_len(&f), before);
}

#[tokio::test]
async fn acknowledging_keeps_the_row_and_records_who_and_when() {
    let f = bootstrapped().await;
    let d = device(&f).await;
    let counter = staff(&f, "ctr", ROLE_COUNTER).await;
    let raised = raise(&f, system(), AlertKind::Fixture, Some(&d))
        .await
        .unwrap();
    let id = raised.changes[0].row_id.clone();
    f.clock.set(T0 + 9_000);
    let a = ack(&f, counter.clone(), &id).await.unwrap();
    assert_eq!(
        a.changes
            .iter()
            .map(|c| (c.tbl.as_str(), c.op))
            .collect::<Vec<_>>(),
        [("alerts", ChangeOp::Update)]
    );
    let row = &alerts(&f)[0];
    assert_eq!(row["acked_at"], RowValue::Integer(T0 + 9_000));
    assert_eq!(text(row, "acked_by"), counter.id);
    assert_eq!(row["rev"], RowValue::Integer(2));
    let snap = crate::snapshot::read_snapshot(&reopen(&f), T0 + 10_000).unwrap();
    assert!(snap.alerts.is_empty());
    let change = reopen(&f)
        .changes()
        .unwrap()
        .into_iter()
        .rfind(|c| c.tbl == "alerts")
        .unwrap();
    assert_eq!(
        (change.actor_kind.as_str(), change.actor_id.as_str()),
        ("staff", counter.id.as_str())
    );
    assert_eq!(change.intent_kind, "alert.ack");
}

#[tokio::test]
async fn acknowledging_twice_or_an_unknown_alert_writes_nothing() {
    let f = bootstrapped().await;
    let d = device(&f).await;
    let id = raise(&f, system(), AlertKind::Fixture, Some(&d))
        .await
        .unwrap()
        .changes[0]
        .row_id
        .clone();
    ack(&f, owner(), &id).await.unwrap();
    let before = log_len(&f);
    let again = ack(&f, owner(), &id).await.unwrap_err();
    assert_eq!(again.code, ErrorCode::Stale);
    let gone = ack(&f, owner(), "NOPE").await.unwrap_err();
    assert_eq!(
        (gone.code, gone.field.as_deref()),
        (ErrorCode::NotFound, Some("alert_id"))
    );
    assert_eq!(log_len(&f), before);
}

#[tokio::test]
async fn only_counter_staff_acknowledge_and_only_the_system_raises() {
    let f = bootstrapped().await;
    let d = device(&f).await;
    let kitchen = staff(&f, "kit", ROLE_KITCHEN).await;
    let counter = staff(&f, "ctr", ROLE_COUNTER).await;
    let denied = raise(&f, counter.clone(), AlertKind::Fixture, Some(&d))
        .await
        .unwrap_err();
    assert_eq!(denied.code, ErrorCode::PermissionDenied);
    let denied = raise(&f, owner(), AlertKind::Fixture, Some(&d))
        .await
        .unwrap_err();
    assert_eq!(denied.code, ErrorCode::PermissionDenied);
    let id = raise(&f, system(), AlertKind::Fixture, Some(&d))
        .await
        .unwrap()
        .changes[0]
        .row_id
        .clone();
    let denied = ack(&f, kitchen, &id).await.unwrap_err();
    assert_eq!(denied.code, ErrorCode::PermissionDenied);
    let denied = ack(&f, system(), &id).await.unwrap_err();
    assert_eq!(denied.code, ErrorCode::PermissionDenied);
    assert_eq!(alerts(&f)[0]["acked_at"], RowValue::Null);
    ack(&f, counter, &id).await.unwrap();
}

#[tokio::test]
async fn neither_alert_intent_takes_a_base_rev() {
    let f = bootstrapped().await;
    let d = device(&f).await;
    let mut raise_it = intent(
        system(),
        IntentPayload::AlertRaise(AlertRaise::new(AlertKind::Fixture, Some(d.clone()))),
    );
    raise_it.base_rev = Some(1);
    let e = f.writer.submit_intent(raise_it).await.unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::ValidationFailed, Some("base_rev"))
    );
    let id = raise(&f, system(), AlertKind::Fixture, Some(&d))
        .await
        .unwrap()
        .changes[0]
        .row_id
        .clone();
    let mut ack_it = intent(owner(), IntentPayload::AlertAck(AlertAck { alert_id: id }));
    ack_it.base_rev = Some(1);
    let e = f.writer.submit_intent(ack_it).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::ValidationFailed);
}

#[tokio::test]
async fn open_alerts_are_listed_oldest_first() {
    let f = bootstrapped().await;
    let a = device(&f).await;
    let b = device(&f).await;
    f.clock.set(T0 + 1_000);
    raise(&f, system(), AlertKind::Fixture, Some(&b))
        .await
        .unwrap();
    f.clock.set(T0 + 2_000);
    raise(&f, system(), AlertKind::TimeUp, Some(&a))
        .await
        .unwrap();
    let snap = crate::snapshot::read_snapshot(&reopen(&f), T0 + 3_000).unwrap();
    assert_eq!(
        snap.alerts
            .iter()
            .map(|x| (x.kind, x.device_id.clone().unwrap()))
            .collect::<Vec<_>>(),
        [(AlertKind::Fixture, b), (AlertKind::TimeUp, a)]
    );
}
