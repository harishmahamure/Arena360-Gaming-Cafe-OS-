use super::players::{add, form};
use super::stock::{intent, owner, staff};
use super::*;
use crate::rbac::{ROLE_COUNTER, ROLE_KITCHEN};
use arena_proto::{
    ClientClass, DeviceAssign, DeviceKind, DeviceMaintenance, DevicePower, DeviceRegister,
    GroupUpsert, PcAction, RateUpsert, SessionMode, SessionStart,
};

pub(super) fn text(row: &Row, col: &str) -> String {
    match &row[col] {
        RowValue::Text(t) => t.clone(),
        other => panic!("{col}: {other:?}"),
    }
}

pub(super) fn int(row: &Row, col: &str) -> i64 {
    match &row[col] {
        RowValue::Integer(n) => *n,
        other => panic!("{col}: {other:?}"),
    }
}

pub(super) async fn submit(
    f: &Fixture,
    actor: Actor,
    p: IntentPayload,
) -> Result<Applied, AppError> {
    let mut i = intent(actor, p);
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

pub(super) async fn pc(
    f: &Fixture,
    kind: DeviceKind,
    group_kind: &str,
    rate: Option<i64>,
) -> String {
    let mut d = DeviceRegister::new(
        ClientClass::Paired,
        "PC",
        crate::devices::hash_token_secret("s").unwrap(),
    );
    d.device_kind = kind;
    let id = submit(f, owner(), IntentPayload::DeviceRegister(d))
        .await
        .unwrap()
        .changes[0]
        .row_id
        .clone();
    let g = submit(
        f,
        owner(),
        IntentPayload::GroupUpsert(GroupUpsert {
            id: None,
            name: format!("G{id}"),
            group_kind: group_kind.into(),
            colour: "teal".into(),
            sort_order: 1,
        }),
    )
    .await
    .unwrap()
    .changes[0]
        .row_id
        .clone();
    if let Some(price) = rate {
        submit(
            f,
            owner(),
            IntentPayload::RateUpsert(RateUpsert {
                group_id: g.clone(),
                price_per_hour_minor: price,
            }),
        )
        .await
        .unwrap();
    }
    submit(
        f,
        owner(),
        IntentPayload::DeviceAssign(DeviceAssign {
            device_id: id.clone(),
            group_id: Some(g),
        }),
    )
    .await
    .unwrap();
    id
}

pub(super) fn start_of(device: &str, player: Option<&str>) -> IntentPayload {
    IntentPayload::SessionStart(SessionStart {
        group_with: None,
        pay_from_wallet: false,
        device_id: device.into(),
        mode: SessionMode::Postpaid,
        player_id: player.map(Into::into),
        pack_id: None,
        plan_id: None,
    })
}

pub(super) async fn start(
    f: &Fixture,
    device: &str,
    player: Option<&str>,
) -> Result<Applied, AppError> {
    submit(f, owner(), start_of(device, player)).await
}

pub(super) fn refusal(r: Result<Applied, AppError>) -> (ErrorCode, Option<String>) {
    let e = r.unwrap_err();
    (e.code, e.field)
}

pub(super) fn sessions(f: &Fixture) -> Vec<Row> {
    reopen(f).rows(Table::Sessions).unwrap()
}

pub(super) fn state_of(f: &Fixture, device: &str) -> String {
    reopen(f)
        .rows(Table::DevicesMeta)
        .unwrap()
        .iter()
        .find(|r| text(r, "device_id") == device)
        .map(|r| text(r, "state"))
        .unwrap()
}

#[tokio::test]
async fn a_player_session_writes_session_segment_and_flips_the_device() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(12_000)).await;
    let (player, _) = add(&f, owner(), &form("asha", "+919876543210"))
        .await
        .unwrap();
    let a = start(&f, &d, Some(&player)).await.unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        ["sessions", "session_segments", "devices_meta"]
    );
    let s = &sessions(&f)[0];
    assert_eq!(text(s, "player_id"), player);
    assert_eq!(s["guest_no"], RowValue::Null);
    assert_eq!(text(s, "device_id"), d);
    assert_eq!(text(s, "mode"), "postpaid");
    assert_eq!(text(s, "state"), "active");
    assert_eq!(int(s, "started_at"), T0);
    assert_eq!(int(s, "rev"), 1);
    assert_eq!(
        (
            int(s, "rounding_min"),
            int(s, "minimum_min"),
            int(s, "grace_min"),
            text(s, "time_up_mode")
        ),
        (15, 30, 2, "clean_up".into())
    );
    assert_eq!(text(s, "business_day"), "2027-01-15");
    assert!(!text(s, "fy").is_empty() && !text(s, "epoch").is_empty());
    let seg = &reopen(&f).rows(Table::SessionSegments).unwrap()[0];
    assert_eq!(text(seg, "session_id"), text(s, "id"));
    assert_eq!(int(seg, "from_at"), T0);
    assert_eq!(seg["to_at"], RowValue::Null);
    assert_eq!(int(seg, "rate_minor_per_hour"), 12_000);
    assert_eq!(seg["rule_id"], RowValue::Null);
    assert_eq!(state_of(&f, &d), "in_session");

    let log: Vec<u8> = reopen(&f)
        .changes()
        .unwrap()
        .into_iter()
        .filter(|c| c.tbl == "sessions")
        .flat_map(|c| c.row)
        .collect();
    let log = String::from_utf8_lossy(&log).into_owned();
    assert!(!log.contains("Asha") && !log.contains("9876543210") && !log.contains("asha"));
    let snap = crate::snapshot::read_snapshot(&reopen(&f), T0).unwrap();
    assert_eq!(snap.sessions.len(), 1);
    assert_eq!(snap.sessions[0].username.as_deref(), Some("asha"));
    assert_eq!(snap.sessions[0].guest_no, None);
}

#[tokio::test]
async fn guests_number_per_business_day() {
    let f = bootstrapped().await;
    let a = pc(&f, DeviceKind::Pc, "pc", Some(100)).await;
    let b = pc(&f, DeviceKind::Pc, "pc", Some(100)).await;
    start(&f, &a, None).await.unwrap();
    start(&f, &b, None).await.unwrap();
    let nos: Vec<i64> = sessions(&f).iter().map(|s| int(s, "guest_no")).collect();
    assert_eq!(nos, [1, 2]);
    f.clock.advance(24 * 3_600_000);
    f.writer
        .submit(|tx| {
            tx.execute_batch(
                "UPDATE sessions SET state = 'ended'; UPDATE devices_meta SET state = 'online'",
            )
            .map_err(arena_store::db_error)
        })
        .await
        .unwrap();
    start(&f, &a, None).await.unwrap();
    let last = sessions(&f).pop().unwrap();
    assert_eq!(
        (text(&last, "business_day"), int(&last, "guest_no")),
        ("2027-01-16".into(), 1)
    );
}

#[tokio::test]
async fn refusals_name_the_code_and_write_nothing() {
    let f = bootstrapped().await;
    let ok = pc(&f, DeviceKind::Pc, "pc", Some(100)).await;
    let norate = pc(&f, DeviceKind::Pc, "pc", None).await;
    let vr = pc(&f, DeviceKind::Vr, "vr", Some(100)).await;
    let loose = {
        let d = DeviceRegister::new(
            ClientClass::Paired,
            "Loose",
            crate::devices::hash_token_secret("s").unwrap(),
        );
        submit(&f, owner(), IntentPayload::DeviceRegister(d))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone()
    };
    let maint = pc(&f, DeviceKind::Pc, "pc", Some(100)).await;
    submit(
        &f,
        owner(),
        IntentPayload::DeviceMaintenance(DeviceMaintenance {
            device_id: maint.clone(),
            on: true,
            note: None,
        }),
    )
    .await
    .unwrap();
    let kitchen = staff(&f, "cook", ROLE_KITCHEN).await;
    let counter = staff(&f, "till", ROLE_COUNTER).await;
    let before = reopen(&f).changes().unwrap().len();
    let field = |c, f: &str| (c, Some(f.to_owned()));
    assert_eq!(
        refusal(start(&f, "nope", None).await),
        field(ErrorCode::NotFound, "device_id")
    );
    assert_eq!(
        refusal(start(&f, &vr, None).await),
        field(ErrorCode::ValidationFailed, "device_id")
    );
    assert_eq!(
        refusal(start(&f, &loose, None).await),
        field(ErrorCode::DeviceUnassigned, "device_id")
    );
    assert_eq!(
        refusal(start(&f, &maint, None).await),
        field(ErrorCode::PcUnavailable, "device_id")
    );
    assert_eq!(
        refusal(start(&f, &norate, None).await),
        field(ErrorCode::NoRate, "device_id")
    );
    assert_eq!(
        refusal(start(&f, &ok, Some("01JNOSUCHPLAYER")).await),
        field(ErrorCode::NotFound, "player_id")
    );
    let pass = IntentPayload::SessionStart(SessionStart {
        group_with: None,
        pay_from_wallet: false,
        device_id: ok.clone(),
        mode: SessionMode::Pass,
        player_id: None,
        pack_id: None,
        plan_id: None,
    });
    assert_eq!(
        refusal(submit(&f, owner(), pass).await),
        field(ErrorCode::ValidationFailed, "player_id")
    );
    let mut with_rev = intent(owner(), start_of(&ok, None));
    with_rev.base_rev = Some(1);
    assert_eq!(
        refusal(f.writer.submit_intent(with_rev).await),
        field(ErrorCode::ValidationFailed, "base_rev")
    );
    assert_eq!(
        refusal(submit(&f, kitchen, start_of(&ok, None)).await).0,
        ErrorCode::PermissionDenied
    );
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    assert!(sessions(&f).is_empty());
    submit(&f, counter, start_of(&ok, None)).await.unwrap();
}

#[tokio::test]
async fn a_busy_pc_is_refused_and_power_commands_are_blocked() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(100)).await;
    start(&f, &d, None).await.unwrap();
    let before = reopen(&f).changes().unwrap().len();
    assert_eq!(
        refusal(start(&f, &d, None).await),
        (ErrorCode::PcBusy, Some("device_id".into()))
    );
    for action in [PcAction::Restart, PcAction::Shutdown] {
        let power = IntentPayload::DevicePower(DevicePower {
            device_id: d.clone(),
            action,
        });
        assert_eq!(
            refusal(submit(&f, owner(), power).await).0,
            ErrorCode::PcBusy
        );
    }
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    assert_eq!(sessions(&f).len(), 1);
}

#[tokio::test]
async fn a_pc_held_in_session_without_a_live_row_is_busy() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(100)).await;
    f.writer
        .submit({
            let d = d.clone();
            move |tx| arena_store::set_device_state(tx, &d, "in_session", None, None)
        })
        .await
        .unwrap();
    assert_eq!(
        refusal(start(&f, &d, None).await),
        (ErrorCode::PcBusy, Some("device_id".into()))
    );
    let power = IntentPayload::DevicePower(DevicePower {
        device_id: d.clone(),
        action: PcAction::Restart,
    });
    assert_eq!(
        refusal(submit(&f, owner(), power).await).0,
        ErrorCode::PcBusy
    );
    assert!(sessions(&f).is_empty());
}

#[tokio::test]
async fn a_retried_key_returns_the_original_outcome_and_one_session() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(100)).await;
    let i = intent(owner(), start_of(&d, None));
    let first = f.writer.submit_intent(i.clone()).await.unwrap();
    let again = f.writer.submit_intent(i).await.unwrap();
    assert_eq!(first, again);
    assert_eq!(sessions(&f).len(), 1);
}

#[tokio::test]
async fn the_database_allows_one_live_session_per_device() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(100)).await;
    let g = text(
        &reopen(&f).rows(Table::DeviceGroups).unwrap()[0],
        "group_id",
    );
    let sql = move |id: &str, state: &str, guest: i64| {
        format!(
            "INSERT INTO sessions (id, device_id, group_id, mode, state, started_at, rounding_min,
                 minimum_min, grace_min, time_up_mode, epoch, business_day, fy, guest_no, rev,
                 created_at, updated_at)
             VALUES ('{id}', '{d}', '{g}', 'postpaid', '{state}', 1, 15, 30, 2, 'clean_up', 'e',
                     'd', 'fy', {guest}, 0, 0, 0)"
        )
    };
    f.writer
        .submit(move |tx| {
            let run = |q: String| tx.execute_batch(&q);
            run(sql("A", "ended", 1)).unwrap();
            run(sql("B", "paused", 2)).unwrap();
            assert!(run(sql("C", "active", 3)).is_err());
            assert!(run(sql("D", "ended", 2)).is_err());
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn two_simultaneous_starts_one_wins_and_the_other_is_busy() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(100)).await;
    let (a, b) = tokio::join!(start(&f, &d, None), start(&f, &d, None));
    let mut codes: Vec<_> = [a, b].into_iter().map(|r| r.map_err(|e| e.code)).collect();
    codes.sort_by_key(|r| r.is_err());
    assert!(codes[0].is_ok());
    assert_eq!(codes[1], Err(ErrorCode::PcBusy));
    assert_eq!(sessions(&f).len(), 1);
}

pub(super) fn end_intent(session: &str, rev: Option<i64>) -> Intent {
    let mut i = intent(
        owner(),
        IntentPayload::SessionEnd(arena_proto::SessionEnd {
            session_id: session.into(),
        }),
    );
    i.base_rev = rev;
    i
}

pub(super) async fn end(f: &Fixture, i: Intent) -> Result<Applied, AppError> {
    let mut i = i;
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

pub(super) fn table(f: &Fixture, t: Table) -> Vec<Row> {
    reopen(f).rows(t).unwrap()
}

#[tokio::test]
async fn ending_a_session_writes_an_open_order_one_line_and_frees_the_device() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(10_000)).await;
    let (player, _) = add(&f, owner(), &form("asha", "+919876543210"))
        .await
        .unwrap();
    let free = state_of(&f, &d);
    start(&f, &d, Some(&player)).await.unwrap();
    f.clock.advance(41 * 60_000);
    let s = sessions(&f)[0].clone();
    let live = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    assert_eq!(live.bills.len(), 1);
    let (b, l) = (&live.bills[0], &live.bills[0].lines[0]);
    assert_eq!(b.player_id.as_deref(), Some(player.as_str()));
    assert_eq!(
        (b.session_id.as_str(), b.order_id.as_deref()),
        (text(&s, "id").as_str(), None)
    );
    assert_eq!((l.minutes, l.amount_minor), (45, 7500));
    assert_eq!(
        (b.subtotal_minor, b.paid_minor, b.remaining_minor),
        (7500, 0, 7500)
    );
    let i = end_intent(&text(&s, "id"), Some(int(&s, "rev")));
    let a = end(&f, i.clone()).await.unwrap();
    let after = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    assert_eq!(after.bills.len(), 1);
    assert!(after.bills[0].order_id.is_some());
    assert_eq!(after.bills[0].player_id, b.player_id);
    assert_eq!(after.bills[0].lines, b.lines);
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        [
            "sessions",
            "orders",
            "session_segments",
            "order_lines",
            "devices_meta"
        ]
    );
    let s = &sessions(&f)[0];
    assert_eq!(text(s, "state"), "ended");
    assert_eq!(text(s, "end_reason"), "staff");
    assert_eq!(int(s, "ended_at"), T0 + 41 * 60_000);
    assert_eq!(int(s, "rev"), 2);
    let o = &table(&f, Table::Orders)[0];
    assert_eq!(text(o, "status"), "open");
    assert_eq!(text(o, "session_id"), text(s, "id"));
    assert_eq!(text(o, "player_id"), player);
    assert_eq!(text(o, "device_id"), d);
    assert_eq!(
        (int(o, "subtotal_minor"), int(o, "total_minor")),
        (7500, 7500)
    );
    assert_eq!(text(o, "business_day"), text(s, "business_day"));
    let seg = &table(&f, Table::SessionSegments)[0];
    assert_eq!(int(seg, "to_at"), T0 + 41 * 60_000);
    let l = &table(&f, Table::OrderLines);
    assert_eq!(l.len(), 1);
    assert_eq!(text(&l[0], "order_id"), text(o, "id"));
    assert_eq!(text(&l[0], "session_segment_id"), text(seg, "id"));
    assert_eq!(
        (
            int(&l[0], "minutes"),
            int(&l[0], "rate_minor_per_hour"),
            int(&l[0], "amount_minor"),
            int(&l[0], "position")
        ),
        (45, 10_000, 7500, 0)
    );
    assert_eq!(state_of(&f, &d), free);

    let again = end(&f, i).await.unwrap();
    assert_eq!(again.intent_id, a.intent_id);
    assert_eq!(table(&f, Table::Orders).len(), 1);
    assert_eq!(table(&f, Table::OrderLines).len(), 1);
}

#[tokio::test]
async fn a_short_guest_session_bills_the_minimum_and_a_new_session_can_start() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(10_000)).await;
    start(&f, &d, None).await.unwrap();
    f.clock.advance(10 * 60_000);
    let s = sessions(&f)[0].clone();
    end(&f, end_intent(&text(&s, "id"), Some(int(&s, "rev"))))
        .await
        .unwrap();
    let o = &table(&f, Table::Orders)[0];
    assert_eq!(int(o, "subtotal_minor"), 5000);
    assert_eq!(int(o, "guest_no"), 1);
    assert_eq!(o["player_id"], RowValue::Null);
    assert_eq!(int(&table(&f, Table::OrderLines)[0], "minutes"), 30);
    start(&f, &d, None).await.unwrap();
}

#[tokio::test]
async fn ending_is_refused_without_writing_for_each_bad_case() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(10_000)).await;
    start(&f, &d, None).await.unwrap();
    let s = sessions(&f)[0].clone();
    let id = text(&s, "id");
    let before = reopen(&f).changes().unwrap().len();
    let field = |c, f: &str| (c, Some(f.to_owned()));
    assert_eq!(
        refusal(end(&f, end_intent("nope", Some(1))).await),
        field(ErrorCode::NotFound, "session_id")
    );
    assert_eq!(
        refusal(end(&f, end_intent(&id, None)).await),
        field(ErrorCode::ValidationFailed, "base_rev")
    );
    assert_eq!(
        refusal(end(&f, end_intent(&id, Some(9))).await),
        field(ErrorCode::Stale, "base_rev")
    );
    let counter = staff(&f, "ctr", ROLE_COUNTER).await;
    let kitchen = staff(&f, "kit", ROLE_KITCHEN).await;
    let mut denied = end_intent(&id, Some(1));
    denied.actor = kitchen;
    assert_eq!(
        refusal(end(&f, denied).await).0,
        ErrorCode::PermissionDenied
    );
    assert_eq!(reopen(&f).changes().unwrap().len(), before + 2);
    let mut by_counter = end_intent(&id, Some(1));
    by_counter.actor = counter;
    end(&f, by_counter).await.unwrap();
    assert_eq!(
        refusal(end(&f, end_intent(&id, Some(1))).await),
        field(ErrorCode::Stale, "base_rev")
    );
    assert_eq!(
        refusal(end(&f, end_intent(&id, Some(2))).await),
        field(ErrorCode::ValidationFailed, "session_id")
    );
    assert_eq!(table(&f, Table::Orders).len(), 1);
}

#[tokio::test]
async fn a_database_allows_one_order_per_session() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(100)).await;
    start(&f, &d, None).await.unwrap();
    let sid = text(&sessions(&f)[0], "id");
    f.writer
        .submit(move |tx| {
            let sql = |id: &str| {
                format!(
                    "INSERT INTO orders (id, session_id, status, subtotal_minor, total_minor,
                         business_day, fy, rev, created_at, updated_at)
                     VALUES ('{id}', '{sid}', 'open', 0, 0, 'd', 'fy', 0, 0, 0)"
                )
            };
            tx.execute_batch(&sql("A")).unwrap();
            assert!(tx.execute_batch(&sql("B")).is_err());
            Ok(())
        })
        .await
        .unwrap();
}

pub(super) fn pay(
    method: arena_proto::PaymentMethod,
    amount: i64,
    reference: Option<&str>,
) -> arena_proto::PaymentRow {
    arena_proto::PaymentRow {
        method,
        amount_minor: amount,
        reference: reference.map(Into::into),
    }
}

pub(super) fn settle_intent(
    order: &str,
    rev: Option<i64>,
    payments: Vec<arena_proto::PaymentRow>,
) -> Intent {
    let mut i = intent(
        owner(),
        IntentPayload::SessionSettle(arena_proto::SessionSettle {
            business: None,
            coupon: None,
            discount: None,
            order_id: order.into(),
            credit_override: None,
            payments,
            receipt: None,
        }),
    );
    i.base_rev = rev;
    i
}

pub(super) async fn billed(f: &Fixture, rate: i64, minutes: i64) -> (String, String, i64) {
    let d = pc(f, DeviceKind::Pc, "pc", Some(rate)).await;
    start(f, &d, None).await.unwrap();
    f.clock.advance(minutes * 60_000);
    let s = sessions(f).pop().unwrap();
    end(f, end_intent(&text(&s, "id"), Some(int(&s, "rev"))))
        .await
        .unwrap();
    let o = table(f, Table::Orders).pop().unwrap();
    (text(&o, "id"), d, int(&o, "rev"))
}

#[tokio::test]
async fn settling_writes_payments_and_closes_the_order() {
    use arena_proto::PaymentMethod::{Cash, Upi};
    let f = bootstrapped().await;
    let (order, _, rev) = billed(&f, 10_000, 41).await;
    let before = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    assert_eq!(before.bills[0].order_rev, Some(rev));
    let i = settle_intent(
        &order,
        Some(rev),
        vec![pay(Cash, 5000, None), pay(Upi, 2500, Some("1234"))],
    );
    let a = end(&f, i.clone()).await.unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        ["payments", "payments", "cash_events", "orders"]
    );
    let o = &table(&f, Table::Orders)[0];
    assert_eq!(text(o, "status"), "settled");
    assert_eq!(int(o, "settled_at"), f.clock.now_ms());
    assert_eq!(int(o, "rev"), rev + 1);
    let p = table(&f, Table::Payments);
    assert_eq!(p.len(), 2);
    let mut p = p;
    p.sort_by_key(|r| int(r, "position"));
    assert_eq!(
        (text(&p[0], "method"), int(&p[0], "amount_minor")),
        ("cash".into(), 5000)
    );
    assert_eq!(p[0]["reference"], RowValue::Null);
    assert_eq!(text(&p[1], "reference"), "1234");
    assert_eq!(text(&p[1], "order_id"), order);
    assert_eq!(text(&p[1], "business_day"), text(o, "business_day"));
    let after = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    assert!(after.bills.is_empty());
    let s = reopen(&f).settlement(&order).unwrap().unwrap();
    assert_eq!(s.settled_at, f.clock.now_ms());
    assert!(!s.device_busy);

    let again = end(&f, i).await.unwrap();
    assert_eq!(again.intent_id, a.intent_id);
    assert_eq!(table(&f, Table::Payments).len(), 2);
}

#[tokio::test]
async fn a_settle_with_the_wrong_total_or_a_bad_reference_writes_nothing() {
    use arena_proto::PaymentMethod::{Card, Cash, Upi};
    let f = bootstrapped().await;
    let (order, _, rev) = billed(&f, 10_000, 41).await;
    let before = reopen(&f).changes().unwrap().len();
    let field = |c, f: &str| (c, Some(f.to_owned()));
    let try_settle = |payments| {
        let f = &f;
        let order = order.clone();
        async move { refusal(end(f, settle_intent(&order, Some(rev), payments)).await) }
    };
    assert_eq!(
        try_settle(vec![pay(Cash, 7000, None)]).await,
        field(ErrorCode::PaymentShort, "payments")
    );
    assert_eq!(
        try_settle(vec![pay(Cash, 8000, None)]).await,
        field(ErrorCode::PaymentShort, "payments")
    );
    assert_eq!(
        try_settle(vec![]).await,
        field(ErrorCode::PaymentShort, "payments")
    );
    for bad in [None, Some("123"), Some("12a4"), Some("12345"), Some("  ")] {
        assert_eq!(
            try_settle(vec![pay(Cash, 5000, None), pay(Upi, 2500, bad)]).await,
            field(ErrorCode::ReferenceRequired, "payments[1].reference"),
            "{bad:?}"
        );
    }
    assert_eq!(
        try_settle(vec![pay(Card, 7500, None)]).await,
        field(ErrorCode::ReferenceRequired, "payments[0].reference")
    );
    assert_eq!(
        try_settle(vec![pay(Card, 7500, Some(&"x".repeat(65)))]).await,
        field(ErrorCode::ReferenceRequired, "payments[0].reference")
    );
    assert_eq!(
        try_settle(vec![pay(Cash, 7500, Some("12"))]).await,
        field(ErrorCode::ValidationFailed, "payments[0].reference")
    );
    assert_eq!(
        try_settle(vec![pay(Cash, 0, None), pay(Cash, 7500, None)]).await,
        field(ErrorCode::ValidationFailed, "payments[0].amount_minor")
    );
    assert_eq!(
        try_settle(vec![pay(Cash, -100, None), pay(Cash, 7600, None)]).await,
        field(ErrorCode::ValidationFailed, "payments[0].amount_minor")
    );
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    assert!(table(&f, Table::Payments).is_empty());
    assert_eq!(text(&table(&f, Table::Orders)[0], "status"), "open");
    end(
        &f,
        settle_intent(&order, Some(rev), vec![pay(Card, 7500, Some("TID-9"))]),
    )
    .await
    .unwrap();
    assert_eq!(text(&table(&f, Table::Payments)[0], "reference"), "TID-9");
}

#[tokio::test]
async fn settle_is_refused_for_an_unknown_missing_stale_or_settled_order() {
    use arena_proto::PaymentMethod::Cash;
    let f = bootstrapped().await;
    let (order, _, rev) = billed(&f, 10_000, 41).await;
    let field = |c, f: &str| (c, Some(f.to_owned()));
    let all = || vec![pay(Cash, 7500, None)];
    assert_eq!(
        refusal(end(&f, settle_intent("nope", Some(1), all())).await),
        field(ErrorCode::NotFound, "order_id")
    );
    assert_eq!(
        refusal(end(&f, settle_intent(&order, None, all())).await),
        field(ErrorCode::ValidationFailed, "base_rev")
    );
    assert_eq!(
        refusal(end(&f, settle_intent(&order, Some(rev + 5), all())).await),
        field(ErrorCode::Stale, "base_rev")
    );
    let mut denied = settle_intent(&order, Some(rev), all());
    denied.actor = staff(&f, "kit", ROLE_KITCHEN).await;
    assert_eq!(
        refusal(end(&f, denied).await).0,
        ErrorCode::PermissionDenied
    );
    let mut by_counter = settle_intent(&order, Some(rev), all());
    by_counter.actor = staff(&f, "ctr", ROLE_COUNTER).await;
    let staff_id = by_counter.actor.id.clone();
    end(&f, by_counter).await.unwrap();
    assert_eq!(text(&table(&f, Table::Payments)[0], "staff_id"), staff_id);
    assert_eq!(
        refusal(end(&f, settle_intent(&order, Some(rev), all())).await),
        field(ErrorCode::Stale, "base_rev")
    );
    assert_eq!(
        refusal(end(&f, settle_intent(&order, Some(rev + 1), all())).await),
        field(ErrorCode::Stale, "order_id")
    );
    assert_eq!(table(&f, Table::Payments).len(), 1);
}

#[tokio::test]
async fn a_new_session_on_the_device_marks_the_settlement_busy() {
    use arena_proto::PaymentMethod::Cash;
    let f = bootstrapped().await;
    let (order, device, rev) = billed(&f, 10_000, 10).await;
    start(&f, &device, None).await.unwrap();
    end(
        &f,
        settle_intent(&order, Some(rev), vec![pay(Cash, 5000, None)]),
    )
    .await
    .unwrap();
    assert!(reopen(&f).settlement(&order).unwrap().unwrap().device_busy);
}

fn with_actor(mut i: Intent, actor: Actor) -> Intent {
    i.actor = actor;
    i
}

pub(super) fn end_revert_intent(session: &str, rev: Option<i64>) -> Intent {
    let mut i = intent(
        owner(),
        IntentPayload::SessionEndReverted(arena_proto::SessionEndReverted {
            session_id: session.into(),
        }),
    );
    i.base_rev = rev;
    i
}

pub(super) fn settle_revert_intent(order: &str, rev: Option<i64>) -> Intent {
    let mut i = intent(
        owner(),
        IntentPayload::SessionSettleReverted(arena_proto::SessionSettleReverted {
            order_id: order.into(),
        }),
    );
    i.base_rev = rev;
    i
}

pub(super) fn rev_of(f: &Fixture, t: Table, id: &str) -> i64 {
    int(
        table(f, t).iter().find(|r| text(r, "id") == id).unwrap(),
        "rev",
    )
}

struct Ended {
    session: String,
    order: String,
    device: String,
    free: String,
    applied: Applied,
}

async fn ended_by(f: &Fixture, actor: Actor, rate: i64, minutes: i64) -> Ended {
    let device = pc(f, DeviceKind::Pc, "pc", Some(rate)).await;
    let free = state_of(f, &device);
    start(f, &device, None).await.unwrap();
    f.clock.advance(minutes * 60_000);
    let s = sessions(f).pop().unwrap();
    let session = text(&s, "id");
    let applied = end(
        f,
        with_actor(end_intent(&session, Some(int(&s, "rev"))), actor),
    )
    .await
    .unwrap();
    let order = table(f, Table::Orders)
        .pop()
        .map(|o| text(&o, "id"))
        .unwrap();
    Ended {
        session,
        order,
        device,
        free,
        applied,
    }
}

async fn settled_by(f: &Fixture, actor: Actor, order: &str) -> Applied {
    let rev = rev_of(f, Table::Orders, order);
    let total = int(
        table(f, Table::Orders)
            .iter()
            .find(|o| text(o, "id") == order)
            .unwrap(),
        "total_minor",
    );
    end(
        f,
        with_actor(
            settle_intent(
                order,
                Some(rev),
                vec![pay(arena_proto::PaymentMethod::Cash, total, None)],
            ),
            actor,
        ),
    )
    .await
    .unwrap()
}

pub(super) async fn record_effect(
    f: &Fixture,
    kind: arena_proto::EffectKind,
    intent: &str,
    order: &str,
) {
    let (intent, order) = (intent.to_owned(), order.to_owned());
    f.writer
        .submit(move |tx| {
            let id = ulid::Ulid::generate().to_string();
            let intent = (!intent.is_empty()).then_some(intent.as_str());
            let order = (!order.is_empty()).then_some(order.as_str());
            arena_store::insert_effect(tx, &id, kind, intent, order, 0)
        })
        .await
        .unwrap();
}

fn nothing_changed(f: &Fixture, before: usize) {
    assert_eq!(reopen(f).changes().unwrap().len(), before);
}

#[tokio::test]
async fn undoing_an_end_reopens_the_session_deletes_the_bill_and_bills_the_gap() {
    let f = bootstrapped().await;
    let e = ended_by(&f, owner(), 10_000, 41).await;
    let ended_at = f.clock.now_ms();
    let s = sessions(&f)[0].clone();
    assert_eq!(text(&s, "ended_by"), "o");
    assert_eq!(int(&s, "rev"), 2);
    assert_eq!(state_of(&f, &e.device), e.free);
    let seg_rev = rev_of(
        &f,
        Table::SessionSegments,
        &text(&table(&f, Table::SessionSegments)[0], "id"),
    );

    f.clock.advance(3_000);
    let i = end_revert_intent(&e.session, Some(2));
    let a = end(&f, i.clone()).await.unwrap();
    assert_eq!(
        a.changes
            .iter()
            .map(|c| (c.tbl.as_str(), c.op))
            .collect::<Vec<_>>(),
        [
            ("sessions", ChangeOp::Update),
            ("session_segments", ChangeOp::Update),
            ("order_lines", ChangeOp::Delete),
            ("orders", ChangeOp::Delete),
            ("devices_meta", ChangeOp::Update),
        ]
    );
    let s = &sessions(&f)[0];
    assert_eq!(text(s, "state"), "active");
    for col in ["ended_at", "end_reason", "ended_by"] {
        assert_eq!(s[col], RowValue::Null, "{col}");
    }
    assert_eq!(int(s, "rev"), 3);
    let seg = &table(&f, Table::SessionSegments)[0];
    assert_eq!(seg["to_at"], RowValue::Null);
    assert_eq!(int(seg, "rev"), seg_rev + 1);
    assert!(table(&f, Table::Orders).is_empty());
    assert!(table(&f, Table::OrderLines).is_empty());
    assert_eq!(state_of(&f, &e.device), "in_session");
    let live = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    assert_eq!(live.sessions.len(), 1);
    assert_eq!(live.bills[0].order_id, None);

    let deleted = reopen(&f)
        .changes()
        .unwrap()
        .into_iter()
        .filter(|c| c.op == ChangeOp::Delete)
        .count();
    assert_eq!(deleted, 2);

    let again = end(&f, i).await.unwrap();
    assert_eq!(again.intent_id, a.intent_id);
    assert_eq!(sessions(&f)[0]["state"], RowValue::Text("active".into()));

    f.clock.advance(20 * 60_000);
    let s = sessions(&f)[0].clone();
    end(&f, end_intent(&e.session, Some(int(&s, "rev"))))
        .await
        .unwrap();
    assert!(f.clock.now_ms() - ended_at >= 20 * 60_000);
    let line = &table(&f, Table::OrderLines)[0];
    assert_eq!(int(line, "minutes"), 75);
    assert_eq!(int(&table(&f, Table::Orders)[0], "total_minor"), 12_500);
}

#[tokio::test]
async fn undoing_a_settle_deletes_the_payments_and_reopens_the_order() {
    use arena_proto::PaymentMethod::{Cash, Upi};
    let f = bootstrapped().await;
    let e = ended_by(&f, owner(), 10_000, 41).await;
    let rev = rev_of(&f, Table::Orders, &e.order);
    end(
        &f,
        settle_intent(
            &e.order,
            Some(rev),
            vec![pay(Cash, 5000, None), pay(Upi, 2500, Some("1234"))],
        ),
    )
    .await
    .unwrap();
    assert!(
        crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms())
            .unwrap()
            .bills
            .is_empty()
    );
    f.clock.advance(3_000);
    let i = settle_revert_intent(&e.order, Some(rev + 1));
    let a = end(&f, i.clone()).await.unwrap();
    assert_eq!(
        a.changes
            .iter()
            .map(|c| (c.tbl.as_str(), c.op))
            .collect::<Vec<_>>(),
        [
            ("cash_events", ChangeOp::Insert),
            ("payments", ChangeOp::Delete),
            ("payments", ChangeOp::Delete),
            ("orders", ChangeOp::Update),
        ]
    );
    assert!(table(&f, Table::Payments).is_empty());
    let o = &table(&f, Table::Orders)[0];
    assert_eq!(text(o, "status"), "open");
    assert_eq!(o["settled_at"], RowValue::Null);
    assert_eq!(o["settled_by"], RowValue::Null);
    assert_eq!(int(o, "rev"), rev + 2);
    let live = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    assert_eq!(live.bills.len(), 1);
    assert_eq!(live.bills[0].order_rev, Some(rev + 2));
    assert_eq!(live.bills[0].remaining_minor, 7500);
    assert_eq!(state_of(&f, &e.device), e.free);

    let again = end(&f, i).await.unwrap();
    assert_eq!(again.intent_id, a.intent_id);

    end(
        &f,
        settle_intent(&e.order, Some(rev + 2), vec![pay(Cash, 7500, None)]),
    )
    .await
    .unwrap();
    assert_eq!(table(&f, Table::Payments).len(), 1);
}

#[tokio::test]
async fn the_undo_window_is_eight_seconds_from_the_end_or_settle() {
    let f = bootstrapped().await;
    let e = ended_by(&f, owner(), 10_000, 10).await;
    let rev = rev_of(&f, Table::Sessions, &e.session);
    f.clock.advance(8_001);
    let before = reopen(&f).changes().unwrap().len();
    assert_eq!(
        refusal(end(&f, end_revert_intent(&e.session, Some(rev))).await),
        (ErrorCode::UndoNotAllowed, None)
    );
    nothing_changed(&f, before);

    let g = bootstrapped().await;
    let e = ended_by(&g, owner(), 10_000, 10).await;
    let rev = rev_of(&g, Table::Sessions, &e.session);
    g.clock.advance(8_000);
    end(&g, end_revert_intent(&e.session, Some(rev)))
        .await
        .unwrap();

    let h = bootstrapped().await;
    let e = ended_by(&h, owner(), 10_000, 10).await;
    settled_by(&h, owner(), &e.order).await;
    let rev = rev_of(&h, Table::Orders, &e.order);
    h.clock.advance(9_000);
    let before = reopen(&h).changes().unwrap().len();
    assert_eq!(
        refusal(end(&h, settle_revert_intent(&e.order, Some(rev))).await),
        (ErrorCode::UndoNotAllowed, None)
    );
    nothing_changed(&h, before);
    assert_eq!(table(&h, Table::Payments).len(), 1);
}

#[tokio::test]
async fn only_the_staff_member_who_did_it_can_undo() {
    let f = bootstrapped().await;
    let asha = staff(&f, "asha", ROLE_COUNTER).await;
    let ravi = staff(&f, "ravi", ROLE_COUNTER).await;
    let e = ended_by(&f, asha.clone(), 10_000, 10).await;
    let rev = rev_of(&f, Table::Sessions, &e.session);
    let before = reopen(&f).changes().unwrap().len();
    for other in [ravi.clone(), owner()] {
        assert_eq!(
            refusal(
                end(
                    &f,
                    with_actor(end_revert_intent(&e.session, Some(rev)), other)
                )
                .await
            ),
            (ErrorCode::UndoNotAllowed, None)
        );
    }
    nothing_changed(&f, before);
    end(
        &f,
        with_actor(end_revert_intent(&e.session, Some(rev)), asha.clone()),
    )
    .await
    .unwrap();

    let g = bootstrapped().await;
    let asha = staff(&g, "asha", ROLE_COUNTER).await;
    let ravi = staff(&g, "ravi", ROLE_COUNTER).await;
    let e = ended_by(&g, owner(), 10_000, 10).await;
    settled_by(&g, asha.clone(), &e.order).await;
    let rev = rev_of(&g, Table::Orders, &e.order);
    let before = reopen(&g).changes().unwrap().len();
    assert_eq!(
        refusal(
            end(
                &g,
                with_actor(settle_revert_intent(&e.order, Some(rev)), ravi)
            )
            .await
        ),
        (ErrorCode::UndoNotAllowed, None)
    );
    nothing_changed(&g, before);
    end(
        &g,
        with_actor(settle_revert_intent(&e.order, Some(rev)), asha),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn an_undo_needs_the_current_rev_and_a_target_in_the_right_state() {
    let f = bootstrapped().await;
    let e = ended_by(&f, owner(), 10_000, 10).await;
    let rev = rev_of(&f, Table::Sessions, &e.session);
    let field = |c, f: &str| (c, Some(f.to_owned()));
    assert_eq!(
        refusal(end(&f, end_revert_intent("nope", Some(1))).await),
        field(ErrorCode::NotFound, "session_id")
    );
    assert_eq!(
        refusal(end(&f, end_revert_intent(&e.session, None)).await),
        field(ErrorCode::ValidationFailed, "base_rev")
    );
    assert_eq!(
        refusal(end(&f, end_revert_intent(&e.session, Some(rev - 1))).await),
        field(ErrorCode::Stale, "base_rev")
    );
    assert_eq!(
        refusal(end(&f, settle_revert_intent("nope", Some(1))).await),
        field(ErrorCode::NotFound, "order_id")
    );
    assert_eq!(
        refusal(end(&f, settle_revert_intent(&e.order, None)).await),
        field(ErrorCode::ValidationFailed, "base_rev")
    );
    let order_rev = rev_of(&f, Table::Orders, &e.order);
    assert_eq!(
        refusal(end(&f, settle_revert_intent(&e.order, Some(order_rev + 3))).await),
        field(ErrorCode::Stale, "base_rev")
    );
    assert_eq!(
        refusal(end(&f, settle_revert_intent(&e.order, Some(order_rev))).await),
        field(ErrorCode::Stale, "order_id")
    );
    settled_by(&f, owner(), &e.order).await;
    let before = reopen(&f).changes().unwrap().len();
    assert_eq!(
        refusal(end(&f, settle_revert_intent(&e.order, Some(order_rev))).await),
        field(ErrorCode::Stale, "base_rev")
    );
    nothing_changed(&f, before);
    let live = sessions(&f)[0].clone();
    assert_eq!(text(&live, "state"), "ended");
}

#[tokio::test]
async fn an_end_cannot_be_undone_after_the_bill_was_settled_or_the_pc_was_reused() {
    let f = bootstrapped().await;
    let e = ended_by(&f, owner(), 10_000, 10).await;
    let rev = rev_of(&f, Table::Sessions, &e.session);
    settled_by(&f, owner(), &e.order).await;
    let before = reopen(&f).changes().unwrap().len();
    assert_eq!(
        refusal(end(&f, end_revert_intent(&e.session, Some(rev))).await),
        (ErrorCode::UndoNotAllowed, None)
    );
    nothing_changed(&f, before);
    assert_eq!(state_of(&f, &e.device), e.free);

    let g = bootstrapped().await;
    let e = ended_by(&g, owner(), 10_000, 10).await;
    let rev = rev_of(&g, Table::Sessions, &e.session);
    start(&g, &e.device, None).await.unwrap();
    let before = reopen(&g).changes().unwrap().len();
    assert_eq!(
        refusal(end(&g, end_revert_intent(&e.session, Some(rev))).await),
        (ErrorCode::UndoNotAllowed, None)
    );
    nothing_changed(&g, before);
    assert_eq!(table(&g, Table::Orders).len(), 1);
    assert_eq!(
        sessions(&g)
            .iter()
            .filter(|s| text(s, "state") == "active")
            .count(),
        1
    );
}

#[tokio::test]
async fn a_recorded_effect_blocks_undo_while_a_bill_with_none_stays_undoable() {
    use arena_proto::EffectKind;
    for kind in EffectKind::ALL {
        let f = bootstrapped().await;
        let e = ended_by(&f, owner(), 10_000, 10).await;
        let forward = e.applied.intent_id.clone();
        let rev = rev_of(&f, Table::Sessions, &e.session);
        record_effect(&f, kind, &forward, "").await;
        let before = reopen(&f).changes().unwrap().len();
        assert_eq!(
            refusal(end(&f, end_revert_intent(&e.session, Some(rev))).await),
            (ErrorCode::UndoNotAllowed, None),
            "{}",
            kind.as_str()
        );
        nothing_changed(&f, before);

        let g = bootstrapped().await;
        let e = ended_by(&g, owner(), 10_000, 10).await;
        let settled = settled_by(&g, owner(), &e.order).await;
        let rev = rev_of(&g, Table::Orders, &e.order);
        g.clock.advance(5_000);
        record_effect(&g, kind, &settled.intent_id, "").await;
        g.clock.advance(1_000);
        let before = reopen(&g).changes().unwrap().len();
        assert_eq!(
            refusal(end(&g, settle_revert_intent(&e.order, Some(rev))).await),
            (ErrorCode::UndoNotAllowed, None),
            "{}",
            kind.as_str()
        );
        nothing_changed(&g, before);
        assert_eq!(table(&g, Table::Payments).len(), 1);
    }

    let f = bootstrapped().await;
    let e = ended_by(&f, owner(), 10_000, 10).await;
    let settled = settled_by(&f, owner(), &e.order).await;
    let rev = rev_of(&f, Table::Orders, &e.order);
    f.clock.advance(7_000);
    let read = reopen(&f).settlement(&e.order).unwrap().unwrap();
    assert!(!read.irreversible);
    record_effect(&f, EffectKind::InvoiceNumberAssigned, "", &e.order).await;
    assert!(
        reopen(&f)
            .settlement(&e.order)
            .unwrap()
            .unwrap()
            .irreversible
    );
    assert_eq!(
        refusal(end(&f, settle_revert_intent(&e.order, Some(rev))).await),
        (ErrorCode::UndoNotAllowed, None)
    );
    assert_eq!(settled.changes.len(), 3);
}

#[tokio::test]
async fn the_end_outcome_reads_back_the_rev_total_and_effect_state() {
    let f = bootstrapped().await;
    let e = ended_by(&f, owner(), 10_000, 41).await;
    let ending = reopen(&f).ending(&e.session).unwrap().unwrap();
    assert_eq!(ending.order_id, e.order);
    assert_eq!(ending.ended_at, f.clock.now_ms());
    assert_eq!(ending.session_rev, rev_of(&f, Table::Sessions, &e.session));
    assert_eq!(ending.total_minor, 7500);
    assert!(!ending.irreversible);
    record_effect(
        &f,
        arena_proto::EffectKind::ReceiptPrinted,
        &e.applied.intent_id,
        "",
    )
    .await;
    assert!(reopen(&f).ending(&e.session).unwrap().unwrap().irreversible);
}

#[tokio::test]
async fn the_audit_links_a_reverted_intent_to_its_forward_intent() {
    let f = bootstrapped().await;
    let e = ended_by(&f, owner(), 10_000, 10).await;
    let settled = settled_by(&f, owner(), &e.order).await;
    let rev = rev_of(&f, Table::Orders, &e.order);
    f.clock.advance(1_000);
    let undone = end(&f, settle_revert_intent(&e.order, Some(rev)))
        .await
        .unwrap();
    let page = reopen(&f)
        .audit_page(&arena_store::AuditFilter::default(), 100)
        .unwrap();
    let reverted: Vec<_> = page
        .iter()
        .filter(|r| r.intent_id == undone.intent_id)
        .collect();
    assert_eq!(reverted.len(), 3);
    assert!(
        // The compensating cash event is a new row, so it has no earlier change to link to.
        reverted
            .iter()
            .filter(|r| r.tbl != "cash_events")
            .all(|r| r.reverts.as_deref() == Some(settled.intent_id.as_str()))
    );
    assert!(
        page.iter()
            .filter(|r| r.intent_id == settled.intent_id)
            .all(|r| r.reverts.is_none())
    );
    let change = reopen(&f)
        .changes()
        .unwrap()
        .into_iter()
        .find(|c| c.intent_id == undone.intent_id && c.tbl != "cash_events")
        .unwrap();
    let detail = crate::audit::read_detail(&reopen(&f), &change.epoch, change.seq)
        .unwrap()
        .unwrap();
    assert_eq!(
        detail.item.reverts.as_deref(),
        Some(settled.intent_id.as_str())
    );

    let g = bootstrapped().await;
    let e = ended_by(&g, owner(), 10_000, 10).await;
    let rev = rev_of(&g, Table::Sessions, &e.session);
    g.clock.advance(1_000);
    let undone = end(&g, end_revert_intent(&e.session, Some(rev)))
        .await
        .unwrap();
    let page = reopen(&g)
        .audit_page(&arena_store::AuditFilter::default(), 100)
        .unwrap();
    assert!(
        page.iter()
            .filter(|r| r.intent_id == undone.intent_id)
            .all(|r| r.reverts.as_deref() == Some(e.applied.intent_id.as_str()))
    );
}

fn pause_intent(
    session: &str,
    reason: arena_proto::PauseReason,
    note: Option<&str>,
    rev: Option<i64>,
) -> Intent {
    let mut i = intent(
        owner(),
        IntentPayload::SessionPause(arena_proto::SessionPause {
            session_id: session.into(),
            reason,
            note: note.map(Into::into),
        }),
    );
    i.base_rev = rev;
    i
}

fn resume_intent(session: &str, rev: Option<i64>) -> Intent {
    let mut i = intent(
        owner(),
        IntentPayload::SessionResume(arena_proto::SessionResume {
            session_id: session.into(),
        }),
    );
    i.base_rev = rev;
    i
}

async fn pause(
    f: &Fixture,
    session: &str,
    reason: arena_proto::PauseReason,
    note: Option<&str>,
) -> Result<Applied, AppError> {
    let rev = rev_of(f, Table::Sessions, session);
    end(f, pause_intent(session, reason, note, Some(rev))).await
}

async fn resume(f: &Fixture, session: &str) -> Result<Applied, AppError> {
    let rev = rev_of(f, Table::Sessions, session);
    end(f, resume_intent(session, Some(rev))).await
}

pub(super) fn row_of(f: &Fixture, t: Table, id: &str) -> Row {
    table(f, t)
        .into_iter()
        .find(|r| text(r, "id") == id)
        .unwrap()
}

async fn running(f: &Fixture, rate: i64) -> (String, String) {
    let d = pc(f, DeviceKind::Pc, "pc", Some(rate)).await;
    start(f, &d, None).await.unwrap();
    (text(&sessions(f)[0], "id"), d)
}

#[tokio::test]
async fn pausing_freezes_the_session_and_resuming_adds_the_paused_time() {
    use arena_proto::PauseReason::HardwareIssue;
    let f = bootstrapped().await;
    let (id, d) = running(&f, 6_000).await;
    f.clock.advance(20 * 60_000);
    let a = pause(&f, &id, HardwareIssue, None).await.unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        ["sessions"]
    );
    let s = row_of(&f, Table::Sessions, &id);
    assert_eq!(text(&s, "state"), "paused");
    assert_eq!(int(&s, "paused_at"), T0 + 20 * 60_000);
    assert_eq!(text(&s, "pause_reason"), "hardware_issue");
    assert_eq!(s["pause_note"], RowValue::Null);
    assert_eq!(int(&s, "rev"), 2);
    assert_eq!(state_of(&f, &d), "in_session");
    assert_eq!(
        refusal(start(&f, &d, None).await),
        (ErrorCode::PcBusy, Some("device_id".into()))
    );

    f.clock.advance(5 * 60_000);
    let snap = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    let live = &snap.sessions[0];
    assert_eq!(live.state, "paused");
    assert_eq!(live.paused_at, Some(T0 + 20 * 60_000));
    assert_eq!(live.paused_total_ms, 0);
    assert_eq!(live.pause_reason, Some(HardwareIssue));
    assert_eq!(snap.bills[0].lines[0].minutes, 30);
    let screens = crate::session_screens(&reopen(&f), f.clock.now_ms()).unwrap();
    assert_eq!(screens[0].1.elapsed_ms, 20 * 60_000);
    assert_eq!(screens[0].1.remaining_ms, None);
    assert_eq!(screens[0].1.balance_minor, None);

    f.clock.advance(15 * 60_000);
    let a = resume(&f, &id).await.unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        ["sessions", "session_segments"]
    );
    let s = row_of(&f, Table::Sessions, &id);
    assert_eq!(text(&s, "state"), "active");
    assert_eq!(int(&s, "paused_total_ms"), 20 * 60_000);
    for col in ["paused_at", "pause_reason", "pause_note"] {
        assert_eq!(s[col], RowValue::Null, "{col}");
    }
    assert_eq!(int(&s, "rev"), 3);
    assert_eq!(
        int(&table(&f, Table::SessionSegments)[0], "paused_ms"),
        20 * 60_000
    );
    let screens = crate::session_screens(&reopen(&f), f.clock.now_ms()).unwrap();
    assert_eq!(screens[0].1.elapsed_ms, 20 * 60_000);
    f.clock.advance(10 * 60_000);
    let screens = crate::session_screens(&reopen(&f), f.clock.now_ms()).unwrap();
    assert_eq!(screens[0].1.elapsed_ms, 30 * 60_000);
}

#[tokio::test]
async fn other_needs_a_note_and_a_listed_reason_takes_none() {
    use arena_proto::PauseReason::{Other, PowerCut};
    let f = bootstrapped().await;
    let (id, _) = running(&f, 6_000).await;
    let before = reopen(&f).changes().unwrap().len();
    for (reason, note) in [
        (Other, None),
        (Other, Some("")),
        (Other, Some("   ")),
        (Other, Some("bad\u{7}note")),
        (Other, Some("x".repeat(141).leak() as &str)),
        (PowerCut, Some("storm")),
    ] {
        assert_eq!(
            refusal(pause(&f, &id, reason, note).await),
            (ErrorCode::ValidationFailed, Some("note".into())),
            "{reason:?} {note:?}"
        );
    }
    assert_eq!(text(&sessions(&f)[0], "state"), "active");
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    pause(&f, &id, Other, Some("  Fan noise ")).await.unwrap();
    let s = row_of(&f, Table::Sessions, &id);
    assert_eq!(text(&s, "pause_reason"), "other");
    assert_eq!(text(&s, "pause_note"), "Fan noise");
    resume(&f, &id).await.unwrap();
    assert_eq!(
        row_of(&f, Table::Sessions, &id)["pause_note"],
        RowValue::Null
    );
}

#[tokio::test]
async fn pause_and_resume_refuse_the_wrong_state_a_stale_or_missing_rev_and_change_nothing() {
    use arena_proto::PauseReason::PowerCut;
    let f = bootstrapped().await;
    let (id, _) = running(&f, 6_000).await;
    let before = reopen(&f).changes().unwrap().len();
    let invalid = (ErrorCode::ValidationFailed, Some("session_id".into()));
    let field = |c, f: &str| (c, Some(f.to_owned()));
    assert_eq!(refusal(resume(&f, &id).await), invalid);
    assert_eq!(
        refusal(end(&f, pause_intent(&id, PowerCut, None, None)).await),
        field(ErrorCode::ValidationFailed, "base_rev")
    );
    assert_eq!(
        refusal(end(&f, resume_intent(&id, None)).await),
        field(ErrorCode::ValidationFailed, "base_rev")
    );
    assert_eq!(
        refusal(end(&f, pause_intent(&id, PowerCut, None, Some(0))).await),
        field(ErrorCode::Stale, "base_rev")
    );
    assert_eq!(
        refusal(end(&f, resume_intent(&id, Some(0))).await),
        field(ErrorCode::Stale, "base_rev")
    );
    assert_eq!(
        refusal(end(&f, resume_intent("nope", Some(1))).await),
        field(ErrorCode::NotFound, "session_id")
    );
    nothing_changed(&f, before);
    pause(&f, &id, PowerCut, None).await.unwrap();
    let before = reopen(&f).changes().unwrap().len();
    assert_eq!(refusal(pause(&f, &id, PowerCut, None).await), invalid);
    nothing_changed(&f, before);
    resume(&f, &id).await.unwrap();
    let before = reopen(&f).changes().unwrap().len();
    assert_eq!(refusal(resume(&f, &id).await), invalid);
    nothing_changed(&f, before);
    let ended = rev_of(&f, Table::Sessions, &id);
    end(&f, end_intent(&id, Some(ended))).await.unwrap();
    let before = reopen(&f).changes().unwrap().len();
    let rev = rev_of(&f, Table::Sessions, &id);
    assert_eq!(
        refusal(end(&f, pause_intent(&id, PowerCut, None, Some(rev))).await),
        invalid
    );
    assert_eq!(
        refusal(end(&f, resume_intent(&id, Some(rev))).await),
        invalid
    );
    nothing_changed(&f, before);
}

#[tokio::test]
async fn pausing_needs_the_pause_permission() {
    let f = bootstrapped().await;
    let (id, _) = running(&f, 6_000).await;
    let kitchen = staff(&f, "kit", ROLE_KITCHEN).await;
    let counter = staff(&f, "ctr", ROLE_COUNTER).await;
    let rev = rev_of(&f, Table::Sessions, &id);
    let i = with_actor(
        pause_intent(&id, arena_proto::PauseReason::PowerCut, None, Some(rev)),
        kitchen.clone(),
    );
    assert_eq!(
        end(&f, i).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    let i = with_actor(resume_intent(&id, Some(rev)), kitchen);
    assert_eq!(
        end(&f, i).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    let i = with_actor(
        pause_intent(&id, arena_proto::PauseReason::PowerCut, None, Some(rev)),
        counter,
    );
    end(&f, i).await.unwrap();
}

#[tokio::test]
async fn golden_a_sixty_minute_session_with_a_twenty_minute_pause_bills_forty_before_rounding() {
    use arena_proto::PauseReason::PlayerSteppedAway;
    let f = bootstrapped().await;
    let (id, _) = running(&f, 6_000).await;
    f.clock.advance(20 * 60_000);
    pause(&f, &id, PlayerSteppedAway, None).await.unwrap();
    f.clock.advance(20 * 60_000);
    resume(&f, &id).await.unwrap();
    f.clock.advance(20 * 60_000);
    let live = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    assert_eq!(live.bills[0].lines[0].minutes, 45);
    let rev = rev_of(&f, Table::Sessions, &id);
    end(&f, end_intent(&id, Some(rev))).await.unwrap();
    let line = &table(&f, Table::OrderLines)[0];
    assert_eq!(
        (int(line, "minutes"), int(line, "amount_minor")),
        (45, 4_500)
    );
    assert_eq!(
        int(&table(&f, Table::SessionSegments)[0], "paused_ms"),
        20 * 60_000
    );
}

#[tokio::test]
async fn ending_a_paused_session_folds_the_pause_and_bills_to_the_end_time() {
    use arena_proto::PauseReason::HardwareIssue;
    let f = bootstrapped().await;
    let (id, d) = running(&f, 6_000).await;
    f.clock.advance(40 * 60_000);
    pause(&f, &id, HardwareIssue, None).await.unwrap();
    f.clock.advance(35 * 60_000);
    let live = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    assert_eq!(live.bills[0].lines[0].minutes, 45);
    let rev = rev_of(&f, Table::Sessions, &id);
    end(&f, end_intent(&id, Some(rev))).await.unwrap();
    let s = row_of(&f, Table::Sessions, &id);
    assert_eq!(text(&s, "state"), "ended");
    assert_eq!(int(&s, "paused_total_ms"), 35 * 60_000);
    for col in ["paused_at", "pause_reason", "pause_note"] {
        assert_eq!(s[col], RowValue::Null, "{col}");
    }
    let seg = &table(&f, Table::SessionSegments)[0];
    assert_eq!(int(seg, "paused_ms"), 35 * 60_000);
    assert_eq!(int(seg, "to_at"), T0 + 75 * 60_000);
    assert_eq!(int(&table(&f, Table::OrderLines)[0], "minutes"), 45);
    assert_ne!(state_of(&f, &d), "in_session");

    let rev = rev_of(&f, Table::Sessions, &id);
    end(&f, end_revert_intent(&id, Some(rev))).await.unwrap();
    let s = row_of(&f, Table::Sessions, &id);
    assert_eq!(text(&s, "state"), "active");
    assert_eq!(s["paused_at"], RowValue::Null);
    assert_eq!(state_of(&f, &d), "in_session");
}

pub(super) fn topup_intent(
    player: &str,
    amount: i64,
    payments: Vec<arena_proto::PaymentRow>,
) -> Intent {
    intent(
        owner(),
        IntentPayload::WalletTopup(arena_proto::WalletTopup {
            player_id: player.into(),
            amount_minor: amount,
            payments,
        }),
    )
}

pub(super) fn topup_revert_intent(order: &str, rev: Option<i64>) -> Intent {
    let mut i = intent(
        owner(),
        IntentPayload::WalletTopupReverted(arena_proto::WalletTopupReverted {
            order_id: order.into(),
        }),
    );
    i.base_rev = rev;
    i
}

pub(super) fn balance(f: &Fixture, player: &str) -> i64 {
    int(
        table(f, Table::Wallets)
            .iter()
            .find(|w| text(w, "player_id") == player)
            .unwrap(),
        "balance_minor",
    )
}

pub(super) fn ledger_sum(f: &Fixture, player: &str) -> i64 {
    table(f, Table::WalletTxns)
        .iter()
        .filter(|t| text(t, "player_id") == player)
        .map(|t| int(t, "delta_minor"))
        .sum()
}

pub(super) async fn player(f: &Fixture, username: &str, phone: &str) -> String {
    add(f, owner(), &form(username, phone)).await.unwrap().0
}

pub(super) async fn topped_up(f: &Fixture, player: &str, amount: i64) -> String {
    end(
        f,
        topup_intent(
            player,
            amount,
            vec![pay(arena_proto::PaymentMethod::Cash, amount, None)],
        ),
    )
    .await
    .unwrap();
    table(f, Table::Orders)
        .iter()
        .rev()
        .find(|o| text(o, "kind") == "topup")
        .map(|o| text(o, "id"))
        .unwrap()
}

#[tokio::test]
async fn a_new_player_gets_a_zero_wallet_in_the_same_transaction() {
    let f = bootstrapped().await;
    let (id, applied) = add(&f, owner(), &form("asha", "+919876543210"))
        .await
        .unwrap();
    assert!(
        applied
            .changes
            .iter()
            .any(|c| c.tbl == "wallets" && c.op == ChangeOp::Insert && c.row_id == id)
    );
    assert_eq!(balance(&f, &id), 0);
}

#[tokio::test]
async fn a_top_up_writes_the_order_payments_ledger_row_and_balance() {
    use arena_proto::PaymentMethod::{Cash, Upi};
    let f = bootstrapped().await;
    let p = player(&f, "asha", "+919876543210").await;
    let split = || {
        topup_intent(
            &p,
            50_000,
            vec![pay(Cash, 30_000, None), pay(Upi, 20_000, Some("4321"))],
        )
    };
    let a = end(&f, split()).await.unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        [
            "orders",
            "payments",
            "payments",
            "cash_events",
            "wallets",
            "wallet_txns"
        ]
    );
    assert_eq!(balance(&f, &p), 50_000);
    let o = &table(&f, Table::Orders)[0];
    assert_eq!(text(o, "kind"), "topup");
    assert_eq!(text(o, "status"), "settled");
    assert_eq!(text(o, "player_id"), p);
    assert_eq!(o["session_id"], RowValue::Null);
    assert_eq!(int(o, "total_minor"), 50_000);
    assert_eq!(int(o, "settled_at"), f.clock.now_ms());
    let txns = table(&f, Table::WalletTxns);
    assert_eq!(txns.len(), 1);
    assert_eq!(int(&txns[0], "delta_minor"), 50_000);
    assert_eq!(text(&txns[0], "reason"), "topup");
    assert_eq!(text(&txns[0], "method"), "mixed");
    assert_eq!(text(&txns[0], "order_id"), text(o, "id"));
    assert_eq!(table(&f, Table::Payments).len(), 2);
    assert_eq!(ledger_sum(&f, &p), balance(&f, &p));
    let s = reopen(&f).settlement(&text(o, "id")).unwrap().unwrap();
    assert_eq!(s.total_minor, 50_000);
    assert!(
        crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms())
            .unwrap()
            .bills
            .is_empty()
    );

    let second = end(&f, split()).await.unwrap();
    assert_ne!(second.intent_id, a.intent_id);
    assert_eq!(balance(&f, &p), 100_000);
}

#[tokio::test]
async fn a_single_method_top_up_records_that_method() {
    let f = bootstrapped().await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    assert_eq!(text(&table(&f, Table::WalletTxns)[0], "method"), "cash");
}

#[tokio::test]
async fn a_bad_top_up_is_refused_and_writes_nothing() {
    use arena_proto::PaymentMethod::{Card, Cash, Upi, Wallet};
    let f = bootstrapped().await;
    let p = player(&f, "asha", "+919876543210").await;
    let before = reopen(&f).changes().unwrap().len();
    let mut with_rev = topup_intent(&p, 100, vec![pay(Cash, 100, None)]);
    with_rev.base_rev = Some(1);
    let cases: Vec<(Intent, ErrorCode, &str)> = vec![
        (
            topup_intent("nobody", 100, vec![pay(Cash, 100, None)]),
            ErrorCode::NotFound,
            "player_id",
        ),
        (
            topup_intent(&p, 0, vec![pay(Cash, 1, None)]),
            ErrorCode::ValidationFailed,
            "amount_minor",
        ),
        (
            topup_intent(&p, 10_000_001, vec![pay(Cash, 10_000_001, None)]),
            ErrorCode::ValidationFailed,
            "amount_minor",
        ),
        (
            topup_intent(&p, 100, vec![]),
            ErrorCode::ValidationFailed,
            "payments",
        ),
        (
            topup_intent(&p, 100, vec![pay(Wallet, 100, None)]),
            ErrorCode::ValidationFailed,
            "payments[0].method",
        ),
        (
            topup_intent(&p, 100, vec![pay(Cash, 60, None)]),
            ErrorCode::PaymentShort,
            "payments",
        ),
        (
            topup_intent(&p, 100, vec![pay(Upi, 100, Some("12"))]),
            ErrorCode::ReferenceRequired,
            "payments[0].reference",
        ),
        (
            topup_intent(&p, 100, vec![pay(Card, 100, None)]),
            ErrorCode::ReferenceRequired,
            "payments[0].reference",
        ),
        (with_rev, ErrorCode::ValidationFailed, "base_rev"),
    ];
    for (i, code, field) in cases {
        let (got, at) = refusal(end(&f, i).await);
        assert_eq!((got, at.as_deref()), (code, Some(field)));
    }
    nothing_changed(&f, before);
    assert_eq!(balance(&f, &p), 0);
}

#[tokio::test]
async fn a_top_up_needs_the_wallet_permission() {
    let f = bootstrapped().await;
    let p = player(&f, "asha", "+919876543210").await;
    let cash = || vec![pay(arena_proto::PaymentMethod::Cash, 100, None)];
    let kitchen = staff(&f, "kit", ROLE_KITCHEN).await;
    assert_eq!(
        refusal(end(&f, with_actor(topup_intent(&p, 100, cash()), kitchen)).await),
        (ErrorCode::PermissionDenied, None)
    );
    let counter = staff(&f, "cnt", ROLE_COUNTER).await;
    end(&f, with_actor(topup_intent(&p, 100, cash()), counter))
        .await
        .unwrap();
}

#[tokio::test]
async fn undoing_a_top_up_deletes_its_rows_and_lowers_the_balance() {
    let f = bootstrapped().await;
    let p = player(&f, "asha", "+919876543210").await;
    let order = topped_up(&f, &p, 20_000).await;
    let rev = rev_of(&f, Table::Orders, &order);
    f.clock.advance(3_000);
    let i = topup_revert_intent(&order, Some(rev));
    let a = end(&f, i.clone()).await.unwrap();
    assert_eq!(
        a.changes
            .iter()
            .map(|c| (c.tbl.as_str(), c.op))
            .collect::<Vec<_>>(),
        [
            ("cash_events", ChangeOp::Insert),
            ("payments", ChangeOp::Delete),
            ("wallet_txns", ChangeOp::Delete),
            ("wallets", ChangeOp::Update),
            ("orders", ChangeOp::Delete),
        ]
    );
    assert!(table(&f, Table::Orders).is_empty());
    assert!(table(&f, Table::Payments).is_empty());
    assert!(table(&f, Table::WalletTxns).is_empty());
    assert_eq!(balance(&f, &p), 0);
    let again = end(&f, i).await.unwrap();
    assert_eq!(again.intent_id, a.intent_id);
}

#[tokio::test]
async fn a_top_up_undo_is_refused_when_late_by_other_staff_stale_effected_or_spent() {
    use arena_proto::PaymentMethod::{Cash, Wallet};
    let f = bootstrapped().await;
    let asha = staff(&f, "asha", ROLE_COUNTER).await;
    let ravi = staff(&f, "ravi", ROLE_COUNTER).await;
    let p = player(&f, "kid", "+919876543211").await;
    end(
        &f,
        with_actor(
            topup_intent(&p, 20_000, vec![pay(Cash, 20_000, None)]),
            asha.clone(),
        ),
    )
    .await
    .unwrap();
    let order = text(&table(&f, Table::Orders)[0], "id");
    let rev = rev_of(&f, Table::Orders, &order);
    let before = reopen(&f).changes().unwrap().len();
    let revert =
        |rev: i64, actor: &Actor| with_actor(topup_revert_intent(&order, Some(rev)), actor.clone());
    assert_eq!(
        refusal(end(&f, revert(rev, &ravi)).await),
        (ErrorCode::UndoNotAllowed, None)
    );
    assert_eq!(
        refusal(end(&f, revert(rev + 5, &asha)).await),
        (ErrorCode::Stale, Some("base_rev".into()))
    );
    record_effect(&f, arena_proto::EffectKind::ReceiptPrinted, "", &order).await;
    assert_eq!(
        refusal(end(&f, revert(rev, &asha)).await),
        (ErrorCode::UndoNotAllowed, None)
    );
    nothing_changed(&f, before);

    let g = bootstrapped().await;
    let p = player(&g, "kid", "+919876543211").await;
    let order = topped_up(&g, &p, 20_000).await;
    let rev = rev_of(&g, Table::Orders, &order);
    let (bill, bill_rev) = player_bill(&g, &p, 60_000, 60).await;
    end(
        &g,
        settle_intent(
            &bill,
            Some(bill_rev),
            vec![pay(Wallet, 20_000, None), pay(Cash, 40_000, None)],
        ),
    )
    .await
    .unwrap();
    assert_eq!(balance(&g, &p), 0);
    assert_eq!(
        refusal(end(&g, topup_revert_intent(&order, Some(rev))).await),
        (ErrorCode::UndoNotAllowed, None)
    );

    let h = bootstrapped().await;
    let p = player(&h, "kid", "+919876543211").await;
    let order = topped_up(&h, &p, 20_000).await;
    let rev = rev_of(&h, Table::Orders, &order);
    h.clock.advance(9_000);
    assert_eq!(
        refusal(end(&h, topup_revert_intent(&order, Some(rev))).await),
        (ErrorCode::UndoNotAllowed, None)
    );
}

pub(super) async fn player_bill(
    f: &Fixture,
    player: &str,
    rate: i64,
    minutes: i64,
) -> (String, i64) {
    let d = pc(f, DeviceKind::Pc, "pc", Some(rate)).await;
    start(f, &d, Some(player)).await.unwrap();
    f.clock.advance(minutes * 60_000);
    let s = sessions(f).pop().unwrap();
    end(f, end_intent(&text(&s, "id"), Some(int(&s, "rev"))))
        .await
        .unwrap();
    let o = table(f, Table::Orders)
        .into_iter()
        .rfind(|o| text(o, "kind") == "session" && text(o, "status") == "open")
        .unwrap();
    (text(&o, "id"), int(&o, "rev"))
}

async fn billed_guest(f: &Fixture) -> (String, i64) {
    let d = pc(f, DeviceKind::Pc, "pc", Some(10_000)).await;
    start(f, &d, None).await.unwrap();
    f.clock.advance(30 * 60_000);
    let s = sessions(f).pop().unwrap();
    end(f, end_intent(&text(&s, "id"), Some(int(&s, "rev"))))
        .await
        .unwrap();
    let o = table(f, Table::Orders)
        .into_iter()
        .rfind(|o| o["player_id"] == RowValue::Null)
        .unwrap();
    (text(&o, "id"), int(&o, "rev"))
}

#[tokio::test]
async fn a_bill_is_paid_partly_from_the_wallet_and_the_ledger_follows() {
    use arena_proto::PaymentMethod::{Cash, Wallet};
    let f = bootstrapped().await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 20_000).await;
    f.clock.advance(60_000);
    let (order, rev) = player_bill(&f, &p, 12_000, 60).await;
    let snap = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    assert_eq!(snap.bills[0].wallet_minor, Some(20_000));
    let a = end(
        &f,
        settle_intent(
            &order,
            Some(rev),
            vec![pay(Wallet, 7_000, None), pay(Cash, 5_000, None)],
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        [
            "payments",
            "payments",
            "cash_events",
            "orders",
            "wallet_txns",
            "wallets"
        ]
    );
    assert_eq!(balance(&f, &p), 13_000);
    assert_eq!(ledger_sum(&f, &p), 13_000);
    let spend = table(&f, Table::WalletTxns)
        .into_iter()
        .find(|t| text(t, "reason") == "bill")
        .unwrap();
    assert_eq!(int(&spend, "delta_minor"), -7_000);
    assert_eq!(text(&spend, "order_id"), order);
    assert_eq!(spend["method"], RowValue::Null);
    let wp = table(&f, Table::Payments)
        .into_iter()
        .find(|r| text(r, "method") == "wallet")
        .unwrap();
    assert_eq!(wp["reference"], RowValue::Null);

    f.clock.advance(2_000);
    end(&f, settle_revert_intent(&order, Some(rev + 1)))
        .await
        .unwrap();
    assert_eq!(balance(&f, &p), 20_000);
    assert_eq!(ledger_sum(&f, &p), 20_000);
    assert_eq!(table(&f, Table::WalletTxns).len(), 1);
}

#[tokio::test]
async fn a_wallet_payment_above_the_balance_or_on_a_guest_bill_is_refused() {
    use arena_proto::PaymentMethod::{Cash, Wallet};
    let f = bootstrapped().await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 5_000).await;
    f.clock.advance(60_000);
    let (order, rev) = player_bill(&f, &p, 12_000, 60).await;
    let before = reopen(&f).changes().unwrap().len();
    let settle = |payments| settle_intent(&order, Some(rev), payments);
    assert_eq!(
        refusal(end(&f, settle(vec![pay(Wallet, 12_000, None)])).await),
        (
            ErrorCode::InsufficientBalance,
            Some("payments[0].amount_minor".into())
        )
    );
    assert_eq!(
        refusal(
            end(
                &f,
                settle(vec![
                    pay(Wallet, 3_000, None),
                    pay(Wallet, 3_000, None),
                    pay(Cash, 6_000, None)
                ])
            )
            .await
        ),
        (
            ErrorCode::ValidationFailed,
            Some("payments[0].method".into())
        )
    );
    assert_eq!(
        refusal(
            end(
                &f,
                settle(vec![
                    pay(Wallet, 5_000, Some("1234")),
                    pay(Cash, 7_000, None)
                ])
            )
            .await
        ),
        (
            ErrorCode::ValidationFailed,
            Some("payments[0].reference".into())
        )
    );
    nothing_changed(&f, before);
    assert_eq!(balance(&f, &p), 5_000);

    let (guest_order, guest_rev) = billed_guest(&f).await;
    assert_eq!(
        refusal(
            end(
                &f,
                settle_intent(
                    &guest_order,
                    Some(guest_rev),
                    vec![pay(Wallet, 100, None), pay(Cash, 4_900, None)]
                )
            )
            .await
        ),
        (
            ErrorCode::ValidationFailed,
            Some("payments[0].method".into())
        )
    );
    let snap = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    let guest_bill = snap
        .bills
        .iter()
        .find(|b| b.order_id.as_deref() == Some(guest_order.as_str()))
        .unwrap();
    assert_eq!(guest_bill.wallet_minor, None);
}

#[tokio::test]
async fn a_wallet_never_goes_negative_when_two_bills_race_for_the_same_balance() {
    use arena_proto::PaymentMethod::{Cash, Wallet};
    let f = bootstrapped().await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 6_000).await;
    f.clock.advance(60_000);
    let (one, one_rev) = player_bill(&f, &p, 10_000, 60).await;
    let (two, two_rev) = player_bill(&f, &p, 10_000, 60).await;
    let both = || vec![pay(Wallet, 6_000, None), pay(Cash, 4_000, None)];
    end(&f, settle_intent(&one, Some(one_rev), both()))
        .await
        .unwrap();
    assert_eq!(
        refusal(end(&f, settle_intent(&two, Some(two_rev), both())).await).0,
        ErrorCode::InsufficientBalance
    );
    assert_eq!(balance(&f, &p), 0);
    assert_eq!(ledger_sum(&f, &p), 0);
}

pub(super) fn assign_intent(
    session: &str,
    player: &str,
    create: Option<arena_proto::PlayerCreate>,
    rev: Option<i64>,
) -> Intent {
    let mut i = intent(
        owner(),
        IntentPayload::SessionAssignPlayer(arena_proto::SessionAssignPlayer {
            session_id: session.into(),
            player_id: player.into(),
            create,
        }),
    );
    i.base_rev = rev;
    i
}

pub(super) fn new_profile(username: &str, phone: &str) -> (String, arena_proto::PlayerCreate) {
    match super::players::create(&form(username, phone)) {
        (id, IntentPayload::PlayerCreate(c)) => (id, c),
        _ => unreachable!(),
    }
}

fn session_row(f: &Fixture, id: &str) -> Row {
    row_of(f, Table::Sessions, id)
}

#[tokio::test]
async fn converting_a_running_guest_creates_the_profile_and_keeps_the_guest_number() {
    let f = bootstrapped().await;
    let (id, _) = running(&f, 6_000).await;
    let before = session_row(&f, &id);
    assert_eq!(before["player_id"], RowValue::Null);
    let guest = int(&before, "guest_no");
    let (player, create) = new_profile("asha", "+919876543210");
    let applied = end(
        &f,
        assign_intent(&id, &player, Some(create), Some(int(&before, "rev"))),
    )
    .await
    .unwrap();
    let tables: Vec<_> = applied.changes.iter().map(|c| c.tbl.as_str()).collect();
    assert!(tables.contains(&"players") && tables.contains(&"wallets"));
    assert!(tables.contains(&"consents") && tables.contains(&"sessions"));
    let after = session_row(&f, &id);
    assert_eq!(text(&after, "player_id"), player);
    assert_eq!(int(&after, "guest_no"), guest);
    assert_eq!(int(&after, "rev"), int(&before, "rev") + 1);
    assert_eq!(balance(&f, &player), 0);
}

#[tokio::test]
async fn converting_an_unpaid_guest_moves_the_bill_to_the_player() {
    let f = bootstrapped().await;
    let (id, _) = running(&f, 6_000).await;
    f.clock.advance(60 * 60_000);
    let s = session_row(&f, &id);
    end(&f, end_intent(&id, Some(int(&s, "rev"))))
        .await
        .unwrap();
    let order = table(&f, Table::Orders).pop().unwrap();
    assert_eq!(order["player_id"], RowValue::Null);
    let (player, create) = new_profile("asha", "+919876543210");
    let rev = rev_of(&f, Table::Sessions, &id);
    let applied = end(&f, assign_intent(&id, &player, Some(create), Some(rev)))
        .await
        .unwrap();
    assert!(applied.changes.iter().any(|c| c.tbl == "orders"));
    let after = row_of(&f, Table::Orders, &text(&order, "id"));
    assert_eq!(text(&after, "player_id"), player);
    assert_eq!(int(&after, "rev"), int(&order, "rev") + 1);
    topped_up(&f, &player, 6_000).await;
    end(
        &f,
        settle_intent(
            &text(&order, "id"),
            Some(int(&after, "rev")),
            vec![pay(arena_proto::PaymentMethod::Wallet, 6_000, None)],
        ),
    )
    .await
    .unwrap();
    assert_eq!(balance(&f, &player), 0);
}

#[tokio::test]
async fn linking_to_an_existing_player_attaches_the_session_and_its_bill() {
    let f = bootstrapped().await;
    let p = player(&f, "asha", "+919876543210").await;
    let (id, _) = running(&f, 6_000).await;
    f.clock.advance(30 * 60_000);
    let s = session_row(&f, &id);
    end(&f, end_intent(&id, Some(int(&s, "rev"))))
        .await
        .unwrap();
    let rev = rev_of(&f, Table::Sessions, &id);
    let applied = end(&f, assign_intent(&id, &p, None, Some(rev)))
        .await
        .unwrap();
    assert!(applied.changes.iter().all(|c| c.tbl != "players"));
    assert_eq!(text(&session_row(&f, &id), "player_id"), p);
    assert_eq!(
        text(&table(&f, Table::Orders).pop().unwrap(), "player_id"),
        p
    );
}

#[tokio::test]
async fn a_conversion_is_refused_for_each_bad_case_and_writes_nothing() {
    let f = bootstrapped().await;
    let p = player(&f, "asha", "+919876543210").await;
    let (id, _) = running(&f, 6_000).await;
    let rev = rev_of(&f, Table::Sessions, &id);
    let changes = || reopen(&f).changes().unwrap().len();
    let before = changes();
    let (other, create) = new_profile("ben", "+919876543211");
    assert_eq!(
        refusal(end(&f, assign_intent(&id, &other, Some(create.clone()), None)).await).0,
        ErrorCode::ValidationFailed
    );
    assert_eq!(
        refusal(
            end(
                &f,
                assign_intent(&id, &other, Some(create.clone()), Some(rev + 5))
            )
            .await
        )
        .0,
        ErrorCode::Stale
    );
    assert_eq!(
        refusal(end(&f, assign_intent("nope", &p, None, Some(rev))).await).0,
        ErrorCode::NotFound
    );
    assert_eq!(
        refusal(
            end(
                &f,
                assign_intent(&id, "01HZZZZZZZZZZZZZZZZZZZZZZZ", None, Some(rev))
            )
            .await
        )
        .0,
        ErrorCode::NotFound
    );
    let (_, taken) = new_profile("asha2", "+919876543210");
    let (taken_id, taken) = (taken.player_id.clone(), taken);
    assert_eq!(
        refusal(end(&f, assign_intent(&id, &taken_id, Some(taken), Some(rev))).await).0,
        ErrorCode::PhoneTaken
    );
    assert_eq!(
        refusal(end(&f, assign_intent(&id, &p, Some(create), Some(rev))).await).0,
        ErrorCode::ValidationFailed
    );
    nothing_changed(&f, before);
    end(&f, assign_intent(&id, &p, None, Some(rev)))
        .await
        .unwrap();
    let rev = rev_of(&f, Table::Sessions, &id);
    assert_eq!(
        refusal(end(&f, assign_intent(&id, &p, None, Some(rev))).await).0,
        ErrorCode::ValidationFailed
    );
}

#[tokio::test]
async fn a_settled_guest_bill_cannot_be_converted() {
    let f = bootstrapped().await;
    let p = player(&f, "asha", "+919876543210").await;
    let (order, order_rev) = billed_guest(&f).await;
    end(
        &f,
        settle_intent(
            &order,
            Some(order_rev),
            vec![pay(arena_proto::PaymentMethod::Cash, 5_000, None)],
        ),
    )
    .await
    .unwrap();
    let s = sessions(&f).pop().unwrap();
    let rev = int(&s, "rev");
    assert_eq!(
        refusal(end(&f, assign_intent(&text(&s, "id"), &p, None, Some(rev))).await).0,
        ErrorCode::ValidationFailed
    );
}

#[tokio::test]
async fn creating_a_profile_in_a_conversion_needs_player_manage() {
    use arena_proto::{PermEffect, RolePerm, RoleUpsert};
    let f = bootstrapped().await;
    let (id, _) = running(&f, 6_000).await;
    submit(
        &f,
        owner(),
        IntentPayload::RoleUpsert(RoleUpsert {
            role_id: None,
            name: "Greeter".into(),
            perms: vec![RolePerm {
                permission: "session.start".into(),
                effect: PermEffect::Allow,
            }],
        }),
    )
    .await
    .unwrap();
    let greeter = super::stock::staff(&f, "greet", "Greeter").await;
    let rev = rev_of(&f, Table::Sessions, &id);
    let (player, create) = new_profile("asha", "+919876543210");
    let denied = with_actor(
        assign_intent(&id, &player, Some(create), Some(rev)),
        greeter.clone(),
    );
    assert_eq!(
        end(&f, denied).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    let existing = self::player(&f, "ben", "+919876543211").await;
    end(
        &f,
        with_actor(assign_intent(&id, &existing, None, Some(rev)), greeter),
    )
    .await
    .unwrap();
}

pub(super) fn credit_intent(player: &str, limit: i64, days: &[u16]) -> Intent {
    intent(
        owner(),
        IntentPayload::CreditSetLimit(arena_proto::CreditSetLimit {
            player_id: player.into(),
            limit_minor: limit,
            reminder_days: days.to_vec(),
        }),
    )
}

fn credit_row(f: &Fixture, player: &str) -> Row {
    table(f, Table::CreditBalances)
        .into_iter()
        .find(|c| text(c, "player_id") == player)
        .unwrap()
}

pub(super) fn owed(f: &Fixture, player: &str) -> i64 {
    int(&credit_row(f, player), "outstanding_minor")
}

pub(super) async fn credit_player(f: &Fixture, username: &str, phone: &str, limit: i64) -> String {
    let p = player(f, username, phone).await;
    end(f, credit_intent(&p, limit, &[3, 7, 14])).await.unwrap();
    p
}

async fn billed_for(f: &Fixture, player: &str, rate: i64, minutes: i64) -> (String, i64) {
    let d = pc(f, DeviceKind::Pc, "pc", Some(rate)).await;
    start(f, &d, Some(player)).await.unwrap();
    f.clock.advance(minutes * 60_000);
    let s = sessions(f).pop().unwrap();
    end(f, end_intent(&text(&s, "id"), Some(int(&s, "rev"))))
        .await
        .unwrap();
    let o = table(f, Table::Orders).pop().unwrap();
    (text(&o, "id"), int(&o, "rev"))
}

fn settle_on_credit(order: &str, rev: i64, amount: i64, over: bool) -> Intent {
    let mut i = settle_intent(
        order,
        Some(rev),
        vec![pay(arena_proto::PaymentMethod::Credit, amount, None)],
    );
    if let IntentPayload::SessionSettle(s) = &mut i.payload {
        s.credit_override = over.then_some(true);
    }
    i
}

#[tokio::test]
async fn setting_a_credit_limit_creates_then_edits_the_row() {
    let f = bootstrapped().await;
    let p = player(&f, "asha", "+919876543210").await;
    let a = end(&f, credit_intent(&p, 50_000, &[3, 7, 14]))
        .await
        .unwrap();
    assert_eq!(a.changes.len(), 1);
    assert_eq!(a.changes[0].tbl, "credit_balances");
    assert_eq!(a.changes[0].op, ChangeOp::Insert);
    let row = credit_row(&f, &p);
    assert_eq!(int(&row, "limit_minor"), 50_000);
    assert_eq!(int(&row, "outstanding_minor"), 0);
    assert_eq!(int(&row, "credit_paused"), 0);
    assert_eq!(row["last_override_day"], RowValue::Null);
    assert_eq!(text(&row, "reminder_days"), "3,7,14");
    let b = end(&f, credit_intent(&p, 80_000, &[2, 10])).await.unwrap();
    assert_eq!(b.changes[0].op, ChangeOp::Update);
    let row = credit_row(&f, &p);
    assert_eq!(int(&row, "limit_minor"), 80_000);
    assert_eq!(text(&row, "reminder_days"), "2,10");
    assert_eq!(table(&f, Table::CreditBalances).len(), 1);
}

#[tokio::test]
async fn a_bad_credit_limit_is_refused_and_writes_nothing() {
    let f = bootstrapped().await;
    let p = player(&f, "asha", "+919876543210").await;
    let ctr = staff(&f, "ctr", ROLE_COUNTER).await;
    let before = reopen(&f).changes().unwrap().len();
    let field = |c, s: &str| (c, Some(s.to_owned()));
    let cases = vec![
        (
            credit_intent("nobody", 100, &[3]),
            field(ErrorCode::NotFound, "player_id"),
        ),
        (
            credit_intent(&p, -1, &[3]),
            field(ErrorCode::ValidationFailed, "limit_minor"),
        ),
        (
            credit_intent(&p, 10_000_001, &[3]),
            field(ErrorCode::ValidationFailed, "limit_minor"),
        ),
        (
            credit_intent(&p, 100, &[]),
            field(ErrorCode::ValidationFailed, "reminder_days"),
        ),
        (
            credit_intent(&p, 100, &[7, 3]),
            field(ErrorCode::ValidationFailed, "reminder_days"),
        ),
        (
            credit_intent(&p, 100, &[3, 3]),
            field(ErrorCode::ValidationFailed, "reminder_days"),
        ),
        (
            credit_intent(&p, 100, &[0]),
            field(ErrorCode::ValidationFailed, "reminder_days"),
        ),
        (
            credit_intent(&p, 100, &[366]),
            field(ErrorCode::ValidationFailed, "reminder_days"),
        ),
        (
            credit_intent(&p, 100, &(1..=11).collect::<Vec<u16>>()),
            field(ErrorCode::ValidationFailed, "reminder_days"),
        ),
    ];
    for (i, want) in cases {
        assert_eq!(refusal(end(&f, i).await), want);
    }
    let mut with_rev = credit_intent(&p, 100, &[3]);
    with_rev.base_rev = Some(1);
    assert_eq!(
        refusal(end(&f, with_rev).await),
        field(ErrorCode::ValidationFailed, "base_rev")
    );
    let mut counter = credit_intent(&p, 100, &[3]);
    counter.actor = ctr;
    assert_eq!(
        refusal(end(&f, counter).await).0,
        ErrorCode::PermissionDenied
    );
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
}

#[tokio::test]
async fn a_limit_below_what_is_owed_is_refused() {
    let f = bootstrapped().await;
    let p = credit_player(&f, "asha", "+919876543210", 50_000).await;
    let (order, rev) = billed_for(&f, &p, 12_000, 60).await;
    end(&f, settle_on_credit(&order, rev, 12_000, false))
        .await
        .unwrap();
    assert_eq!(owed(&f, &p), 12_000);
    let before = reopen(&f).changes().unwrap().len();
    assert_eq!(
        refusal(end(&f, credit_intent(&p, 11_999, &[3])).await),
        (
            ErrorCode::CreditBelowOutstanding,
            Some("limit_minor".into())
        )
    );
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    end(&f, credit_intent(&p, 12_000, &[3])).await.unwrap();
}

#[tokio::test]
async fn settling_on_credit_appends_a_ledger_row_and_raises_outstanding() {
    let f = bootstrapped().await;
    let p = credit_player(&f, "asha", "+919876543210", 50_000).await;
    topped_up(&f, &p, 5_000).await;
    let (order, rev) = billed_for(&f, &p, 12_000, 60).await;
    let snapshot = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    let credit = snapshot.bills[0].credit.clone().unwrap();
    assert_eq!(
        (credit.limit_minor, credit.outstanding_minor, credit.paused),
        (50_000, 0, false)
    );
    assert_eq!(credit.reminder_days, vec![3, 7, 14]);
    let a = end(
        &f,
        settle_intent(
            &order,
            Some(rev),
            vec![
                pay(arena_proto::PaymentMethod::Wallet, 2_000, None),
                pay(arena_proto::PaymentMethod::Credit, 10_000, None),
            ],
        ),
    )
    .await
    .unwrap();
    let tables: Vec<_> = a.changes.iter().map(|c| c.tbl.as_str()).collect();
    assert!(tables.contains(&"credit_balances") && tables.contains(&"wallets"));
    assert_eq!(owed(&f, &p), 10_000);
    assert_eq!(balance(&f, &p), 3_000);
    let credit_rows: Vec<_> = table(&f, Table::WalletTxns)
        .into_iter()
        .filter(|t| text(t, "reason") == "credit")
        .collect();
    assert_eq!(credit_rows.len(), 1);
    assert_eq!(int(&credit_rows[0], "delta_minor"), 10_000);
    assert_eq!(credit_rows[0]["method"], RowValue::Null);
    assert_eq!(text(&credit_rows[0], "order_id"), order);
    let ledger: i64 = table(&f, Table::WalletTxns)
        .iter()
        .filter(|t| text(t, "reason") != "credit")
        .map(|t| int(t, "delta_minor"))
        .sum();
    assert_eq!(ledger, balance(&f, &p));
    let methods: Vec<_> = table(&f, Table::Payments)
        .iter()
        .map(|p| text(p, "method"))
        .collect();
    assert!(methods.contains(&"credit".to_owned()));
    let txns = reopen(&f).wallet_txns(&p).unwrap();
    assert!(txns.iter().all(|t| t.reason != "credit"));
}

#[tokio::test]
async fn credit_is_refused_for_guests_and_players_without_credit() {
    let f = bootstrapped().await;
    let (guest_order, _, guest_rev) = billed(&f, 10_000, 41).await;
    let method = || {
        (
            ErrorCode::ValidationFailed,
            Some("payments[0].method".to_owned()),
        )
    };
    assert_eq!(
        refusal(end(&f, settle_on_credit(&guest_order, guest_rev, 7_500, false)).await),
        method()
    );
    let plain = player(&f, "ravi", "+919876543211").await;
    let (order, rev) = billed_for(&f, &plain, 12_000, 60).await;
    assert_eq!(
        refusal(end(&f, settle_on_credit(&order, rev, 12_000, false)).await),
        method()
    );
    let zero = credit_player(&f, "zed", "+919876543212", 0).await;
    let (zero_order, zero_rev) = billed_for(&f, &zero, 12_000, 60).await;
    assert_eq!(
        refusal(end(&f, settle_on_credit(&zero_order, zero_rev, 12_000, false)).await),
        method()
    );
    let full = credit_player(&f, "full", "+919876543213", 50_000).await;
    let (full_order, full_rev) = billed_for(&f, &full, 12_000, 60).await;
    let twice = settle_intent(
        &full_order,
        Some(full_rev),
        vec![
            pay(arena_proto::PaymentMethod::Credit, 6_000, None),
            pay(arena_proto::PaymentMethod::Credit, 6_000, None),
        ],
    );
    assert_eq!(refusal(end(&f, twice).await), method());
    let referenced = settle_intent(
        &full_order,
        Some(full_rev),
        vec![pay(arena_proto::PaymentMethod::Credit, 12_000, Some("x"))],
    );
    assert_eq!(
        refusal(end(&f, referenced).await),
        (
            ErrorCode::ValidationFailed,
            Some("payments[0].reference".into())
        )
    );
    assert!(table(&f, Table::Payments).is_empty());
    assert!(table(&f, Table::WalletTxns).is_empty());
}

#[tokio::test]
async fn a_top_up_cannot_be_paid_on_credit() {
    let f = bootstrapped().await;
    let p = credit_player(&f, "asha", "+919876543210", 50_000).await;
    assert_eq!(
        refusal(
            end(
                &f,
                topup_intent(
                    &p,
                    100,
                    vec![pay(arena_proto::PaymentMethod::Credit, 100, None)]
                )
            )
            .await
        ),
        (
            ErrorCode::ValidationFailed,
            Some("payments[0].method".into())
        )
    );
}

#[tokio::test]
async fn going_over_the_limit_needs_a_manager_override_once_a_day() {
    let f = bootstrapped().await;
    let p = credit_player(&f, "asha", "+919876543210", 20_000).await;
    let (first, first_rev) = billed_for(&f, &p, 12_000, 60).await;
    end(&f, settle_on_credit(&first, first_rev, 12_000, false))
        .await
        .unwrap();
    let (second, second_rev) = billed_for(&f, &p, 12_000, 60).await;
    let before = reopen(&f).changes().unwrap().len();
    assert_eq!(
        refusal(end(&f, settle_on_credit(&second, second_rev, 12_000, false)).await),
        (
            ErrorCode::CreditLimit,
            Some("payments[0].amount_minor".into())
        )
    );
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    let mut counter = settle_on_credit(&second, second_rev, 12_000, true);
    counter.actor = staff(&f, "ctr", ROLE_COUNTER).await;
    assert_eq!(
        refusal(end(&f, counter).await).0,
        ErrorCode::PermissionDenied
    );
    end(&f, settle_on_credit(&second, second_rev, 12_000, true))
        .await
        .unwrap();
    assert_eq!(owed(&f, &p), 24_000);
    assert!(!text(&credit_row(&f, &p), "last_override_day").is_empty());
    let override_rows = table(&f, Table::WalletTxns)
        .into_iter()
        .filter(|t| {
            text(t, "reason") == "credit" && t["method"] == RowValue::Text("override".into())
        })
        .count();
    assert_eq!(override_rows, 1);

    let (third, third_rev) = billed_for(&f, &p, 12_000, 60).await;
    let before = reopen(&f).changes().unwrap().len();
    assert_eq!(
        refusal(end(&f, settle_on_credit(&third, third_rev, 12_000, true)).await),
        (
            ErrorCode::OverrideUsedToday,
            Some("payments[0].amount_minor".into())
        )
    );
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    assert_eq!(owed(&f, &p), 24_000);
}

#[tokio::test]
async fn an_override_inside_the_limit_is_not_spent() {
    let f = bootstrapped().await;
    let p = credit_player(&f, "asha", "+919876543210", 50_000).await;
    let (order, rev) = billed_for(&f, &p, 12_000, 60).await;
    end(&f, settle_on_credit(&order, rev, 12_000, true))
        .await
        .unwrap();
    assert_eq!(credit_row(&f, &p)["last_override_day"], RowValue::Null);
    assert_eq!(owed(&f, &p), 12_000);
}

#[tokio::test]
async fn undoing_a_credit_settle_restores_outstanding_and_the_override() {
    let f = bootstrapped().await;
    let p = credit_player(&f, "asha", "+919876543210", 10_000).await;
    let (order, rev) = billed_for(&f, &p, 12_000, 60).await;
    end(&f, settle_on_credit(&order, rev, 12_000, true))
        .await
        .unwrap();
    assert_eq!(owed(&f, &p), 12_000);
    assert_ne!(credit_row(&f, &p)["last_override_day"], RowValue::Null);
    let settled_rev = rev_of(&f, Table::Orders, &order);
    let a = end(&f, settle_revert_intent(&order, Some(settled_rev)))
        .await
        .unwrap();
    assert!(a.changes.iter().any(|c| c.tbl == "credit_balances"));
    assert_eq!(owed(&f, &p), 0);
    assert_eq!(credit_row(&f, &p)["last_override_day"], RowValue::Null);
    assert!(table(&f, Table::WalletTxns).is_empty());
    assert_eq!(balance(&f, &p), 0);
    let order_rev = rev_of(&f, Table::Orders, &order);
    end(&f, settle_on_credit(&order, order_rev, 12_000, true))
        .await
        .unwrap();
    assert_eq!(owed(&f, &p), 12_000);
}

#[tokio::test]
async fn a_guest_bill_linked_to_a_credit_player_offers_credit() {
    let f = bootstrapped().await;
    let p = credit_player(&f, "asha", "+919876543210", 50_000).await;
    let (id, _) = running(&f, 6_000).await;
    let s = session_row(&f, &id);
    end(&f, assign_intent(&id, &p, None, Some(int(&s, "rev"))))
        .await
        .unwrap();
    f.clock.advance(60 * 60_000);
    let s = session_row(&f, &id);
    end(&f, end_intent(&id, Some(int(&s, "rev"))))
        .await
        .unwrap();
    let snapshot = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    let bill = snapshot
        .bills
        .iter()
        .find(|b| b.order_id.is_some())
        .unwrap();
    assert_eq!(bill.credit.as_ref().unwrap().limit_minor, 50_000);
}

pub(super) fn collect_intent(
    player: &str,
    amount: i64,
    payments: Vec<arena_proto::PaymentRow>,
) -> Intent {
    intent(
        owner(),
        IntentPayload::CreditCollect(arena_proto::CreditCollect {
            player_id: player.into(),
            amount_minor: amount,
            payments,
        }),
    )
}

fn collect_revert_intent(order: &str, rev: Option<i64>) -> Intent {
    let mut i = intent(
        owner(),
        IntentPayload::CreditCollectReverted(arena_proto::CreditCollectReverted {
            order_id: order.into(),
        }),
    );
    i.base_rev = rev;
    i
}

fn collect_cash(player: &str, amount: i64) -> Intent {
    collect_intent(
        player,
        amount,
        vec![pay(arena_proto::PaymentMethod::Cash, amount, None)],
    )
}

fn collection_order(f: &Fixture) -> String {
    table(f, Table::Orders)
        .iter()
        .rev()
        .find(|o| text(o, "kind") == "credit_collect")
        .map(|o| text(o, "id"))
        .unwrap()
}

async fn owing(f: &Fixture, player: &str, spends: &[(i64, i64)]) -> Vec<String> {
    let mut orders = Vec::new();
    for (rate, minutes) in spends {
        let (order, rev) = billed_for(f, player, *rate, *minutes).await;
        let amount = int(&row_of(f, Table::Orders, &order), "total_minor");
        end(f, settle_on_credit(&order, rev, amount, false))
            .await
            .unwrap();
        orders.push(order);
    }
    orders
}

#[tokio::test]
async fn collecting_part_of_the_tab_clears_the_oldest_spends_first() {
    let f = bootstrapped().await;
    let p = credit_player(&f, "asha", "+919876543210", 50_000).await;
    let spends = owing(&f, &p, &[(12_000, 60), (12_000, 30)]).await;
    assert_eq!(owed(&f, &p), 18_000);
    let a = end(&f, collect_cash(&p, 15_000)).await.unwrap();
    let tables: Vec<_> = a.changes.iter().map(|c| c.tbl.as_str()).collect();
    assert!(tables.contains(&"credit_balances") && tables.contains(&"payments"));
    assert_eq!(owed(&f, &p), 3_000);
    let order = collection_order(&f);
    let o = row_of(&f, Table::Orders, &order);
    assert_eq!(text(&o, "status"), "settled");
    assert_eq!(int(&o, "total_minor"), 15_000);
    let txn = table(&f, Table::WalletTxns)
        .into_iter()
        .find(|t| text(t, "order_id") == order)
        .unwrap();
    assert_eq!(
        (text(&txn, "reason"), int(&txn, "delta_minor")),
        ("credit".to_owned(), -15_000)
    );
    let summary = reopen(&f).credit_collection(&order).unwrap().unwrap();
    assert_eq!(summary.amount_minor, 15_000);
    assert_eq!(summary.outstanding_after_minor, 3_000);
    let cleared: Vec<_> = summary
        .cleared
        .iter()
        .map(|c| (c.order_id.clone(), c.spend_minor, c.cleared_minor))
        .collect();
    assert_eq!(
        cleared,
        [
            (spends[0].clone(), 12_000, 12_000),
            (spends[1].clone(), 6_000, 3_000)
        ]
    );
    end(&f, collect_cash(&p, 3_000)).await.unwrap();
    assert_eq!(owed(&f, &p), 0);
    let second = reopen(&f)
        .credit_collection(&collection_order(&f))
        .unwrap()
        .unwrap();
    assert_eq!(second.cleared.len(), 1);
    assert_eq!(
        (
            second.cleared[0].cleared_minor,
            second.outstanding_after_minor
        ),
        (3_000, 0)
    );
    assert!(reopen(&f).credit_collection(&spends[0]).unwrap().is_none());
}

#[tokio::test]
async fn a_collection_can_be_paid_from_the_wallet_and_by_upi() {
    let f = bootstrapped().await;
    let p = credit_player(&f, "asha", "+919876543210", 50_000).await;
    topped_up(&f, &p, 5_000).await;
    owing(&f, &p, &[(12_000, 60)]).await;
    end(
        &f,
        collect_intent(
            &p,
            9_000,
            vec![
                pay(arena_proto::PaymentMethod::Wallet, 4_000, None),
                pay(arena_proto::PaymentMethod::Upi, 5_000, Some("1234")),
            ],
        ),
    )
    .await
    .unwrap();
    assert_eq!(owed(&f, &p), 3_000);
    assert_eq!(balance(&f, &p), 1_000);
    let order = collection_order(&f);
    let mut methods: Vec<_> = table(&f, Table::Payments)
        .iter()
        .filter(|x| text(x, "order_id") == order)
        .map(|x| text(x, "method"))
        .collect();
    methods.sort();
    assert_eq!(methods, ["upi", "wallet"]);
    let shown = reopen(&f).wallet_txns(&p).unwrap();
    assert!(shown.iter().all(|t| t.reason != "credit"));
    assert_eq!(shown[0].delta_minor, -4_000);
}

#[tokio::test]
async fn a_bad_collection_is_refused_and_writes_nothing() {
    let f = bootstrapped().await;
    let p = credit_player(&f, "asha", "+919876543210", 50_000).await;
    let plain = player(&f, "ben", "+919876543211").await;
    topped_up(&f, &p, 500).await;
    owing(&f, &p, &[(12_000, 60)]).await;
    let before = reopen(&f).changes().unwrap().len();
    let cash = |amount| pay(arena_proto::PaymentMethod::Cash, amount, None);
    let cases = [
        (
            collect_cash(&p, 12_001),
            ErrorCode::ValidationFailed,
            Some("amount_minor"),
        ),
        (
            collect_cash(&p, 0),
            ErrorCode::ValidationFailed,
            Some("amount_minor"),
        ),
        (
            collect_intent(&p, 5_000, vec![cash(4_000)]),
            ErrorCode::PaymentShort,
            Some("payments"),
        ),
        (
            collect_intent(&p, 5_000, vec![]),
            ErrorCode::ValidationFailed,
            Some("payments"),
        ),
        (
            collect_intent(
                &p,
                5_000,
                vec![pay(arena_proto::PaymentMethod::Credit, 5_000, None)],
            ),
            ErrorCode::ValidationFailed,
            Some("payments[0].method"),
        ),
        (
            collect_intent(
                &p,
                5_000,
                vec![pay(arena_proto::PaymentMethod::Wallet, 5_000, None)],
            ),
            ErrorCode::InsufficientBalance,
            Some("payments[0].amount_minor"),
        ),
        (
            collect_intent(
                &p,
                5_000,
                vec![pay(arena_proto::PaymentMethod::Upi, 5_000, None)],
            ),
            ErrorCode::ReferenceRequired,
            Some("payments[0].reference"),
        ),
        (
            collect_cash(&plain, 100),
            ErrorCode::NotFound,
            Some("player_id"),
        ),
    ];
    for (i, code, field) in cases {
        assert_eq!(refusal(end(&f, i).await), (code, field.map(str::to_owned)));
    }
    let mut with_rev = collect_cash(&p, 100);
    with_rev.base_rev = Some(1);
    assert_eq!(
        refusal(end(&f, with_rev).await).0,
        ErrorCode::ValidationFailed
    );
    nothing_changed(&f, before);
    assert_eq!(owed(&f, &p), 12_000);
}

#[tokio::test]
async fn collecting_needs_the_collect_permission() {
    let f = bootstrapped().await;
    let p = credit_player(&f, "asha", "+919876543210", 50_000).await;
    owing(&f, &p, &[(12_000, 60)]).await;
    let kitchen = staff(&f, "kit", ROLE_KITCHEN).await;
    let counter = staff(&f, "cnt", ROLE_COUNTER).await;
    let before = reopen(&f).changes().unwrap().len();
    assert_eq!(
        refusal(end(&f, with_actor(collect_cash(&p, 100), kitchen)).await).0,
        ErrorCode::PermissionDenied
    );
    nothing_changed(&f, before);
    end(&f, with_actor(collect_cash(&p, 100), counter))
        .await
        .unwrap();
    assert_eq!(owed(&f, &p), 11_900);
}

#[tokio::test]
async fn undoing_a_collection_restores_outstanding_and_the_wallet() {
    let f = bootstrapped().await;
    let p = credit_player(&f, "asha", "+919876543210", 50_000).await;
    topped_up(&f, &p, 5_000).await;
    let spends = owing(&f, &p, &[(12_000, 60)]).await;
    end(
        &f,
        collect_intent(
            &p,
            7_000,
            vec![
                pay(arena_proto::PaymentMethod::Wallet, 2_000, None),
                pay(arena_proto::PaymentMethod::Cash, 5_000, None),
            ],
        ),
    )
    .await
    .unwrap();
    assert_eq!((owed(&f, &p), balance(&f, &p)), (5_000, 3_000));
    let order = collection_order(&f);
    let rev = rev_of(&f, Table::Orders, &order);
    let a = end(&f, collect_revert_intent(&order, Some(rev)))
        .await
        .unwrap();
    assert!(a.changes.iter().any(|c| c.tbl == "credit_balances"));
    assert_eq!((owed(&f, &p), balance(&f, &p)), (12_000, 5_000));
    assert!(
        table(&f, Table::Orders)
            .iter()
            .all(|o| text(o, "kind") != "credit_collect")
    );
    assert!(
        table(&f, Table::WalletTxns)
            .iter()
            .all(|t| text(t, "order_id") != order)
    );
    assert!(
        table(&f, Table::Payments)
            .iter()
            .all(|x| text(x, "order_id") != order)
    );
    end(&f, collect_cash(&p, 12_000)).await.unwrap();
    let again = reopen(&f)
        .credit_collection(&collection_order(&f))
        .unwrap()
        .unwrap();
    assert_eq!(again.cleared.len(), 1);
    assert_eq!(again.cleared[0].order_id, spends[0]);
    assert_eq!(again.outstanding_after_minor, 0);
}

#[tokio::test]
async fn a_collection_undo_is_refused_late_by_other_staff_or_for_a_settle() {
    let f = bootstrapped().await;
    let p = credit_player(&f, "asha", "+919876543210", 50_000).await;
    let spends = owing(&f, &p, &[(12_000, 60)]).await;
    end(&f, collect_cash(&p, 4_000)).await.unwrap();
    let order = collection_order(&f);
    let rev = rev_of(&f, Table::Orders, &order);
    let counter = staff(&f, "cnt", ROLE_COUNTER).await;
    let before = reopen(&f).changes().unwrap().len();
    assert_eq!(
        refusal(
            end(
                &f,
                with_actor(collect_revert_intent(&order, Some(rev)), counter)
            )
            .await
        )
        .0,
        ErrorCode::UndoNotAllowed
    );
    assert_eq!(
        refusal(end(&f, collect_revert_intent(&order, Some(rev + 1))).await).0,
        ErrorCode::Stale
    );
    assert_eq!(
        refusal(
            end(
                &f,
                collect_revert_intent(&spends[0], Some(rev_of(&f, Table::Orders, &spends[0])))
            )
            .await
        )
        .0,
        ErrorCode::ValidationFailed
    );
    assert_eq!(
        refusal(end(&f, collect_revert_intent("NOPE", Some(1))).await).0,
        ErrorCode::NotFound
    );
    f.clock.advance(8_001);
    assert_eq!(
        refusal(end(&f, collect_revert_intent(&order, Some(rev))).await).0,
        ErrorCode::UndoNotAllowed
    );
    nothing_changed(&f, before);
    assert_eq!(owed(&f, &p), 8_000);
}

#[tokio::test]
async fn a_credit_settle_cannot_be_undone_once_its_credit_was_collected() {
    let f = bootstrapped().await;
    let p = credit_player(&f, "asha", "+919876543210", 50_000).await;
    let (order, rev) = billed_for(&f, &p, 12_000, 60).await;
    end(&f, settle_on_credit(&order, rev, 12_000, false))
        .await
        .unwrap();
    end(&f, collect_cash(&p, 12_000)).await.unwrap();
    let settled_rev = rev_of(&f, Table::Orders, &order);
    assert_eq!(
        refusal(end(&f, settle_revert_intent(&order, Some(settled_rev))).await).0,
        ErrorCode::UndoNotAllowed
    );
}

#[tokio::test]
async fn a_settle_undo_cannot_target_a_collection() {
    let f = bootstrapped().await;
    let p = credit_player(&f, "asha", "+919876543210", 50_000).await;
    owing(&f, &p, &[(12_000, 60)]).await;
    end(&f, collect_cash(&p, 4_000)).await.unwrap();
    let order = collection_order(&f);
    let rev = rev_of(&f, Table::Orders, &order);
    let before = reopen(&f).changes().unwrap().len();
    assert_eq!(
        refusal(end(&f, settle_revert_intent(&order, Some(rev))).await).0,
        ErrorCode::ValidationFailed
    );
    nothing_changed(&f, before);
    assert_eq!(owed(&f, &p), 8_000);
}

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

fn paused(f: &Fixture, player: &str) -> bool {
    int(&credit_row(f, player), "credit_paused") == 1
}

fn aging_of(f: &Fixture, player: &str) -> arena_proto::CreditAging {
    crate::SnapshotReader::open(&f.path)
        .unwrap()
        .wallet(player, f.clock.now_ms())
        .unwrap()
        .aging
        .unwrap()
}

async fn sweep(f: &Fixture) -> Option<Applied> {
    f.writer.credit_auto_pause(None).await.unwrap()
}

async fn auto_pause_rule(f: &Fixture, value: &str) {
    let rev = int(
        &setting(&reopen(f), arena_proto::KEY_BILLING_CREDIT_AUTO_PAUSE),
        "rev",
    );
    end(
        f,
        set(arena_proto::KEY_BILLING_CREDIT_AUTO_PAUSE, value, Some(rev)),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn aging_buckets_follow_the_oldest_unpaid_spend_first() {
    let f = bootstrapped().await;
    let p = credit_player(&f, "asha", "+919876543210", 100_000).await;
    auto_pause_rule(&f, "false").await;
    owing(&f, &p, &[(12_000, 60)]).await;
    f.clock.advance(3 * DAY_MS);
    owing(&f, &p, &[(6_000, 60)]).await;
    f.clock.advance(13 * DAY_MS);
    owing(&f, &p, &[(3_000, 60)]).await;
    let a = aging_of(&f, &p);
    assert_eq!(
        (
            a.bucket_0_7_minor,
            a.bucket_8_14_minor,
            a.bucket_15_plus_minor
        ),
        (3_000, 6_000, 12_000)
    );
    assert!(a.oldest_age_days >= 16 && a.overdue());
    end(&f, collect_cash(&p, 12_000)).await.unwrap();
    let a = aging_of(&f, &p);
    assert_eq!(
        (
            a.bucket_0_7_minor,
            a.bucket_8_14_minor,
            a.bucket_15_plus_minor
        ),
        (3_000, 6_000, 0)
    );
    assert!(!a.overdue());
}

#[tokio::test]
async fn the_check_pauses_players_15_days_overdue_and_refuses_new_credit() {
    let f = bootstrapped().await;
    let late = credit_player(&f, "asha", "+919876543210", 100_000).await;
    let fresh = credit_player(&f, "ben", "+919876543211", 100_000).await;
    owing(&f, &late, &[(12_000, 60)]).await;
    f.clock.advance(14 * DAY_MS);
    owing(&f, &fresh, &[(12_000, 60)]).await;
    assert!(sweep(&f).await.is_none());
    f.clock.advance(2 * DAY_MS);
    let a = sweep(&f).await.expect("one player is overdue");
    assert!(a.changes.iter().all(|c| c.tbl == "credit_balances"));
    assert!(paused(&f, &late) && !paused(&f, &fresh));
    assert!(sweep(&f).await.is_none());
    let (order, rev) = billed_for(&f, &late, 6_000, 60).await;
    let before = reopen(&f).changes().unwrap().len();
    for over in [false, true] {
        let e = refusal(end(&f, settle_on_credit(&order, rev, 6_000, over)).await);
        assert_eq!(e.0, ErrorCode::CreditPaused);
    }
    nothing_changed(&f, before);
}

#[tokio::test]
async fn settling_on_credit_is_refused_for_an_overdue_player_the_check_has_not_reached() {
    let f = bootstrapped().await;
    let p = credit_player(&f, "asha", "+919876543210", 100_000).await;
    owing(&f, &p, &[(12_000, 60)]).await;
    f.clock.advance(20 * DAY_MS);
    assert!(!paused(&f, &p));
    let (order, rev) = billed_for(&f, &p, 6_000, 60).await;
    assert_eq!(
        refusal(end(&f, settle_on_credit(&order, rev, 6_000, false)).await).0,
        ErrorCode::CreditPaused
    );
}

#[tokio::test]
async fn a_lookup_check_for_one_player_leaves_the_others_alone() {
    let f = bootstrapped().await;
    let a = credit_player(&f, "asha", "+919876543210", 100_000).await;
    let b = credit_player(&f, "ben", "+919876543211", 100_000).await;
    owing(&f, &a, &[(12_000, 60)]).await;
    owing(&f, &b, &[(12_000, 60)]).await;
    f.clock.advance(16 * DAY_MS);
    f.writer
        .credit_auto_pause(Some(a.clone()))
        .await
        .unwrap()
        .unwrap();
    assert!(paused(&f, &a) && !paused(&f, &b));
}

#[tokio::test]
async fn switching_the_rule_off_reopens_paused_players_and_stops_pausing() {
    let f = bootstrapped().await;
    let p = credit_player(&f, "asha", "+919876543210", 100_000).await;
    owing(&f, &p, &[(12_000, 60)]).await;
    f.clock.advance(16 * DAY_MS);
    sweep(&f).await.unwrap();
    assert!(paused(&f, &p));
    auto_pause_rule(&f, "false").await;
    let (order, rev) = billed_for(&f, &p, 6_000, 60).await;
    sweep(&f).await.unwrap();
    assert!(!paused(&f, &p));
    assert!(sweep(&f).await.is_none());
    end(&f, settle_on_credit(&order, rev, 6_000, false))
        .await
        .unwrap();
    auto_pause_rule(&f, "true").await;
    sweep(&f).await.unwrap();
    assert!(paused(&f, &p));
}

#[tokio::test]
async fn a_collection_reopens_credit_and_its_undo_pauses_it_again() {
    let f = bootstrapped().await;
    let p = credit_player(&f, "asha", "+919876543210", 100_000).await;
    owing(&f, &p, &[(12_000, 60)]).await;
    f.clock.advance(10 * DAY_MS);
    owing(&f, &p, &[(6_000, 60)]).await;
    f.clock.advance(6 * DAY_MS);
    sweep(&f).await.unwrap();
    assert!(paused(&f, &p));

    end(&f, collect_cash(&p, 5_000)).await.unwrap();
    assert!(paused(&f, &p));

    end(&f, collect_cash(&p, 7_000)).await.unwrap();
    assert!(!paused(&f, &p));
    let order = collection_order(&f);
    let txn = table(&f, Table::WalletTxns)
        .into_iter()
        .find(|t| text(t, "order_id") == order && text(t, "reason") == "credit")
        .unwrap();
    assert_eq!(text(&txn, "method"), "reopened");
    let rev = rev_of(&f, Table::Orders, &order);
    end(&f, collect_revert_intent(&order, Some(rev)))
        .await
        .unwrap();
    assert!(paused(&f, &p));
    assert_eq!(owed(&f, &p), 13_000);
    assert!(sweep(&f).await.is_none());

    end(&f, collect_cash(&p, 13_000)).await.unwrap();
    assert!(!paused(&f, &p));
    assert_eq!(owed(&f, &p), 0);
}

#[tokio::test]
async fn a_collection_leaves_an_unpaused_player_unpaused_and_its_undo_changes_nothing() {
    let f = bootstrapped().await;
    let p = credit_player(&f, "asha", "+919876543210", 100_000).await;
    owing(&f, &p, &[(12_000, 60)]).await;
    end(&f, collect_cash(&p, 4_000)).await.unwrap();
    assert!(!paused(&f, &p));
    let order = collection_order(&f);
    let rev = rev_of(&f, Table::Orders, &order);
    end(&f, collect_revert_intent(&order, Some(rev)))
        .await
        .unwrap();
    assert!(!paused(&f, &p));
}

#[tokio::test]
async fn the_auto_pause_intent_is_system_only() {
    let f = bootstrapped().await;
    let e = refusal(
        end(
            &f,
            intent(
                owner(),
                IntentPayload::CreditAutoPause(arena_proto::CreditAutoPause { player_id: None }),
            ),
        )
        .await,
    );
    assert_eq!(e.0, ErrorCode::PermissionDenied);
}

pub(super) async fn plan_of(f: &Fixture, kind: &str, price: i64, active: bool) -> String {
    let g = submit(
        f,
        owner(),
        IntentPayload::GroupUpsert(GroupUpsert {
            id: None,
            name: format!("PG{}", table(f, Table::DeviceGroups).len() + 1),
            group_kind: "pc".into(),
            colour: "teal".into(),
            sort_order: 1,
        }),
    )
    .await
    .unwrap()
    .changes[0]
        .row_id
        .clone();
    let (minutes, window) = match kind {
        "pack" => (Some(300), (None, None)),
        "block" => (None, (Some(1080), Some(1410))),
        _ => (None, (Some(1320), Some(360))),
    };
    submit(
        f,
        owner(),
        IntentPayload::PlanUpsert(arena_proto::PlanUpsert {
            id: None,
            plan_kind: kind.into(),
            name: format!("{kind} plan"),
            price_minor: price,
            minutes,
            validity_days: (kind != "block").then_some(7),
            window_start_min: window.0,
            window_end_min: window.1,
            group_ids: vec![g],
            active,
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

pub(super) fn sell_intent(
    player: &str,
    plan: &str,
    payments: Vec<arena_proto::PaymentRow>,
) -> Intent {
    intent(
        owner(),
        IntentPayload::PlanSell(arena_proto::PlanSell {
            business: None,
            coupon: None,
            discount: None,
            player_id: player.into(),
            plan_id: plan.into(),
            payments,
            credit_override: None,
            receipt: None,
        }),
    )
}

pub(super) fn sell_revert_intent(order: &str, rev: Option<i64>) -> Intent {
    let mut i = intent(
        owner(),
        IntentPayload::PlanSellReverted(arena_proto::PlanSellReverted {
            order_id: order.into(),
        }),
    );
    i.base_rev = rev;
    i
}

pub(super) fn sale_order(f: &Fixture) -> Row {
    table(f, Table::Orders)
        .into_iter()
        .rev()
        .find(|o| text(o, "kind") == "plan_sale")
        .unwrap()
}

#[tokio::test]
async fn selling_a_pack_writes_the_bill_line_payment_and_owned_copy() {
    use arena_proto::PaymentMethod::Cash;
    let f = bootstrapped().await;
    let p = player(&f, "asha", "+919876543210").await;
    let plan = plan_of(&f, "pack", 30_000, true).await;
    let a = end(&f, sell_intent(&p, &plan, vec![pay(Cash, 30_000, None)]))
        .await
        .unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        [
            "orders",
            "order_lines",
            "payments",
            "cash_events",
            "player_packs"
        ]
    );
    let o = sale_order(&f);
    assert_eq!(
        (
            text(&o, "status"),
            int(&o, "total_minor"),
            text(&o, "player_id")
        ),
        ("settled".to_owned(), 30_000, p.clone())
    );
    let line = &table(&f, Table::OrderLines)[0];
    assert_eq!(
        (
            text(line, "plan_id"),
            text(line, "label"),
            int(line, "amount_minor")
        ),
        (plan.clone(), "pack plan".to_owned(), 30_000)
    );
    let pack = &table(&f, Table::PlayerPacks)[0];
    assert_eq!(text(pack, "order_id"), text(&o, "id"));
    assert_eq!(
        (int(pack, "minutes_total"), int(pack, "minutes_left")),
        (300, 300)
    );
    assert_eq!(int(pack, "bought_at"), f.clock.now_ms());
    assert_eq!(
        int(pack, "expires_at"),
        f.clock.now_ms() + 7 * 24 * 60 * 60_000
    );
    assert_eq!(text(pack, "group_ids").split(',').count(), 1);
    let s = reopen(&f).settlement(&text(&o, "id")).unwrap().unwrap();
    assert_eq!(s.total_minor, 30_000);
    let live = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    assert!(live.bills.is_empty());
}

#[tokio::test]
async fn a_plan_can_be_paid_from_the_wallet_and_on_credit_and_undo_reverts_all() {
    use arena_proto::PaymentMethod::{Cash, Credit, Wallet};
    let f = bootstrapped().await;
    let p = credit_player(&f, "asha", "+919876543210", 50_000).await;
    topped_up(&f, &p, 10_000).await;
    let plan = plan_of(&f, "pass", 30_000, true).await;
    end(
        &f,
        sell_intent(
            &p,
            &plan,
            vec![
                pay(Wallet, 10_000, None),
                pay(Credit, 15_000, None),
                pay(Cash, 5_000, None),
            ],
        ),
    )
    .await
    .unwrap();
    assert_eq!((balance(&f, &p), owed(&f, &p)), (0, 15_000));
    let o = sale_order(&f);
    let pack = &table(&f, Table::PlayerPacks)[0];
    assert_eq!(pack["minutes_left"], RowValue::Null);
    assert_eq!(int(pack, "window_start_min"), 1320);

    let rev = int(&o, "rev");
    end(&f, sell_revert_intent(&text(&o, "id"), Some(rev)))
        .await
        .unwrap();
    assert_eq!((balance(&f, &p), owed(&f, &p)), (10_000, 0));
    assert!(table(&f, Table::PlayerPacks).is_empty());
    assert!(table(&f, Table::OrderLines).is_empty());
    assert!(
        table(&f, Table::Payments)
            .iter()
            .all(|x| text(x, "order_id") != text(&o, "id"))
    );
    assert!(
        table(&f, Table::Orders)
            .iter()
            .all(|x| text(x, "kind") != "plan_sale")
    );
}

#[tokio::test]
async fn a_second_unexpired_pass_is_refused_but_an_expired_one_can_be_replaced() {
    use arena_proto::PaymentMethod::Cash;
    let f = bootstrapped().await;
    let p = player(&f, "asha", "+919876543210").await;
    let plan = plan_of(&f, "pass", 90_000, true).await;
    let again = || sell_intent(&p, &plan, vec![pay(Cash, 90_000, None)]);
    end(&f, again()).await.unwrap();
    assert_eq!(
        refusal(end(&f, again()).await),
        (ErrorCode::PassAlreadyActive, Some("plan_id".into()))
    );
    assert_eq!(table(&f, Table::PlayerPacks).len(), 1);
    f.clock.advance(7 * 24 * 60 * 60_000);
    end(&f, again()).await.unwrap();
    assert_eq!(table(&f, Table::PlayerPacks).len(), 2);
    let pack = plan_of(&f, "pack", 100, true).await;
    end(&f, sell_intent(&p, &pack, vec![pay(Cash, 100, None)]))
        .await
        .unwrap();
}

#[tokio::test]
async fn a_bad_plan_sale_is_refused_and_writes_nothing() {
    use arena_proto::PaymentMethod::{Cash, Credit, Upi, Wallet};
    let f = bootstrapped().await;
    let p = player(&f, "asha", "+919876543210").await;
    let pack = plan_of(&f, "pack", 1_000, true).await;
    let off = plan_of(&f, "pack", 1_000, false).await;
    let block = plan_of(&f, "block", 1_000, true).await;
    let before = reopen(&f).changes().unwrap().len();
    let cash = |n: i64| vec![pay(Cash, n, None)];
    let cases: Vec<(Intent, ErrorCode, &str)> = vec![
        (
            sell_intent("nobody", &pack, cash(1_000)),
            ErrorCode::NotFound,
            "player_id",
        ),
        (
            sell_intent(&p, "nope", cash(1_000)),
            ErrorCode::NotFound,
            "plan_id",
        ),
        (
            sell_intent(&p, &off, cash(1_000)),
            ErrorCode::ValidationFailed,
            "plan_id",
        ),
        (
            sell_intent(&p, &block, cash(1_000)),
            ErrorCode::ValidationFailed,
            "plan_id",
        ),
        (
            sell_intent(&p, &pack, cash(999)),
            ErrorCode::PaymentShort,
            "payments",
        ),
        (
            sell_intent(&p, &pack, vec![]),
            ErrorCode::PaymentShort,
            "payments",
        ),
        (
            sell_intent(&p, &pack, vec![pay(Wallet, 1_000, None)]),
            ErrorCode::InsufficientBalance,
            "payments[0].amount_minor",
        ),
        (
            sell_intent(&p, &pack, vec![pay(Credit, 1_000, None)]),
            ErrorCode::ValidationFailed,
            "payments[0].method",
        ),
        (
            sell_intent(&p, &pack, vec![pay(Upi, 1_000, None)]),
            ErrorCode::ReferenceRequired,
            "payments[0].reference",
        ),
    ];
    for (i, code, field) in cases {
        let got = refusal(end(&f, i).await);
        assert_eq!(got, (code, Some(field.to_owned())), "{field}");
    }
    let mut with_rev = sell_intent(&p, &pack, cash(1_000));
    with_rev.base_rev = Some(1);
    assert_eq!(
        refusal(end(&f, with_rev).await).0,
        ErrorCode::ValidationFailed
    );
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
}

#[tokio::test]
async fn undoing_a_plan_sale_needs_the_same_staff_and_the_window() {
    use arena_proto::PaymentMethod::Cash;
    let f = bootstrapped().await;
    let p = player(&f, "asha", "+919876543210").await;
    let plan = plan_of(&f, "pack", 1_000, true).await;
    end(&f, sell_intent(&p, &plan, vec![pay(Cash, 1_000, None)]))
        .await
        .unwrap();
    let o = sale_order(&f);
    let (id, rev) = (text(&o, "id"), int(&o, "rev"));
    let other = staff(&f, "someone", ROLE_COUNTER).await;
    let mut by_other = sell_revert_intent(&id, Some(rev));
    by_other.actor = other;
    assert_eq!(
        end(&f, by_other).await.unwrap_err().code,
        ErrorCode::UndoNotAllowed
    );
    assert_eq!(
        end(&f, sell_revert_intent(&id, Some(rev + 1)))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Stale
    );
    f.clock.advance(9_000);
    assert_eq!(
        end(&f, sell_revert_intent(&id, Some(rev)))
            .await
            .unwrap_err()
            .code,
        ErrorCode::UndoNotAllowed
    );
    assert_eq!(table(&f, Table::PlayerPacks).len(), 1);
}

#[tokio::test]
async fn the_wallet_view_lists_owned_plans_with_an_expired_flag() {
    use arena_proto::PaymentMethod::Cash;
    let f = bootstrapped().await;
    let p = player(&f, "asha", "+919876543210").await;
    let plan = plan_of(&f, "pack", 1_000, true).await;
    end(&f, sell_intent(&p, &plan, vec![pay(Cash, 1_000, None)]))
        .await
        .unwrap();
    let now = f.clock.now_ms();
    let reader = crate::SnapshotReader::open(&f.path).unwrap();
    let view = reader.wallet(&p, now).unwrap();
    assert_eq!(view.plans.len(), 1);
    assert!(!view.plans[0].expired);
    assert_eq!(view.plans[0].minutes_left, Some(300));
    let late = reader.wallet(&p, now + 8 * 24 * 60 * 60_000).unwrap();
    assert!(late.plans[0].expired);
}

/// Story 5.27: Sessions counts a session's time and the play-time revenue of its settled bill.
#[tokio::test]
async fn the_sessions_report_counts_minutes_and_revenue_once_the_bill_is_settled() {
    use arena_proto::PaymentMethod::Cash;
    let f = bootstrapped().await;
    let (order, device, rev) = billed(&f, 10_000, 41).await;
    let reader = crate::SnapshotReader::open(&f.path).unwrap();
    let day = "2027-01-15";
    let r = reader.sessions_report(day, day, f.clock.now_ms()).unwrap();
    assert_eq!((r.sessions, r.minutes, r.revenue_minor), (1, 41, 0));
    assert_eq!(r.by_device[0].device_id, device);
    assert_eq!(r.by_group.len(), 1);
    assert_eq!(r.by_group[0].utilisation_bp.len(), 24);
    assert!(r.by_group[0].utilisation_bp.iter().any(|u| *u > 0));

    let total = int(&table(&f, Table::Orders)[0], "total_minor");
    end(
        &f,
        settle_intent(&order, Some(rev), vec![pay(Cash, total, None)]),
    )
    .await
    .unwrap();
    let r = reader.sessions_report(day, day, f.clock.now_ms()).unwrap();
    // Billed minutes: 41 rounded up to the branch step.
    assert_eq!((r.sessions, r.minutes, r.revenue_minor), (1, 45, total));
    assert_eq!(r.by_device[0].revenue_minor, total);
    assert_eq!(r.by_group[0].revenue_minor, total);
    let sales = reader.daily_sales(day, day, f.clock.now_ms()).unwrap();
    assert_eq!((sales.orders, sales.sales_minor), (1, total));
    let play = sales.by_type.iter().find(|t| t.key == "play_time").unwrap();
    assert_eq!(play.amount_minor, total);
    assert_eq!(sales.by_group[0].id, r.by_group[0].group_id);
}

/// Story 5.27: a 90-day range over a busy branch (about 43,000 settled sessions, 480 a day, like
/// 60 devices at 8 sessions each) reads and renders inside the 2 s budget, off `apply()`.
#[tokio::test]
async fn a_ninety_day_report_over_a_busy_branch_builds_inside_two_seconds() {
    let f = bootstrapped().await;
    let (_, _, _) = billed(&f, 10_000, 41).await;
    let conn = arena_store::DbConn::open(&f.path).unwrap();
    conn.execute_batch(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 43000)
         INSERT INTO sessions (id, player_id, guest_no, device_id, group_id, mode, state,
             started_at, ended_at, rounding_min, minimum_min, grace_min, time_up_mode, epoch,
             business_day, fy, rev, created_at, updated_at)
         SELECT 'load-' || n.i, NULL, NULL, s.device_id, s.group_id, 'postpaid', 'ended',
             s.started_at, s.ended_at, s.rounding_min, s.minimum_min, s.grace_min, s.time_up_mode,
             s.epoch, date(s.business_day, '-' || (n.i % 90) || ' days'), s.fy, 0, 0, 0
         FROM sessions s, n WHERE s.id NOT LIKE 'load-%';
         INSERT INTO session_segments (id, session_id, from_at, to_at, device_id,
             rate_minor_per_hour, rev, created_at, updated_at)
         SELECT 'seg-' || s.id, s.id, s.started_at + (abs(random()) % 86400000), NULL,
             s.device_id, 10000, 0, 0, 0
         FROM sessions s WHERE s.id LIKE 'load-%';
         UPDATE session_segments SET to_at = from_at + 2700000 WHERE id LIKE 'seg-load-%';
         INSERT INTO orders (id, session_id, status, subtotal_minor, total_minor, business_day, fy,
             rev, created_at, updated_at, settled_at, kind)
         SELECT 'o-' || s.id, s.id, 'settled', 7500, 7500, s.business_day, s.fy, 0, 0, 0,
             s.started_at, 'session'
         FROM sessions s WHERE s.id LIKE 'load-%';
         INSERT INTO order_lines (id, order_id, from_at, to_at, minutes, rate_minor_per_hour,
             amount_minor, position, rev, created_at, updated_at)
         SELECT 'l-' || s.id, 'o-' || s.id, 0, 0, 45, 10000, 7500, 0, 0, 0, 0
         FROM sessions s WHERE s.id LIKE 'load-%';",
    )
    .unwrap();
    drop(conn);
    let reader = crate::SnapshotReader::open(&f.path).unwrap();
    let (from, to) = ("2026-10-18", "2027-01-15");
    let started = std::time::Instant::now();
    let sales = reader.daily_sales(from, to, f.clock.now_ms()).unwrap();
    let sessions = reader.sessions_report(from, to, f.clock.now_ms()).unwrap();
    let took = started.elapsed();
    assert_eq!(sales.orders, 43_000);
    assert_eq!(sales.sales_minor, 43_000 * 7_500);
    assert_eq!(
        (sessions.sessions, sessions.minutes),
        (43_001, 43_000 * 45 + 41)
    );
    assert!(took.as_secs_f64() < 2.0, "two reports took {took:?}");
}
