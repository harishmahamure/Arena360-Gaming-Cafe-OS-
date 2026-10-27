use super::prepaid::{begin, group_of, owned_pack};
use super::session::{
    balance, end, end_intent, int, pc, player, refusal, rev_of, sessions, start, state_of, table,
    text, topped_up,
};
use super::*;
use arena_proto::{DeviceKind, SessionMode};

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
