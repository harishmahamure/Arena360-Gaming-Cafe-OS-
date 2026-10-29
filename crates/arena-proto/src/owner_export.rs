//! Owner report exports (Story 8.17): the CSV the cloud streams for Revenue, Cash reconciliation
//! and Stock. It is built from the same [`OwnerRange`] answers the Reports screen shows, so a
//! download can never disagree with the screen. Amounts are in major units with the currency code
//! beside them.

use std::collections::BTreeMap;

use crate::currency::currency;
use crate::owner_read::{OwnerRange, date_from_days};

/// Which report a CSV holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportReport {
    Revenue,
    Cash,
    Stock,
}

impl ExportReport {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "revenue" => Some(Self::Revenue),
            "cash" => Some(Self::Cash),
            "stock" => Some(Self::Stock),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Revenue => "revenue",
            Self::Cash => "cash",
            Self::Stock => "stock",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Revenue => "Revenue",
            Self::Cash => "Cash reconciliation",
            Self::Stock => "Stock",
        }
    }
}

/// `n` minor units of `code` as a decimal in major units (`12345` INR is `123.45`).
pub fn major(n: i64, code: &str) -> String {
    let exp = currency(code).map_or(2, |c| u32::from(c.exponent));
    let unit = 10_u64.pow(exp);
    let abs = n.unsigned_abs();
    let sign = if n < 0 { "-" } else { "" };
    if exp == 0 {
        return format!("{sign}{abs}");
    }
    format!(
        "{sign}{}.{:0width$}",
        abs / unit,
        abs % unit,
        width = exp as usize
    )
}

/// A CSV field: quoted when it holds a comma, quote or line break, and a leading `=`, `+`, `-`,
/// `@`, tab or return is neutralised so a spreadsheet never reads a branch or product name as a
/// formula.
fn field(s: &str) -> String {
    let guarded = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) && s.parse::<f64>().is_err() {
        format!("'{s}")
    } else {
        s.to_owned()
    };
    if guarded.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

fn row(out: &mut String, cells: &[String]) {
    out.push_str(&cells.iter().map(|c| field(c)).collect::<Vec<_>>().join(","));
    out.push_str("\r\n");
}

/// A timestamp (ms since the epoch) as `yyyy-mm-dd hh:mm UTC`.
pub fn utc_minute(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    format!(
        "{} {:02}:{:02} UTC",
        date_from_days(days),
        rem / 3600,
        rem % 3600 / 60
    )
}

/// The CSV of `report` over the range of `branches` (name and answer, in the order shown):
/// a header naming the report, range, branches and generated time, then one row per day per
/// branch (Stock: one row per level, waste line and expiring lot).
pub fn owner_csv(
    report: ExportReport,
    branches: &[(String, OwnerRange)],
    generated_ms: i64,
) -> String {
    let mut out = String::from("\u{feff}");
    let (from, to) = branches
        .first()
        .map_or((String::new(), String::new()), |(_, r)| {
            (r.from.clone(), r.to.clone())
        });
    row(&mut out, &["Report".into(), report.title().into()]);
    row(&mut out, &["Range".into(), format!("{from} to {to}")]);
    let names: Vec<&str> = branches.iter().map(|(n, _)| n.as_str()).collect();
    row(&mut out, &["Branches".into(), names.join("; ")]);
    row(&mut out, &["Generated".into(), utc_minute(generated_ms)]);
    out.push_str("\r\n");
    match report {
        ExportReport::Revenue => revenue(&mut out, branches),
        ExportReport::Cash => cash(&mut out, branches),
        ExportReport::Stock => stock(&mut out, branches),
    }
    out
}

fn revenue(out: &mut String, branches: &[(String, OwnerRange)]) {
    let head = [
        "Date",
        "Branch",
        "Currency",
        "Revenue",
        "Play",
        "Products",
        "Plans",
        "Other",
        "Cash",
        "UPI",
        "Card",
        "Wallet",
        "Credit",
        "Refunds",
        "Refund amount",
        "Discounts",
        "Discount amount",
        "Voids",
        "Void amount",
        "Sessions",
        "Session minutes",
        "Data",
    ];
    row(out, &head.map(String::from));
    for (name, r) in branches {
        let m = |n: i64| major(n, &r.currency);
        let mut days: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for p in &r.rows {
            if p.totals.days == 0 {
                continue;
            }
            let t = &p.totals;
            days.insert(
                &p.period,
                vec![
                    m(t.revenue.total),
                    m(t.revenue.by_type.play),
                    m(t.revenue.by_type.products),
                    m(t.revenue.by_type.plans),
                    m(t.revenue.by_type.other),
                    m(t.revenue.by_method.cash),
                    m(t.revenue.by_method.upi),
                    m(t.revenue.by_method.card),
                    m(t.revenue.by_method.wallet),
                    m(t.revenue.by_method.credit),
                    t.refunds.count.to_string(),
                    m(t.refunds.amount),
                    t.discounts.count.to_string(),
                    m(t.discounts.amount),
                    t.voids.count.to_string(),
                    m(t.voids.amount),
                    t.sessions.count.to_string(),
                    t.sessions.minutes.to_string(),
                    "Rollup".into(),
                ],
            );
        }
        for d in &r.missing_days {
            days.insert(d, Vec::new());
        }
        for (day, cells) in days {
            let mut cols = vec![day.to_owned(), name.clone(), r.currency.clone()];
            if cells.is_empty() {
                cols.extend(std::iter::repeat_n(String::new(), 18));
                cols.push("No figures".into());
            } else {
                cols.extend(cells);
            }
            row(out, &cols);
        }
    }
}

/// One business day's shifts added up: count, float, expected, counted, variance, over threshold.
#[derive(Default)]
struct DayCash {
    shifts: i64,
    float: i64,
    expected: i64,
    counted: i64,
    variance: i64,
    over: bool,
}

fn cash(out: &mut String, branches: &[(String, OwnerRange)]) {
    let head = [
        "Date",
        "Branch",
        "Currency",
        "Shifts",
        "Float",
        "Expected",
        "Counted",
        "Variance",
        "Over threshold",
        "Data",
    ];
    row(out, &head.map(String::from));
    for (name, r) in branches {
        let m = |n: i64| major(n, &r.currency);
        let mut days: BTreeMap<&str, DayCash> = BTreeMap::new();
        for s in &r.shifts {
            let d = days.entry(&s.business_day).or_default();
            d.shifts += 1;
            d.float += s.shift.float;
            d.expected += s.shift.expected;
            d.counted += s.shift.counted;
            d.variance += s.shift.variance;
            d.over |= s.shift.over_threshold == Some(true);
        }
        let mut dates: Vec<&str> = days.keys().copied().collect();
        dates.extend(r.missing_days.iter().map(String::as_str));
        dates.sort_unstable();
        for day in dates {
            let mut cols = vec![day.to_owned(), name.clone(), r.currency.clone()];
            if let Some(d) = days.get(day) {
                cols.extend([
                    d.shifts.to_string(),
                    m(d.float),
                    m(d.expected),
                    m(d.counted),
                    m(d.variance),
                    if d.over { "Yes" } else { "No" }.into(),
                    "Rollup".into(),
                ]);
            } else {
                cols.extend(std::iter::repeat_n(String::new(), 6));
                cols.push("No figures".into());
            }
            row(out, &cols);
        }
    }
}

fn stock(out: &mut String, branches: &[(String, OwnerRange)]) {
    let head = [
        "Section",
        "Branch",
        "Currency",
        "Product",
        "Category",
        "Reason or lot",
        "Quantity",
        "Low at",
        "Value",
        "Date",
    ];
    row(out, &head.map(String::from));
    for (name, r) in branches {
        let s = &r.stock;
        let on = s.levels_as_of.clone().unwrap_or_default();
        let base = |section: &str| vec![section.to_owned(), name.clone(), r.currency.clone()];
        for l in &s.levels {
            let mut c = base("Level");
            c.extend([
                l.name.clone(),
                l.category.clone().unwrap_or_default(),
                String::new(),
                l.qty.to_string(),
                l.low_qty.map(|q| q.to_string()).unwrap_or_default(),
                String::new(),
                on.clone(),
            ]);
            row(out, &c);
        }
        for w in &s.waste {
            let mut c = base("Waste");
            c.extend([
                w.name.clone(),
                w.category.clone().unwrap_or_default(),
                w.reason.clone(),
                w.qty.to_string(),
                String::new(),
                major(w.value, &r.currency),
                format!("{} to {}", r.from, r.to),
            ]);
            row(out, &c);
        }
        for e in &s.expiring {
            let mut c = base("Expiring");
            c.extend([
                e.name.clone(),
                String::new(),
                e.lot.clone(),
                e.qty.to_string(),
                String::new(),
                String::new(),
                e.expires_on.clone(),
            ]);
            row(out, &c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::owner_read::{
        GroupBy, OwnerRangeRow, OwnerRangeShift, OwnerRangeStock, OwnerRangeTotals,
    };
    use crate::rollup::RollupShift;

    fn range(currency: &str) -> OwnerRange {
        let mut totals = OwnerRangeTotals {
            days: 1,
            ..Default::default()
        };
        totals.revenue.total = 123_456;
        totals.revenue.by_method.cash = 100_000;
        totals.refunds.count = 1;
        totals.refunds.amount = -5_000;
        OwnerRange {
            branch_id: "b1".into(),
            currency: currency.into(),
            from: "2026-10-28".into(),
            to: "2026-10-30".into(),
            group_by: GroupBy::Day,
            totals: totals.clone(),
            rows: vec![OwnerRangeRow {
                period: "2026-10-29".into(),
                totals,
            }],
            missing_days: vec!["2026-10-28".into(), "2026-10-30".into()],
            shifts: vec![OwnerRangeShift {
                business_day: "2026-10-29".into(),
                shift: RollupShift {
                    shift_id: "s1".into(),
                    staff_name: "A".into(),
                    opened_at: 0,
                    closed_at: 1,
                    float: 10_000,
                    expected: 50_000,
                    counted: 49_700,
                    variance: -300,
                    reason: None,
                    approver_name: None,
                    over_threshold: Some(true),
                },
            }],
            stock: OwnerRangeStock::default(),
        }
    }

    #[test]
    fn major_units_follow_the_currency_exponent() {
        assert_eq!(major(12_345, "INR"), "123.45");
        assert_eq!(major(-5, "INR"), "-0.05");
        assert_eq!(major(1_234, "KWD"), "1.234");
        assert_eq!(major(1_234, "JPY"), "1234");
        assert_eq!(major(0, "INR"), "0.00");
    }

    #[test]
    fn fields_are_quoted_and_formula_prefixes_neutralised() {
        assert_eq!(field("a,b"), "\"a,b\"");
        assert_eq!(field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(field("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(field("@x"), "'@x");
        assert_eq!(field("plain"), "plain");
        assert_eq!(field("-3.00"), "-3.00");
    }

    #[test]
    fn revenue_has_a_header_naming_range_and_branches_and_a_row_per_day() {
        let csv = owner_csv(
            ExportReport::Revenue,
            &[("Cafe, One".into(), range("INR"))],
            1_793_356_800_000,
        );
        let lines: Vec<&str> = csv.trim_start_matches('\u{feff}').lines().collect();
        assert_eq!(lines[0], "Report,Revenue");
        assert_eq!(lines[1], "Range,2026-10-28 to 2026-10-30");
        assert_eq!(lines[2], "Branches,\"Cafe, One\"");
        assert!(lines[3].starts_with("Generated,2026-10-"));
        assert!(lines[5].starts_with("Date,Branch,Currency,Revenue"));
        assert!(lines[6].starts_with("2026-10-28,\"Cafe, One\",INR,,"));
        assert!(lines[6].ends_with("No figures"));
        assert!(lines[7].starts_with("2026-10-29,\"Cafe, One\",INR,1234.56,"));
        assert!(lines[7].contains(",1,-50.00,"));
        assert!(lines[7].ends_with("Rollup"));
        assert_eq!(lines.len(), 9);
    }

    #[test]
    fn cash_lists_a_row_per_day_with_variance() {
        let csv = owner_csv(ExportReport::Cash, &[("Main".into(), range("INR"))], 0);
        assert!(csv.contains("2026-10-29,Main,INR,1,100.00,500.00,497.00,-3.00,Yes,Rollup"));
        assert!(csv.contains("2026-10-28,Main,INR,,,,,,,No figures"));
    }
}
