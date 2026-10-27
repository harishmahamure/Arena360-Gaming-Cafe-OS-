use super::prepaid::{
    begin, begin_pass, expire, group_of, ist, order_of, owned_pack, owned_pass, pack_left,
    pack_session, sql, stop, wallet_session,
};
use super::session::{
    balance, credit_intent, int, ledger_sum, pay, pc, player, refusal, row_of, sessions,
    settle_intent, state_of, submit, table, text, topped_up,
};
use super::stock::{intent, owner};
use super::*;
use arena_proto::{DeviceKind, ExtendSource, SessionExtend, SessionMode};

const MIN: i64 = 60_000;

fn extend_intent(
    session: &str,
    minutes: i64,
    source: ExtendSource,
    pack: Option<&str>,
    over: bool,
) -> Intent {
    intent(
        owner(),
        IntentPayload::SessionExtend(SessionExtend {
            session_id: session.into(),
            minutes,
            source,
            pack_id: pack.map(Into::into),
            credit_override: over.then_some(true),
        }),
    )
}

async fn extend(
    f: &Fixture,
    session: &str,
    minutes: i64,
    source: ExtendSource,
    pack: Option<&str>,
) -> Result<Applied, AppError> {
    let mut i = extend_intent(session, minutes, source, pack, false);
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

fn ends_at(f: &Fixture, session: &str) -> i64 {
    int(&row_of(f, Table::Sessions, session), "ends_at")
}

fn extension_orders(f: &Fixture, session: &str) -> Vec<Row> {
    table(f, Table::Orders)
        .into_iter()
        .filter(|o| matches!(&o["extends_session_id"], RowValue::Text(t) if t == session))
        .collect()
}

fn tables(a: &Applied) -> Vec<&str> {
    a.changes.iter().map(|c| c.tbl.as_str()).collect()
}

#[tokio::test]
async fn extending_a_pack_session_from_the_wallet_moves_the_end_and_debits_the_wallet() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 60).await;
    let s = pack_session(&f, &d, &p, &pack).await;
    let a = extend(&f, &s, 60, ExtendSource::Wallet, None)
        .await
        .unwrap();
    assert_eq!(
        tables(&a),
        [
            "sessions",
            "orders",
            "order_lines",
            "payments",
            "wallet_txns",
            "wallets"
        ]
    );
    assert_eq!(ends_at(&f, &s), T0 + 120 * MIN);
    let row = row_of(&f, Table::Sessions, &s);
    assert_eq!(int(&row, "extended_min"), 60);
    assert_eq!(text(&row, "state"), "active");
    let o = &extension_orders(&f, &s)[0];
    assert_eq!(text(o, "status"), "settled");
    assert_eq!(text(o, "kind"), "session");
    assert_eq!(o["session_id"], RowValue::Null);
    assert_eq!(int(o, "total_minor"), 6_000);
    assert_eq!(text(o, "settled_by"), "o");
    let line = table(&f, Table::OrderLines)
        .into_iter()
        .find(|l| text(l, "order_id") == text(o, "id"))
        .unwrap();
    assert_eq!(int(&line, "from_at"), T0 + 60 * MIN);
    assert_eq!(int(&line, "to_at"), T0 + 120 * MIN);
    assert_eq!(int(&line, "minutes"), 60);
    assert_eq!(int(&line, "amount_minor"), 6_000);
    assert_eq!(balance(&f, &p), 4_000);
    assert_eq!(ledger_sum(&f, &p), 4_000);
    assert_eq!(pack_left(&f, &pack), 60);
    assert_eq!(state_of(&f, &d), "in_session");
}

#[tokio::test]
async fn a_thirty_minute_extend_is_priced_half_up_at_the_groups_live_rate() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(5_001)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 20_000).await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 30).await;
    let s = pack_session(&f, &d, &p, &pack).await;
    extend(&f, &s, 30, ExtendSource::Wallet, None)
        .await
        .unwrap();
    assert_eq!(balance(&f, &p), 20_000 - 2_501);
    extend(&f, &s, 120, ExtendSource::Wallet, None)
        .await
        .unwrap();
    assert_eq!(balance(&f, &p), 20_000 - 2_501 - 10_002);
    assert_eq!(ends_at(&f, &s), T0 + 180 * MIN);
}

#[tokio::test]
async fn a_session_in_its_grace_period_returns_to_active_when_extended() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 30).await;
    let s = pack_session(&f, &d, &p, &pack).await;
    f.clock.set(T0 + 31 * MIN);
    sql(
        &f,
        format!("UPDATE sessions SET state = 'grace' WHERE id = '{s}'"),
    )
    .await;
    extend(&f, &s, 30, ExtendSource::Wallet, None)
        .await
        .unwrap();
    let row = row_of(&f, Table::Sessions, &s);
    assert_eq!(text(&row, "state"), "active");
    assert_eq!(int(&row, "ends_at"), T0 + 60 * MIN);
}

#[tokio::test]
async fn extending_from_another_pack_takes_its_minutes_and_charges_nothing() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    let group = vec![group_of(&f, &d)];
    let running = owned_pack(&f, &p, &group, 60).await;
    let spare = owned_pack(&f, &p, &group, 100).await;
    let s = pack_session(&f, &d, &p, &running).await;
    let a = extend(&f, &s, 60, ExtendSource::Pack, Some(&spare))
        .await
        .unwrap();
    assert_eq!(
        tables(&a),
        ["sessions", "orders", "order_lines", "player_packs"]
    );
    assert_eq!(pack_left(&f, &spare), 40);
    assert_eq!(pack_left(&f, &running), 60);
    assert_eq!(ends_at(&f, &s), T0 + 120 * MIN);
    let o = &extension_orders(&f, &s)[0];
    assert_eq!(int(o, "total_minor"), 0);
    assert_eq!(text(o, "status"), "settled");
    f.clock.advance(100 * MIN);
    stop(&f, &s).await.unwrap();
    assert_eq!(pack_left(&f, &running), 0);
    assert_eq!(pack_left(&f, &spare), 40);
}

#[tokio::test]
async fn a_pack_source_is_refused_for_every_unusable_pack() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let other = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    let q = player(&f, "ben", "+919876543211").await;
    let group = vec![group_of(&f, &d)];
    let running = owned_pack(&f, &p, &group, 60).await;
    let s = pack_session(&f, &d, &p, &running).await;
    let wrong_group = owned_pack(&f, &p, &[group_of(&f, &other)], 90).await;
    let expired = owned_pack(&f, &p, &group, 90).await;
    sql(
        &f,
        format!("UPDATE player_packs SET expires_at = {T0} WHERE id = '{expired}'"),
    )
    .await;
    let short = owned_pack(&f, &p, &group, 20).await;
    let empty = owned_pack(&f, &p, &group, 0).await;
    let foreign = owned_pack(&f, &q, &group, 90).await;
    let busy = owned_pack(&f, &p, &[group_of(&f, &d), group_of(&f, &other)], 90).await;
    pack_session(&f, &other, &p, &busy).await;
    let field = |c, f: &str| (c, Some(f.to_owned()));
    let before = reopen(&f).changes().unwrap().len();
    let go = |pack: Option<&str>| {
        let (f, s) = (&f, &s);
        let pack = pack.map(str::to_owned);
        async move { extend(f, s, 30, ExtendSource::Pack, pack.as_deref()).await }
    };
    assert_eq!(
        refusal(go(Some(&wrong_group)).await),
        field(ErrorCode::PlanNotForGroup, "pack_id")
    );
    assert_eq!(
        refusal(go(Some(&expired)).await),
        field(ErrorCode::PackExpired, "pack_id")
    );
    assert_eq!(
        refusal(go(Some(&empty)).await),
        field(ErrorCode::PackExpired, "pack_id")
    );
    let e = go(Some(&short)).await.unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::InsufficientBalance, Some("pack_id"))
    );
    assert!(e.message.contains("20 min"), "{}", e.message);
    assert_eq!(
        refusal(go(Some(&foreign)).await),
        field(ErrorCode::NotFound, "pack_id")
    );
    assert_eq!(
        refusal(go(Some("NOPE")).await),
        field(ErrorCode::NotFound, "pack_id")
    );
    assert_eq!(
        refusal(go(None).await),
        field(ErrorCode::ValidationFailed, "pack_id")
    );
    assert_eq!(
        refusal(go(Some(&busy)).await),
        field(ErrorCode::ValidationFailed, "pack_id")
    );
    assert_eq!(
        refusal(go(Some(&running)).await),
        field(ErrorCode::ValidationFailed, "pack_id")
    );
    assert_eq!(
        refusal(extend(&f, &s, 30, ExtendSource::Wallet, Some(&running)).await),
        field(ErrorCode::ValidationFailed, "pack_id")
    );
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    assert_eq!(ends_at(&f, &s), T0 + 60 * MIN);
}

#[tokio::test]
async fn a_wallet_source_short_of_the_price_is_refused_with_the_balance() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 60).await;
    let s = pack_session(&f, &d, &p, &pack).await;
    topped_up(&f, &p, 2_900).await;
    let e = extend(&f, &s, 30, ExtendSource::Wallet, None)
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::InsufficientBalance, Some("source"))
    );
    assert!(
        e.message.starts_with("Balance ₹29.00. Top up at counter."),
        "{}",
        e.message
    );
    assert_eq!(ends_at(&f, &s), T0 + 60 * MIN);
    assert_eq!(balance(&f, &p), 2_900);
}

#[tokio::test]
async fn a_wallet_session_extends_only_with_what_its_reservation_leaves() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    let s = wallet_session(&f, &d, &p).await;
    assert_eq!(
        int(&row_of(&f, Table::Sessions, &s), "reserved_minor"),
        9_000
    );
    let e = extend(&f, &s, 30, ExtendSource::Wallet, None)
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::InsufficientBalance);
    assert!(e.message.starts_with("Balance ₹10.00."), "{}", e.message);
    topped_up(&f, &p, 3_000).await;
    extend(&f, &s, 30, ExtendSource::Wallet, None)
        .await
        .unwrap();
    assert_eq!(balance(&f, &p), 10_000);
    assert_eq!(ends_at(&f, &s), T0 + 120 * MIN);
}

#[tokio::test]
async fn a_wallet_session_is_billed_only_up_to_its_original_end() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    let group = vec![group_of(&f, &d)];
    topped_up(&f, &p, 10_000).await;
    let spare = owned_pack(&f, &p, &group, 60).await;
    let s = wallet_session(&f, &d, &p).await;
    extend(&f, &s, 30, ExtendSource::Pack, Some(&spare))
        .await
        .unwrap();
    assert_eq!(ends_at(&f, &s), T0 + 120 * MIN);
    f.clock.advance(110 * MIN);
    stop(&f, &s).await.unwrap();
    let o = order_of(&f, &s);
    assert_eq!(int(&o, "total_minor"), 9_000);
    assert_eq!(balance(&f, &p), 1_000);
    let lines: Vec<_> = table(&f, Table::OrderLines)
        .into_iter()
        .filter(|l| text(l, "order_id") == text(&o, "id"))
        .collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(int(&lines[0], "minutes"), 90);
    assert_eq!(int(&lines[0], "to_at"), T0 + 90 * MIN);
}

#[tokio::test]
async fn a_wallet_session_ended_before_its_original_end_bills_normally() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    let spare = owned_pack(&f, &p, &[group_of(&f, &d)], 60).await;
    let s = wallet_session(&f, &d, &p).await;
    extend(&f, &s, 30, ExtendSource::Pack, Some(&spare))
        .await
        .unwrap();
    f.clock.advance(41 * MIN);
    stop(&f, &s).await.unwrap();
    assert_eq!(int(&order_of(&f, &s), "total_minor"), 4_500);
    assert_eq!(balance(&f, &p), 5_500);
    assert_eq!(pack_left(&f, &spare), 30);
}

#[tokio::test]
async fn an_extended_session_that_times_up_closes_out_at_the_new_end() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    let spare = owned_pack(&f, &p, &[group_of(&f, &d)], 60).await;
    let free = state_of(&f, &d);
    let s = wallet_session(&f, &d, &p).await;
    extend(&f, &s, 30, ExtendSource::Pack, Some(&spare))
        .await
        .unwrap();
    let due = ends_at(&f, &s);
    f.clock.set(due);
    expire(&f, &s, due).await.unwrap();
    let row = row_of(&f, Table::Sessions, &s);
    assert_eq!(text(&row, "state"), "expired");
    assert_eq!(int(&order_of(&f, &s), "total_minor"), 9_000);
    assert_eq!(state_of(&f, &d), free);
}

#[tokio::test]
async fn extending_on_credit_adds_to_the_outstanding_balance() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 60).await;
    let s = pack_session(&f, &d, &p, &pack).await;
    let e = extend(&f, &s, 60, ExtendSource::Credit, None)
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::ValidationFailed, Some("source"))
    );
    submit_credit(&f, &p, 10_000).await;
    let a = extend(&f, &s, 60, ExtendSource::Credit, None)
        .await
        .unwrap();
    assert_eq!(
        tables(&a),
        [
            "sessions",
            "orders",
            "order_lines",
            "payments",
            "wallet_txns",
            "credit_balances"
        ]
    );
    let credit = &table(&f, Table::CreditBalances)[0];
    assert_eq!(int(credit, "outstanding_minor"), 6_000);
    assert_eq!(ends_at(&f, &s), T0 + 120 * MIN);
    let e = extend(&f, &s, 120, ExtendSource::Credit, None)
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::CreditLimit, Some("source"))
    );
    assert_eq!(ends_at(&f, &s), T0 + 120 * MIN);
}

async fn submit_credit(f: &Fixture, player: &str, limit: i64) {
    submit(
        f,
        owner(),
        credit_intent(player, limit, &[3, 7, 14]).payload,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn a_credit_extend_over_the_limit_needs_the_override_once_a_day() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 60).await;
    let s = pack_session(&f, &d, &p, &pack).await;
    submit_credit(&f, &p, 5_000).await;
    let mut over = extend_intent(&s, 60, ExtendSource::Credit, None, true);
    over.issued_at = f.clock.now_ms();
    f.writer.submit_intent(over.clone()).await.unwrap();
    let credit = &table(&f, Table::CreditBalances)[0];
    assert_eq!(int(credit, "outstanding_minor"), 6_000);
    assert!(matches!(&credit["last_override_day"], RowValue::Text(_)));
    over.intent_id = ulid::Ulid::generate().to_string();
    over.idempotency_key = ulid::Ulid::generate().to_string();
    let e = f.writer.submit_intent(over).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::OverrideUsedToday);
}

#[tokio::test]
async fn a_paused_credit_refuses_an_extend() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 60).await;
    let s = pack_session(&f, &d, &p, &pack).await;
    submit_credit(&f, &p, 50_000).await;
    sql(
        &f,
        format!("UPDATE credit_balances SET credit_paused = 1 WHERE player_id = '{p}'"),
    )
    .await;
    let e = extend(&f, &s, 30, ExtendSource::Credit, None)
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::CreditPaused, Some("source"))
    );
}

#[tokio::test]
async fn an_extend_past_closing_time_is_refused_and_nothing_is_written() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 20_000).await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 60).await;
    let open = r#"{"open_min":600,"close_min":1380}"#;
    let week = [open; 7].join(",");
    sql(
        &f,
        format!("UPDATE settings SET value = '[{week}]' WHERE key = 'branch.opening_hours'"),
    )
    .await;
    f.clock.set(ist("2027-01-15 21:30"));
    let s = pack_session(&f, &d, &p, &pack).await;
    let before = reopen(&f).changes().unwrap().len();
    let e = extend(&f, &s, 120, ExtendSource::Wallet, None)
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::PastClosing, Some("minutes"))
    );
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    assert_eq!(balance(&f, &p), 20_000);
    extend(&f, &s, 30, ExtendSource::Wallet, None)
        .await
        .unwrap();
    assert_eq!(ends_at(&f, &s), ist("2027-01-15 23:00"));
}

#[tokio::test]
async fn the_extend_options_and_the_session_are_checked() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let q = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 50_000).await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 60).await;
    let s = pack_session(&f, &d, &p, &pack).await;
    let field = |c, f: &str| (c, Some(f.to_owned()));
    for minutes in [0, 15, 45, 90, -30, 121] {
        assert_eq!(
            refusal(extend(&f, &s, minutes, ExtendSource::Wallet, None).await),
            field(ErrorCode::ValidationFailed, "minutes"),
            "{minutes}"
        );
    }
    assert_eq!(
        refusal(extend(&f, "NOPE", 30, ExtendSource::Wallet, None).await),
        field(ErrorCode::NotFound, "session_id")
    );
    let mut with_rev = extend_intent(&s, 30, ExtendSource::Wallet, None, false);
    with_rev.base_rev = Some(0);
    with_rev.issued_at = f.clock.now_ms();
    assert_eq!(
        refusal(f.writer.submit_intent(with_rev).await),
        field(ErrorCode::ValidationFailed, "base_rev")
    );
    begin(&f, &q, None, SessionMode::Postpaid, None)
        .await
        .unwrap();
    let postpaid = text(sessions(&f).last().unwrap(), "id");
    assert_eq!(
        refusal(extend(&f, &postpaid, 30, ExtendSource::Wallet, None).await),
        field(ErrorCode::ValidationFailed, "session_id")
    );
    stop(&f, &s).await.unwrap();
    assert_eq!(
        refusal(extend(&f, &s, 30, ExtendSource::Wallet, None).await),
        field(ErrorCode::Stale, "session_id")
    );
    assert_eq!(balance(&f, &p), 50_000);
}

#[tokio::test]
async fn a_pass_session_cannot_be_extended() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 50_000).await;
    let pass = owned_pass(&f, &p, &[group_of(&f, &d)]).await;
    f.clock.set(ist("2027-01-15 23:30"));
    begin_pass(&f, &d, &p, &pass).await.unwrap();
    let s = text(sessions(&f).last().unwrap(), "id");
    assert_eq!(
        refusal(extend(&f, &s, 30, ExtendSource::Wallet, None).await),
        (ErrorCode::ValidationFailed, Some("session_id".into()))
    );
}

#[tokio::test]
async fn a_held_expired_session_cannot_be_extended() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 50_000).await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 30).await;
    let s = pack_session(&f, &d, &p, &pack).await;
    sql(
        &f,
        format!("UPDATE sessions SET state = 'expired', end_reason = 'expired' WHERE id = '{s}'"),
    )
    .await;
    assert_eq!(
        refusal(extend(&f, &s, 30, ExtendSource::Wallet, None).await),
        (ErrorCode::Stale, Some("session_id".into()))
    );
}

#[tokio::test]
async fn an_extension_bill_cannot_be_undone_as_a_session_settle() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 60).await;
    let s = pack_session(&f, &d, &p, &pack).await;
    extend(&f, &s, 30, ExtendSource::Wallet, None)
        .await
        .unwrap();
    let order = text(&extension_orders(&f, &s)[0], "id");
    let mut undo = intent(
        owner(),
        IntentPayload::SessionSettleReverted(arena_proto::SessionSettleReverted {
            order_id: order.clone(),
        }),
    );
    undo.base_rev = Some(int(&row_of(&f, Table::Orders, &order), "rev"));
    undo.issued_at = f.clock.now_ms();
    assert_eq!(
        refusal(f.writer.submit_intent(undo).await),
        (ErrorCode::ValidationFailed, Some("order_id".into()))
    );
    let rev = Some(int(&row_of(&f, Table::Orders, &order), "rev"));
    let mut again = settle_intent(
        &order,
        rev,
        vec![pay(arena_proto::PaymentMethod::Cash, 3_000, None)],
    );
    again.issued_at = f.clock.now_ms();
    assert_eq!(
        refusal(f.writer.submit_intent(again).await),
        (ErrorCode::Stale, Some("order_id".into()))
    );
}

#[tokio::test]
async fn a_retried_extend_with_the_same_key_applies_once() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 60).await;
    let s = pack_session(&f, &d, &p, &pack).await;
    let mut i = extend_intent(&s, 30, ExtendSource::Wallet, None, false);
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i.clone()).await.unwrap();
    f.writer.submit_intent(i).await.unwrap();
    assert_eq!(extension_orders(&f, &s).len(), 1);
    assert_eq!(ends_at(&f, &s), T0 + 90 * MIN);
    assert_eq!(balance(&f, &p), 7_000);
}
