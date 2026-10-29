use arena_proto::PaymentMethod::{Cash, Wallet};

use super::prepaid::{begin, group_of, ist, owned_pack, pack_session};
use super::session::{
    end, end_intent, end_revert_intent, int, pay, pc, player, refusal, rev_of, row_of,
    settle_intent, settle_revert_intent, table, text, topped_up,
};
use super::stock::{intent, owner};
use super::transfer::{online, postpaid, spare};
use super::*;
use arena_proto::{DeviceKind, GroupPayer, SessionGroup, SessionMode};

fn group_intent(host: &str, members: &[&str], rev: Option<i64>) -> Intent {
    let mut i = intent(
        owner(),
        IntentPayload::SessionGroup(SessionGroup {
            session_id: host.into(),
            member_ids: members.iter().map(|m| (*m).to_owned()).collect(),
        }),
    );
    i.base_rev = rev;
    i
}

async fn group(f: &Fixture, host: &str, members: &[&str]) -> Result<Applied, AppError> {
    let rev = rev_of(f, Table::Sessions, host);
    end(f, group_intent(host, members, Some(rev))).await
}

fn payer_intent(group: &str, player: Option<&str>, guest: Option<i64>, rev: Option<i64>) -> Intent {
    let mut i = intent(
        owner(),
        IntentPayload::GroupPayer(GroupPayer {
            group_bill_id: group.into(),
            player_id: player.map(Into::into),
            guest_no: guest,
        }),
    );
    i.base_rev = rev;
    i
}

async fn set_payer(
    f: &Fixture,
    group: &str,
    player: Option<&str>,
    guest: Option<i64>,
) -> Result<Applied, AppError> {
    let rev = int(&group_row(f), "rev");
    end(f, payer_intent(group, player, guest, Some(rev))).await
}

fn group_row(f: &Fixture) -> Row {
    table(f, Table::GroupBills).pop().unwrap()
}

fn group_id(f: &Fixture) -> String {
    text(&group_row(f), "id")
}

async fn finish(f: &Fixture, session: &str, at: &str) -> Result<Applied, AppError> {
    f.clock.set(ist(at));
    let rev = rev_of(f, Table::Sessions, session);
    end(f, end_intent(session, Some(rev))).await
}

struct Trio {
    f: Fixture,
    a: String,
    b: String,
    c: String,
}

async fn trio() -> Trio {
    let f = bootstrapped().await;
    let a = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let g = group_of(&f, &a);
    let b = spare(&f, Some(&g), DeviceKind::Pc).await;
    let c = spare(&f, Some(&g), DeviceKind::Pc).await;
    online(&f, &[&a, &b, &c]).await;
    Trio { f, a, b, c }
}

#[tokio::test]
async fn adding_sessions_to_a_group_links_them_under_one_bill_paid_by_the_host() {
    let Trio { f, a, b, c } = trio().await;
    let player = player(&f, "asha", "9000000001").await;
    f.clock.set(ist("2027-01-15 15:00"));
    begin(&f, &a, Some(&player), SessionMode::Postpaid, None)
        .await
        .unwrap();
    let sa = text(table(&f, Table::Sessions).last().unwrap(), "id");
    let sb = postpaid(&f, &b, "2027-01-15 15:05").await;
    let sc = postpaid(&f, &c, "2027-01-15 15:10").await;
    let applied = group(&f, &sa, &[&sb]).await.unwrap();
    assert_eq!(
        applied
            .changes
            .iter()
            .map(|c| c.tbl.as_str())
            .collect::<Vec<_>>(),
        ["group_bills", "sessions", "sessions"]
    );
    let gid = group_id(&f);
    let g = group_row(&f);
    assert_eq!(text(&g, "state"), "open");
    assert_eq!(text(&g, "payer_player_id"), player);
    for id in [&sa, &sb] {
        assert_eq!(text(&row_of(&f, Table::Sessions, id), "group_bill_id"), gid);
    }
    group(&f, &sb, &[&sc]).await.unwrap();
    assert_eq!(table(&f, Table::GroupBills).len(), 1);
    assert_eq!(
        text(&row_of(&f, Table::Sessions, &sc), "group_bill_id"),
        gid
    );
}

#[tokio::test]
async fn a_guest_host_names_its_guest_number_as_the_payer() {
    let Trio { f, a, b, .. } = trio().await;
    let sa = postpaid(&f, &a, "2027-01-15 15:00").await;
    let sb = postpaid(&f, &b, "2027-01-15 15:05").await;
    group(&f, &sa, &[&sb]).await.unwrap();
    let g = group_row(&f);
    assert_eq!(
        int(&g, "payer_guest_no"),
        int(&row_of(&f, Table::Sessions, &sa), "guest_no")
    );
}

#[tokio::test]
async fn only_running_postpaid_sessions_share_a_bill() {
    let Trio { f, a, b, c } = trio().await;
    let player = player(&f, "ravi", "9000000002").await;
    topped_up(&f, &player, 50_000).await;
    let g = group_of(&f, &a);
    let pack = owned_pack(&f, &player, &[g], 120).await;
    let sa = postpaid(&f, &a, "2027-01-15 15:00").await;
    let sb = pack_session(&f, &b, &player, &pack).await;
    let bad = group(&f, &sa, &[&sb]).await;
    assert_eq!(
        bad.unwrap_err().message,
        "Only postpaid sessions can share a bill"
    );
    assert!(table(&f, Table::GroupBills).is_empty());
    let rev = rev_of(&f, Table::Sessions, &sb);
    let host = end(&f, group_intent(&sb, &[&sa], Some(rev))).await;
    assert_eq!(
        host.unwrap_err().message,
        "Only postpaid sessions can share a bill"
    );
    let sc = postpaid(&f, &c, "2027-01-15 15:20").await;
    finish(&f, &sc, "2027-01-15 15:30").await.unwrap();
    assert_eq!(
        refusal(group(&f, &sa, &[&sc]).await),
        (ErrorCode::ValidationFailed, Some("member_ids".into()))
    );
}

#[tokio::test]
async fn a_session_already_in_another_group_is_refused() {
    let Trio { f, a, b, c } = trio().await;
    let sa = postpaid(&f, &a, "2027-01-15 15:00").await;
    let sb = postpaid(&f, &b, "2027-01-15 15:05").await;
    let sc = postpaid(&f, &c, "2027-01-15 15:10").await;
    group(&f, &sa, &[&sb]).await.unwrap();
    assert_eq!(
        refusal(group(&f, &sc, &[&sb]).await),
        (ErrorCode::ValidationFailed, Some("member_ids".into()))
    );
    assert_eq!(
        refusal(group(&f, &sa, &[&sa]).await),
        (ErrorCode::ValidationFailed, Some("member_ids".into()))
    );
    assert_eq!(
        refusal(group(&f, &sa, &[]).await),
        (ErrorCode::ValidationFailed, Some("member_ids".into()))
    );
    assert_eq!(
        refusal(group(&f, &sa, &["nope"]).await),
        (ErrorCode::NotFound, Some("member_ids".into()))
    );
}

#[tokio::test]
async fn grouping_needs_the_current_session_revision() {
    let Trio { f, a, b, .. } = trio().await;
    let sa = postpaid(&f, &a, "2027-01-15 15:00").await;
    let sb = postpaid(&f, &b, "2027-01-15 15:05").await;
    assert_eq!(
        refusal(end(&f, group_intent(&sa, &[&sb], None)).await),
        (ErrorCode::ValidationFailed, Some("base_rev".into()))
    );
    let stale = rev_of(&f, Table::Sessions, &sa) + 1;
    assert_eq!(
        refusal(end(&f, group_intent(&sa, &[&sb], Some(stale))).await),
        (ErrorCode::Stale, Some("base_rev".into()))
    );
    assert!(table(&f, Table::GroupBills).is_empty());
}

#[tokio::test]
async fn members_end_and_free_individually_and_the_bill_appears_with_the_last() {
    let Trio { f, a, b, c } = trio().await;
    let sa = postpaid(&f, &a, "2027-01-15 15:00").await;
    let sb = postpaid(&f, &b, "2027-01-15 15:10").await;
    let sc = postpaid(&f, &c, "2027-01-15 15:20").await;
    group(&f, &sa, &[&sb, &sc]).await.unwrap();
    let first = finish(&f, &sa, "2027-01-15 15:40").await.unwrap();
    assert_eq!(
        first
            .changes
            .iter()
            .map(|c| c.tbl.as_str())
            .collect::<Vec<_>>(),
        ["sessions", "session_segments", "devices_meta"]
    );
    assert!(table(&f, Table::Orders).is_empty());
    assert_eq!(super::session::state_of(&f, &a), "online");
    assert_eq!(text(&group_row(&f), "state"), "open");
    finish(&f, &sb, "2027-01-15 16:10").await.unwrap();
    assert!(table(&f, Table::Orders).is_empty());
    let last = finish(&f, &sc, "2027-01-15 16:20").await.unwrap();
    let tables: Vec<_> = last.changes.iter().map(|c| c.tbl.as_str()).collect();
    assert_eq!(tables.iter().filter(|t| **t == "orders").count(), 1);
    assert_eq!(tables.iter().filter(|t| **t == "order_lines").count(), 3);
    assert!(tables.contains(&"group_bills"));
    let orders = table(&f, Table::Orders);
    assert_eq!(orders.len(), 1);
    let o = &orders[0];
    assert_eq!(text(o, "session_id"), sc);
    assert_eq!(text(o, "group_bill_id"), group_id(&f));
    assert_eq!(text(o, "status"), "open");
    assert_eq!(text(&group_row(&f), "state"), "closed");
    let mut lines = table(&f, Table::OrderLines);
    lines.sort_by_key(|l| int(l, "position"));
    let shape: Vec<_> = lines
        .iter()
        .map(|l| {
            (
                text(l, "device_id"),
                int(l, "minutes"),
                int(l, "amount_minor"),
            )
        })
        .collect();
    assert_eq!(
        shape,
        [
            (a.clone(), 45, 4_500),
            (b.clone(), 60, 6_000),
            (c.clone(), 60, 6_000)
        ]
    );
    assert_eq!(int(o, "total_minor"), 16_500);
    let snap = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    assert_eq!(snap.bills.len(), 1);
    assert_eq!(snap.bills[0].lines.len(), 3);
    assert_eq!(
        snap.bills[0].group_bill_id.as_deref(),
        Some(group_id(&f).as_str())
    );
    assert_eq!(snap.bills[0].subtotal_minor, 16_500);
}

#[tokio::test]
async fn a_running_member_shows_what_the_ended_members_owe() {
    let Trio { f, a, b, .. } = trio().await;
    let sa = postpaid(&f, &a, "2027-01-15 15:00").await;
    let sb = postpaid(&f, &b, "2027-01-15 15:05").await;
    group(&f, &sa, &[&sb]).await.unwrap();
    finish(&f, &sa, "2027-01-15 15:40").await.unwrap();
    let snap = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    assert_eq!(snap.group_bills.len(), 1);
    let g = &snap.group_bills[0];
    assert_eq!(g.member_ids, std::slice::from_ref(&sb));
    assert_eq!(g.ended_lines.len(), 1);
    assert_eq!(g.ended_lines[0].amount_minor, 4_500);
    let live = snap.sessions.iter().find(|s| s.id == sb).unwrap();
    assert_eq!(live.group_bill_id.as_deref(), Some(g.id.as_str()));
}

#[tokio::test]
async fn the_payer_changes_to_a_member_or_a_looked_up_player_and_settle_follows() {
    let Trio { f, a, b, .. } = trio().await;
    let asha = player(&f, "asha", "9000000011").await;
    let ravi = player(&f, "ravi", "9000000012").await;
    topped_up(&f, &ravi, 50_000).await;
    f.clock.set(ist("2027-01-15 15:00"));
    begin(&f, &a, Some(&asha), SessionMode::Postpaid, None)
        .await
        .unwrap();
    let sa = text(table(&f, Table::Sessions).last().unwrap(), "id");
    let sb = postpaid(&f, &b, "2027-01-15 15:05").await;
    group(&f, &sa, &[&sb]).await.unwrap();
    let gid = group_id(&f);
    let guest = int(&row_of(&f, Table::Sessions, &sb), "guest_no");
    set_payer(&f, &gid, None, Some(guest)).await.unwrap();
    assert_eq!(int(&group_row(&f), "payer_guest_no"), guest);
    set_payer(&f, &gid, Some(&ravi), None).await.unwrap();
    assert_eq!(text(&group_row(&f), "payer_player_id"), ravi);
    assert_eq!(
        refusal(set_payer(&f, &gid, None, Some(guest + 9)).await),
        (ErrorCode::ValidationFailed, Some("guest_no".into()))
    );
    assert_eq!(
        refusal(set_payer(&f, &gid, Some("nobody"), None).await),
        (ErrorCode::NotFound, Some("player_id".into()))
    );
    assert_eq!(
        refusal(set_payer(&f, &gid, Some(&ravi), Some(guest)).await),
        (ErrorCode::ValidationFailed, Some("player_id".into()))
    );
    finish(&f, &sa, "2027-01-15 15:30").await.unwrap();
    finish(&f, &sb, "2027-01-15 16:00").await.unwrap();
    let o = table(&f, Table::Orders).pop().unwrap();
    assert_eq!(text(&o, "player_id"), ravi);
    let total = int(&o, "total_minor");
    end(
        &f,
        settle_intent(
            &text(&o, "id"),
            Some(int(&o, "rev")),
            vec![pay(Wallet, total, None)],
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        text(&row_of(&f, Table::Orders, &text(&o, "id")), "status"),
        "settled"
    );
}

#[tokio::test]
async fn changing_the_payer_after_the_bill_exists_moves_the_bill_and_stops_once_settled() {
    let Trio { f, a, b, .. } = trio().await;
    let ravi = player(&f, "ravi", "9000000013").await;
    let sa = postpaid(&f, &a, "2027-01-15 15:00").await;
    let sb = postpaid(&f, &b, "2027-01-15 15:05").await;
    group(&f, &sa, &[&sb]).await.unwrap();
    finish(&f, &sa, "2027-01-15 15:30").await.unwrap();
    finish(&f, &sb, "2027-01-15 16:00").await.unwrap();
    let gid = group_id(&f);
    let applied = set_payer(&f, &gid, Some(&ravi), None).await.unwrap();
    assert_eq!(
        applied
            .changes
            .iter()
            .map(|c| c.tbl.as_str())
            .collect::<Vec<_>>(),
        ["group_bills", "orders"]
    );
    let o = table(&f, Table::Orders).pop().unwrap();
    assert_eq!(text(&o, "player_id"), ravi);
    let total = int(&o, "total_minor");
    end(
        &f,
        settle_intent(
            &text(&o, "id"),
            Some(int(&o, "rev")),
            vec![pay(Cash, total, None)],
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        refusal(set_payer(&f, &gid, Some(&ravi), None).await),
        (ErrorCode::Stale, Some("group_bill_id".into()))
    );
}

#[tokio::test]
async fn undoing_the_group_settle_returns_the_whole_bill_to_unpaid() {
    let Trio { f, a, b, .. } = trio().await;
    let sa = postpaid(&f, &a, "2027-01-15 15:00").await;
    let sb = postpaid(&f, &b, "2027-01-15 15:05").await;
    group(&f, &sa, &[&sb]).await.unwrap();
    finish(&f, &sa, "2027-01-15 15:30").await.unwrap();
    finish(&f, &sb, "2027-01-15 16:00").await.unwrap();
    let o = table(&f, Table::Orders).pop().unwrap();
    let id = text(&o, "id");
    let total = int(&o, "total_minor");
    end(
        &f,
        settle_intent(&id, Some(int(&o, "rev")), vec![pay(Cash, total, None)]),
    )
    .await
    .unwrap();
    let rev = rev_of(&f, Table::Orders, &id);
    end(&f, settle_revert_intent(&id, Some(rev))).await.unwrap();
    let after = row_of(&f, Table::Orders, &id);
    assert_eq!(text(&after, "status"), "open");
    assert!(table(&f, Table::Payments).is_empty());
    assert_eq!(table(&f, Table::OrderLines).len(), 2);
}

#[tokio::test]
async fn undoing_the_last_end_removes_the_bill_and_reopens_the_group() {
    let Trio { f, a, b, .. } = trio().await;
    let sa = postpaid(&f, &a, "2027-01-15 15:00").await;
    let sb = postpaid(&f, &b, "2027-01-15 15:05").await;
    group(&f, &sa, &[&sb]).await.unwrap();
    finish(&f, &sa, "2027-01-15 15:30").await.unwrap();
    f.clock.set(ist("2027-01-15 16:00"));
    let rev = rev_of(&f, Table::Sessions, &sa);
    let denied = end(&f, end_revert_intent(&sa, Some(rev))).await;
    assert_eq!(denied.unwrap_err().code, ErrorCode::UndoNotAllowed);
    finish(&f, &sb, "2027-01-15 16:00").await.unwrap();
    let denied_first = end(
        &f,
        end_revert_intent(&sa, Some(rev_of(&f, Table::Sessions, &sa))),
    )
    .await;
    assert_eq!(denied_first.unwrap_err().code, ErrorCode::UndoNotAllowed);
    let rev = rev_of(&f, Table::Sessions, &sb);
    end(&f, end_revert_intent(&sb, Some(rev))).await.unwrap();
    assert!(table(&f, Table::Orders).is_empty());
    assert!(table(&f, Table::OrderLines).is_empty());
    assert_eq!(text(&group_row(&f), "state"), "open");
    assert_eq!(text(&row_of(&f, Table::Sessions, &sb), "state"), "active");
    finish(&f, &sb, "2027-01-15 16:10").await.unwrap();
    assert_eq!(table(&f, Table::Orders).len(), 1);
    assert_eq!(table(&f, Table::OrderLines).len(), 2);
}

#[tokio::test]
async fn a_session_that_is_not_grouped_still_bills_alone() {
    let Trio { f, a, .. } = trio().await;
    let sa = postpaid(&f, &a, "2027-01-15 15:00").await;
    finish(&f, &sa, "2027-01-15 15:30").await.unwrap();
    let o = table(&f, Table::Orders).pop().unwrap();
    assert!(!o.contains_key("group_bill_id") || o["group_bill_id"] == RowValue::Null);
    assert!(table(&f, Table::GroupBills).is_empty());
}

async fn start_with(
    f: &Fixture,
    device: &str,
    mode: SessionMode,
    pack: Option<&str>,
    player: Option<&str>,
    host: &str,
) -> Result<Applied, AppError> {
    let mut i = intent(
        owner(),
        IntentPayload::SessionStart(arena_proto::SessionStart {
            device_id: device.into(),
            mode,
            player_id: player.map(Into::into),
            pack_id: pack.map(Into::into),
            plan_id: None,
            group_with: Some(host.into()),
            pay_from_wallet: false,
        }),
    );
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

#[tokio::test]
async fn a_session_started_in_a_group_joins_it_and_makes_the_group_when_there_is_none() {
    let Trio { f, a, b, c } = trio().await;
    let sa = postpaid(&f, &a, "2027-01-15 15:00").await;
    f.clock.set(ist("2027-01-15 15:05"));
    let applied = start_with(&f, &b, SessionMode::Postpaid, None, None, &sa)
        .await
        .unwrap();
    assert_eq!(
        applied
            .changes
            .iter()
            .map(|c| c.tbl.as_str())
            .collect::<Vec<_>>(),
        [
            "sessions",
            "session_segments",
            "devices_meta",
            "group_bills",
            "sessions"
        ]
    );
    let sb = text(table(&f, Table::Sessions).last().unwrap(), "id");
    let gid = group_id(&f);
    for id in [&sa, &sb] {
        assert_eq!(text(&row_of(&f, Table::Sessions, id), "group_bill_id"), gid);
    }
    f.clock.set(ist("2027-01-15 15:10"));
    start_with(&f, &c, SessionMode::Postpaid, None, None, &sb)
        .await
        .unwrap();
    assert_eq!(table(&f, Table::GroupBills).len(), 1);
    let sc = text(table(&f, Table::Sessions).last().unwrap(), "id");
    assert_eq!(
        text(&row_of(&f, Table::Sessions, &sc), "group_bill_id"),
        gid
    );
}

#[tokio::test]
async fn starting_in_a_group_is_refused_for_a_prepaid_session_or_a_missing_host() {
    let Trio { f, a, b, .. } = trio().await;
    let player = player(&f, "asha", "9000000021").await;
    topped_up(&f, &player, 50_000).await;
    let sa = postpaid(&f, &a, "2027-01-15 15:00").await;
    let wallet = start_with(&f, &b, SessionMode::Wallet, None, Some(&player), &sa).await;
    let e = wallet.unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref(), e.message.as_str()),
        (
            ErrorCode::ValidationFailed,
            Some("group_with"),
            "Only postpaid sessions can share a bill"
        )
    );
    assert_eq!(
        refusal(start_with(&f, &b, SessionMode::Postpaid, None, None, "nope").await),
        (ErrorCode::NotFound, Some("group_with".into()))
    );
    assert!(table(&f, Table::GroupBills).is_empty());
    assert_eq!(table(&f, Table::Sessions).len(), 1);
}

#[tokio::test]
async fn a_grouped_session_cannot_be_converted_to_a_player() {
    let Trio { f, a, b, .. } = trio().await;
    let sa = postpaid(&f, &a, "2027-01-15 15:00").await;
    let sb = postpaid(&f, &b, "2027-01-15 15:05").await;
    group(&f, &sa, &[&sb]).await.unwrap();
    let (id, create) = super::session::new_profile("grouped", "9000000001");
    let rev = rev_of(&f, Table::Sessions, &sa);
    let denied = end(
        &f,
        super::session::assign_intent(&sa, &id, Some(create), Some(rev)),
    )
    .await;
    assert_eq!(
        refusal(denied),
        (ErrorCode::ValidationFailed, Some("session_id".into()))
    );
}

#[tokio::test]
async fn the_group_stays_in_the_snapshot_until_its_bill_settles() {
    let Trio { f, a, b, .. } = trio().await;
    let sa = postpaid(&f, &a, "2027-01-15 15:00").await;
    let sb = postpaid(&f, &b, "2027-01-15 15:05").await;
    group(&f, &sa, &[&sb]).await.unwrap();
    finish(&f, &sa, "2027-01-15 15:30").await.unwrap();
    finish(&f, &sb, "2027-01-15 16:00").await.unwrap();
    let snap = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    assert_eq!(snap.group_bills.len(), 1);
    assert!(snap.group_bills[0].member_ids.is_empty());
    let o = table(&f, Table::Orders).pop().unwrap();
    let order = text(&o, "id");
    let rev = int(&o, "rev");
    let both = || vec![pay(Cash, int(&o, "total_minor"), None)];
    end(&f, settle_intent(&order, Some(rev), both()))
        .await
        .unwrap();
    let snap = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    assert!(snap.group_bills.is_empty());
}
