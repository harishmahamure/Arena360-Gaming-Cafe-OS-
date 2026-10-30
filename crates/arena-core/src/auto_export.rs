//! When the daily export is due (Story 9.5). Pure arithmetic over the branch's opening hours, its
//! business-day rule, its shifts and the export history `backup.last_export` keeps, so the task
//! only has to export and record what this says.
//!
//! A business day `D` is due at the first of the closing time plus 30 minutes (or the roll to the
//! next business day plus 30 minutes, see [`crate::rollup_plan::due_at`]) and the close of its last
//! shift when none is open now. It is done once the Hub made an auto export at or after that
//! instant. Only the latest day that is due is exported, so a Hub that was off over several closes
//! exports once on starting.

use std::collections::BTreeSet;

use arena_proto::{
    AppError, BackupExport, DEFAULT_OPENING_HOURS, KEY_BACKUP_LAST_EXPORT, KEY_OPENING_HOURS,
    OpeningDay, parse_opening_hours,
};
use arena_store::Store;
use chrono::{Days, NaiveDate};

use crate::locale::{locale_rows, resolve};
use crate::rollup_plan::{PlanInput, due_at};

/// How far back shift closes are looked at: today, yesterday and the roll between them.
const LOOKBACK_MS: i64 = 3 * 24 * 60 * 60 * 1000;

/// What the planner decided.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AutoExportPlan {
    /// An export is due now.
    pub due: bool,
    /// The next instant (Hub clock, ms) one falls due by the clock, if one does.
    pub next_at: Option<i64>,
}

/// Everything the planner reads.
pub struct ExportPlanInput<'a> {
    pub plan: PlanInput<'a>,
    /// When shifts closed, ms.
    pub shift_closes: &'a [i64],
    /// When the Hub made its auto exports, ms.
    pub auto_exports: &'a [i64],
    /// The business day of a moment.
    pub day_of: &'a dyn Fn(i64) -> NaiveDate,
}

/// The instant business day `day` falls due: the clock time, or the last shift close if earlier.
fn trigger(input: &ExportPlanInput<'_>, day: NaiveDate) -> i64 {
    let clock = due_at(&input.plan, day);
    let by_shift = if input.plan.shift_open {
        None
    } else {
        input
            .shift_closes
            .iter()
            .copied()
            .filter(|at| (input.day_of)(*at) == day)
            .max()
    };
    by_shift.map_or(clock, |at| at.min(clock))
}

/// Plans the daily export from what is known.
pub fn plan(input: &ExportPlanInput<'_>) -> AutoExportPlan {
    let now = input.plan.now_ms;
    let today = input.plan.today;
    let yesterday = today.checked_sub_days(Days::new(1)).unwrap_or(today);
    let tomorrow = today.checked_add_days(Days::new(1)).unwrap_or(today);
    let latest_due = [today, yesterday]
        .into_iter()
        .map(|day| trigger(input, day))
        .find(|at| *at <= now);
    let due = latest_due.is_some_and(|at| !input.auto_exports.iter().any(|made| *made >= at));
    let next_at = [today, tomorrow]
        .into_iter()
        .map(|day| trigger(input, day))
        .filter(|at| *at > now)
        .min();
    AutoExportPlan { due, next_at }
}

/// Plans the daily export of the branch as of `now_ms`; `None` before the branch is set up.
pub fn plan_auto_export(store: &Store, now_ms: i64) -> Result<Option<AutoExportPlan>, AppError> {
    let Some(branch) = store.branch_locale()? else {
        return Ok(None);
    };
    let settings = store.settings()?;
    let rows = locale_rows(&branch, &settings);
    let (today, locale) = resolve(&rows, now_ms);
    let hours = settings
        .iter()
        .find(|r| r.key == KEY_OPENING_HOURS)
        .map_or(DEFAULT_OPENING_HOURS, |r| r.value.as_str());
    let week = parse_opening_hours(hours)
        .or_else(|_| parse_opening_hours(DEFAULT_OPENING_HOURS))
        .unwrap_or([OpeningDay::Closed; 7]);
    let (shift_closes, shift_open) = store.shift_closes_since(now_ms - LOOKBACK_MS)?;
    let auto_exports: Vec<i64> = settings
        .iter()
        .find(|r| r.key == KEY_BACKUP_LAST_EXPORT)
        .and_then(|r| BackupExport::parse(&r.value))
        .map(|last| {
            last.entries()
                .into_iter()
                .filter(|e| e.by.is_none())
                .map(|e| e.exported_at)
                .collect()
        })
        .unwrap_or_default();
    let none = BTreeSet::new();
    let day_of = |ms: i64| resolve(&rows, ms).0;
    Ok(Some(plan(&ExportPlanInput {
        plan: PlanInput {
            now_ms,
            tz: locale.tz,
            day_start_min: locale.day_start_min,
            week,
            today,
            first_day: today,
            published: &none,
            shift_closed_days: &none,
            shift_open,
            changed_days: &none,
        },
        shift_closes: &shift_closes,
        auto_exports: &auto_exports,
        day_of: &day_of,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono_tz::Tz;

    const TZ: Tz = chrono_tz::UTC;

    fn ms(day: u32, hour: u32, min: u32) -> i64 {
        TZ.with_ymd_and_hms(2026, 11, day, hour, min, 0)
            .unwrap()
            .timestamp_millis()
    }

    fn run(now: i64, shift_closes: &[i64], shift_open: bool, made: &[i64]) -> AutoExportPlan {
        let none = BTreeSet::new();
        let day_of = |at: i64| TZ.timestamp_millis_opt(at).unwrap().date_naive();
        let week = [OpeningDay::Open {
            open_min: 10 * 60,
            close_min: 23 * 60,
        }; 7];
        plan(&ExportPlanInput {
            plan: PlanInput {
                now_ms: now,
                tz: TZ,
                day_start_min: 0,
                week,
                today: day_of(now),
                first_day: day_of(now),
                published: &none,
                shift_closed_days: &none,
                shift_open,
                changed_days: &none,
            },
            shift_closes,
            auto_exports: made,
            day_of: &day_of,
        })
    }

    #[test]
    fn it_is_due_thirty_minutes_after_closing_and_once() {
        // Closes 23:00 on the 10th, due 23:30.
        let before = run(ms(10, 23, 20), &[], false, &[ms(9, 23, 31)]);
        assert!(!before.due);
        assert_eq!(before.next_at, Some(ms(10, 23, 30)));
        let at = run(ms(10, 23, 30), &[], false, &[ms(9, 23, 31)]);
        assert!(at.due);
        let done = run(ms(10, 23, 45), &[], false, &[ms(10, 23, 31)]);
        assert!(!done.due);
        assert_eq!(done.next_at, Some(ms(11, 23, 30)));
    }

    #[test]
    fn the_last_shift_close_brings_it_forward_only_with_no_shift_open() {
        let closed = run(ms(10, 18, 0), &[ms(10, 17, 50)], false, &[ms(9, 23, 31)]);
        assert!(closed.due);
        let open = run(ms(10, 18, 0), &[ms(10, 17, 50)], true, &[ms(9, 23, 31)]);
        assert!(!open.due);
        let after = run(ms(10, 18, 0), &[ms(10, 17, 50)], false, &[ms(10, 17, 55)]);
        assert!(!after.due);
    }

    #[test]
    fn a_hub_that_was_off_over_the_close_exports_once_on_starting() {
        let late = run(ms(11, 9, 0), &[], false, &[ms(8, 23, 31)]);
        assert!(late.due);
        let manual_only = run(ms(11, 9, 0), &[], false, &[]);
        assert!(manual_only.due);
    }
}
