use super::players::{add, form};
use super::stock::{intent, owner};
use super::*;
use arena_proto::{DeviceKind, SessionExpire, SessionMode, SessionStart};

fn text(row: &Row, col: &str) -> String {
    match &row[col] {
        RowValue::Text(t) => t.clone(),
        other => panic!("{col}: {other:?}"),
    }
}

fn int(row: &Row, col: &str) -> i64 {
    match &row[col] {
        RowValue::Integer(n) => *n,
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

async fn timed_session(f: &Fixture, ends_in_ms: i64) -> (String, String) {
    timed_with(f, ends_in_ms, 0, "clean_up").await
}

async fn timed_with(f: &Fixture, ends_in_ms: i64, grace_min: u32, mode: &str) -> (String, String) {
    let d = device(f).await;
    let (player, _) = add(f, owner(), &form("asha", "+919876543210"))
        .await
        .unwrap();
    let mut i = intent(
        owner(),
        IntentPayload::SessionStart(SessionStart {
            device_id: d.clone(),
            mode: SessionMode::Postpaid,
            player_id: Some(player),
            pack_id: None,
        }),
    );
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await.unwrap();
    let id = text(&reopen(f).rows(Table::Sessions).unwrap()[0], "id");
    let (sid, at, mode) = (id.clone(), f.clock.now_ms() + ends_in_ms, mode.to_owned());
    f.writer
        .submit(move |tx| {
            tx.execute_batch(&format!(
                "UPDATE sessions SET ends_at = {at}, grace_min = {grace_min}, time_up_mode = '{mode}'
                 WHERE id = '{sid}'"
            ))
            .unwrap();
            Ok(())
        })
        .await
        .unwrap();
    (id, d)
}

async fn expire(f: &Fixture, actor: Actor, session: &str, at: i64) -> Result<Applied, AppError> {
    let mut i = intent(
        actor,
        IntentPayload::SessionExpire(SessionExpire {
            session_id: session.into(),
        }),
    );
    i.issued_at = at;
    f.writer.submit_intent(i).await
}

fn session_row(f: &Fixture) -> Row {
    reopen(f).rows(Table::Sessions).unwrap().remove(0)
}

#[tokio::test]
async fn a_due_session_is_expired_at_its_end_time_and_frees_the_device() {
    let f = bootstrapped().await;
    let (id, d) = timed_session(&f, 600_000).await;
    let ends = T0 + 600_000;
    f.clock.set(ends + 3_000);
    let a = expire(&f, system(), &id, ends + 3_000).await.unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        ["sessions", "session_segments", "devices_meta"]
    );
    let s = session_row(&f);
    assert_eq!(text(&s, "state"), "expired");
    assert_eq!(text(&s, "end_reason"), "expired");
    assert_eq!(text(&s, "ended_by"), "hub");
    assert_eq!(int(&s, "ended_at"), ends);
    let seg = &reopen(&f).rows(Table::SessionSegments).unwrap()[0];
    assert_eq!(int(seg, "to_at"), ends);
    let state = reopen(&f)
        .rows(Table::DevicesMeta)
        .unwrap()
        .iter()
        .find(|r| text(r, "device_id") == d)
        .map(|r| text(r, "state"))
        .unwrap();
    assert_ne!(state, "in_session");
    assert_eq!(reopen(&f).rows(Table::Orders).unwrap().len(), 0);
}

#[tokio::test]
async fn a_session_expires_exactly_once() {
    let f = bootstrapped().await;
    let (id, _) = timed_session(&f, 1_000).await;
    let at = T0 + 1_000;
    expire(&f, system(), &id, at).await.unwrap();
    let before = reopen(&f).changes().unwrap().len();
    let e = expire(&f, system(), &id, at + 1_000).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Stale);
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
}

#[tokio::test]
async fn expiry_refuses_a_session_that_is_not_due_paused_untimed_or_unknown() {
    let f = bootstrapped().await;
    let (id, _) = timed_session(&f, 60_000).await;
    let before = reopen(&f).changes().unwrap().len();
    let early = expire(&f, system(), &id, T0 + 59_999).await.unwrap_err();
    assert_eq!(early.code, ErrorCode::Stale);
    let gone = expire(&f, system(), "NOPE", T0 + 60_000).await.unwrap_err();
    assert_eq!(gone.code, ErrorCode::NotFound);
    let sid = id.clone();
    f.writer
        .submit(move |tx| {
            tx.execute_batch(&format!(
                "UPDATE sessions SET state = 'paused', paused_at = {T0} WHERE id = '{sid}'"
            ))
            .unwrap();
            Ok(())
        })
        .await
        .unwrap();
    let paused = expire(&f, system(), &id, T0 + 60_000).await.unwrap_err();
    assert_eq!(paused.code, ErrorCode::Stale);
    let sid = id.clone();
    f.writer
        .submit(move |tx| {
            tx.execute_batch(&format!(
                "UPDATE sessions SET state = 'active', paused_at = NULL, ends_at = NULL
                 WHERE id = '{sid}'"
            ))
            .unwrap();
            Ok(())
        })
        .await
        .unwrap();
    let untimed = expire(&f, system(), &id, T0 + 60_000).await.unwrap_err();
    assert_eq!(
        (untimed.code, untimed.field.as_deref()),
        (ErrorCode::ValidationFailed, Some("session_id"))
    );
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
}

#[tokio::test]
async fn only_the_system_may_expire_a_session() {
    let f = bootstrapped().await;
    let (id, _) = timed_session(&f, 1_000).await;
    let e = expire(&f, owner(), &id, T0 + 1_000).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    assert_eq!(text(&session_row(&f), "state"), "active");
}

#[tokio::test]
async fn an_expired_session_lets_the_device_start_another() {
    let f = bootstrapped().await;
    let (id, d) = timed_session(&f, 1_000).await;
    expire(&f, system(), &id, T0 + 1_000).await.unwrap();
    f.clock.set(T0 + 2_000);
    let mut i = intent(
        owner(),
        IntentPayload::SessionStart(SessionStart {
            device_id: d,
            mode: SessionMode::Postpaid,
            player_id: None,
            pack_id: None,
        }),
    );
    i.issued_at = T0 + 2_000;
    f.writer.submit_intent(i).await.unwrap();
    assert_eq!(reopen(&f).rows(Table::Sessions).unwrap().len(), 2);
}

#[tokio::test]
async fn the_snapshot_and_the_kiosk_screen_carry_the_end_time() {
    let f = bootstrapped().await;
    let (_, d) = timed_session(&f, 90_000).await;
    let snap = crate::snapshot::read_snapshot(&reopen(&f), T0).unwrap();
    assert_eq!(snap.sessions[0].ends_at, Some(T0 + 90_000));
    let screens = crate::session_screens(&reopen(&f), T0 + 30_000).unwrap();
    let (device, screen) = &screens[0];
    assert_eq!(device, &d);
    assert_eq!(screen.lock_after_ms, 60_000);
}

fn device_state(f: &Fixture, device: &str) -> String {
    reopen(f)
        .rows(Table::DevicesMeta)
        .unwrap()
        .iter()
        .find(|r| text(r, "device_id") == device)
        .map(|r| text(r, "state"))
        .unwrap()
}

fn alerts(f: &Fixture) -> Vec<Row> {
    reopen(f).rows(Table::Alerts).unwrap()
}

#[tokio::test]
async fn a_session_with_a_grace_enters_grace_at_its_end_and_closes_when_it_runs_out() {
    let f = bootstrapped().await;
    let (id, d) = timed_with(&f, 60_000, 2, "clean_up").await;
    let ends = T0 + 60_000;
    let early = expire(&f, system(), &id, ends - 1).await.unwrap_err();
    assert_eq!(early.code, ErrorCode::Stale);
    f.clock.set(ends);
    let a = expire(&f, system(), &id, ends).await.unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        ["sessions"]
    );
    let s = session_row(&f);
    assert_eq!(text(&s, "state"), "grace");
    assert!(matches!(s["ended_at"], RowValue::Null));
    assert_eq!(device_state(&f, &d), "in_session");
    let seg = &reopen(&f).rows(Table::SessionSegments).unwrap()[0];
    assert!(matches!(seg["to_at"], RowValue::Null));
    let snap = crate::snapshot::read_snapshot(&reopen(&f), ends).unwrap();
    assert_eq!(snap.sessions[0].state, "grace");
    assert_eq!(snap.sessions[0].grace_ends_at, Some(ends + 120_000));
    let screens = crate::session_screens(&reopen(&f), ends + 30_000).unwrap();
    assert_eq!(screens[0].1.lock_after_ms, 90_000);
    assert_eq!(screens[0].1.grace_ms, 120_000);

    let again = expire(&f, system(), &id, ends + 119_999).await.unwrap_err();
    assert_eq!(again.code, ErrorCode::Stale);
    f.clock.set(ends + 120_000);
    expire(&f, system(), &id, ends + 120_000).await.unwrap();
    let s = session_row(&f);
    assert_eq!(text(&s, "state"), "expired");
    assert_eq!(int(&s, "ended_at"), ends);
    assert_ne!(device_state(&f, &d), "in_session");
    assert!(alerts(&f).is_empty());
    let twice = expire(&f, system(), &id, ends + 130_000).await.unwrap_err();
    assert_eq!(twice.code, ErrorCode::Stale);
}

#[tokio::test]
async fn a_session_found_past_its_grace_closes_at_once() {
    let f = bootstrapped().await;
    let (id, _) = timed_with(&f, 1_000, 2, "clean_up").await;
    let at = T0 + 1_000 + 120_000;
    f.clock.set(at);
    expire(&f, system(), &id, at).await.unwrap();
    assert_eq!(text(&session_row(&f), "state"), "expired");
}

#[tokio::test]
async fn staff_handles_holds_the_session_expired_with_a_time_up_alert_until_staff_end_it() {
    let f = bootstrapped().await;
    let (id, d) = timed_with(&f, 1_000, 1, "staff_handles").await;
    let ends = T0 + 1_000;
    f.clock.set(ends);
    expire(&f, system(), &id, ends).await.unwrap();
    assert_eq!(text(&session_row(&f), "state"), "grace");
    assert!(alerts(&f).is_empty());

    f.clock.set(ends + 60_000);
    let a = expire(&f, system(), &id, ends + 60_000).await.unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        ["sessions", "session_segments", "alerts"]
    );
    let s = session_row(&f);
    assert_eq!(text(&s, "state"), "expired");
    assert!(matches!(s["ended_at"], RowValue::Null));
    let seg = &reopen(&f).rows(Table::SessionSegments).unwrap()[0];
    assert_eq!(int(seg, "to_at"), ends);
    assert_eq!(device_state(&f, &d), "in_session");
    let raised = alerts(&f);
    assert_eq!(raised.len(), 1);
    assert_eq!(text(&raised[0], "kind"), "time_up");
    assert_eq!(text(&raised[0], "device_id"), d);
    assert!(matches!(raised[0]["acked_at"], RowValue::Null));
    let snap = crate::snapshot::read_snapshot(&reopen(&f), ends + 60_000).unwrap();
    assert_eq!(snap.sessions[0].state, "expired");
    assert_eq!(snap.alerts.len(), 1);
    let held = &crate::session_screens(&reopen(&f), ends + 61_000).unwrap()[0].1;
    assert_eq!(held.lock_after_ms, 0);
    assert_eq!(held.grace_ms, 0);

    let blocked = expire(&f, system(), &id, ends + 70_000).await.unwrap_err();
    assert_eq!(blocked.code, ErrorCode::Stale);
    let mut start = intent(
        owner(),
        IntentPayload::SessionStart(SessionStart {
            device_id: d.clone(),
            mode: SessionMode::Postpaid,
            player_id: None,
            pack_id: None,
        }),
    );
    start.issued_at = ends + 70_000;
    assert!(f.writer.submit_intent(start).await.is_err());

    f.clock.set(ends + 300_000);
    let rev = int(&session_row(&f), "rev");
    let mut end = super::session::end_intent(&id, Some(rev));
    end.issued_at = ends + 300_000;
    super::session::end(&f, end).await.unwrap();
    let s = session_row(&f);
    assert_eq!(text(&s, "state"), "ended");
    assert_ne!(device_state(&f, &d), "in_session");
}
