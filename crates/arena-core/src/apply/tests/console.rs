use super::prepaid::{begin, group_of, owned_pack, sql};
use super::session::{
    balance, end, end_intent, int, pc, player, refusal, rev_of, sessions, start, state_of, table,
    text, topped_up,
};
use super::stock::{intent, owner};
use super::*;
use arena_proto::{
    ControllerCheck, ControllerResult, DeviceKind, IntentPayload, SessionExpire, SessionMode,
};

#[tokio::test]
async fn a_console_takes_a_postpaid_session_and_frees_at_the_end() {
    let f = bootstrapped().await;
    let tv = pc(&f, DeviceKind::Console, "console", Some(12_000)).await;
    let a = start(&f, &tv, None).await.unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        ["sessions", "session_segments", "devices_meta"]
    );
    let s = &sessions(&f)[0];
    assert_eq!(text(s, "device_id"), tv);
    assert_eq!(text(s, "mode"), "postpaid");
    assert_eq!(state_of(&f, &tv), "in_session");
    let id = text(s, "id");
    let rev = rev_of(&f, Table::Sessions, &id);
    f.clock.advance(45 * 60_000);
    end(&f, end_intent(&id, Some(rev))).await.unwrap();
    assert_ne!(state_of(&f, &tv), "in_session");
    assert_eq!(table(&f, Table::Orders).len(), 1);
}

#[tokio::test]
async fn a_second_session_on_a_busy_console_is_refused() {
    let f = bootstrapped().await;
    let tv = pc(&f, DeviceKind::Console, "console", Some(12_000)).await;
    start(&f, &tv, None).await.unwrap();
    assert_eq!(
        refusal(start(&f, &tv, None).await),
        (ErrorCode::PcBusy, Some("device_id".into()))
    );
}

#[tokio::test]
async fn a_console_pack_and_the_wallet_start_a_timed_console_session() {
    let f = bootstrapped().await;
    let tv = pc(&f, DeviceKind::Console, "console", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &tv)], 60).await;
    begin(&f, &tv, Some(&p), SessionMode::Pack, Some(&pack))
        .await
        .unwrap();
    let s = &sessions(&f)[0];
    assert_eq!(text(s, "mode"), "pack");
    assert_eq!(int(s, "ends_at"), T0 + 60 * 60_000);

    let tv2 = pc(&f, DeviceKind::Console, "console", Some(6_000)).await;
    topped_up(&f, &p, 10_000).await;
    begin(&f, &tv2, Some(&p), SessionMode::Wallet, None)
        .await
        .unwrap();
    assert_eq!(balance(&f, &p), 10_000);
    assert_eq!(state_of(&f, &tv2), "in_session");
}

#[tokio::test]
async fn a_pc_pack_never_starts_on_a_console_nor_a_console_pack_on_a_pc() {
    let f = bootstrapped().await;
    let tv = pc(&f, DeviceKind::Console, "console", Some(6_000)).await;
    let desk = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    let pc_pack = owned_pack(&f, &p, &[group_of(&f, &desk)], 60).await;
    let console_pack = owned_pack(&f, &p, &[group_of(&f, &tv)], 60).await;
    let before = reopen(&f).changes().unwrap().len();
    assert_eq!(
        refusal(begin(&f, &tv, Some(&p), SessionMode::Pack, Some(&pc_pack)).await),
        (ErrorCode::PlanNotForGroup, Some("pack_id".into()))
    );
    assert_eq!(
        refusal(begin(&f, &desk, Some(&p), SessionMode::Pack, Some(&console_pack)).await),
        (ErrorCode::PlanNotForGroup, Some("pack_id".into()))
    );
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    assert!(sessions(&f).is_empty());
}

#[tokio::test]
async fn a_vr_rig_still_takes_no_session() {
    let f = bootstrapped().await;
    let vr = pc(&f, DeviceKind::Vr, "vr", Some(6_000)).await;
    assert_eq!(
        refusal(start(&f, &vr, None).await),
        (ErrorCode::ValidationFailed, Some("device_id".into()))
    );
}

async fn expire(f: &Fixture, session: &str) -> Result<Applied, AppError> {
    let mut i = intent(
        Actor {
            kind: ActorKind::System,
            id: "hub".into(),
        },
        IntentPayload::SessionExpire(SessionExpire {
            session_id: session.into(),
        }),
    );
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

async fn end_now(f: &Fixture, id: &str) {
    let rev = rev_of(f, Table::Sessions, id);
    end(f, end_intent(id, Some(rev))).await.unwrap();
}

async fn check(f: &Fixture, session: &str, result: ControllerResult) -> Result<Applied, AppError> {
    let mut i = intent(
        owner(),
        IntentPayload::ControllerCheck(ControllerCheck {
            session_id: session.into(),
            result,
        }),
    );
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

async fn ended_console_session(f: &Fixture) -> (String, String) {
    let tv = pc(f, DeviceKind::Console, "console", Some(12_000)).await;
    start(f, &tv, None).await.unwrap();
    let id = text(&sessions(f)[0], "id");
    f.clock.advance(45 * 60_000);
    end_now(f, &id).await;
    (tv, id)
}

#[tokio::test]
async fn a_console_at_its_end_time_has_no_grace_and_holds_a_time_up_alert() {
    let f = bootstrapped().await;
    let tv = pc(&f, DeviceKind::Console, "console", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &tv)], 60).await;
    begin(&f, &tv, Some(&p), SessionMode::Pack, Some(&pack))
        .await
        .unwrap();
    let s = &sessions(&f)[0];
    assert_eq!(int(s, "grace_min"), 0);
    assert_eq!(text(s, "time_up_mode"), "staff_handles");
    let id = text(s, "id");
    f.clock.advance(60 * 60_000);
    expire(&f, &id).await.unwrap();
    let s = &sessions(&f)[0];
    assert_eq!(text(s, "state"), "expired");
    assert!(matches!(s["ended_at"], RowValue::Null));
    let alerts = table(&f, Table::Alerts);
    assert_eq!(alerts.len(), 1);
    assert_eq!(text(&alerts[0], "kind"), "time_up");
    assert_eq!(text(&alerts[0], "device_id"), tv);
    assert_eq!(state_of(&f, &tv), "in_session");
    end_now(&f, &id).await;
    assert!(!matches!(sessions(&f)[0]["ended_at"], RowValue::Null));
    assert_ne!(state_of(&f, &tv), "in_session");
}

#[tokio::test]
async fn a_console_session_found_in_grace_state_still_holds_without_waiting() {
    let f = bootstrapped().await;
    let tv = pc(&f, DeviceKind::Console, "console", Some(6_000)).await;
    start(&f, &tv, None).await.unwrap();
    let id = text(&sessions(&f)[0], "id");
    let at = f.clock.now_ms() + 60_000;
    sql(
        &f,
        format!("UPDATE sessions SET ends_at = {at}, grace_min = 2, time_up_mode = 'clean_up' WHERE id = '{id}'"),
    )
    .await;
    f.clock.advance(60_000);
    expire(&f, &id).await.unwrap();
    assert_eq!(text(&sessions(&f)[0], "state"), "expired");
}

#[tokio::test]
async fn an_ended_console_session_blocks_the_next_start_until_its_controllers_are_marked() {
    let f = bootstrapped().await;
    let (tv, id) = ended_console_session(&f).await;
    assert_eq!(
        refusal(start(&f, &tv, None).await),
        (ErrorCode::ControllersUnchecked, Some("device_id".into()))
    );
    let a = check(&f, &id, ControllerResult::Returned).await.unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        ["sessions"]
    );
    let s = &sessions(&f)[0];
    assert_eq!(text(s, "controller_check"), "returned");
    assert_eq!(text(s, "controller_checked_by"), "o");
    assert_eq!(int(s, "controller_checked_at"), f.clock.now_ms());
    assert!(table(&f, Table::Alerts).is_empty());
    start(&f, &tv, None).await.unwrap();
}

#[tokio::test]
async fn missing_controllers_raise_an_alert_for_the_manager() {
    let f = bootstrapped().await;
    let (tv, id) = ended_console_session(&f).await;
    let a = check(&f, &id, ControllerResult::Missing).await.unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        ["sessions", "alerts"]
    );
    assert_eq!(text(&sessions(&f)[0], "controller_check"), "missing");
    let alerts = table(&f, Table::Alerts);
    assert_eq!(text(&alerts[0], "kind"), "controller_missing");
    assert_eq!(text(&alerts[0], "device_id"), tv);
    start(&f, &tv, None).await.unwrap();
}

#[tokio::test]
async fn a_controller_check_is_refused_when_it_does_not_apply() {
    let f = bootstrapped().await;
    let tv = pc(&f, DeviceKind::Console, "console", Some(12_000)).await;
    start(&f, &tv, None).await.unwrap();
    let running = text(&sessions(&f)[0], "id");
    assert_eq!(
        refusal(check(&f, &running, ControllerResult::Returned).await),
        (ErrorCode::ValidationFailed, Some("session_id".into()))
    );
    end_now(&f, &running).await;
    check(&f, &running, ControllerResult::Returned)
        .await
        .unwrap();
    assert_eq!(
        check(&f, &running, ControllerResult::Missing)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Stale
    );
    assert_eq!(
        refusal(check(&f, "nope", ControllerResult::Returned).await).0,
        ErrorCode::NotFound
    );
    let desk = pc(&f, DeviceKind::Pc, "pc", Some(12_000)).await;
    start(&f, &desk, None).await.unwrap();
    let pc_session = text(sessions(&f).last().unwrap(), "id");
    end_now(&f, &pc_session).await;
    assert_eq!(
        refusal(check(&f, &pc_session, ControllerResult::Returned).await).0,
        ErrorCode::ValidationFailed
    );
    start(&f, &desk, None).await.unwrap();
}
