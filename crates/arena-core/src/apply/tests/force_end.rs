use super::prepaid::{
    begin_block, group_of, ist, order_of, owned_pack, pack_left, pack_session, wallet_session,
    window_plan,
};
use super::session::{
    balance, end, end_intent, int, pc, player, refusal, rev_of, row_of, sessions, start, state_of,
    table, text, topped_up,
};
use super::stock::{intent, owner};
use super::*;
use arena_proto::{
    DeviceKind, ReasonCode, SessionForceEnd, SessionForceEndReverted, SessionMode, UNDO_WINDOW_MS,
};

const MIN: i64 = 60_000;

fn force_intent(
    session: &str,
    rev: Option<i64>,
    reason: Option<ReasonCode>,
    note: Option<&str>,
) -> Intent {
    let mut i = intent(
        owner(),
        IntentPayload::SessionForceEnd(SessionForceEnd {
            session_id: session.into(),
            note: note.map(Into::into),
        }),
    );
    i.base_rev = rev;
    i.reason = reason;
    i
}

fn revert_intent(session: &str, rev: Option<i64>) -> Intent {
    let mut i = intent(
        owner(),
        IntentPayload::SessionForceEndReverted(SessionForceEndReverted {
            session_id: session.into(),
        }),
    );
    i.base_rev = rev;
    i
}

async fn force(f: &Fixture, session: &str) -> Result<Applied, AppError> {
    let rev = rev_of(f, Table::Sessions, session);
    end(
        f,
        force_intent(session, Some(rev), Some(ReasonCode::Closing), None),
    )
    .await
}

async fn unforce(f: &Fixture, session: &str) -> Result<Applied, AppError> {
    let rev = rev_of(f, Table::Sessions, session);
    end(f, revert_intent(session, Some(rev))).await
}

fn has_order(f: &Fixture, session: &str) -> bool {
    table(f, Table::Orders)
        .iter()
        .any(|o| o["session_id"] == RowValue::Text(session.into()))
}

#[tokio::test]
async fn a_forced_end_needs_a_reason_and_a_note_only_for_other() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6000)).await;
    start(&f, &d, None).await.unwrap();
    let session = text(sessions(&f).last().unwrap(), "id");
    let rev = rev_of(&f, Table::Sessions, &session);
    let before = table(&f, Table::Sessions);
    assert_eq!(
        refusal(end(&f, force_intent(&session, Some(rev), None, None)).await),
        (ErrorCode::ReasonRequired, Some("reason".into()))
    );
    for (reason, note) in [
        (ReasonCode::Other, None),
        (ReasonCode::Other, Some("  ")),
        (ReasonCode::Closing, Some("late")),
    ] {
        assert_eq!(
            refusal(end(&f, force_intent(&session, Some(rev), Some(reason), note)).await),
            (ErrorCode::ValidationFailed, Some("note".into()))
        );
    }
    assert_eq!(
        refusal(
            end(
                &f,
                force_intent(&session, Some(rev), Some(ReasonCode::Lost), None)
            )
            .await
        ),
        (ErrorCode::ValidationFailed, Some("reason".into()))
    );
    assert_eq!(table(&f, Table::Sessions), before);
    end(
        &f,
        force_intent(
            &session,
            Some(rev),
            Some(ReasonCode::Other),
            Some(" Fight over a seat "),
        ),
    )
    .await
    .unwrap();
    let s = row_of(&f, Table::Sessions, &session);
    assert_eq!(text(&s, "state"), "ended");
    assert_eq!(text(&s, "end_reason"), "forced");
    assert_eq!(text(&s, "forced_reason"), "other");
    assert_eq!(text(&s, "forced_note"), "Fight over a seat");
}

#[tokio::test]
async fn a_forced_end_of_a_postpaid_session_bills_the_time_used_and_undoes() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6000)).await;
    start(&f, &d, None).await.unwrap();
    let session = text(sessions(&f).last().unwrap(), "id");
    f.clock.advance(30 * MIN);
    let a = force(&f, &session).await.unwrap();
    assert!(a.changes.iter().any(|c| c.tbl == "orders"));
    let s = row_of(&f, Table::Sessions, &session);
    assert_eq!(text(&s, "end_reason"), "forced");
    assert_eq!(text(&s, "forced_reason"), "closing");
    assert_eq!(int(&s, "forced_left_min"), 0);
    let order = order_of(&f, &session);
    assert_eq!(text(&order, "status"), "open");
    assert_eq!(int(&order, "total_minor"), 3000);
    assert_ne!(state_of(&f, &d), "in_session");
    assert!(!reopen(&f).ending(&session).unwrap().unwrap().irreversible);

    unforce(&f, &session).await.unwrap();
    let s = row_of(&f, Table::Sessions, &session);
    assert_eq!(text(&s, "state"), "active");
    assert_eq!(s["end_reason"], RowValue::Null);
    assert_eq!(s["forced_reason"], RowValue::Null);
    assert!(!has_order(&f, &session));
    assert_eq!(state_of(&f, &d), "in_session");
}

#[tokio::test]
async fn a_forced_end_of_a_pack_session_deducts_only_the_time_used_and_the_undo_gives_it_back() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let p = player(&f, "asha", "+919876543210").await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 90).await;
    let session = pack_session(&f, &d, &p, &pack).await;
    f.clock.advance(12 * MIN + 10_000);
    force(&f, &session).await.unwrap();
    assert_eq!(pack_left(&f, &pack), 77);
    let s = row_of(&f, Table::Sessions, &session);
    assert_eq!(int(&s, "forced_left_min"), 78);
    assert_eq!(int(&s, "forced_pack_min"), 13);
    assert_eq!(text(&order_of(&f, &session), "status"), "settled");
    assert!(!reopen(&f).ending(&session).unwrap().unwrap().irreversible);

    unforce(&f, &session).await.unwrap();
    assert_eq!(pack_left(&f, &pack), 90);
    assert_eq!(
        text(&row_of(&f, Table::Sessions, &session), "state"),
        "active"
    );
    assert_eq!(state_of(&f, &d), "in_session");
    assert!(!has_order(&f, &session));
}

#[tokio::test]
async fn a_forced_end_of_a_wallet_session_pays_for_the_time_used_and_the_undo_refunds_it() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 20_000).await;
    let session = wallet_session(&f, &d, &p).await;
    f.clock.advance(30 * MIN);
    force(&f, &session).await.unwrap();
    assert_eq!(balance(&f, &p), 17_000);
    let order = text(&order_of(&f, &session), "id");
    assert!(
        table(&f, Table::Payments)
            .iter()
            .any(|r| r["order_id"] == RowValue::Text(order.clone()))
    );

    unforce(&f, &session).await.unwrap();
    assert_eq!(balance(&f, &p), 20_000);
    assert_eq!(
        text(&row_of(&f, Table::Sessions, &session), "state"),
        "active"
    );
    assert!(!has_order(&f, &session));
    assert!(
        table(&f, Table::Payments)
            .iter()
            .all(|r| r["order_id"] != RowValue::Text(order.clone()))
    );
}

#[tokio::test]
async fn a_forced_block_session_ends_and_comes_back_with_its_bill_kept() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let p = player(&f, "asha", "+919876543210").await;
    let plan = window_plan(&f, "block", &[group_of(&f, &d)], 20_000).await;
    f.clock.set(ist("2027-01-16 03:00"));
    begin_block(&f, &d, Some(&p), &plan, SessionMode::Block)
        .await
        .unwrap();
    let session = text(&sessions(&f)[0], "id");
    let order = text(&order_of(&f, &session), "id");
    f.clock.advance(MIN);
    force(&f, &session).await.unwrap();
    let s = row_of(&f, Table::Sessions, &session);
    assert_eq!(text(&s, "state"), "ended");
    assert!(int(&s, "forced_left_min") > 170);
    assert_ne!(state_of(&f, &d), "in_session");
    unforce(&f, &session).await.unwrap();
    assert_eq!(
        text(&row_of(&f, Table::Sessions, &session), "state"),
        "active"
    );
    assert_eq!(text(&order_of(&f, &session), "id"), order);
    assert_eq!(state_of(&f, &d), "in_session");
}

#[tokio::test]
async fn only_a_forced_end_undoes_as_one_and_only_within_the_window() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6000)).await;
    start(&f, &d, None).await.unwrap();
    let session = text(sessions(&f).last().unwrap(), "id");
    let rev = rev_of(&f, Table::Sessions, &session);
    end(&f, end_intent(&session, Some(rev))).await.unwrap();
    let err = unforce(&f, &session).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::UndoNotAllowed);

    let d2 = pc(&f, DeviceKind::Pc, "pc", Some(6000)).await;
    start(&f, &d2, None).await.unwrap();
    let second = text(sessions(&f).last().unwrap(), "id");
    force(&f, &second).await.unwrap();
    let stale = rev_of(&f, Table::Sessions, &second) + 1;
    assert_eq!(
        refusal(end(&f, revert_intent(&second, Some(stale))).await).0,
        ErrorCode::Stale
    );
    f.clock.advance(UNDO_WINDOW_MS + 1);
    let err = unforce(&f, &second).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::UndoNotAllowed);
    assert_eq!(
        text(&row_of(&f, Table::Sessions, &second), "state"),
        "ended"
    );
}
