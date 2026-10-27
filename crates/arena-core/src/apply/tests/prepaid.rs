use super::session::{
    balance, collect_intent, credit_intent, end, end_intent, int, ledger_sum, pay, pc, player,
    refusal, rev_of, row_of, sale_order, sell_intent, sell_revert_intent, sessions, settle_intent,
    state_of, submit, table, text, topped_up, topup_revert_intent,
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
            plan_id: None,
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

async fn begin_block(
    f: &Fixture,
    device: &str,
    player: Option<&str>,
    plan: &str,
    mode: SessionMode,
) -> Result<Applied, AppError> {
    let mut i = intent(
        owner(),
        IntentPayload::SessionStart(SessionStart {
            device_id: device.into(),
            mode,
            player_id: player.map(Into::into),
            pack_id: None,
            plan_id: Some(plan.into()),
        }),
    );
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
    sql(
        f,
        format!("UPDATE sessions SET grace_min = 0 WHERE id = '{session}'"),
    )
    .await;
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
async fn a_postpaid_start_takes_no_pack_or_plan_and_pass_and_block_need_theirs() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    let field = |c, f: &str| (c, Some(f.to_owned()));
    assert_eq!(
        refusal(begin(&f, &d, Some(&p), SessionMode::Postpaid, Some("X")).await),
        field(ErrorCode::ValidationFailed, "pack_id")
    );
    assert_eq!(
        refusal(begin(&f, &d, Some(&p), SessionMode::Pass, None).await),
        field(ErrorCode::ValidationFailed, "pack_id")
    );
    assert_eq!(
        refusal(begin(&f, &d, Some(&p), SessionMode::Block, None).await),
        field(ErrorCode::ValidationFailed, "plan_id")
    );
    assert_eq!(
        refusal(begin_block(&f, &d, Some(&p), "X", SessionMode::Postpaid).await),
        field(ErrorCode::ValidationFailed, "plan_id")
    );
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

fn ist(local: &str) -> i64 {
    use chrono::TimeZone;
    let naive = chrono::NaiveDateTime::parse_from_str(local, "%Y-%m-%d %H:%M").unwrap();
    chrono_tz::Tz::Asia__Kolkata
        .from_local_datetime(&naive)
        .unwrap()
        .timestamp_millis()
}

async fn window_plan(f: &Fixture, kind: &str, groups: &[String], price: i64) -> String {
    submit(
        f,
        owner(),
        IntentPayload::PlanUpsert(PlanUpsert {
            id: None,
            plan_kind: kind.into(),
            name: format!("Night {kind}"),
            price_minor: price,
            minutes: None,
            validity_days: (kind == "pass").then_some(30),
            window_start_min: Some(22 * 60),
            window_end_min: Some(6 * 60),
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

async fn owned_pass(f: &Fixture, player: &str, groups: &[String]) -> String {
    let plan = window_plan(f, "pass", groups, 90_000).await;
    end(f, sell_intent(player, &plan, vec![pay(Cash, 90_000, None)]))
        .await
        .unwrap();
    text(table(f, Table::PlayerPacks).last().unwrap(), "id")
}

async fn begin_pass(f: &Fixture, d: &str, p: &str, pass: &str) -> Result<Applied, AppError> {
    begin(f, d, Some(p), SessionMode::Pass, Some(pass)).await
}

#[tokio::test]
async fn a_pass_started_before_midnight_ends_at_the_window_end_the_next_morning() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let p = player(&f, "asha", "+919876543210").await;
    let pass = owned_pass(&f, &p, &[group_of(&f, &d)]).await;
    f.clock.set(ist("2027-01-15 23:30"));
    let a = begin_pass(&f, &d, &p, &pass).await.unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        ["sessions", "session_segments", "devices_meta"]
    );
    let s = &sessions(&f)[0];
    assert_eq!(text(s, "mode"), "pass");
    assert_eq!(text(s, "pack_id"), pass);
    assert_eq!(s["rate_id"], RowValue::Null);
    assert_eq!(int(s, "ends_at"), ist("2027-01-16 06:00"));
}

#[tokio::test]
async fn a_pass_started_after_midnight_ends_that_morning() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let p = player(&f, "asha", "+919876543210").await;
    let pass = owned_pass(&f, &p, &[group_of(&f, &d)]).await;
    f.clock.set(ist("2027-01-16 02:15"));
    begin_pass(&f, &d, &p, &pass).await.unwrap();
    assert_eq!(int(&sessions(&f)[0], "ends_at"), ist("2027-01-16 06:00"));
}

#[tokio::test]
async fn a_pass_outside_its_window_is_refused_naming_the_window() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let p = player(&f, "asha", "+919876543210").await;
    let pass = owned_pass(&f, &p, &[group_of(&f, &d)]).await;
    for at in ["2027-01-15 14:00", "2027-01-16 06:00", "2027-01-15 21:59"] {
        f.clock.set(ist(at));
        let e = begin_pass(&f, &d, &p, &pass).await.unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::PassOutsideWindow, Some("pack_id")),
            "{at}"
        );
        assert!(e.message.contains("22:00–06:00"), "{}", e.message);
    }
    f.clock.set(ist("2027-01-15 22:00"));
    begin_pass(&f, &d, &p, &pass).await.unwrap();
}

#[tokio::test]
async fn a_pass_is_refused_for_another_group_a_pack_id_and_a_busy_pass() {
    let f = bootstrapped().await;
    let (d, other) = (
        pc(&f, DeviceKind::Pc, "pc", None).await,
        pc(&f, DeviceKind::Pc, "pc", None).await,
    );
    let p = player(&f, "asha", "+919876543210").await;
    let pass = owned_pass(&f, &p, &[group_of(&f, &d)]).await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 60).await;
    f.clock.set(ist("2027-01-15 23:30"));
    let field = |c, f: &str| (c, Some(f.to_owned()));
    assert_eq!(
        refusal(begin_pass(&f, &other, &p, &pass).await),
        field(ErrorCode::PlanNotForGroup, "pack_id")
    );
    assert_eq!(
        refusal(begin_pass(&f, &d, &p, &pack).await),
        field(ErrorCode::NotFound, "pack_id")
    );
    assert_eq!(
        refusal(begin(&f, &d, Some(&p), SessionMode::Pack, Some(&pass)).await),
        field(ErrorCode::NotFound, "pack_id")
    );
    assert_eq!(
        refusal(begin(&f, &d, None, SessionMode::Pass, Some(&pass)).await),
        field(ErrorCode::ValidationFailed, "player_id")
    );
    sql(
        &f,
        format!(
            "UPDATE player_packs SET expires_at = {} WHERE id = '{pass}'",
            ist("2027-01-15 23:00")
        ),
    )
    .await;
    assert_eq!(
        refusal(begin_pass(&f, &d, &p, &pass).await),
        field(ErrorCode::PackExpired, "pack_id")
    );
    sql(
        &f,
        format!(
            "UPDATE player_packs SET expires_at = {} WHERE id = '{pass}'",
            ist("2027-02-15 23:00")
        ),
    )
    .await;
    begin_pass(&f, &d, &p, &pass).await.unwrap();
    assert_eq!(sessions(&f).len(), 1);
}

#[tokio::test]
async fn a_pass_already_running_a_session_cannot_start_another() {
    let f = bootstrapped().await;
    let (d, second) = (
        pc(&f, DeviceKind::Pc, "pc", None).await,
        pc(&f, DeviceKind::Pc, "pc", None).await,
    );
    let p = player(&f, "asha", "+919876543210").await;
    let groups = [group_of(&f, &d), group_of(&f, &second)];
    let pass = owned_pass(&f, &p, &groups).await;
    f.clock.set(ist("2027-01-15 23:30"));
    begin_pass(&f, &d, &p, &pass).await.unwrap();
    assert_eq!(
        refusal(begin_pass(&f, &second, &p, &pass).await),
        (ErrorCode::ValidationFailed, Some("pack_id".to_owned()))
    );
}

#[tokio::test]
async fn a_pass_session_closes_with_a_zero_bill_and_the_pass_can_be_used_again() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let p = player(&f, "asha", "+919876543210").await;
    let pass = owned_pass(&f, &p, &[group_of(&f, &d)]).await;
    f.clock.set(ist("2027-01-15 23:30"));
    begin_pass(&f, &d, &p, &pass).await.unwrap();
    let session = text(&sessions(&f)[0], "id");
    f.clock.advance(20 * MIN);
    stop(&f, &session).await.unwrap();
    let o = order_of(&f, &session);
    assert_eq!(
        (text(&o, "status").as_str(), int(&o, "total_minor")),
        ("settled", 0)
    );
    assert_eq!(
        row_of(&f, Table::PlayerPacks, &pass)["minutes_left"],
        RowValue::Null
    );
    assert!(reopen(&f).ending(&session).unwrap().unwrap().irreversible);
    f.clock.advance(MIN);
    begin_pass(&f, &d, &p, &pass).await.unwrap();
}

#[tokio::test]
async fn a_pass_session_expires_at_the_window_end() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let p = player(&f, "asha", "+919876543210").await;
    let pass = owned_pass(&f, &p, &[group_of(&f, &d)]).await;
    f.clock.set(ist("2027-01-15 23:30"));
    begin_pass(&f, &d, &p, &pass).await.unwrap();
    let session = text(&sessions(&f)[0], "id");
    f.clock.set(ist("2027-01-16 06:00"));
    expire(&f, &session, ist("2027-01-16 06:00")).await.unwrap();
    let s = row_of(&f, Table::Sessions, &session);
    assert_eq!(text(&s, "state"), "expired");
    assert_eq!(int(&s, "ended_at"), ist("2027-01-16 06:00"));
    assert_eq!(text(&order_of(&f, &session), "status"), "settled");
    assert_ne!(state_of(&f, &d), "in_session");
}

#[tokio::test]
async fn a_block_for_a_guest_opens_its_bill_at_once_and_ends_at_the_window_end() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let plan = window_plan(&f, "block", &[group_of(&f, &d)], 20_000).await;
    f.clock.set(ist("2027-01-15 23:45"));
    let a = begin_block(&f, &d, None, &plan, SessionMode::Block)
        .await
        .unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        [
            "sessions",
            "session_segments",
            "devices_meta",
            "orders",
            "order_lines"
        ]
    );
    let s = &sessions(&f)[0];
    assert_eq!(text(s, "mode"), "block");
    assert_eq!(s["player_id"], RowValue::Null);
    assert_eq!(int(s, "ends_at"), ist("2027-01-16 06:00"));
    let o = order_of(&f, &text(s, "id"));
    assert_eq!(
        (text(&o, "status").as_str(), int(&o, "total_minor")),
        ("open", 20_000)
    );
    let line = &table(&f, Table::OrderLines)[0];
    assert_eq!(
        (int(line, "amount_minor"), int(line, "minutes")),
        (20_000, 375)
    );
    assert_eq!(state_of(&f, &d), "in_session");
}

#[tokio::test]
async fn a_block_ends_at_the_same_time_whenever_it_starts_in_the_window() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let p = player(&f, "asha", "+919876543210").await;
    let plan = window_plan(&f, "block", &[group_of(&f, &d)], 20_000).await;
    f.clock.set(ist("2027-01-16 03:00"));
    begin_block(&f, &d, Some(&p), &plan, SessionMode::Block)
        .await
        .unwrap();
    let s = &sessions(&f)[0];
    assert_eq!(int(s, "ends_at"), ist("2027-01-16 06:00"));
    assert_eq!(text(s, "player_id"), p);
    assert_eq!(text(&order_of(&f, &text(s, "id")), "player_id"), p);
}

#[tokio::test]
async fn a_block_is_refused_outside_its_window_for_another_group_or_when_unknown() {
    let f = bootstrapped().await;
    let (d, other) = (
        pc(&f, DeviceKind::Pc, "pc", None).await,
        pc(&f, DeviceKind::Pc, "pc", None).await,
    );
    let plan = window_plan(&f, "block", &[group_of(&f, &d)], 20_000).await;
    let field = |c, f: &str| (c, Some(f.to_owned()));
    f.clock.set(ist("2027-01-15 14:00"));
    let e = begin_block(&f, &d, None, &plan, SessionMode::Block)
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::PassOutsideWindow, Some("plan_id"))
    );
    assert!(e.message.contains("22:00–06:00"), "{}", e.message);
    f.clock.set(ist("2027-01-15 23:00"));
    assert_eq!(
        refusal(begin_block(&f, &other, None, &plan, SessionMode::Block).await),
        field(ErrorCode::PlanNotForGroup, "plan_id")
    );
    assert_eq!(
        refusal(begin_block(&f, &d, None, "NOPE", SessionMode::Block).await),
        field(ErrorCode::NotFound, "plan_id")
    );
    let pass = window_plan(&f, "pass", &[group_of(&f, &d)], 90_000).await;
    assert_eq!(
        refusal(begin_block(&f, &d, None, &pass, SessionMode::Block).await),
        field(ErrorCode::NotFound, "plan_id")
    );
    assert!(sessions(&f).is_empty());
    assert!(table(&f, Table::Orders).is_empty());
}

#[tokio::test]
async fn a_block_bill_can_be_settled_while_the_session_runs_and_expiry_adds_no_second_bill() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let plan = window_plan(&f, "block", &[group_of(&f, &d)], 20_000).await;
    f.clock.set(ist("2027-01-15 23:45"));
    begin_block(&f, &d, None, &plan, SessionMode::Block)
        .await
        .unwrap();
    let session = text(&sessions(&f)[0], "id");
    let order = order_of(&f, &session);
    end(
        &f,
        settle_intent(
            &text(&order, "id"),
            Some(int(&order, "rev")),
            vec![pay(Cash, 20_000, None)],
        ),
    )
    .await
    .unwrap();
    f.clock.set(ist("2027-01-16 06:00"));
    expire(&f, &session, ist("2027-01-16 06:00")).await.unwrap();
    let s = row_of(&f, Table::Sessions, &session);
    assert_eq!(text(&s, "state"), "expired");
    assert_eq!(
        table(&f, Table::Orders)
            .iter()
            .filter(|o| text(o, "session_id") == session)
            .count(),
        1
    );
    assert_eq!(text(&order_of(&f, &session), "status"), "settled");
    assert_ne!(state_of(&f, &d), "in_session");
}

#[tokio::test]
async fn ending_a_block_early_keeps_its_one_bill_open_for_settling() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let plan = window_plan(&f, "block", &[group_of(&f, &d)], 20_000).await;
    f.clock.set(ist("2027-01-15 23:45"));
    begin_block(&f, &d, None, &plan, SessionMode::Block)
        .await
        .unwrap();
    let session = text(&sessions(&f)[0], "id");
    f.clock.advance(30 * MIN);
    stop(&f, &session).await.unwrap();
    let s = row_of(&f, Table::Sessions, &session);
    assert_eq!(text(&s, "state"), "ended");
    assert_eq!(int(&s, "ended_at"), ist("2027-01-16 00:15"));
    let orders = table(&f, Table::Orders);
    assert_eq!(orders.len(), 1);
    assert_eq!(
        (
            text(&orders[0], "status").as_str(),
            int(&orders[0], "total_minor")
        ),
        ("open", 20_000)
    );
    assert!(reopen(&f).ending(&session).unwrap().unwrap().irreversible);
    assert_ne!(state_of(&f, &d), "in_session");
}

#[tokio::test]
async fn a_held_block_session_ends_at_its_window_end_in_staff_handles_mode() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let plan = window_plan(&f, "block", &[group_of(&f, &d)], 20_000).await;
    f.clock.set(ist("2027-01-15 23:45"));
    begin_block(&f, &d, None, &plan, SessionMode::Block)
        .await
        .unwrap();
    let session = text(&sessions(&f)[0], "id");
    sql(
        &f,
        format!("UPDATE sessions SET time_up_mode = 'staff_handles' WHERE id = '{session}'"),
    )
    .await;
    f.clock.set(ist("2027-01-16 06:00"));
    expire(&f, &session, ist("2027-01-16 06:00")).await.unwrap();
    assert_eq!(
        text(&row_of(&f, Table::Sessions, &session), "state"),
        "expired"
    );
    assert_eq!(state_of(&f, &d), "in_session");
    f.clock.set(ist("2027-01-16 06:20"));
    stop(&f, &session).await.unwrap();
    let s = row_of(&f, Table::Sessions, &session);
    assert_eq!(int(&s, "ended_at"), ist("2027-01-16 06:00"));
    assert_eq!(table(&f, Table::Orders).len(), 1);
    assert_ne!(state_of(&f, &d), "in_session");
}

#[tokio::test]
async fn pass_and_block_sessions_on_a_rated_group_take_no_group_rate() {
    let f = bootstrapped().await;
    let (a, b) = (
        pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await,
        pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await,
    );
    let p = player(&f, "asha", "+919876543210").await;
    let pass = owned_pass(&f, &p, &[group_of(&f, &a)]).await;
    let block = window_plan(&f, "block", &[group_of(&f, &b)], 20_000).await;
    f.clock.set(ist("2027-01-15 23:30"));
    begin_pass(&f, &a, &p, &pass).await.unwrap();
    begin_block(&f, &b, None, &block, SessionMode::Block)
        .await
        .unwrap();
    for s in sessions(&f) {
        assert_eq!(s["rate_id"], RowValue::Null);
    }
    for seg in table(&f, Table::SessionSegments) {
        assert_eq!(int(&seg, "rate_minor_per_hour"), 0);
    }
}

#[tokio::test]
async fn an_inactive_block_or_a_block_with_a_pack_id_is_refused() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let group = group_of(&f, &d);
    let plan = window_plan(&f, "block", std::slice::from_ref(&group), 20_000).await;
    f.clock.set(ist("2027-01-15 23:30"));
    let mut i = start_as(&d, None, SessionMode::Block, Some("PACK"));
    i.issued_at = f.clock.now_ms();
    if let IntentPayload::SessionStart(s) = &mut i.payload {
        s.plan_id = Some(plan.clone());
    }
    assert_eq!(
        refusal(f.writer.submit_intent(i).await),
        (ErrorCode::ValidationFailed, Some("pack_id".to_owned()))
    );
    submit(
        &f,
        owner(),
        IntentPayload::PlanUpsert(PlanUpsert {
            id: Some(plan.clone()),
            plan_kind: "block".into(),
            name: "Night block".into(),
            price_minor: 20_000,
            minutes: None,
            validity_days: None,
            window_start_min: Some(22 * 60),
            window_end_min: Some(6 * 60),
            group_ids: vec![group],
            active: false,
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        refusal(begin_block(&f, &d, None, &plan, SessionMode::Block).await),
        (ErrorCode::NotFound, Some("plan_id".to_owned()))
    );
    assert!(sessions(&f).is_empty());
}
