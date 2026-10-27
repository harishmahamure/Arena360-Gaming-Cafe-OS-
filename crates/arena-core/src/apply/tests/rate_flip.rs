use super::prepaid::{begin, group_of, ist, owned_pack, pack_session, sql, system};
use super::session::{
    end, end_intent, int, pc, player, refusal, rev_of, row_of, sessions, submit, table, text,
    topped_up,
};
use super::stock::{intent, owner};
use super::*;
use arena_proto::{DeviceKind, PauseReason, RuleUpsert, SessionMode, SessionRateFlip};

const MIN: i64 = 60_000;

async fn rule(f: &Fixture, group: &str, start: i64, end: i64, adjust: i64) -> String {
    submit(
        f,
        owner(),
        IntentPayload::RuleUpsert(RuleUpsert {
            id: None,
            name: "Happy hour".into(),
            group_id: group.into(),
            rule_kind: "weekly".into(),
            days_mask: Some(0b111_1111),
            start_min: Some(start),
            end_min: Some(end),
            date_from: None,
            date_to: None,
            adjust_kind: "percent_bp".into(),
            adjust_value: adjust,
            priority: 0,
            active: true,
        }),
    )
    .await
    .unwrap()
    .changes[0]
        .row_id
        .clone()
}

async fn flip(f: &Fixture, session: &str) -> Result<Applied, AppError> {
    submit(
        f,
        system(),
        IntentPayload::SessionRateFlip(SessionRateFlip {
            session_id: session.into(),
        }),
    )
    .await
}

fn segments(f: &Fixture, session: &str) -> Vec<Row> {
    let mut rows: Vec<Row> = table(f, Table::SessionSegments)
        .into_iter()
        .filter(|g| text(g, "session_id") == session)
        .collect();
    rows.sort_by_key(|g| int(g, "from_at"));
    rows
}

fn shape(f: &Fixture, session: &str) -> Vec<(i64, Option<i64>, i64)> {
    segments(f, session)
        .iter()
        .map(|g| {
            (
                int(g, "from_at"),
                match &g["to_at"] {
                    RowValue::Integer(n) => Some(*n),
                    _ => None,
                },
                int(g, "rate_minor_per_hour"),
            )
        })
        .collect()
}

async fn postpaid(f: &Fixture, d: &str, at: &str) -> String {
    f.clock.set(ist(at));
    begin(f, d, None, SessionMode::Postpaid, None)
        .await
        .unwrap();
    text(sessions(f).last().unwrap(), "id")
}

fn stale() -> (ErrorCode, Option<String>) {
    (ErrorCode::Stale, Some("session_id".into()))
}

async fn pause_now(f: &Fixture, id: &str, reason: PauseReason) {
    let rev = rev_of(f, Table::Sessions, id);
    let mut i = intent(
        owner(),
        IntentPayload::SessionPause(arena_proto::SessionPause {
            session_id: id.into(),
            reason,
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

#[tokio::test]
async fn a_rule_that_begins_and_ends_opens_a_segment_at_each_boundary() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let r = rule(&f, &group_of(&f, &d), 16 * 60, 18 * 60, -2000).await;
    let id = postpaid(&f, &d, "2027-01-15 15:30").await;
    f.clock.set(ist("2027-01-15 15:59"));
    assert_eq!(refusal(flip(&f, &id).await), stale());
    f.clock.set(ist("2027-01-15 16:07"));
    let a = flip(&f, &id).await.unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        ["session_segments", "session_segments"]
    );
    let segs = segments(&f, &id);
    assert_eq!(
        shape(&f, &id),
        [
            (
                ist("2027-01-15 15:30"),
                Some(ist("2027-01-15 16:00")),
                6_000
            ),
            (ist("2027-01-15 16:00"), None, 4_800)
        ]
    );
    assert_eq!(segs[0]["rule_id"], RowValue::Null);
    assert_eq!(text(&segs[1], "rule_id"), r);
    assert_eq!(refusal(flip(&f, &id).await), stale());
    f.clock.set(ist("2027-01-15 18:05"));
    flip(&f, &id).await.unwrap();
    assert_eq!(shape(&f, &id)[2], (ist("2027-01-15 18:00"), None, 6_000));
    assert_eq!(segments(&f, &id)[2]["rule_id"], RowValue::Null);
    f.clock.set(ist("2027-01-15 18:30"));
    let rev = rev_of(&f, Table::Sessions, &id);
    end(&f, end_intent(&id, Some(rev))).await.unwrap();
    let mut lines = table(&f, Table::OrderLines);
    lines.sort_by_key(|l| int(l, "position"));
    let got: Vec<(i64, i64, i64)> = lines
        .iter()
        .map(|l| {
            (
                int(l, "minutes"),
                int(l, "rate_minor_per_hour"),
                int(l, "amount_minor"),
            )
        })
        .collect();
    assert_eq!(
        got,
        [(30, 6_000, 3_000), (120, 4_800, 9_600), (30, 6_000, 3_000)]
    );
    assert_eq!(int(&table(&f, Table::Orders)[0], "total_minor"), 15_600);
}

#[tokio::test]
async fn missed_boundaries_are_applied_in_order_each_at_its_true_time() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    rule(&f, &group_of(&f, &d), 16 * 60, 18 * 60, -2000).await;
    let id = postpaid(&f, &d, "2027-01-15 15:30").await;
    f.clock.set(ist("2027-01-15 19:00"));
    flip(&f, &id).await.unwrap();
    flip(&f, &id).await.unwrap();
    assert_eq!(refusal(flip(&f, &id).await), stale());
    assert_eq!(
        shape(&f, &id),
        [
            (
                ist("2027-01-15 15:30"),
                Some(ist("2027-01-15 16:00")),
                6_000
            ),
            (
                ist("2027-01-15 16:00"),
                Some(ist("2027-01-15 18:00")),
                4_800
            ),
            (ist("2027-01-15 18:00"), None, 6_000)
        ]
    );
}

#[tokio::test]
async fn an_edit_or_a_switch_off_flips_at_the_moment_it_is_applied() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let r = rule(&f, &group_of(&f, &d), 16 * 60, 18 * 60, -2000).await;
    let id = postpaid(&f, &d, "2027-01-15 16:10").await;
    f.clock.set(ist("2027-01-15 16:30"));
    assert_eq!(refusal(flip(&f, &id).await), stale());
    sql(
        &f,
        format!("UPDATE pricing_rules SET adjust_value = -3000 WHERE rule_id = '{r}'"),
    )
    .await;
    flip(&f, &id).await.unwrap();
    let segs = segments(&f, &id);
    assert_eq!(shape(&f, &id)[1], (ist("2027-01-15 16:30"), None, 4_200));
    assert_eq!(text(&segs[1], "rule_id"), r);
    f.clock.set(ist("2027-01-15 16:45"));
    sql(
        &f,
        format!("UPDATE pricing_rules SET active = 0 WHERE rule_id = '{r}'"),
    )
    .await;
    flip(&f, &id).await.unwrap();
    assert_eq!(shape(&f, &id)[2], (ist("2027-01-15 16:45"), None, 6_000));
    assert_eq!(segments(&f, &id)[2]["rule_id"], RowValue::Null);
}

#[tokio::test]
async fn editing_the_groups_base_rate_never_reprices_a_running_session() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let group = group_of(&f, &d);
    rule(&f, &group, 16 * 60, 18 * 60, -2000).await;
    let id = postpaid(&f, &d, "2027-01-15 15:30").await;
    submit(
        &f,
        owner(),
        IntentPayload::RateUpsert(arena_proto::RateUpsert {
            group_id: group,
            price_per_hour_minor: 9_000,
        }),
    )
    .await
    .unwrap();
    f.clock.set(ist("2027-01-15 16:05"));
    flip(&f, &id).await.unwrap();
    assert_eq!(shape(&f, &id)[1].2, 4_800);
}

#[tokio::test]
async fn a_pause_across_a_boundary_is_split_between_the_two_segments() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    rule(&f, &group_of(&f, &d), 16 * 60, 18 * 60, -2000).await;
    let id = postpaid(&f, &d, "2027-01-15 15:30").await;
    f.clock.set(ist("2027-01-15 15:50"));
    pause_now(&f, &id, PauseReason::PlayerSteppedAway).await;
    f.clock.set(ist("2027-01-15 16:05"));
    flip(&f, &id).await.unwrap();
    let segs = segments(&f, &id);
    assert_eq!(int(&segs[0], "paused_ms"), 10 * MIN);
    assert_eq!(int(&segs[1], "paused_ms"), 0);
    assert_eq!(text(&row_of(&f, Table::Sessions, &id), "state"), "paused");
    f.clock.set(ist("2027-01-15 16:20"));
    resume_now(&f, &id).await;
    let segs = segments(&f, &id);
    assert_eq!(int(&segs[0], "paused_ms"), 10 * MIN);
    assert_eq!(int(&segs[1], "paused_ms"), 20 * MIN);
    assert_eq!(
        int(&row_of(&f, Table::Sessions, &id), "paused_total_ms"),
        30 * MIN
    );
    f.clock.set(ist("2027-01-15 17:00"));
    let live = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    let lines = &live.bills[0].lines;
    assert_eq!(
        lines
            .iter()
            .map(|l| (l.minutes, l.amount_minor, l.rule_name.clone()))
            .collect::<Vec<_>>(),
        [
            (30, 3_000, None),
            (45, 3_600, Some("Happy hour".to_owned()))
        ]
    );
    let rev = rev_of(&f, Table::Sessions, &id);
    end(&f, end_intent(&id, Some(rev))).await.unwrap();
    let mut lines = table(&f, Table::OrderLines);
    lines.sort_by_key(|l| int(l, "position"));
    let got: Vec<(i64, i64)> = lines
        .iter()
        .map(|l| (int(l, "minutes"), int(l, "amount_minor")))
        .collect();
    assert_eq!(got, [(30, 3_000), (45, 3_600)]);
}

#[tokio::test]
async fn a_pause_that_began_after_the_boundary_all_falls_in_the_new_segment() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    rule(&f, &group_of(&f, &d), 16 * 60, 18 * 60, -2000).await;
    let id = postpaid(&f, &d, "2027-01-15 15:30").await;
    f.clock.set(ist("2027-01-15 16:10"));
    flip(&f, &id).await.unwrap();
    pause_now(&f, &id, PauseReason::HardwareIssue).await;
    f.clock.set(ist("2027-01-15 16:25"));
    resume_now(&f, &id).await;
    let segs = segments(&f, &id);
    assert_eq!(int(&segs[0], "paused_ms"), 0);
    assert_eq!(int(&segs[1], "paused_ms"), 15 * MIN);
}

async fn wallet(f: &Fixture, d: &str, p: &str, at: &str) -> String {
    f.clock.set(ist(at));
    begin(f, d, Some(p), SessionMode::Wallet, None)
        .await
        .unwrap();
    text(sessions(f).last().unwrap(), "id")
}

#[tokio::test]
async fn a_wallet_rate_drop_ends_it_later_on_the_same_reserve() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    rule(&f, &group_of(&f, &d), 16 * 60, 18 * 60, -2000).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    let id = wallet(&f, &d, &p, "2027-01-15 15:30").await;
    let s = row_of(&f, Table::Sessions, &id);
    assert_eq!(
        (int(&s, "ends_at"), int(&s, "reserved_minor")),
        (ist("2027-01-15 17:00"), 9_000)
    );
    f.clock.set(ist("2027-01-15 16:01"));
    let a = flip(&f, &id).await.unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        ["sessions", "session_segments", "session_segments"]
    );
    let s = row_of(&f, Table::Sessions, &id);
    assert_eq!(
        (int(&s, "ends_at"), int(&s, "reserved_minor")),
        (ist("2027-01-15 17:15"), 9_000)
    );
}

#[tokio::test]
async fn a_wallet_rate_rise_ends_it_sooner_and_the_wallet_stays_non_negative() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    rule(&f, &group_of(&f, &d), 16 * 60, 18 * 60, 5000).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    let id = wallet(&f, &d, &p, "2027-01-15 15:30").await;
    f.clock.set(ist("2027-01-15 16:01"));
    flip(&f, &id).await.unwrap();
    let s = row_of(&f, Table::Sessions, &id);
    assert_eq!(
        (int(&s, "ends_at"), int(&s, "reserved_minor")),
        (ist("2027-01-15 16:30"), 7_500)
    );
    f.clock.set(ist("2027-01-15 16:30"));
    let rev = rev_of(&f, Table::Sessions, &id);
    end(&f, end_intent(&id, Some(rev))).await.unwrap();
    assert_eq!(super::session::balance(&f, &p), 10_000 - 7_500);
}

#[tokio::test]
async fn a_rise_that_leaves_no_reserve_ends_the_session_at_the_flip() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    rule(&f, &group_of(&f, &d), 16 * 60, 18 * 60, 100_000).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 4_500).await;
    let id = wallet(&f, &d, &p, "2027-01-15 15:30").await;
    f.clock.set(ist("2027-01-15 16:01"));
    flip(&f, &id).await.unwrap();
    let s = row_of(&f, Table::Sessions, &id);
    assert_eq!(int(&s, "ends_at"), ist("2027-01-15 16:00"));
    assert_eq!(int(&s, "reserved_minor"), 3_000);
}

#[tokio::test]
async fn a_wallet_session_past_its_own_end_is_not_flipped() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    rule(&f, &group_of(&f, &d), 17 * 60 + 15, 18 * 60, -2000).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    let id = wallet(&f, &d, &p, "2027-01-15 15:30").await;
    f.clock.set(ist("2027-01-15 17:30"));
    assert_eq!(refusal(flip(&f, &id).await), stale());
    sql(
        &f,
        format!(
            "UPDATE sessions SET extended_min = 60, ends_at = {} WHERE id = '{id}'",
            ist("2027-01-15 18:00")
        ),
    )
    .await;
    assert_eq!(refusal(flip(&f, &id).await), stale());
    assert_eq!(shape(&f, &id).len(), 1);
}

#[tokio::test]
async fn a_pack_session_never_flips() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let group = group_of(&f, &d);
    rule(&f, &group, 16 * 60, 18 * 60, -2000).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    let pack = owned_pack(&f, &p, &[group], 120).await;
    f.clock.set(ist("2027-01-15 15:30"));
    let id = pack_session(&f, &d, &p, &pack).await;
    f.clock.set(ist("2027-01-15 16:30"));
    assert_eq!(refusal(flip(&f, &id).await), stale());
}

#[tokio::test]
async fn only_the_system_flips_and_a_flip_takes_no_base_rev() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    rule(&f, &group_of(&f, &d), 16 * 60, 18 * 60, -2000).await;
    let id = postpaid(&f, &d, "2027-01-15 15:30").await;
    f.clock.set(ist("2027-01-15 16:30"));
    let denied = submit(
        &f,
        owner(),
        IntentPayload::SessionRateFlip(SessionRateFlip {
            session_id: id.clone(),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(denied.code, ErrorCode::PermissionDenied);
    let mut i = intent(
        system(),
        IntentPayload::SessionRateFlip(SessionRateFlip {
            session_id: id.clone(),
        }),
    );
    i.base_rev = Some(1);
    assert_eq!(
        refusal(end(&f, i).await),
        (ErrorCode::ValidationFailed, Some("base_rev".into()))
    );
    let missing = flip(&f, "nope").await.unwrap_err();
    assert_eq!(missing.code, ErrorCode::NotFound);
    assert_eq!(shape(&f, &id).len(), 1);
}

#[tokio::test]
async fn the_snapshot_scan_agrees_with_apply() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    rule(&f, &group_of(&f, &d), 16 * 60, 18 * 60, -2000).await;
    let id = postpaid(&f, &d, "2027-01-15 15:30").await;
    f.clock.set(ist("2027-01-15 15:45"));
    let reader = crate::SnapshotReader::open(reopen(&f).path()).unwrap();
    let scan = reader.rate_flips(f.clock.now_ms()).unwrap();
    assert_eq!(scan.due, Vec::<String>::new());
    assert_eq!(scan.next_at, Some(ist("2027-01-15 16:00")));
    f.clock.set(ist("2027-01-15 16:30"));
    assert_eq!(
        reader.rate_flips(f.clock.now_ms()).unwrap().due,
        vec![id.clone()]
    );
    flip(&f, &id).await.unwrap();
    assert!(reader.rate_flips(f.clock.now_ms()).unwrap().due.is_empty());
    let rev = rev_of(&f, Table::Sessions, &id);
    end(&f, end_intent(&id, Some(rev))).await.unwrap();
    assert!(reader.rate_flips(f.clock.now_ms()).unwrap().due.is_empty());
    assert_eq!(refusal(flip(&f, &id).await), stale());
}
