use super::prepaid::{begin, group_of, ist, owned_pack, pack_session};
use super::session::{balance, int, pc, player, sessions, submit, table, text, topped_up};
use super::stock::{intent, owner};
use super::*;
use arena_proto::{DeviceKind, ExtendSource, RuleUpsert, SessionExtend, SessionMode};

const MIN: i64 = 60_000;

/// Weekdays 16:00–18:00 at 20% off.
async fn happy_hour(f: &Fixture, group: &str) -> String {
    submit(
        f,
        owner(),
        IntentPayload::RuleUpsert(RuleUpsert {
            id: None,
            name: "Happy hour".into(),
            group_id: group.into(),
            rule_kind: "weekly".into(),
            days_mask: Some(0b001_1111),
            start_min: Some(16 * 60),
            end_min: Some(18 * 60),
            date_from: None,
            date_to: None,
            adjust_kind: "percent_bp".into(),
            adjust_value: -2000,
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

fn segment_of(f: &Fixture, session: &str) -> Row {
    table(f, Table::SessionSegments)
        .into_iter()
        .find(|g| text(g, "session_id") == session)
        .unwrap()
}

#[tokio::test]
async fn a_postpaid_session_started_in_a_rule_runs_at_the_rule_rate_and_records_it() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let rule = happy_hour(&f, &group_of(&f, &d)).await;
    f.clock.set(ist("2027-01-15 16:30"));
    begin(&f, &d, None, SessionMode::Postpaid, None)
        .await
        .unwrap();
    let g = segment_of(&f, &text(&sessions(&f)[0], "id"));
    assert_eq!(int(&g, "rate_minor_per_hour"), 4_800);
    assert_eq!(text(&g, "rule_id"), rule);
}

#[tokio::test]
async fn a_session_started_outside_every_rule_runs_at_the_base_rate_with_no_rule() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    happy_hour(&f, &group_of(&f, &d)).await;
    f.clock.set(ist("2027-01-15 18:00"));
    begin(&f, &d, None, SessionMode::Postpaid, None)
        .await
        .unwrap();
    let g = segment_of(&f, &text(&sessions(&f)[0], "id"));
    assert_eq!(int(&g, "rate_minor_per_hour"), 6_000);
    assert_eq!(g["rule_id"], RowValue::Null);
}

#[tokio::test]
async fn a_rule_of_another_group_does_not_price_the_session() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let other = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    happy_hour(&f, &group_of(&f, &other)).await;
    f.clock.set(ist("2027-01-15 16:30"));
    begin(&f, &d, None, SessionMode::Postpaid, None)
        .await
        .unwrap();
    let g = segment_of(&f, &text(&sessions(&f)[0], "id"));
    assert_eq!(int(&g, "rate_minor_per_hour"), 6_000);
    assert_eq!(g["rule_id"], RowValue::Null);
}

#[tokio::test]
async fn a_wallet_session_in_a_rule_is_sized_and_refused_on_the_rule_rate() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    happy_hour(&f, &group_of(&f, &d)).await;
    let p = player(&f, "asha", "+919876543210").await;
    let start = ist("2027-01-15 16:30");
    f.clock.set(start);
    topped_up(&f, &p, 2_000).await;
    let e = begin(&f, &d, Some(&p), SessionMode::Wallet, None)
        .await
        .unwrap_err();
    assert!(e.message.contains("Rate ₹48.00 an hour"), "{}", e.message);
    assert!(e.message.contains("₹24.00"), "{}", e.message);
    topped_up(&f, &p, 8_000).await;
    begin(&f, &d, Some(&p), SessionMode::Wallet, None)
        .await
        .unwrap();
    let s = &sessions(&f)[0];
    assert_eq!(int(s, "ends_at"), start + 120 * MIN);
    assert_eq!(int(s, "reserved_minor"), 9_600);
}

#[tokio::test]
async fn an_extension_is_priced_at_the_rule_rate() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 60).await;
    f.clock.set(ist("2027-01-15 16:30"));
    let s = pack_session(&f, &d, &p, &pack).await;
    happy_hour(&f, &group_of(&f, &d)).await;
    let before = balance(&f, &p);
    let mut i = intent(
        owner(),
        IntentPayload::SessionExtend(SessionExtend {
            session_id: s,
            minutes: 60,
            source: ExtendSource::Wallet,
            pack_id: None,
            credit_override: None,
        }),
    );
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await.unwrap();
    assert_eq!(before - balance(&f, &p), 4_800);
}

#[tokio::test]
async fn the_snapshot_names_the_rule_in_effect_and_when_it_gives_way() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let group = group_of(&f, &d);
    let rule = happy_hour(&f, &group).await;
    f.clock.set(ist("2027-01-15 15:00"));
    let g = f.writer.snapshot().await.unwrap().groups.remove(0);
    assert_eq!((g.rate_minor, g.active_rule), (Some(6_000), None));
    assert_eq!(g.next_change_at, Some(ist("2027-01-15 16:00")));
    f.clock.set(ist("2027-01-15 16:30"));
    let g = f.writer.snapshot().await.unwrap().groups.remove(0);
    let active = g.active_rule.unwrap();
    assert_eq!(
        (
            g.rate_minor,
            active.rule_id,
            active.name,
            active.rate_minor,
            active.until
        ),
        (
            Some(6_000),
            rule,
            "Happy hour".to_owned(),
            4_800,
            Some(ist("2027-01-15 18:00"))
        )
    );
    assert_eq!(g.next_change_at, Some(ist("2027-01-15 18:00")));
}
