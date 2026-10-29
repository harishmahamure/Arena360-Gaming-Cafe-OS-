use super::*;

const IST: Tz = chrono_tz::Asia::Kolkata;

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn set(days: &[&str]) -> BTreeSet<String> {
    days.iter().map(|s| (*s).to_owned()).collect()
}

/// 2026-10-30 is a Friday. Open 10:00 to 23:00 every day, the day starting at 06:00.
fn week(day: OpeningDay) -> [OpeningDay; 7] {
    [day; 7]
}

fn evening() -> [OpeningDay; 7] {
    week(OpeningDay::Open {
        open_min: 600,
        close_min: 23 * 60,
    })
}

fn at(date: &str, hh: i64, mm: i64) -> i64 {
    local_ms(IST, d(date), hh * 60 + mm)
}

struct Case {
    week: [OpeningDay; 7],
    day_start: u16,
    now: i64,
    today: &'static str,
    first: &'static str,
    published: BTreeSet<String>,
    shifts: BTreeSet<String>,
    shift_open: bool,
    changed: BTreeSet<String>,
}

impl Case {
    fn new(now: i64, today: &'static str, first: &'static str) -> Self {
        Self {
            week: evening(),
            day_start: 360,
            now,
            today,
            first,
            published: BTreeSet::new(),
            shifts: BTreeSet::new(),
            shift_open: false,
            changed: BTreeSet::new(),
        }
    }

    fn plan(&self) -> RollupPlan {
        plan(&PlanInput {
            now_ms: self.now,
            tz: IST,
            day_start_min: self.day_start,
            week: self.week,
            today: d(self.today),
            first_day: d(self.first),
            published: &self.published,
            shift_closed_days: &self.shifts,
            shift_open: self.shift_open,
            changed_days: &self.changed,
        })
    }
}

#[test]
fn a_day_is_due_thirty_minutes_after_closing_and_not_before() {
    let mut c = Case::new(at("2026-10-30", 23, 29), "2026-10-30", "2026-10-30");
    let p = c.plan();
    assert!(p.due.is_empty());
    assert_eq!(p.next_at, Some(at("2026-10-30", 23, 30)));
    c.now = at("2026-10-30", 23, 30);
    assert_eq!(c.plan().due, ["2026-10-30"]);
}

#[test]
fn the_last_shift_closing_makes_the_day_due_at_once() {
    let mut c = Case::new(at("2026-10-30", 18, 0), "2026-10-30", "2026-10-30");
    c.shifts = set(&["2026-10-30"]);
    assert_eq!(c.plan().due, ["2026-10-30"]);
    // Another shift still open: not the last one.
    c.shift_open = true;
    assert!(c.plan().due.is_empty());
    // A shift closed on another day does not count.
    c.shift_open = false;
    c.shifts = set(&["2026-10-29"]);
    assert!(c.plan().due.is_empty());
}

#[test]
fn a_round_the_clock_branch_rolls_up_thirty_minutes_after_the_day_roll() {
    let mut c = Case::new(at("2026-10-31", 6, 29), "2026-10-30", "2026-10-30");
    c.week = week(OpeningDay::Open {
        open_min: 0,
        close_min: LAST_MINUTE,
    });
    assert!(c.plan().due.is_empty());
    assert_eq!(c.plan().next_at, Some(at("2026-10-31", 6, 30)));
    // By 06:30 the roll at 06:00 has made 2026-10-31 the current business day.
    let mut after = Case::new(at("2026-10-31", 6, 30), "2026-10-31", "2026-10-30");
    after.week = c.week;
    assert_eq!(after.plan().due, ["2026-10-30"]);
}

#[test]
fn a_closed_day_follows_the_roll_rule_too() {
    let mut c = Case::new(at("2026-10-31", 6, 30), "2026-10-31", "2026-10-30");
    c.week = week(OpeningDay::Closed);
    assert_eq!(c.plan().due, ["2026-10-30"]);
}

#[test]
fn closing_after_midnight_belongs_to_the_day_that_opened() {
    // Open 18:00 to 02:00: the day closes at 02:00 the next calendar day, inside the business day.
    let mut c = Case::new(at("2026-10-31", 2, 29), "2026-10-30", "2026-10-30");
    c.week = week(OpeningDay::Open {
        open_min: 18 * 60,
        close_min: 2 * 60,
    });
    assert!(c.plan().due.is_empty());
    c.now = at("2026-10-31", 2, 30);
    assert_eq!(c.plan().due, ["2026-10-30"]);
}

#[test]
fn every_past_day_without_a_log_row_is_due_oldest_first() {
    let mut c = Case::new(at("2026-10-30", 9, 0), "2026-10-30", "2026-10-26");
    c.published = set(&["2026-10-27"]);
    let p = c.plan();
    assert_eq!(p.due, ["2026-10-26", "2026-10-28", "2026-10-29"]);
    // Today is not due yet: its closing time is ahead.
    assert_eq!(p.next_at, Some(at("2026-10-30", 23, 30)));
}

#[test]
fn days_before_the_branch_went_paid_are_not_asked_for() {
    let c = Case::new(at("2026-10-30", 9, 0), "2026-10-30", "2026-10-29");
    assert_eq!(c.plan().due, ["2026-10-29"]);
}

#[test]
fn a_published_day_is_due_again_only_when_a_row_stamped_to_it_changed() {
    let mut c = Case::new(at("2026-10-30", 23, 40), "2026-10-30", "2026-10-30");
    c.published = set(&["2026-10-30"]);
    assert!(c.plan().due.is_empty());
    c.changed = set(&["2026-10-30"]);
    assert_eq!(c.plan().due, ["2026-10-30"]);
    // A change stamped to a day that was never published is not a re-run: the first run covers it.
    c.published.clear();
    c.changed = set(&["2026-10-29"]);
    assert_eq!(c.plan().due, ["2026-10-30"]);
}

#[test]
fn the_next_days_clock_time_is_offered_as_the_next_wake() {
    let mut c = Case::new(at("2026-10-30", 23, 45), "2026-10-30", "2026-10-30");
    c.published = set(&["2026-10-30"]);
    let p = c.plan();
    assert!(p.due.is_empty());
    assert_eq!(p.next_at, Some(at("2026-10-31", 23, 30)));
}

#[test]
fn a_dst_gap_still_gives_an_instant() {
    let ny: Tz = chrono_tz::America::New_York;
    // 2026-03-08 02:30 does not exist in New York.
    let ms = local_ms(ny, d("2026-03-08"), 150);
    assert!(ms > local_ms(ny, d("2026-03-08"), 60));
}
