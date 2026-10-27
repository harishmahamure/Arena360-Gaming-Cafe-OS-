use super::session::{
    balance, collect_intent, credit_intent, end, end_intent, int, ledger_sum, pay, pc, player,
    refusal, rev_of, row_of, sale_order, sell_intent, sell_revert_intent, sessions, state_of,
    submit, table, text, topped_up, topup_revert_intent,
};
use super::stock::{intent, owner};
use super::*;
use arena_proto::PaymentMethod::{Cash, Wallet};
use arena_proto::{DeviceKind, PlanUpsert, SessionExpire, SessionMode, SessionStart};

const MIN: i64 = 60_000;

fn system() -> Actor {
    Actor {
        kind: ActorKind::System,
        id: "hub".into(),
    }
}

async fn sql(f: &Fixture, statement: String) {
    f.writer
        .submit(move |tx| {
            tx.execute_batch(&statement).unwrap();
            Ok(())
        })
        .await
        .unwrap();
}

fn group_of(f: &Fixture, device: &str) -> String {
    table(f, Table::DevicesMeta)
        .iter()
        .find(|r| text(r, "device_id") == device)
        .map(|r| text(r, "group_id"))
        .unwrap()
}

async fn plan_for(f: &Fixture, groups: &[String], price: i64) -> String {
    submit(
        f,
        owner(),
        IntentPayload::PlanUpsert(PlanUpsert {
            id: None,
            plan_kind: "pack".into(),
            name: "5 h".into(),
            price_minor: price,
            minutes: Some(300),
            validity_days: Some(7),
            window_start_min: None,
            window_end_min: None,
            group_ids: groups.to_vec(),
            active: true,
        }),
    )
    .await
    .unwrap()
    .changes
    .iter()
    .find(|c| c.tbl == "plans")
    .unwrap()
    .row_id
    .clone()
}

async fn owned_pack(f: &Fixture, player: &str, groups: &[String], left: i64) -> String {
    let plan = plan_for(f, groups, 30_000).await;
    end(f, sell_intent(player, &plan, vec![pay(Cash, 30_000, None)]))
        .await
        .unwrap();
    let id = text(table(f, Table::PlayerPacks).last().unwrap(), "id");
    sql(
        f,
        format!("UPDATE player_packs SET minutes_left = {left} WHERE id = '{id}'"),
    )
    .await;
    id
}

fn start_as(device: &str, player: Option<&str>, mode: SessionMode, pack: Option<&str>) -> Intent {
    let mut i = intent(
        owner(),
        IntentPayload::SessionStart(SessionStart {
            device_id: device.into(),
            mode,
            player_id: player.map(Into::into),
            pack_id: pack.map(Into::into),
        }),
    );
    i.issued_at = T0;
    i
}

async fn begin(
    f: &Fixture,
    device: &str,
    player: Option<&str>,
    mode: SessionMode,
    pack: Option<&str>,
) -> Result<Applied, AppError> {
    let mut i = start_as(device, player, mode, pack);
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

async fn pack_session(f: &Fixture, device: &str, player: &str, pack: &str) -> String {
    begin(f, device, Some(player), SessionMode::Pack, Some(pack))
        .await
        .unwrap();
    text(sessions(f).last().unwrap(), "id")
}

async fn wallet_session(f: &Fixture, device: &str, player: &str) -> String {
    begin(f, device, Some(player), SessionMode::Wallet, None)
        .await
        .unwrap();
    text(sessions(f).last().unwrap(), "id")
}

async fn expire(f: &Fixture, session: &str, at: i64) -> Result<Applied, AppError> {
    let mut i = intent(
        system(),
        IntentPayload::SessionExpire(SessionExpire {
            session_id: session.into(),
        }),
    );
    i.issued_at = at;
    f.writer.submit_intent(i).await
}

async fn stop(f: &Fixture, session: &str) -> Result<Applied, AppError> {
    let rev = rev_of(f, Table::Sessions, session);
    end(f, end_intent(session, Some(rev))).await
}

fn order_of(f: &Fixture, session: &str) -> Row {
    table(f, Table::Orders)
        .into_iter()
        .find(|o| matches!(&o["session_id"], RowValue::Text(t) if t == session))
        .unwrap()
}

fn pack_left(f: &Fixture, pack: &str) -> i64 {
    int(&row_of(f, Table::PlayerPacks, pack), "minutes_left")
}

#[tokio::test]
async fn a_pack_session_ends_when_the_minutes_left_run_out() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let p = player(&f, "asha", "+919876543210").await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 90).await;
    let a = begin(&f, &d, Some(&p), SessionMode::Pack, Some(&pack))
        .await
        .unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        ["sessions", "session_segments", "devices_meta"]
    );
    let s = &sessions(&f)[0];
    assert_eq!(text(s, "mode"), "pack");
    assert_eq!(text(s, "pack_id"), pack);
    assert_eq!(s["rate_id"], RowValue::Null);
    assert_eq!(int(s, "ends_at"), T0 + 90 * MIN);
    assert_eq!(int(s, "reserved_minor"), 0);
    let seg = &table(&f, Table::SessionSegments)[0];
    assert_eq!(int(seg, "rate_minor_per_hour"), 0);
    assert_eq!(state_of(&f, &d), "in_session");
}

#[tokio::test]
async fn a_pack_session_needs_no_group_rate() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let p = player(&f, "asha", "+919876543210").await;
    assert_eq!(
        refusal(begin(&f, &d, Some(&p), SessionMode::Wallet, None).await),
        (ErrorCode::NoRate, Some("device_id".into()))
    );
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 60).await;
    pack_session(&f, &d, &p, &pack).await;
}

#[tokio::test]
async fn a_wallet_session_ends_where_the_postpaid_bill_reaches_the_balance() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    begin(&f, &d, Some(&p), SessionMode::Wallet, None)
        .await
        .unwrap();
    let s = &sessions(&f)[0];
    assert_eq!(text(s, "mode"), "wallet");
    assert_eq!(int(s, "ends_at"), T0 + 90 * MIN);
    assert_eq!(int(s, "reserved_minor"), 9_000);
    assert!(matches!(&s["rate_id"], RowValue::Text(_)));
    assert_eq!(balance(&f, &p), 10_000);
}

#[tokio::test]
async fn a_wallet_below_the_minimum_bill_is_refused_with_the_balance() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 2_900).await;
    let before = reopen(&f).changes().unwrap().len();
    let e = begin(&f, &d, Some(&p), SessionMode::Wallet, None)
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::InsufficientBalance, Some("player_id"))
    );
    assert!(
        e.message.starts_with("Balance ₹29.00. Top up at counter."),
        "{}",
        e.message
    );
    assert!(
        e.message.contains("₹60.00") && e.message.contains("₹30.00"),
        "{}",
        e.message
    );
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    assert!(sessions(&f).is_empty());
}

#[tokio::test]
async fn a_pack_start_is_refused_for_every_unusable_pack() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let other = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    let q = player(&f, "ben", "+919876543211").await;
    let field = |c, f: &str| (c, Some(f.to_owned()));
    let wrong_group = owned_pack(&f, &p, &[group_of(&f, &other)], 60).await;
    let expired = owned_pack(&f, &p, &[group_of(&f, &d)], 60).await;
    sql(
        &f,
        format!("UPDATE player_packs SET expires_at = {T0} WHERE id = '{expired}'"),
    )
    .await;
    let empty = owned_pack(&f, &p, &[group_of(&f, &d)], 0).await;
    let good = owned_pack(&f, &p, &[group_of(&f, &d)], 60).await;
    let before = reopen(&f).changes().unwrap().len();
    assert_eq!(
        refusal(pack_start(&f, &d, Some(&p), Some(&wrong_group)).await),
        field(ErrorCode::PlanNotForGroup, "pack_id")
    );
    assert_eq!(
        refusal(pack_start(&f, &d, Some(&p), Some(&expired)).await),
        field(ErrorCode::PackExpired, "pack_id")
    );
    assert_eq!(
        refusal(pack_start(&f, &d, Some(&p), Some(&empty)).await),
        field(ErrorCode::PackExpired, "pack_id")
    );
    assert_eq!(
        refusal(pack_start(&f, &d, Some(&p), Some("NOPE")).await),
        field(ErrorCode::NotFound, "pack_id")
    );
    assert_eq!(
        refusal(pack_start(&f, &d, Some(&p), None).await),
        field(ErrorCode::ValidationFailed, "pack_id")
    );
    assert_eq!(
        refusal(pack_start(&f, &d, None, Some(&good)).await),
        field(ErrorCode::ValidationFailed, "player_id")
    );
    assert_eq!(
        refusal(begin(&f, &d, Some(&q), SessionMode::Pack, Some(&good)).await),
        field(ErrorCode::NotFound, "pack_id")
    );
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    let plan = plan_for(&f, &[group_of(&f, &d)], 100).await;
    end(&f, sell_intent(&p, &plan, vec![pay(Cash, 100, None)]))
        .await
        .unwrap();
    let pass_like = text(table(&f, Table::PlayerPacks).last().unwrap(), "id");
    sql(
        &f,
        format!("UPDATE player_packs SET kind = 'pass' WHERE id = '{pass_like}'"),
    )
    .await;
    assert_eq!(
        refusal(pack_start(&f, &d, Some(&p), Some(&pass_like)).await),
        field(ErrorCode::NotFound, "pack_id")
    );
}

#[tokio::test]
async fn a_pack_already_on_a_live_session_cannot_start_another() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let other = pc(&f, DeviceKind::Pc, "pc", None).await;
    let p = player(&f, "asha", "+919876543210").await;
    let g = vec![group_of(&f, &d), group_of(&f, &other)];
    let pack = owned_pack(&f, &p, &g, 60).await;
    pack_session(&f, &d, &p, &pack).await;
    assert_eq!(
        refusal(begin(&f, &other, Some(&p), SessionMode::Pack, Some(&pack)).await),
        (ErrorCode::ValidationFailed, Some("pack_id".into()))
    );
}

#[tokio::test]
async fn a_second_wallet_session_is_sized_on_what_the_first_left() {
    let f = bootstrapped().await;
    let a = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let b = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 13_000).await;
    wallet_session(&f, &a, &p).await;
    assert_eq!(int(&sessions(&f)[0], "reserved_minor"), 12_000);
    let e = begin(&f, &b, Some(&p), SessionMode::Wallet, None)
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::InsufficientBalance);
    assert!(e.message.starts_with("Balance ₹10.00."), "{}", e.message);
    topped_up(&f, &p, 5_000).await;
    wallet_session(&f, &b, &p).await;
    let second = &sessions(&f)[1];
    assert_eq!(int(second, "reserved_minor"), 6_000);
    assert_eq!(int(second, "ends_at"), T0 + 60 * MIN);
}

#[tokio::test]
async fn a_spend_that_cuts_into_a_reservation_is_refused() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 13_000).await;
    let session = wallet_session(&f, &d, &p).await;
    let plan = plan_for(&f, &[group_of(&f, &d)], 5_000).await;
    let e = end(&f, sell_intent(&p, &plan, vec![pay(Wallet, 5_000, None)]))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::InsufficientBalance);
    end(&f, credit_intent(&p, 10_000, &[3])).await.unwrap();
    sql(
        &f,
        format!("UPDATE credit_balances SET outstanding_minor = 3000 WHERE player_id = '{p}'"),
    )
    .await;
    let e = end(
        &f,
        collect_intent(&p, 2_000, vec![pay(Wallet, 2_000, None)]),
    )
    .await
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::InsufficientBalance);
    assert_eq!(balance(&f, &p), 13_000);
    end(
        &f,
        sell_intent(
            &p,
            &plan,
            vec![pay(Wallet, 1_000, None), pay(Cash, 4_000, None)],
        ),
    )
    .await
    .unwrap();
    stop(&f, &session).await.unwrap();
    let plan2 = plan_for(&f, &[group_of(&f, &d)], 5_000).await;
    end(&f, sell_intent(&p, &plan2, vec![pay(Wallet, 5_000, None)]))
        .await
        .unwrap();
}

#[tokio::test]
async fn ending_a_pack_session_deducts_whole_minutes_and_settles_a_zero_bill() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let p = player(&f, "asha", "+919876543210").await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 90).await;
    let session = pack_session(&f, &d, &p, &pack).await;
    f.clock.advance(12 * MIN + 10_000);
    let a = stop(&f, &session).await.unwrap();
    let tables = a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>();
    assert!(tables.contains(&"player_packs") && !tables.contains(&"payments"));
    assert_eq!(pack_left(&f, &pack), 77);
    let s = row_of(&f, Table::Sessions, &session);
    assert_eq!(text(&s, "state"), "ended");
    assert_eq!(text(&s, "ended_by"), "o");
    let o = order_of(&f, &session);
    assert_eq!(text(&o, "kind"), "session");
    assert_eq!(text(&o, "status"), "settled");
    assert_eq!(int(&o, "total_minor"), 0);
    assert_eq!(text(&o, "settled_by"), "o");
    assert!(
        table(&f, Table::Payments)
            .iter()
            .all(|r| text(r, "order_id") != text(&o, "id"))
    );
    let line = table(&f, Table::OrderLines)
        .into_iter()
        .find(|l| text(l, "order_id") == text(&o, "id"))
        .unwrap();
    assert_eq!((int(&line, "minutes"), int(&line, "amount_minor")), (13, 0));
    assert_ne!(state_of(&f, &d), "in_session");
    let e = end(&f, {
        let mut i = intent(
            owner(),
            IntentPayload::SessionEndReverted(arena_proto::SessionEndReverted {
                session_id: session.clone(),
            }),
        );
        i.base_rev = Some(rev_of(&f, Table::Sessions, &session));
        i
    })
    .await
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::UndoNotAllowed);
    assert!(reopen(&f).ending(&session).unwrap().unwrap().irreversible);
}

#[tokio::test]
async fn ending_a_wallet_session_bills_by_the_formula_and_debits_the_wallet() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    let session = wallet_session(&f, &d, &p).await;
    f.clock.advance(41 * MIN);
    let a = stop(&f, &session).await.unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        [
            "sessions",
            "orders",
            "session_segments",
            "order_lines",
            "payments",
            "wallet_txns",
            "wallets",
            "devices_meta"
        ]
    );
    let o = order_of(&f, &session);
    assert_eq!(
        (text(&o, "status"), int(&o, "total_minor")),
        ("settled".into(), 4_500)
    );
    let payment = table(&f, Table::Payments)
        .into_iter()
        .find(|r| text(r, "order_id") == text(&o, "id"))
        .unwrap();
    assert_eq!(
        (text(&payment, "method"), int(&payment, "amount_minor")),
        ("wallet".into(), 4_500)
    );
    assert_eq!(balance(&f, &p), 5_500);
    assert_eq!(ledger_sum(&f, &p), 5_500);
    let txn = table(&f, Table::WalletTxns)
        .into_iter()
        .find(|t| text(t, "reason") == "bill")
        .unwrap();
    assert_eq!(int(&txn, "delta_minor"), -4_500);
    let plan = plan_for(&f, &[group_of(&f, &d)], 5_500).await;
    end(&f, sell_intent(&p, &plan, vec![pay(Wallet, 5_500, None)]))
        .await
        .unwrap();
}

#[tokio::test]
async fn the_debit_is_capped_at_the_balance_and_the_bill_still_balances() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    let session = wallet_session(&f, &d, &p).await;
    sql(
        &f,
        format!("UPDATE wallets SET balance_minor = 2000 WHERE player_id = '{p}'"),
    )
    .await;
    f.clock.advance(41 * MIN);
    stop(&f, &session).await.unwrap();
    let o = order_of(&f, &session);
    assert_eq!(int(&o, "total_minor"), 2_000);
    let line_sum: i64 = table(&f, Table::OrderLines)
        .iter()
        .filter(|l| text(l, "order_id") == text(&o, "id"))
        .map(|l| int(l, "amount_minor"))
        .sum();
    assert_eq!(line_sum, 2_000);
    assert_eq!(balance(&f, &p), 0);
}

#[tokio::test]
async fn expiry_closes_a_pack_session_like_a_staff_end() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let p = player(&f, "asha", "+919876543210").await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 30).await;
    let session = pack_session(&f, &d, &p, &pack).await;
    let ends = T0 + 30 * MIN;
    f.clock.set(ends + 3_000);
    expire(&f, &session, ends + 3_000).await.unwrap();
    let s = row_of(&f, Table::Sessions, &session);
    assert_eq!(
        (text(&s, "state"), text(&s, "ended_by"), int(&s, "ended_at")),
        ("expired".into(), "hub".into(), ends)
    );
    assert_eq!(pack_left(&f, &pack), 0);
    let o = order_of(&f, &session);
    assert_eq!(
        (
            text(&o, "status"),
            text(&o, "settled_by"),
            int(&o, "total_minor")
        ),
        ("settled".into(), "hub".into(), 0)
    );
    assert_ne!(state_of(&f, &d), "in_session");
    assert_eq!(
        expire(&f, &session, ends + 4_000).await.unwrap_err().code,
        ErrorCode::Stale
    );
}

#[tokio::test]
async fn expiry_bills_a_wallet_session_for_its_whole_time() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    let session = wallet_session(&f, &d, &p).await;
    let ends = T0 + 90 * MIN;
    f.clock.set(ends);
    expire(&f, &session, ends).await.unwrap();
    assert_eq!(balance(&f, &p), 1_000);
    let o = order_of(&f, &session);
    assert_eq!(
        (text(&o, "status"), int(&o, "total_minor")),
        ("settled".into(), 9_000)
    );
    let payment = table(&f, Table::Payments)
        .into_iter()
        .find(|r| text(r, "order_id") == text(&o, "id"))
        .unwrap();
    assert_eq!(text(&payment, "staff_id"), "hub");
    assert_eq!(ledger_sum(&f, &p), 1_000);
}

#[tokio::test]
async fn a_finished_wallet_session_releases_its_reservation() {
    let f = bootstrapped().await;
    let a = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let b = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 13_000).await;
    let first = wallet_session(&f, &a, &p).await;
    stop(&f, &first).await.unwrap();
    wallet_session(&f, &b, &p).await;
    assert_eq!(balance(&f, &p), 10_000);
    assert_eq!(int(&sessions(&f)[1], "reserved_minor"), 9_000);
}

#[tokio::test]
async fn a_postpaid_start_takes_no_pack_and_pass_and_block_cannot_start_yet() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    let field = |c, f: &str| (c, Some(f.to_owned()));
    assert_eq!(
        refusal(begin(&f, &d, Some(&p), SessionMode::Postpaid, Some("X")).await),
        field(ErrorCode::ValidationFailed, "pack_id")
    );
    for mode in [SessionMode::Pass, SessionMode::Block] {
        assert_eq!(
            refusal(begin(&f, &d, Some(&p), mode, None).await),
            field(ErrorCode::ValidationFailed, "mode")
        );
    }
    assert!(sessions(&f).is_empty());
}

async fn pack_start(
    f: &Fixture,
    d: &str,
    player: Option<&str>,
    pack: Option<&str>,
) -> Result<Applied, AppError> {
    begin(f, d, player, SessionMode::Pack, pack).await
}

#[tokio::test]
async fn a_top_up_cannot_be_undone_out_from_under_a_running_wallet_session() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    let order = topped_up(&f, &p, 13_000).await;
    wallet_session(&f, &d, &p).await;
    let rev = int(&row_of(&f, Table::Orders, &order), "rev");
    let e = end(&f, topup_revert_intent(&order, Some(rev)))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::UndoNotAllowed);
    assert_eq!(balance(&f, &p), 13_000);
}

#[tokio::test]
async fn a_pack_sale_cannot_be_undone_while_a_session_runs_on_it() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let p = player(&f, "asha", "+919876543210").await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 60).await;
    pack_session(&f, &d, &p, &pack).await;
    let order = text(&sale_order(&f), "id");
    let rev = rev_of(&f, Table::Orders, &order);
    assert_eq!(
        refusal(end(&f, sell_revert_intent(&order, Some(rev))).await),
        (ErrorCode::UndoNotAllowed, None)
    );
}
