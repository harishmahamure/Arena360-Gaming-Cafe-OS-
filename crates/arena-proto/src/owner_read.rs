//! The owner read API (Story 8.9, AD-35): what `GET /v1/branches/{id}/live` and
//! `GET /v1/branches/{id}/summary` answer. The cloud reads the branch's `state/latest.json` and
//! `rollups/<date>.json` files and sums them in memory; nothing here is a row.
//!
//! Money stays in integer minor units. Branches in different currencies are never summed: the
//! `all` answers carry one total per currency.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::rollup::{RollupAmount, RollupRevenue, RollupSessions, RollupV1};
use crate::summary::BranchSummary;

/// How long the cloud serves a branch's summary or a month of rollups from memory before it asks
/// the bucket whether the file changed (its ETag).
pub const OWNER_READ_CACHE_MS: i64 = 30_000;
/// A branch is online when its Hub's last heartbeat is this recent.
pub const BRANCH_ONLINE_WINDOW_MS: i64 = 3 * 60 * 1000;
/// Owner read calls a tenant may make in a minute before `RATE_LIMITED`.
pub const OWNER_API_LIMIT_PER_MIN: u64 = 120;
/// The longest range one `summary` call covers, in days.
pub const OWNER_RANGE_MAX_DAYS: i64 = 400;
/// The path segment that asks for every permitted branch.
pub const OWNER_ALL_BRANCHES: &str = "all";

/// `GET /v1/branches/{id}/live`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerLive {
    pub branch_id: String,
    pub currency: String,
    /// When the Hub built the summary (Hub clock, ms); null until its first summary arrives.
    #[ts(type = "number | null")]
    pub as_of: Option<i64>,
    /// The Hub's last heartbeat was within [`BRANCH_ONLINE_WINDOW_MS`].
    pub branch_online: bool,
    #[ts(type = "number | null")]
    pub last_heartbeat_at: Option<i64>,
    /// `state/latest.json`; null while there is none.
    pub summary: Option<BranchSummary>,
}

/// The live figures of the branches of one currency, added up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerLiveTotal {
    pub currency: String,
    /// Branches that have a summary.
    #[ts(type = "number")]
    pub branches: i64,
    #[ts(type = "number")]
    pub sessions_live: i64,
    #[ts(type = "number")]
    pub revenue_so_far_minor: i64,
    #[ts(type = "number")]
    pub cash_in_drawer_minor: i64,
}

/// `GET /v1/branches/all/live`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerLiveAll {
    pub branches: Vec<OwnerLive>,
    pub totals: Vec<OwnerLiveTotal>,
}

impl OwnerLiveAll {
    /// One entry per branch plus one total per currency (first seen first).
    pub fn new(branches: Vec<OwnerLive>) -> Self {
        let mut totals: Vec<OwnerLiveTotal> = Vec::new();
        for b in &branches {
            let i = totals
                .iter()
                .position(|t| t.currency == b.currency)
                .unwrap_or_else(|| {
                    totals.push(OwnerLiveTotal {
                        currency: b.currency.clone(),
                        branches: 0,
                        sessions_live: 0,
                        revenue_so_far_minor: 0,
                        cash_in_drawer_minor: 0,
                    });
                    totals.len() - 1
                });
            if let Some(s) = &b.summary {
                let t = &mut totals[i];
                t.branches += 1;
                t.sessions_live += s.sessions_live;
                t.revenue_so_far_minor += s.revenue_so_far_minor;
                t.cash_in_drawer_minor += s.cash_in_drawer_minor;
            }
        }
        Self { branches, totals }
    }
}

/// How the rows of a range are grouped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum GroupBy {
    Day,
    Week,
    Month,
}

impl GroupBy {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "day" => Some(Self::Day),
            "week" => Some(Self::Week),
            "month" => Some(Self::Month),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
        }
    }

    /// The label of the period `date` falls in: the date itself, the Monday of its week, or
    /// `yyyy-mm`.
    pub fn period_of(self, date: &str) -> Option<String> {
        let days = days_from_date(date)?;
        match self {
            Self::Day => Some(date.to_owned()),
            // 1970-01-01 was a Thursday: Monday is day 4 mod 7.
            Self::Week => Some(date_from_days(days - (days + 3).rem_euclid(7))),
            Self::Month => date.get(..7).map(str::to_owned),
        }
    }
}

/// The sums of the accepted rollups of a period (or of a whole range).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerRangeTotals {
    /// Days with an accepted rollup in it.
    #[ts(type = "number")]
    pub days: i64,
    pub revenue: RollupRevenue,
    pub refunds: RollupAmount,
    pub discounts: RollupAmount,
    pub voids: RollupAmount,
    pub sessions: RollupSessions,
}

impl OwnerRangeTotals {
    /// Adds one day's rollup.
    pub fn add_rollup(&mut self, r: &RollupV1) {
        self.add(&Self {
            days: 1,
            revenue: r.revenue.clone(),
            refunds: r.refunds.clone(),
            discounts: r.discounts.clone(),
            voids: r.voids.clone(),
            sessions: r.sessions.clone(),
        });
    }

    /// Adds another set of totals. The Hub wrote the files the cloud sums, so the sums saturate
    /// rather than overflow.
    pub fn add(&mut self, o: &Self) {
        fn sat(a: &mut i64, b: i64) {
            *a = a.saturating_add(b);
        }
        sat(&mut self.days, o.days);
        let (a, b) = (&mut self.revenue, &o.revenue);
        sat(&mut a.total, b.total);
        sat(&mut a.by_type.play, b.by_type.play);
        sat(&mut a.by_type.products, b.by_type.products);
        sat(&mut a.by_type.plans, b.by_type.plans);
        sat(&mut a.by_type.other, b.by_type.other);
        sat(&mut a.by_method.cash, b.by_method.cash);
        sat(&mut a.by_method.upi, b.by_method.upi);
        sat(&mut a.by_method.card, b.by_method.card);
        sat(&mut a.by_method.wallet, b.by_method.wallet);
        sat(&mut a.by_method.credit, b.by_method.credit);
        for (s, x) in [
            (&mut self.refunds, &o.refunds),
            (&mut self.discounts, &o.discounts),
            (&mut self.voids, &o.voids),
        ] {
            sat(&mut s.count, x.count);
            sat(&mut s.amount, x.amount);
        }
        sat(&mut self.sessions.count, o.sessions.count);
        sat(&mut self.sessions.minutes, o.sessions.minutes);
    }
}

/// One period of a range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerRangeRow {
    pub period: String,
    pub totals: OwnerRangeTotals,
}

/// `GET /v1/branches/{id}/summary`: one branch over `from`..`to`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerRange {
    pub branch_id: String,
    pub currency: String,
    pub from: String,
    pub to: String,
    pub group_by: GroupBy,
    pub totals: OwnerRangeTotals,
    pub rows: Vec<OwnerRangeRow>,
    /// Days in the range with no accepted rollup (absent, or `ROLLUP_SCHEMA_OLD`), oldest first.
    pub missing_days: Vec<String>,
}

/// The branches of one currency over a range, added up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerRangeCurrency {
    pub currency: String,
    #[ts(type = "number")]
    pub branches: i64,
    pub totals: OwnerRangeTotals,
}

/// `GET /v1/branches/all/summary`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerRangeAll {
    pub branches: Vec<OwnerRange>,
    pub totals: Vec<OwnerRangeCurrency>,
}

impl OwnerRangeAll {
    /// One entry per branch plus one total per currency (first seen first).
    pub fn new(branches: Vec<OwnerRange>) -> Self {
        let mut totals: Vec<OwnerRangeCurrency> = Vec::new();
        for b in &branches {
            let i = totals
                .iter()
                .position(|t| t.currency == b.currency)
                .unwrap_or_else(|| {
                    totals.push(OwnerRangeCurrency {
                        currency: b.currency.clone(),
                        branches: 0,
                        totals: OwnerRangeTotals::default(),
                    });
                    totals.len() - 1
                });
            totals[i].branches += 1;
            totals[i].totals.add(&b.totals);
        }
        Self { branches, totals }
    }
}

/// Days since 1970-01-01 of a `yyyy-mm-dd` date; `None` when it is not a calendar date.
pub fn days_from_date(date: &str) -> Option<i64> {
    if !crate::is_rollup_date(date) {
        return None;
    }
    let y: i64 = date.get(0..4)?.parse().ok()?;
    let m: i64 = date.get(5..7)?.parse().ok()?;
    let d: i64 = date.get(8..10)?.parse().ok()?;
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// The `yyyy-mm-dd` date `days` days after 1970-01-01.
pub fn date_from_days(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Every date from `from` to `to` inclusive; empty when either is not a date or `from` is later.
/// At most [`OWNER_RANGE_MAX_DAYS`] + 1 dates are built, so a caller can tell a range is too long
/// without allocating all of it.
pub fn dates_between(from: &str, to: &str) -> Vec<String> {
    match (days_from_date(from), days_from_date(to)) {
        (Some(a), Some(b)) if a <= b => (a..=b)
            .take(usize::try_from(OWNER_RANGE_MAX_DAYS + 1).unwrap_or(usize::MAX))
            .map(date_from_days)
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip_through_day_numbers() {
        assert_eq!(days_from_date("1970-01-01"), Some(0));
        assert_eq!(days_from_date("2000-03-01"), Some(11_017));
        assert_eq!(date_from_days(11_017), "2000-03-01");
        for d in ["2026-10-30", "2028-02-29", "2026-12-31", "2027-01-01"] {
            assert_eq!(date_from_days(days_from_date(d).unwrap()), d);
        }
        assert_eq!(days_from_date("2027-02-29"), None);
        assert_eq!(days_from_date("latest"), None);
    }

    #[test]
    fn a_range_lists_every_day_across_months_and_leap_days() {
        assert_eq!(
            dates_between("2028-02-28", "2028-03-01"),
            ["2028-02-28", "2028-02-29", "2028-03-01"]
        );
        assert_eq!(dates_between("2026-10-30", "2026-10-30").len(), 1);
        assert!(dates_between("2026-10-31", "2026-10-30").is_empty());
        assert!(dates_between("nope", "2026-10-30").is_empty());
        // A huge range is cut at the cap plus one, never fully built.
        assert_eq!(
            dates_between("0001-01-01", "9999-12-31").len(),
            usize::try_from(OWNER_RANGE_MAX_DAYS + 1).unwrap()
        );
    }

    #[test]
    fn totals_add_every_field_and_saturate() {
        let one = |n: i64| {
            let mut r = RollupV1 {
                schema: 1,
                branch_id: "b".into(),
                business_day: "2026-10-30".into(),
                currency: "INR".into(),
                revenue: RollupRevenue::default(),
                refunds: RollupAmount {
                    count: n,
                    amount: n,
                },
                discounts: RollupAmount {
                    count: n + 1,
                    amount: n + 2,
                },
                voids: RollupAmount {
                    count: n + 3,
                    amount: n + 4,
                },
                sessions: RollupSessions {
                    count: n + 5,
                    minutes: n + 6,
                },
                shifts: Vec::new(),
                stock: Default::default(),
                credit: Default::default(),
                play_hours: Vec::new(),
            };
            r.revenue.total = n;
            r.revenue.by_type.play = n + 1;
            r.revenue.by_type.products = n + 2;
            r.revenue.by_type.plans = n + 3;
            r.revenue.by_type.other = n + 4;
            r.revenue.by_method.cash = n + 5;
            r.revenue.by_method.upi = n + 6;
            r.revenue.by_method.card = n + 7;
            r.revenue.by_method.wallet = n + 8;
            r.revenue.by_method.credit = n + 9;
            r
        };
        let mut t = OwnerRangeTotals::default();
        t.add_rollup(&one(10));
        t.add_rollup(&one(100));
        assert_eq!(t.days, 2);
        assert_eq!(t.revenue.total, 110);
        assert_eq!(t.revenue.by_type.play, 112);
        assert_eq!(t.revenue.by_type.products, 114);
        assert_eq!(t.revenue.by_type.plans, 116);
        assert_eq!(t.revenue.by_type.other, 118);
        assert_eq!(t.revenue.by_method.cash, 120);
        assert_eq!(t.revenue.by_method.upi, 122);
        assert_eq!(t.revenue.by_method.card, 124);
        assert_eq!(t.revenue.by_method.wallet, 126);
        assert_eq!(t.revenue.by_method.credit, 128);
        assert_eq!((t.refunds.count, t.refunds.amount), (110, 110));
        assert_eq!((t.discounts.count, t.discounts.amount), (112, 114));
        assert_eq!((t.voids.count, t.voids.amount), (116, 118));
        assert_eq!((t.sessions.count, t.sessions.minutes), (120, 122));
        let mut sum = OwnerRangeTotals::default();
        sum.add(&t);
        sum.add(&t);
        assert_eq!(sum, {
            let mut d = t.clone();
            d.add(&t);
            d
        });
        assert_eq!(sum.revenue.total, 220);
        let mut big = one(i64::MAX - 20);
        big.revenue.total = i64::MAX;
        t.add_rollup(&big);
        t.add_rollup(&big);
        assert_eq!(t.revenue.total, i64::MAX);
    }

    #[test]
    fn periods_are_the_day_the_monday_or_the_month() {
        // 2026-10-30 is a Friday; its Monday is the 26th.
        assert_eq!(GroupBy::Day.period_of("2026-10-30").unwrap(), "2026-10-30");
        assert_eq!(GroupBy::Week.period_of("2026-10-30").unwrap(), "2026-10-26");
        assert_eq!(GroupBy::Week.period_of("2026-10-26").unwrap(), "2026-10-26");
        assert_eq!(GroupBy::Week.period_of("2026-11-01").unwrap(), "2026-10-26");
        assert_eq!(GroupBy::Week.period_of("2027-01-01").unwrap(), "2026-12-28");
        assert_eq!(GroupBy::Month.period_of("2026-10-30").unwrap(), "2026-10");
        assert_eq!(GroupBy::Month.period_of("bad"), None);
        assert_eq!(GroupBy::parse("week"), Some(GroupBy::Week));
        assert_eq!(GroupBy::parse("year"), None);
    }
}
