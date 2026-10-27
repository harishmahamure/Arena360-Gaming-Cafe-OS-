use super::prepaid::{
    begin, begin_block, group_of, ist, owned_pack, pack_session, sql, system, window_plan,
};
use super::rate_flip::{flip, rule};
use super::session::{
    end, end_intent, int, pc, player, refusal, rev_of, row_of, sessions, state_of, submit, table,
    text, topped_up,
};
use super::stock::{intent, owner, staff};
use super::*;
use crate::rbac::ROLE_COUNTER;
use arena_proto::{
    ClientClass, DeviceAssign, DeviceKind, DeviceRegister, PauseReason, SessionMode,
    SessionTransfer,
};

const MIN: i64 = 60_000;

async fn spare(f: &Fixture, group: Option<&str>, kind: DeviceKind) -> String {
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
    if let Some(g) = group {
        submit(
            f,
            owner(),
            IntentPayload::DeviceAssign(DeviceAssign {
                device_id: id.clone(),
                group_id: Some(g.into()),
            }),
        )
        .await
        .unwrap();
    }
    id
}

async fn online(f: &Fixture, ids: &[&str]) {
    for id in ids {
        sql(
            f,
            format!("UPDATE devices_meta SET state = 'online' WHERE device_id = '{id}'"),
        )
        .await;
    }
}

fn transfer_intent(session: &str, device: &str, rev: Option<i64>) -> Intent {
    let mut i = intent(
        owner(),
        IntentPayload::SessionTransfer(SessionTransfer {
            session_id: session.into(),
            device_id: device.into(),
        }),
    );
    i.base_rev = rev;
    i
}

async fn transfer(f: &Fixture, session: &str, device: &str) -> Result<Applied, AppError> {
    let rev = rev_of(f, Table::Sessions, session);
    end(f, transfer_intent(session, device, Some(rev))).await
}

fn field(code: ErrorCode, f: &str) -> (ErrorCode, Option<String>) {
    (code, Some(f.into()))
}

fn segments(f: &Fixture, session: &str) -> Vec<Row> {
    let mut rows: Vec<Row> = table(f, Table::SessionSegments)
        .into_iter()
        .filter(|g| text(g, "session_id") == session)
        .collect();
    rows.sort_by_key(|g| int(g, "from_at"));
    rows
}

fn to_at(g: &Row) -> Option<i64> {
    match &g["to_at"] {
        RowValue::Integer(n) => Some(*n),
        _ => None,
    }
}

async fn postpaid(f: &Fixture, d: &str, at: &str) -> String {
    f.clock.set(ist(at));
    begin(f, d, None, SessionMode::Postpaid, None)
        .await
        .unwrap();
    text(sessions(f).last().unwrap(), "id")
}

async fn pause_now(f: &Fixture, id: &str) {
    let rev = rev_of(f, Table::Sessions, id);
    let mut i = intent(
        owner(),
        IntentPayload::SessionPause(arena_proto::SessionPause {
            session_id: id.into(),
            reason: PauseReason::HardwareIssue,
            note: None,
        }),
    );
    i.base_rev = Some(rev);
    end(f, i).await.unwrap();
}

async fn resume_now(f: &Fixture, id: &str) {
    let rev = rev_of(f, Table::Sessions, id);
    let mut i = intent(
        owner(),
        IntentPayload::SessionResume(arena_proto::SessionResume {
            session_id: id.into(),
        }),
    );
    i.base_rev = Some(rev);
    end(f, i).await.unwrap();
}

fn lines(f: &Fixture) -> Vec<(i64, i64, i64)> {
    let mut rows = table(f, Table::OrderLines);
    rows.sort_by_key(|l| int(l, "position"));
    rows.iter()
        .map(|l| {
            (
                int(l, "minutes"),
                int(l, "rate_minor_per_hour"),
                int(l, "amount_minor"),
            )
        })
        .collect()
}

#[tokio::test]
async fn a_same_group_move_closes_the_segment_and_opens_the_next_on_the_new_pc() {
    let f = bootstrapped().await;
    let a = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let b = spare(&f, Some(&group_of(&f, &a)), DeviceKind::Pc).await;
    online(&f, &[&a, &b]).await;
    let id = postpaid(&f, &a, "2027-01-15 15:30").await;
    f.clock.set(ist("2027-01-15 16:00"));
    let applied = transfer(&f, &id, &b).await.unwrap();
    assert_eq!(
        applied
            .changes
            .iter()
            .map(|c| c.tbl.as_str())
            .collect::<Vec<_>>(),
        [
            "sessions",
            "session_segments",
            "session_segments",
            "devices_meta",
            "devices_meta"
        ]
    );
    let s = row_of(&f, Table::Sessions, &id);
    assert_eq!(text(&s, "device_id"), b);
    assert_eq!(text(&s, "state"), "active");
    assert_eq!(state_of(&f, &a), "online");
    assert_eq!(state_of(&f, &b), "in_session");
    let segs = segments(&f, &id);
    assert_eq!(segs.len(), 2);
    assert_eq!(text(&segs[0], "device_id"), a);
    assert_eq!(to_at(&segs[0]), Some(ist("2027-01-15 16:00")));
    assert_eq!(text(&segs[1], "device_id"), b);
    assert_eq!(int(&segs[1], "from_at"), ist("2027-01-15 16:00"));
    assert_eq!(int(&segs[1], "rate_minor_per_hour"), 6_000);
    assert_eq!(to_at(&segs[1]), None);
    f.clock.set(ist("2027-01-15 16:45"));
    let rev = rev_of(&f, Table::Sessions, &id);
    end(&f, end_intent(&id, Some(rev))).await.unwrap();
    assert_eq!(lines(&f), [(30, 6_000, 3_000), (45, 6_000, 4_500)]);
    assert_eq!(int(&table(&f, Table::Orders)[0], "total_minor"), 7_500);
    assert_eq!(state_of(&f, &b), "online");
    assert_eq!(state_of(&f, &a), "online");
}

#[tokio::test]
async fn the_old_pc_returns_to_its_prior_state_and_the_new_pc_remembers_its_own() {
    let f = bootstrapped().await;
    let a = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let b = spare(&f, Some(&group_of(&f, &a)), DeviceKind::Pc).await;
    sql(
        &f,
        format!("UPDATE devices_meta SET state = 'reserved' WHERE device_id = '{a}'"),
    )
    .await;
    sql(
        &f,
        format!("UPDATE devices_meta SET state = 'online' WHERE device_id = '{b}'"),
    )
    .await;
    let id = postpaid(&f, &a, "2027-01-15 15:30").await;
    transfer(&f, &id, &b).await.unwrap();
    assert_eq!(state_of(&f, &a), "reserved");
    let meta = table(&f, Table::DevicesMeta);
    let prior = meta.iter().find(|r| text(r, "device_id") == b).unwrap();
    assert_eq!(text(prior, "prior_state"), "online");
}

#[tokio::test]
async fn another_group_prices_the_new_segment_at_its_rate_and_rule_and_the_session_follows() {
    let f = bootstrapped().await;
    let a = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let b = pc(&f, DeviceKind::Pc, "pc", Some(9_000)).await;
    let gb = group_of(&f, &b);
    let r = rule(&f, &gb, 16 * 60, 18 * 60, -2000).await;
    let id = postpaid(&f, &a, "2027-01-15 15:30").await;
    f.clock.set(ist("2027-01-15 16:10"));
    transfer(&f, &id, &b).await.unwrap();
    let segs = segments(&f, &id);
    assert_eq!(int(&segs[0], "rate_minor_per_hour"), 6_000);
    assert_eq!(segs[0]["rule_id"], RowValue::Null);
    assert_eq!(int(&segs[1], "rate_minor_per_hour"), 7_200);
    assert_eq!(text(&segs[1], "rule_id"), r);
    let s = row_of(&f, Table::Sessions, &id);
    assert_eq!(text(&s, "group_id"), gb);
    let rate = table(&f, Table::Rates)
        .into_iter()
        .find(|r| text(r, "group_id") == gb)
        .unwrap();
    assert_eq!(text(&s, "rate_id"), text(&rate, "rate_id"));
    f.clock.set(ist("2027-01-15 18:05"));
    flip(&f, &id).await.unwrap();
    let segs = segments(&f, &id);
    assert_eq!(int(&segs[2], "rate_minor_per_hour"), 9_000);
    assert_eq!(segs[2]["rule_id"], RowValue::Null);
}

#[tokio::test]
async fn a_paused_session_stays_paused_and_its_pause_is_split_across_both_segments() {
    let f = bootstrapped().await;
    let a = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let b = spare(&f, Some(&group_of(&f, &a)), DeviceKind::Pc).await;
    let id = postpaid(&f, &a, "2027-01-15 15:30").await;
    f.clock.set(ist("2027-01-15 15:50"));
    pause_now(&f, &id).await;
    let paused_at = int(&row_of(&f, Table::Sessions, &id), "paused_at");
    f.clock.set(ist("2027-01-15 16:05"));
    transfer(&f, &id, &b).await.unwrap();
    let s = row_of(&f, Table::Sessions, &id);
    assert_eq!(text(&s, "state"), "paused");
    assert_eq!(int(&s, "paused_at"), paused_at);
    assert_eq!(text(&s, "device_id"), b);
    let segs = segments(&f, &id);
    assert_eq!(int(&segs[0], "paused_ms"), 15 * MIN);
    assert_eq!(int(&segs[1], "paused_ms"), 0);
    f.clock.set(ist("2027-01-15 16:20"));
    resume_now(&f, &id).await;
    let segs = segments(&f, &id);
    assert_eq!(int(&segs[0], "paused_ms"), 15 * MIN);
    assert_eq!(int(&segs[1], "paused_ms"), 15 * MIN);
    assert_eq!(
        int(&row_of(&f, Table::Sessions, &id), "paused_total_ms"),
        30 * MIN
    );
    f.clock.set(ist("2027-01-15 17:00"));
    let rev = rev_of(&f, Table::Sessions, &id);
    end(&f, end_intent(&id, Some(rev))).await.unwrap();
    assert_eq!(lines(&f), [(30, 6_000, 3_000), (45, 6_000, 4_500)]);
}

#[tokio::test]
async fn a_wallet_session_moved_to_a_dearer_group_ends_sooner_on_the_same_reserve() {
    let f = bootstrapped().await;
    let a = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let b = pc(&f, DeviceKind::Pc, "pc", Some(12_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    f.clock.set(ist("2027-01-15 15:30"));
    begin(&f, &a, Some(&p), SessionMode::Wallet, None)
        .await
        .unwrap();
    let id = text(sessions(&f).last().unwrap(), "id");
    let s = row_of(&f, Table::Sessions, &id);
    assert_eq!(
        (int(&s, "ends_at"), int(&s, "reserved_minor")),
        (ist("2027-01-15 17:00"), 9_000)
    );
    f.clock.set(ist("2027-01-15 16:00"));
    transfer(&f, &id, &b).await.unwrap();
    let s = row_of(&f, Table::Sessions, &id);
    assert_eq!(
        (int(&s, "ends_at"), int(&s, "reserved_minor")),
        (ist("2027-01-15 16:30"), 9_000)
    );
    f.clock.set(ist("2027-01-15 16:30"));
    let rev = rev_of(&f, Table::Sessions, &id);
    end(&f, end_intent(&id, Some(rev))).await.unwrap();
    assert_eq!(super::session::balance(&f, &p), 10_000 - 9_000);
}

#[tokio::test]
async fn a_wallet_session_moved_to_a_cheaper_group_ends_later() {
    let f = bootstrapped().await;
    let a = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let b = pc(&f, DeviceKind::Pc, "pc", Some(3_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    f.clock.set(ist("2027-01-15 15:30"));
    begin(&f, &a, Some(&p), SessionMode::Wallet, None)
        .await
        .unwrap();
    let id = text(sessions(&f).last().unwrap(), "id");
    f.clock.set(ist("2027-01-15 16:00"));
    transfer(&f, &id, &b).await.unwrap();
    let s = row_of(&f, Table::Sessions, &id);
    assert_eq!(int(&s, "ends_at"), ist("2027-01-15 18:00"));
    assert_eq!(int(&s, "reserved_minor"), 9_000);
}

#[tokio::test]
async fn a_wallet_move_at_the_same_rate_leaves_the_end_and_reserve_alone() {
    let f = bootstrapped().await;
    let a = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let b = spare(&f, Some(&group_of(&f, &a)), DeviceKind::Pc).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    f.clock.set(ist("2027-01-15 15:30"));
    begin(&f, &a, Some(&p), SessionMode::Wallet, None)
        .await
        .unwrap();
    let id = text(sessions(&f).last().unwrap(), "id");
    let before = row_of(&f, Table::Sessions, &id);
    f.clock.set(ist("2027-01-15 15:50"));
    transfer(&f, &id, &b).await.unwrap();
    let after = row_of(&f, Table::Sessions, &id);
    assert_eq!(int(&after, "ends_at"), int(&before, "ends_at"));
    assert_eq!(
        int(&after, "reserved_minor"),
        int(&before, "reserved_minor")
    );
}

#[tokio::test]
async fn a_pack_moves_only_to_a_group_its_plan_covers_and_keeps_its_end() {
    let f = bootstrapped().await;
    let a = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let b = pc(&f, DeviceKind::Pc, "pc", Some(9_000)).await;
    let c = pc(&f, DeviceKind::Pc, "pc", Some(9_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 50_000).await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &a), group_of(&f, &b)], 120).await;
    f.clock.set(ist("2027-01-15 15:30"));
    let id = pack_session(&f, &a, &p, &pack).await;
    let ends_at = int(&row_of(&f, Table::Sessions, &id), "ends_at");
    f.clock.set(ist("2027-01-15 15:40"));
    assert_eq!(
        refusal(transfer(&f, &id, &c).await),
        field(ErrorCode::PlanNotForGroup, "device_id")
    );
    assert_eq!(segments(&f, &id).len(), 1);
    transfer(&f, &id, &b).await.unwrap();
    let s = row_of(&f, Table::Sessions, &id);
    assert_eq!(int(&s, "ends_at"), ends_at);
    assert_eq!(text(&s, "device_id"), b);
    assert_eq!(text(&s, "group_id"), group_of(&f, &b));
    let segs = segments(&f, &id);
    assert_eq!(int(&segs[1], "rate_minor_per_hour"), 0);
    assert_eq!(segs[1]["rule_id"], RowValue::Null);
}

#[tokio::test]
async fn a_block_stays_in_its_group_and_its_open_bill_follows_it() {
    let f = bootstrapped().await;
    let a = pc(&f, DeviceKind::Pc, "pc", None).await;
    let other = pc(&f, DeviceKind::Pc, "pc", None).await;
    let b = spare(&f, Some(&group_of(&f, &a)), DeviceKind::Pc).await;
    let plan = window_plan(
        &f,
        "block",
        &[group_of(&f, &a), group_of(&f, &other)],
        20_000,
    )
    .await;
    f.clock.set(ist("2027-01-15 23:45"));
    begin_block(&f, &a, None, &plan, SessionMode::Block)
        .await
        .unwrap();
    let id = text(sessions(&f).last().unwrap(), "id");
    f.clock.set(ist("2027-01-16 00:10"));
    assert_eq!(
        refusal(transfer(&f, &id, &other).await),
        field(ErrorCode::PlanNotForGroup, "device_id")
    );
    let applied = transfer(&f, &id, &b).await.unwrap();
    assert!(applied.changes.iter().any(|c| c.tbl == "orders"));
    let order = table(&f, Table::Orders).pop().unwrap();
    assert_eq!(text(&order, "device_id"), b);
    assert_eq!(text(&order, "status"), "open");
    assert_eq!(
        int(&row_of(&f, Table::Sessions, &id), "ends_at"),
        ist("2027-01-16 06:00")
    );
}

#[tokio::test]
async fn every_unfit_target_is_refused_in_order_and_writes_nothing() {
    let f = bootstrapped().await;
    let a = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let group = group_of(&f, &a);
    let busy = spare(&f, Some(&group), DeviceKind::Pc).await;
    let maint = spare(&f, Some(&group), DeviceKind::Pc).await;
    let loose = spare(&f, None, DeviceKind::Pc).await;
    let console = pc(&f, DeviceKind::Console, "console", Some(6_000)).await;
    let no_rate = pc(&f, DeviceKind::Pc, "pc", None).await;
    let id = postpaid(&f, &a, "2027-01-15 15:30").await;
    begin(&f, &busy, None, SessionMode::Postpaid, None)
        .await
        .unwrap();
    sql(
        &f,
        format!("UPDATE devices_meta SET state = 'maintenance' WHERE device_id = '{maint}'"),
    )
    .await;
    f.clock.set(ist("2027-01-15 16:00"));
    let before = (table(&f, Table::SessionSegments).len(), sessions(&f));
    let changes_before = reopen(&f).changes().unwrap().len();
    for (target, want) in [
        ("NOPE", field(ErrorCode::NotFound, "device_id")),
        (a.as_str(), field(ErrorCode::ValidationFailed, "device_id")),
        (&loose, field(ErrorCode::DeviceUnassigned, "device_id")),
        (&console, field(ErrorCode::GroupKindMismatch, "device_id")),
        (&maint, field(ErrorCode::PcUnavailable, "device_id")),
        (&busy, field(ErrorCode::PcBusy, "device_id")),
        (&no_rate, field(ErrorCode::NoRate, "device_id")),
    ] {
        assert_eq!(refusal(transfer(&f, &id, target).await), want, "{target}");
    }
    assert_eq!(
        (table(&f, Table::SessionSegments).len(), sessions(&f)),
        before
    );
    assert_eq!(reopen(&f).changes().unwrap().len(), changes_before);
    assert_eq!(state_of(&f, &a), "in_session");
}

#[tokio::test]
async fn only_a_running_or_paused_session_moves() {
    let f = bootstrapped().await;
    let a = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let b = spare(&f, Some(&group_of(&f, &a)), DeviceKind::Pc).await;
    let id = postpaid(&f, &a, "2027-01-15 15:30").await;
    for state in ["grace", "expired"] {
        sql(
            &f,
            format!("UPDATE sessions SET state = '{state}' WHERE id = '{id}'"),
        )
        .await;
        assert_eq!(
            refusal(transfer(&f, &id, &b).await),
            field(ErrorCode::ValidationFailed, "session_id"),
            "{state}"
        );
    }
    sql(
        &f,
        format!("UPDATE sessions SET state = 'active' WHERE id = '{id}'"),
    )
    .await;
    f.clock.set(ist("2027-01-15 16:00"));
    let rev = rev_of(&f, Table::Sessions, &id);
    end(&f, end_intent(&id, Some(rev))).await.unwrap();
    assert_eq!(
        refusal(transfer(&f, &id, &b).await),
        field(ErrorCode::ValidationFailed, "session_id")
    );
    assert_eq!(
        end(&f, transfer_intent("NOPE", &b, Some(0)))
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn a_stale_or_missing_base_rev_is_refused() {
    let f = bootstrapped().await;
    let a = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let b = spare(&f, Some(&group_of(&f, &a)), DeviceKind::Pc).await;
    let id = postpaid(&f, &a, "2027-01-15 15:30").await;
    let rev = rev_of(&f, Table::Sessions, &id);
    assert_eq!(
        refusal(end(&f, transfer_intent(&id, &b, Some(rev + 1))).await),
        field(ErrorCode::Stale, "base_rev")
    );
    assert_eq!(
        refusal(end(&f, transfer_intent(&id, &b, None)).await),
        field(ErrorCode::ValidationFailed, "base_rev")
    );
    assert_eq!(segments(&f, &id).len(), 1);
}

#[tokio::test]
async fn a_retried_key_returns_the_stored_outcome_and_moves_once() {
    let f = bootstrapped().await;
    let a = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let b = spare(&f, Some(&group_of(&f, &a)), DeviceKind::Pc).await;
    let id = postpaid(&f, &a, "2027-01-15 15:30").await;
    f.clock.set(ist("2027-01-15 16:00"));
    let rev = rev_of(&f, Table::Sessions, &id);
    let i = transfer_intent(&id, &b, Some(rev));
    let first = end(&f, i.clone()).await.unwrap();
    let again = end(&f, i).await.unwrap();
    assert_eq!(first, again);
    assert_eq!(segments(&f, &id).len(), 2);
}

#[tokio::test]
async fn counter_may_transfer_and_the_system_may_not() {
    let f = bootstrapped().await;
    let a = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let b = spare(&f, Some(&group_of(&f, &a)), DeviceKind::Pc).await;
    let counter = staff(&f, "till", ROLE_COUNTER).await;
    let id = postpaid(&f, &a, "2027-01-15 15:30").await;
    let rev = rev_of(&f, Table::Sessions, &id);
    let mut by_system = transfer_intent(&id, &b, Some(rev));
    by_system.actor = system();
    assert_eq!(
        end(&f, by_system).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    let mut by_counter = transfer_intent(&id, &b, Some(rev));
    by_counter.actor = counter;
    end(&f, by_counter).await.unwrap();
    assert_eq!(text(&row_of(&f, Table::Sessions, &id), "device_id"), b);
}
