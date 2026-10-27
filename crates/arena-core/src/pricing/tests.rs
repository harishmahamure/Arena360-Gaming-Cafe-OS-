use super::*;
use chrono::TimeZone;

const IST: Tz = Tz::Asia__Kolkata;

fn at(local: &str) -> i64 {
    let naive = NaiveDateTime::parse_from_str(local, "%Y-%m-%d %H:%M").unwrap();
    IST.from_local_datetime(&naive).unwrap().timestamp_millis()
}

fn weekly(id: &str, mask: i64, start: i64, end: i64, adjust: (&str, i64)) -> PricingRuleRow {
    PricingRuleRow {
        rule_id: id.into(),
        name: id.into(),
        group_id: "G".into(),
        rule_kind: KIND_WEEKLY.into(),
        days_mask: Some(mask),
        start_min: Some(start),
        end_min: Some(end),
        date_from: None,
        date_to: None,
        adjust_kind: adjust.0.into(),
        adjust_value: adjust.1,
        priority: 0,
        active: true,
        rev: 0,
        updated_at: 0,
    }
}

fn dated(id: &str, from: &str, to: &str, adjust: (&str, i64)) -> PricingRuleRow {
    PricingRuleRow {
        rule_kind: KIND_DATE.into(),
        days_mask: None,
        start_min: None,
        end_min: None,
        date_from: Some(from.into()),
        date_to: Some(to.into()),
        ..weekly(id, 0, 0, 0, adjust)
    }
}

const WEEKDAYS: i64 = 0b001_1111;
const FRI_SAT: i64 = 0b011_0000;

fn happy() -> PricingRuleRow {
    weekly("happy", WEEKDAYS, 16 * 60, 18 * 60, (ADJUST_PERCENT, -2000))
}

fn price(rules: &[PricingRuleRow], local: &str) -> Resolved {
    resolve(rules, "G", 10_000, IST, at(local))
}

#[test]
fn a_weekly_discount_prices_only_its_window() {
    let rules = [happy()];
    let r = price(&rules, "2027-03-10 17:00");
    assert_eq!((r.rate_minor, r.rule_id.as_deref()), (8000, Some("happy")));
    for outside in ["2027-03-10 15:59", "2027-03-10 18:00", "2027-03-13 17:00"] {
        let r = price(&rules, outside);
        assert_eq!((r.rate_minor, r.rule_id), (10_000, None), "{outside}");
    }
    assert_eq!(price(&rules, "2027-03-10 16:00").rate_minor, 8000);
}

#[test]
fn an_override_replaces_the_base_rate() {
    let rules = [weekly(
        "flat",
        0b111_1111,
        0,
        6 * 60,
        (ADJUST_OVERRIDE, 5000),
    )];
    assert_eq!(price(&rules, "2027-03-10 05:59").rate_minor, 5000);
    assert_eq!(price(&rules, "2027-03-10 06:00").rate_minor, 10_000);
}

#[test]
fn a_percentage_rounds_half_up_and_a_surcharge_adds() {
    let rules = [weekly("half", 0b111_1111, 0, 60, (ADJUST_PERCENT, -5000))];
    assert_eq!(
        resolve(&rules, "G", 333, IST, at("2027-03-10 00:30")).rate_minor,
        167
    );
    let up = [weekly("up", 0b111_1111, 0, 60, (ADJUST_PERCENT, 2500))];
    assert_eq!(price(&up, "2027-03-10 00:30").rate_minor, 12_500);
    let free = [weekly("free", 0b111_1111, 0, 60, (ADJUST_PERCENT, -10_000))];
    assert_eq!(price(&free, "2027-03-10 00:30").rate_minor, 0);
}

#[test]
fn a_window_over_midnight_applies_to_the_day_it_started() {
    let rules = [weekly(
        "late",
        FRI_SAT,
        22 * 60,
        2 * 60,
        (ADJUST_OVERRIDE, 7000),
    )];
    for (local, expect) in [
        ("2027-03-12 23:00", 7000),
        ("2027-03-13 01:59", 7000),
        ("2027-03-13 02:00", 10_000),
        ("2027-03-13 23:30", 7000),
        ("2027-03-14 01:00", 7000),
        ("2027-03-12 01:00", 10_000),
        ("2027-03-11 23:00", 10_000),
        ("2027-03-14 23:00", 10_000),
    ] {
        assert_eq!(price(&rules, local).rate_minor, expect, "{local}");
    }
}

#[test]
fn a_window_ending_at_midnight_runs_to_the_end_of_its_day() {
    let rules = [weekly(
        "eve",
        0b000_0001,
        18 * 60,
        0,
        (ADJUST_OVERRIDE, 6000),
    )];
    assert_eq!(price(&rules, "2027-03-08 23:59").rate_minor, 6000);
    assert_eq!(price(&rules, "2027-03-09 00:00").rate_minor, 10_000);
}

#[test]
fn a_date_rule_covers_its_inclusive_range() {
    let rules = [dated(
        "diwali",
        "2027-03-10",
        "2027-03-11",
        (ADJUST_PERCENT, 5000),
    )];
    for (local, expect) in [
        ("2027-03-09 23:59", 10_000),
        ("2027-03-10 00:00", 15_000),
        ("2027-03-11 23:59", 15_000),
        ("2027-03-12 00:00", 10_000),
    ] {
        assert_eq!(price(&rules, local).rate_minor, expect, "{local}");
    }
}

#[test]
fn exactly_one_rule_applies_and_the_highest_priority_wins() {
    let mut a = happy();
    a.priority = 5;
    let b = weekly("flat", WEEKDAYS, 0, 23 * 60, (ADJUST_OVERRIDE, 3000));
    let r = price(&[b.clone(), a.clone()], "2027-03-10 17:00");
    assert_eq!((r.rate_minor, r.rule_id.as_deref()), (8000, Some("happy")));
    a.priority = 0;
    let mut c = b;
    c.priority = 1;
    let r = price(&[a, c], "2027-03-10 17:00");
    assert_eq!((r.rate_minor, r.rule_id.as_deref()), (3000, Some("flat")));
}

#[test]
fn on_equal_priority_a_date_rule_beats_a_weekly_one_and_then_the_later_edit_wins() {
    let mut weekly_rule = happy();
    weekly_rule.updated_at = 900;
    let date_rule = dated(
        "special",
        "2027-03-10",
        "2027-03-10",
        (ADJUST_OVERRIDE, 2000),
    );
    let r = price(&[weekly_rule, date_rule], "2027-03-10 17:00");
    assert_eq!(r.rule_id.as_deref(), Some("special"));

    let mut early = weekly("early", WEEKDAYS, 0, 23 * 60, (ADJUST_OVERRIDE, 4000));
    early.updated_at = 100;
    let mut late = weekly("late", WEEKDAYS, 0, 23 * 60, (ADJUST_OVERRIDE, 4500));
    late.updated_at = 200;
    let r = price(&[late.clone(), early.clone()], "2027-03-10 12:00");
    assert_eq!((r.rate_minor, r.rule_id.as_deref()), (4500, Some("late")));
    let r = price(&[early, late], "2027-03-10 12:00");
    assert_eq!(r.rule_id.as_deref(), Some("late"));
}

#[test]
fn other_groups_and_inactive_rules_are_ignored() {
    let mut other = happy();
    other.group_id = "H".into();
    let mut off = happy();
    off.active = false;
    let r = price(&[other, off], "2027-03-10 17:00");
    assert_eq!(
        (r.rate_minor, r.rule_id, r.next_change_at),
        (10_000, None, None)
    );
}

#[test]
fn the_next_change_is_the_earliest_boundary_within_a_day() {
    let rules = [happy()];
    for (now, next) in [
        ("2027-03-10 15:00", Some("2027-03-10 16:00")),
        ("2027-03-10 16:00", Some("2027-03-10 18:00")),
        ("2027-03-10 17:59", Some("2027-03-10 18:00")),
        ("2027-03-10 18:00", Some("2027-03-11 16:00")),
        ("2027-03-12 17:00", Some("2027-03-12 18:00")),
        ("2027-03-12 18:00", None),
        ("2027-03-13 12:00", None),
        ("2027-03-14 17:00", Some("2027-03-15 16:00")),
    ] {
        assert_eq!(price(&rules, now).next_change_at, next.map(at), "{now}");
    }
}

#[test]
fn the_next_change_follows_a_wrapped_window_and_a_date_rule() {
    let late = [weekly(
        "late",
        FRI_SAT,
        22 * 60,
        2 * 60,
        (ADJUST_OVERRIDE, 7000),
    )];
    assert_eq!(
        price(&late, "2027-03-13 01:00").next_change_at,
        Some(at("2027-03-13 02:00"))
    );
    assert_eq!(
        price(&late, "2027-03-13 12:00").next_change_at,
        Some(at("2027-03-13 22:00"))
    );
    let day = [dated(
        "d",
        "2027-03-10",
        "2027-03-11",
        (ADJUST_PERCENT, 1000),
    )];
    assert_eq!(
        price(&day, "2027-03-09 20:00").next_change_at,
        Some(at("2027-03-10 00:00"))
    );
    assert_eq!(
        price(&day, "2027-03-11 20:00").next_change_at,
        Some(at("2027-03-12 00:00"))
    );
}

#[test]
fn rules_are_read_in_the_branch_zone() {
    let rules = [happy()];
    let t = at("2027-03-10 17:00");
    assert_eq!(resolve(&rules, "G", 10_000, IST, t).rate_minor, 8000);
    assert_eq!(
        resolve(&rules, "G", 10_000, Tz::Asia__Dubai, t).rate_minor,
        10_000
    );
}

#[test]
fn dates_must_be_canonical() {
    assert!(parse_date("2027-03-10").is_some());
    for bad in ["2027-3-10", "2027-02-30", "10/03/2027", "", "2027-03-10 "] {
        assert!(parse_date(bad).is_none(), "{bad}");
    }
}

fn who(c: &[Vec<RuleConflict>], i: usize) -> Vec<(&str, bool)> {
    c[i].iter()
        .map(|x| (x.rule_id.as_str(), x.other_wins))
        .collect()
}

#[test]
fn overlapping_weekly_rules_flag_each_other_and_name_the_winner() {
    let mut a = happy();
    a.priority = 2;
    let b = weekly("b", WEEKDAYS, 17 * 60, 20 * 60, (ADJUST_PERCENT, -1000));
    let c = weekly("c", WEEKDAYS, 18 * 60, 20 * 60, (ADJUST_PERCENT, -1000));
    let found = conflicts(&[a, b, c]);
    assert_eq!(who(&found, 0), vec![("b", false)]);
    assert_eq!(who(&found, 1), vec![("happy", true), ("c", true)]);
    assert_eq!(who(&found, 2), vec![("b", false)]);
}

#[test]
fn touching_windows_and_other_days_do_not_conflict() {
    let a = happy();
    let adjacent = weekly("adj", WEEKDAYS, 18 * 60, 20 * 60, (ADJUST_PERCENT, -1000));
    let weekend = weekly("we", 0b110_0000, 16 * 60, 18 * 60, (ADJUST_PERCENT, -1000));
    let mut elsewhere = happy();
    elsewhere.rule_id = "g2".into();
    elsewhere.group_id = "H".into();
    let mut off = happy();
    off.rule_id = "off".into();
    off.active = false;
    let found = conflicts(&[a, adjacent, weekend, elsewhere, off]);
    assert!(found.iter().all(Vec::is_empty));
}

#[test]
fn a_wrapped_window_conflicts_with_the_next_morning() {
    let late = weekly("late", FRI_SAT, 22 * 60, 2 * 60, (ADJUST_OVERRIDE, 7000));
    let sun = weekly("sun", 0b100_0000, 60, 3 * 60, (ADJUST_OVERRIDE, 6000));
    let mon = weekly("mon", 0b000_0001, 60, 3 * 60, (ADJUST_OVERRIDE, 6000));
    let found = conflicts(&[late, sun, mon]);
    assert_eq!(who(&found, 0), vec![("sun", true)]);
    assert!(found[2].is_empty());
}

#[test]
fn a_sunday_wrap_reaches_monday() {
    let late = weekly("late", 0b100_0000, 22 * 60, 2 * 60, (ADJUST_OVERRIDE, 7000));
    let mon = weekly("mon", 0b000_0001, 60, 3 * 60, (ADJUST_OVERRIDE, 6000));
    assert_eq!(who(&conflicts(&[late, mon]), 0), vec![("mon", true)]);
}

#[test]
fn date_rules_conflict_by_range_and_with_weekly_rules_by_weekday() {
    let a = dated("a", "2027-03-10", "2027-03-12", (ADJUST_PERCENT, 1000));
    let b = dated("b", "2027-03-12", "2027-03-20", (ADJUST_PERCENT, 2000));
    let c = dated("c", "2027-03-13", "2027-03-14", (ADJUST_PERCENT, 2000));
    let found = conflicts(&[a, b, c]);
    assert_eq!(who(&found, 0), vec![("b", true)]);
    assert_eq!(who(&found, 2), vec![("b", false)]);

    let wk = weekly("wk", 0b010_0000, 10 * 60, 12 * 60, (ADJUST_PERCENT, -1000));
    let wed = dated("wed", "2027-03-10", "2027-03-10", (ADJUST_PERCENT, 1000));
    let sat = dated("sat", "2027-03-13", "2027-03-13", (ADJUST_PERCENT, 1000));
    let found = conflicts(&[wk, wed, sat]);
    assert_eq!(who(&found, 0), vec![("sat", true)]);
    assert!(found[1].is_empty());
}

#[test]
fn until_is_when_the_winning_rule_gives_way() {
    let rules = [happy()];
    let r = price(&rules, "2027-03-10 16:30");
    assert_eq!(r.until, Some(at("2027-03-10 18:00")));
    assert_eq!(r.next_change_at, Some(at("2027-03-10 18:00")));
    let before = price(&rules, "2027-03-10 15:00");
    assert_eq!(before.until, None);
    assert_eq!(before.next_change_at, Some(at("2027-03-10 16:00")));
}

#[test]
fn until_skips_boundaries_that_do_not_change_the_winner() {
    let mut top = weekly("top", WEEKDAYS, 15 * 60, 19 * 60, (ADJUST_OVERRIDE, 3000));
    top.priority = 5;
    let rules = [happy(), top];
    let r = price(&rules, "2027-03-10 15:30");
    assert_eq!(r.rule_id.as_deref(), Some("top"));
    assert_eq!(r.next_change_at, Some(at("2027-03-10 16:00")));
    assert_eq!(r.until, Some(at("2027-03-10 19:00")));
}

#[test]
fn until_hands_over_to_the_next_rule() {
    let a = weekly("a", WEEKDAYS, 10 * 60, 12 * 60, (ADJUST_PERCENT, -1000));
    let b = weekly("b", WEEKDAYS, 12 * 60, 14 * 60, (ADJUST_PERCENT, -3000));
    let r = price(&[a, b], "2027-03-10 11:00");
    assert_eq!(
        (r.rate_minor, r.until),
        (9000, Some(at("2027-03-10 12:00")))
    );
}
