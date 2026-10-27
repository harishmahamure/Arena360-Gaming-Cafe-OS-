use super::players::{add, form};
use super::stock::{intent, owner, staff};
use super::*;
use crate::rbac::{ROLE_COUNTER, ROLE_KITCHEN};
use arena_proto::{
    ClientClass, DeviceAssign, DeviceKind, DeviceMaintenance, DevicePower, DeviceRegister,
    GroupUpsert, PcAction, RateUpsert, SessionMode, SessionStart,
};

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

async fn submit(f: &Fixture, actor: Actor, p: IntentPayload) -> Result<Applied, AppError> {
    let mut i = intent(actor, p);
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

async fn pc(f: &Fixture, kind: DeviceKind, group_kind: &str, rate: Option<i64>) -> String {
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

fn start_of(device: &str, player: Option<&str>) -> IntentPayload {
    IntentPayload::SessionStart(SessionStart {
        device_id: device.into(),
        mode: SessionMode::Postpaid,
        player_id: player.map(Into::into),
    })
}

async fn start(f: &Fixture, device: &str, player: Option<&str>) -> Result<Applied, AppError> {
    submit(f, owner(), start_of(device, player)).await
}

fn refusal(r: Result<Applied, AppError>) -> (ErrorCode, Option<String>) {
    let e = r.unwrap_err();
    (e.code, e.field)
}

fn sessions(f: &Fixture) -> Vec<Row> {
    reopen(f).rows(Table::Sessions).unwrap()
}

fn state_of(f: &Fixture, device: &str) -> String {
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
    let console = pc(&f, DeviceKind::Console, "console", Some(100)).await;
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
        refusal(start(&f, &console, None).await),
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
    let pack = IntentPayload::SessionStart(SessionStart {
        device_id: ok.clone(),
        mode: SessionMode::Pack,
        player_id: None,
    });
    assert_eq!(
        refusal(submit(&f, owner(), pack).await),
        field(ErrorCode::ValidationFailed, "mode")
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

fn end_intent(session: &str, rev: Option<i64>) -> Intent {
    let mut i = intent(
        owner(),
        IntentPayload::SessionEnd(arena_proto::SessionEnd {
            session_id: session.into(),
        }),
    );
    i.base_rev = rev;
    i
}

async fn end(f: &Fixture, i: Intent) -> Result<Applied, AppError> {
    let mut i = i;
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

fn table(f: &Fixture, t: Table) -> Vec<Row> {
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

fn pay(
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

fn settle_intent(order: &str, rev: Option<i64>, payments: Vec<arena_proto::PaymentRow>) -> Intent {
    let mut i = intent(
        owner(),
        IntentPayload::SessionSettle(arena_proto::SessionSettle {
            order_id: order.into(),
            payments,
        }),
    );
    i.base_rev = rev;
    i
}

async fn billed(f: &Fixture, rate: i64, minutes: i64) -> (String, String, i64) {
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
        ["payments", "payments", "orders"]
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

fn end_revert_intent(session: &str, rev: Option<i64>) -> Intent {
    let mut i = intent(
        owner(),
        IntentPayload::SessionEndReverted(arena_proto::SessionEndReverted {
            session_id: session.into(),
        }),
    );
    i.base_rev = rev;
    i
}

fn settle_revert_intent(order: &str, rev: Option<i64>) -> Intent {
    let mut i = intent(
        owner(),
        IntentPayload::SessionSettleReverted(arena_proto::SessionSettleReverted {
            order_id: order.into(),
        }),
    );
    i.base_rev = rev;
    i
}

fn rev_of(f: &Fixture, t: Table, id: &str) -> i64 {
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

async fn record_effect(f: &Fixture, kind: arena_proto::EffectKind, intent: &str, order: &str) {
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
    assert_eq!(settled.changes.len(), 2);
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
    assert_eq!(reverted.len(), 2);
    assert!(
        reverted
            .iter()
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
        .find(|c| c.intent_id == undone.intent_id)
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
