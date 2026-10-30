//! Which daily rollups are due (Story 8.6, AD-35). Pure arithmetic over the branch's opening hours,
//! business-day rule and what the Hub has published, so the uploader only has to build, PUT and
//! record what this says.
//!
//! A business day `D` is rolled up, for the first time, at the first of:
//! - the branch's closing time on `D` plus 30 minutes (opening hours, Story 2.26); with no closing
//!   time (a closed day, or open round the clock) the roll to the next business day plus 30
//!   minutes, and never later than that;
//! - the close of the last open shift of `D`: a shift closed on `D` and none open now.
//!
//! Every business day since the branch went paid that has no `rollup_log` row is due, oldest first
//! (the catch-up after the Hub was off or the WAN down). A day that already has a row is due again
//! when a ledger row stamped to it changed since (a late Undo), and its file is overwritten.

use std::collections::BTreeSet;

use arena_proto::{
    AppError, DEFAULT_OPENING_HOURS, KEY_OPENING_HOURS, LAST_MINUTE, OpeningDay,
    parse_opening_hours,
};
use arena_store::{ChangedRow, Store};
use chrono::{Datelike, Days, Duration, NaiveDate, NaiveTime, TimeZone};
use chrono_tz::Tz;

use crate::locale::{locale_rows, resolve};

/// How long after closing (or after the roll) the rollup is built.
pub const ROLLUP_DELAY_MS: i64 = 30 * 60_000;

/// How many days back a rescan re-opens.
pub const RESCAN_DAYS: u64 = 2;

/// What the planner decided.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RollupPlan {
    /// The business days to build and publish now, oldest first.
    pub due: Vec<String>,
    /// The next instant (Hub clock, ms) a day becomes due on its own, if any.
    pub next_at: Option<i64>,
    /// The days with ledger changes not yet published: the caller keeps the ones it could not
    /// publish and hands them back with the next plan.
    pub dirty: BTreeSet<String>,
}

/// Everything the planner reads.
pub struct PlanInput<'a> {
    pub now_ms: i64,
    pub tz: Tz,
    pub day_start_min: u16,
    pub week: [OpeningDay; 7],
    /// The business day of `now_ms`.
    pub today: NaiveDate,
    /// The business day the branch went paid.
    pub first_day: NaiveDate,
    pub published: &'a BTreeSet<String>,
    /// The business days shifts were closed on, from the start of the paid era.
    pub shift_closed_days: &'a BTreeSet<String>,
    pub shift_open: bool,
    /// Business days a ledger row changed on since the last plan.
    pub changed_days: &'a BTreeSet<String>,
}

/// The UTC instant (ms) of `minutes` past local midnight of `date` in `tz`.
fn local_ms(tz: Tz, date: NaiveDate, minutes: i64) -> i64 {
    let naive = date.and_time(NaiveTime::MIN) + Duration::minutes(minutes);
    match tz.from_local_datetime(&naive) {
        chrono::LocalResult::Single(t) | chrono::LocalResult::Ambiguous(t, _) => {
            t.timestamp_millis()
        }
        // A DST gap: the first instant after it.
        chrono::LocalResult::None => tz
            .from_local_datetime(&(naive + Duration::hours(1)))
            .earliest()
            .map_or_else(
                || naive.and_utc().timestamp_millis(),
                |t| t.timestamp_millis(),
            ),
    }
}

/// The instant business day `day` is first rolled up by the clock alone.
pub fn due_at(input: &PlanInput<'_>, day: NaiveDate) -> i64 {
    let next = day.checked_add_days(Days::new(1)).unwrap_or(day);
    let roll = local_ms(input.tz, next, i64::from(input.day_start_min));
    let close = match input.week[day.weekday().num_days_from_monday() as usize] {
        // Open round the clock: no closing time.
        OpeningDay::Open {
            open_min: 0,
            close_min: LAST_MINUTE,
        }
        | OpeningDay::Closed => None,
        OpeningDay::Open {
            open_min,
            close_min,
        } => {
            let date = if close_min < open_min { next } else { day };
            Some(local_ms(input.tz, date, i64::from(close_min)))
        }
    };
    close.map_or(roll, |c| c.min(roll)) + ROLLUP_DELAY_MS
}

/// The days due now and the next instant one falls due.
pub fn plan(input: &PlanInput<'_>) -> RollupPlan {
    let mut due = Vec::new();
    let mut next_at: Option<i64> = None;
    let mut day = input.first_day;
    while day <= input.today {
        let key = day.to_string();
        if input.published.contains(&key) {
            if input.changed_days.contains(&key) {
                due.push(key);
            }
        } else {
            let by_shift = !input.shift_open && input.shift_closed_days.contains(&key);
            let at = due_at(input, day);
            if by_shift || at <= input.now_ms {
                due.push(key);
            } else {
                next_at = Some(next_at.map_or(at, |n| n.min(at)));
            }
        }
        let Some(following) = day.checked_add_days(Days::new(1)) else {
            break;
        };
        day = following;
    }
    // Nothing is waiting on a day after today yet, but its clock time may fall before the next
    // wake of the task.
    if let Some(tomorrow) = input.today.checked_add_days(Days::new(1))
        && !input.published.contains(&tomorrow.to_string())
    {
        let at = due_at(input, tomorrow);
        if at > input.now_ms {
            next_at = Some(next_at.map_or(at, |n| n.min(at)));
        }
    }
    RollupPlan {
        due,
        next_at,
        dirty: input.changed_days.clone(),
    }
}

/// The business day of `now_ms` (`yyyy-mm-dd`) in the branch's zone and day-start rule; the UTC
/// date before the branch exists.
pub fn business_day(store: &Store, now_ms: i64) -> Result<String, AppError> {
    let Some(branch) = store.branch_locale()? else {
        return Ok(chrono::DateTime::from_timestamp_millis(now_ms)
            .map_or_else(String::new, |t| t.date_naive().to_string()));
    };
    let settings = store.settings()?;
    let rows = locale_rows(&branch, &settings);
    Ok(resolve(&rows, now_ms).0.to_string())
}

/// Plans the rollups of the branch as of `now_ms`. `changes` are the ledger changes heard since
/// the last plan and `carried` the days still waiting from earlier plans; `rescan` says changes were
/// missed, so every day published within the last `RESCAN_DAYS` is treated as changed. `None`
/// before the branch is bootstrapped or before the first upload (the branch is not paid yet as far
/// as the Hub can tell).
pub fn plan_rollups(
    store: &Store,
    now_ms: i64,
    changes: &[ChangedRow],
    carried: &BTreeSet<String>,
    rescan: bool,
) -> Result<Option<RollupPlan>, AppError> {
    let Some(branch) = store.branch_locale()? else {
        return Ok(None);
    };
    let facts = store.rollup_facts(changes)?;
    let Some(paid_since) = facts.paid_since else {
        return Ok(None);
    };
    let settings = store.settings()?;
    let rows = locale_rows(&branch, &settings);
    let (today, locale) = resolve(&rows, now_ms);
    let first_day = resolve(&rows, paid_since).0.min(today);
    let hours = settings
        .iter()
        .find(|r| r.key == KEY_OPENING_HOURS)
        .map_or(DEFAULT_OPENING_HOURS, |r| r.value.as_str());
    let week = parse_opening_hours(hours)
        .or_else(|_| parse_opening_hours(DEFAULT_OPENING_HOURS))
        .unwrap_or([OpeningDay::Closed; 7]);
    let shift_closed_days: BTreeSet<String> = facts
        .shifts_closed_at
        .iter()
        .map(|at| resolve(&rows, *at).0.to_string())
        .collect();
    let mut changed_days = facts.changed_days;
    changed_days.extend(carried.iter().cloned());
    if rescan {
        let from = today
            .checked_sub_days(Days::new(RESCAN_DAYS))
            .unwrap_or(today)
            .to_string();
        changed_days.extend(facts.published.iter().filter(|d| **d >= from).cloned());
    }
    Ok(Some(plan(&PlanInput {
        now_ms,
        tz: locale.tz,
        day_start_min: locale.day_start_min,
        week,
        today,
        first_day,
        published: &facts.published,
        shift_closed_days: &shift_closed_days,
        shift_open: facts.shift_open,
        changed_days: &changed_days,
    })))
}

#[cfg(test)]
mod tests;
