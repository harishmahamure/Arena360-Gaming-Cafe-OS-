//! Rate resolution: which pricing rule, if any, prices a device group at an instant. Exactly one
//! rule applies at any moment (rules never stack). Windows are read on the branch wall clock.

use arena_proto::RuleConflict;
use arena_store::PricingRuleRow;
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, Timelike};
use chrono_tz::Tz;

use crate::windows::local_ms;

const DAY_MIN: i64 = 1440;
const WEEK_MIN: i64 = 7 * DAY_MIN;
const DAY_MS: i64 = 86_400_000;
const BP: i64 = 10_000;

pub const KIND_WEEKLY: &str = "weekly";
pub const KIND_DATE: &str = "date";
pub const ADJUST_OVERRIDE: &str = "override_minor";
pub const ADJUST_PERCENT: &str = "percent_bp";

/// The outcome of [`resolve`]: the rate per hour in minor units, the rule that set it (none:
/// the base rate), and the earliest instant in the next 24 hours at which any of the group's
/// rules begins or ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub rate_minor: i64,
    pub rule_id: Option<String>,
    pub next_change_at: Option<i64>,
}

/// A canonical local date, `YYYY-MM-DD`.
pub fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()
        .filter(|d| d.to_string() == s)
}

fn bit(mask: i64, day: NaiveDate) -> bool {
    mask >> day.weekday().num_days_from_monday() & 1 == 1
}

fn weekly_window(r: &PricingRuleRow) -> Option<(i64, i64, i64)> {
    Some((r.days_mask?, r.start_min?, r.end_min?))
}

fn date_range(r: &PricingRuleRow) -> Option<(NaiveDate, NaiveDate)> {
    Some((
        parse_date(r.date_from.as_deref()?)?,
        parse_date(r.date_to.as_deref()?)?,
    ))
}

fn applies(r: &PricingRuleRow, date: NaiveDate, minute: i64) -> bool {
    if r.rule_kind == KIND_DATE {
        return date_range(r).is_some_and(|(from, to)| (from..=to).contains(&date));
    }
    let Some((mask, start, end)) = weekly_window(r) else {
        return false;
    };
    if start < end {
        bit(mask, date) && (start..end).contains(&minute)
    } else {
        (bit(mask, date) && minute >= start)
            || (bit(mask, date - Duration::days(1)) && minute < end)
    }
}

/// Higher wins: priority, then a date rule over a weekly one, then the later edit.
fn rank(r: &PricingRuleRow) -> (i64, bool, i64, &str) {
    (
        r.priority,
        r.rule_kind == KIND_DATE,
        r.updated_at,
        r.rule_id.as_str(),
    )
}

fn adjusted(base_minor: i64, r: &PricingRuleRow) -> i64 {
    let rate = if r.adjust_kind == ADJUST_OVERRIDE {
        r.adjust_value
    } else {
        let scaled = i128::from(base_minor) * i128::from(BP + r.adjust_value);
        i64::try_from((scaled + i128::from(BP / 2)) / i128::from(BP)).unwrap_or(i64::MAX)
    };
    rate.max(0)
}

fn local_of(at_ms: i64, tz: Tz) -> Option<NaiveDateTime> {
    Some(
        DateTime::from_timestamp_millis(at_ms)?
            .with_timezone(&tz)
            .naive_local(),
    )
}

fn boundaries(r: &PricingRuleRow, day: NaiveDate, tz: Tz, at_ms: i64, out: &mut Vec<i64>) {
    let mut push = |min: i64| out.extend(local_ms(tz, day, min, at_ms));
    if r.rule_kind == KIND_DATE {
        if let Some((from, to)) = date_range(r)
            && (day == from || day == to + Duration::days(1))
        {
            push(0);
        }
        return;
    }
    let Some((mask, start, end)) = weekly_window(r) else {
        return;
    };
    if bit(mask, day) {
        push(start);
    }
    let ends_today = if start < end {
        bit(mask, day)
    } else {
        bit(mask, day - Duration::days(1))
    };
    if ends_today {
        push(end);
    }
}

/// The rate of `group_id` at `at_ms`, given every pricing rule and the group's base rate per
/// hour. Inactive rules and other groups' rules are ignored.
pub fn resolve(
    rules: &[PricingRuleRow],
    group_id: &str,
    base_minor: i64,
    tz: Tz,
    at_ms: i64,
) -> Resolved {
    let mine: Vec<&PricingRuleRow> = rules
        .iter()
        .filter(|r| r.active && r.group_id == group_id)
        .collect();
    let Some(local) = local_of(at_ms, tz) else {
        return Resolved {
            rate_minor: base_minor,
            rule_id: None,
            next_change_at: None,
        };
    };
    let minute = i64::from(local.time().num_seconds_from_midnight() / 60);
    let winner = mine
        .iter()
        .filter(|r| applies(r, local.date(), minute))
        .max_by(|a, b| rank(a).cmp(&rank(b)));
    let mut edges = Vec::new();
    for offset in -1..=2 {
        let day = local.date() + Duration::days(offset);
        for r in &mine {
            boundaries(r, day, tz, at_ms, &mut edges);
        }
    }
    Resolved {
        rate_minor: winner.map_or(base_minor, |r| adjusted(base_minor, r)),
        rule_id: winner.map(|r| r.rule_id.clone()),
        next_change_at: edges
            .into_iter()
            .filter(|t| *t > at_ms && *t <= at_ms + DAY_MS)
            .min(),
    }
}

fn week_spans(mask: i64, start: i64, end: i64) -> Vec<(i64, i64)> {
    let mut spans = Vec::new();
    for day in (0..7).filter(|d| mask >> d & 1 == 1) {
        let from = day * DAY_MIN + start;
        let to = if start < end {
            day * DAY_MIN + end
        } else {
            (day + 1) * DAY_MIN + end
        };
        if to > WEEK_MIN {
            spans.push((from, WEEK_MIN));
            spans.push((0, to - WEEK_MIN));
        } else {
            spans.push((from, to));
        }
    }
    spans
}

fn touched_weekdays(mask: i64, start: i64, end: i64) -> i64 {
    if end < start && end > 0 {
        mask | ((mask << 1 | mask >> 6) & 0b111_1111)
    } else {
        mask
    }
}

fn overlaps(a: &PricingRuleRow, b: &PricingRuleRow) -> bool {
    match (a.rule_kind.as_str(), b.rule_kind.as_str()) {
        (KIND_WEEKLY, KIND_WEEKLY) => {
            let (Some((ma, sa, ea)), Some((mb, sb, eb))) = (weekly_window(a), weekly_window(b))
            else {
                return false;
            };
            let spans_b = week_spans(mb, sb, eb);
            week_spans(ma, sa, ea)
                .iter()
                .any(|x| spans_b.iter().any(|y| x.0 < y.1 && y.0 < x.1))
        }
        (KIND_DATE, KIND_DATE) => match (date_range(a), date_range(b)) {
            (Some(x), Some(y)) => x.0 <= y.1 && y.0 <= x.1,
            _ => false,
        },
        (KIND_DATE, KIND_WEEKLY) => overlaps(b, a),
        _ => {
            let (Some((mask, start, end)), Some((from, to))) = (weekly_window(a), date_range(b))
            else {
                return false;
            };
            let touched = touched_weekdays(mask, start, end);
            (0..7)
                .map(|n| from + Duration::days(n))
                .take_while(|d| *d <= to)
                .any(|d| bit(touched, d))
        }
    }
}

/// For each rule (in the order given), the other active rules of its group that apply at the
/// same time, and whether each of them is the one that wins. Inactive rules have none.
pub fn conflicts(rules: &[PricingRuleRow]) -> Vec<Vec<RuleConflict>> {
    rules
        .iter()
        .map(|a| {
            if !a.active {
                return Vec::new();
            }
            rules
                .iter()
                .filter(|b| b.active && b.rule_id != a.rule_id && b.group_id == a.group_id)
                .filter(|b| overlaps(a, b))
                .map(|b| RuleConflict {
                    rule_id: b.rule_id.clone(),
                    name: b.name.clone(),
                    other_wins: rank(b) > rank(a),
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests;
