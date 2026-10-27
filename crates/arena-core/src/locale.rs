//! The branch locale: currency, number format, timezone, day start and financial year, read from
//! the `branch.*` settings, and the business day and financial year every change is stamped
//! with.
//!
//! A business day is the local date (in the branch tz) of `ts - day_start`: with a 06:00 day
//! start, 05:59 local still belongs to the previous date. A `branch.tz`, `branch.day_start_min`
//! or `branch.fy_start_month` change saved during business day D keeps the old value in
//! `prior_value` and applies from `effective_from` = D+1, so D keeps its rules (and its FY label)
//! to the end. Every other key applies at once. Once its day has come, a deferral has expired:
//! the stored value is simply the value, and the next write of that row clears the columns.

use arena_proto::currency::currency;
use arena_proto::{
    AppError, LocalePending, NumberFormat, PendingNumber, PendingText, SnapshotLocale,
    SymbolPosition,
};
use arena_store::{BranchLocale, SettingRow, StoreTx, find_branch_locale, list_settings};
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone};
use chrono_tz::Tz;

use crate::settings_defaults::{
    DEFERRED_KEYS, KEY_CURRENCY, KEY_DAY_START_MIN, KEY_FY_START_MONTH, KEY_NUMBER_FORMAT,
    KEY_SYMBOL_POSITION, KEY_TZ, LOCALE_KEYS,
};

/// One locale setting as stored: its value and any deferred change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleSetting {
    pub key: String,
    pub value: String,
    /// The value in effect before `effective_from`.
    pub prior_value: Option<String>,
    /// The business day from which `value` applies (a deferred change).
    pub effective_from: Option<NaiveDate>,
}

impl LocaleSetting {
    /// A setting that applies at once.
    pub fn now(key: &str, value: impl Into<String>) -> Self {
        Self {
            key: key.to_owned(),
            value: value.into(),
            prior_value: None,
            effective_from: None,
        }
    }

    /// The value in effect on `business_day`.
    pub fn value_on(&self, business_day: NaiveDate) -> &str {
        match (self.deferred(), &self.prior_value) {
            (Some((_, from)), Some(prior)) if business_day < from => prior,
            _ => &self.value,
        }
    }

    /// The deferred change, if any: `(new value, effective_from)`, expired or not.
    pub fn deferred(&self) -> Option<(&str, NaiveDate)> {
        match (&self.prior_value, self.effective_from) {
            (Some(_), Some(from)) if DEFERRED_KEYS.contains(&self.key.as_str()) => {
                Some((&self.value, from))
            }
            _ => None,
        }
    }

    /// The deferred change still waiting on `business_day` (not yet expired).
    pub fn waiting_on(&self, business_day: NaiveDate) -> Option<(&str, NaiveDate)> {
        self.deferred().filter(|(_, from)| *from > business_day)
    }
}

/// The locale in effect on one business day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locale {
    pub currency: String,
    pub symbol_position: SymbolPosition,
    pub number_format: NumberFormat,
    pub tz: Tz,
    pub day_start_min: u16,
    pub fy_start_month: u8,
}

fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

/// The six locale settings: the stored `branch.*` rows, with the branch row's bootstrap columns
/// standing in for a key that has no row (a branch bootstrapped before the keys existed).
pub fn locale_rows(branch: &BranchLocale, settings: &[SettingRow]) -> Vec<LocaleSetting> {
    LOCALE_KEYS
        .iter()
        .map(|key| {
            if let Some(s) = settings.iter().find(|s| s.key == *key) {
                return LocaleSetting {
                    key: s.key.clone(),
                    value: s.value.clone(),
                    prior_value: s.prior_value.clone(),
                    effective_from: s.effective_from.as_deref().and_then(parse_date),
                };
            }
            LocaleSetting::now(key, branch_default(branch, key))
        })
        .collect()
}

/// The value of locale `key` a branch starts with, from its bootstrap columns.
pub fn branch_default(branch: &BranchLocale, key: &str) -> String {
    match key {
        KEY_CURRENCY => branch.currency.clone(),
        KEY_SYMBOL_POSITION => "before".to_owned(),
        KEY_NUMBER_FORMAT if branch.currency == "INR" => "indian".to_owned(),
        KEY_NUMBER_FORMAT => "international".to_owned(),
        KEY_TZ => branch.tz.clone(),
        KEY_DAY_START_MIN => branch.day_start_min.to_string(),
        _ => branch.fy_start_month.to_string(),
    }
}

/// Parses a stored value, logging (and falling back) when it does not parse.
fn parsed<T>(key: &str, raw: Option<String>, parse: impl Fn(&str) -> Option<T>, fallback: T) -> T {
    match raw {
        Some(v) => parse(&v).unwrap_or_else(|| {
            tracing::warn!(key, value = %v, "locale: a stored setting does not parse; using the fallback");
            fallback
        }),
        None => fallback,
    }
}

fn build(get: impl Fn(&str) -> Option<String>) -> Locale {
    Locale {
        currency: parsed(
            KEY_CURRENCY,
            get(KEY_CURRENCY),
            |v| currency(v).map(|c| c.code.to_owned()),
            "INR".to_owned(),
        ),
        symbol_position: match get(KEY_SYMBOL_POSITION).as_deref() {
            Some("after") => SymbolPosition::After,
            _ => SymbolPosition::Before,
        },
        number_format: match get(KEY_NUMBER_FORMAT).as_deref() {
            Some("international") => NumberFormat::International,
            _ => NumberFormat::Indian,
        },
        tz: parsed(KEY_TZ, get(KEY_TZ), |v| v.parse::<Tz>().ok(), Tz::UTC),
        day_start_min: parsed(
            KEY_DAY_START_MIN,
            get(KEY_DAY_START_MIN),
            |v| v.parse().ok().filter(|m| *m <= 1439),
            0,
        ),
        fy_start_month: parsed(
            KEY_FY_START_MONTH,
            get(KEY_FY_START_MONTH),
            |v| v.parse().ok().filter(|m| (1..=12).contains(m)),
            4,
        ),
    }
}

/// The locale in effect on `business_day`: each deferred key takes its prior value before its
/// `effective_from`, and its new value from then on.
pub fn effective_locale(rows: &[LocaleSetting], business_day: NaiveDate) -> Locale {
    build(|key| {
        rows.iter()
            .find(|r| r.key == key)
            .map(|r| r.value_on(business_day).to_owned())
    })
}

/// The local date, in `tz`, of `ts_ms - day_start_min`.
pub fn business_day_of(ts_ms: i64, tz: Tz, day_start_min: u16) -> NaiveDate {
    let utc = DateTime::from_timestamp_millis(ts_ms).unwrap_or_else(|| {
        tracing::warn!(
            ts_ms,
            "locale: a timestamp is out of range; using the epoch"
        );
        DateTime::default()
    });
    (utc.with_timezone(&tz).naive_local() - Duration::minutes(i64::from(day_start_min))).date()
}

/// The financial year `date` falls in, labelled by its start year and the next year's last two
/// digits: with an April start, 2027-03-31 is `FY2026-27` and 2027-04-01 is `FY2027-28`. A
/// January start is a calendar year, labelled `FY2027`.
pub fn fy_of(date: NaiveDate, fy_start_month: u8) -> String {
    let start = u32::from(fy_start_month.clamp(1, 12));
    let year = if date.month() >= start {
        date.year()
    } else {
        date.year() - 1
    };
    if start == 1 {
        format!("FY{year}")
    } else {
        format!("FY{year}-{:02}", (year + 1).rem_euclid(100))
    }
}

/// The business day of `ts_ms` and the locale in effect on it.
///
/// Deferred changes are taken in `effective_from` order. The day is worked out under the rules
/// with none of them applied; each one whose day has been reached is applied (it has expired:
/// its new value is the value) and the day is worked out again under the new rules, never
/// earlier than that `effective_from` (a zone that is behind does not hand back hours of the
/// day that just ended). The first one not reached, and every later one, still waits.
pub fn resolve(rows: &[LocaleSetting], ts_ms: i64) -> (NaiveDate, Locale) {
    let mut deferred: Vec<(&str, NaiveDate)> = rows
        .iter()
        .filter_map(|r| r.deferred().map(|(_, from)| (r.key.as_str(), from)))
        .collect();
    deferred.sort_by_key(|(_, from)| *from);
    let mut applied: Vec<&str> = Vec::new();
    let locale_with = |applied: &[&str]| {
        build(|key| {
            rows.iter()
                .find(|r| r.key == key)
                .map(|r| match (r.deferred(), &r.prior_value) {
                    (Some(_), Some(prior)) if !applied.contains(&key) => prior.clone(),
                    _ => r.value.clone(),
                })
        })
    };
    let l = locale_with(&applied);
    let mut day = business_day_of(ts_ms, l.tz, l.day_start_min);
    for (key, from) in deferred {
        if day < from {
            break;
        }
        applied.push(key);
        let l = locale_with(&applied);
        day = business_day_of(ts_ms, l.tz, l.day_start_min).max(from);
    }
    (day, effective_locale(rows, day))
}

/// When the next deferred change takes effect: the instant (UTC ms) its `effective_from`
/// business day starts under the rules in effect now, or `None` when nothing waits.
pub fn next_rollover(rows: &[LocaleSetting], now_ms: i64) -> Option<i64> {
    let (day, l) = resolve(rows, now_ms);
    let from = rows
        .iter()
        .filter_map(|r| r.waiting_on(day))
        .map(|(_, f)| f)
        .min()?;
    let local = from.and_hms_opt(0, 0, 0)? + Duration::minutes(i64::from(l.day_start_min));
    let at = match l.tz.from_local_datetime(&local) {
        chrono::LocalResult::Single(t) | chrono::LocalResult::Ambiguous(t, _) => t,
        // A gap (DST): the first instant after it.
        chrono::LocalResult::None => {
            l.tz.from_local_datetime(&(local + Duration::hours(1)))
                .earliest()?
        }
    };
    Some(at.timestamp_millis().max(now_ms + 1))
}

/// The locale rows of the branch inside `tx`, or `None` before bootstrap.
pub fn read_rows(tx: &StoreTx<'_>) -> Result<Option<Vec<LocaleSetting>>, AppError> {
    let Some(branch) = find_branch_locale(tx)? else {
        return Ok(None);
    };
    Ok(Some(locale_rows(&branch, &list_settings(tx)?)))
}

/// **stamp** (business day): the `(business_day, fy)` of a change written at `now_ms`, under the
/// locale in effect then; `None` before the branch exists.
pub fn stamp(tx: &StoreTx<'_>, now_ms: i64) -> Result<Option<(String, String)>, AppError> {
    Ok(read_rows(tx)?.map(|rows| {
        let (day, locale) = resolve(&rows, now_ms);
        (day.to_string(), fy_of(day, locale.fy_start_month))
    }))
}

/// The branch time zone in effect at `now_ms` (`None` before the branch exists).
pub fn tz_at(tx: &StoreTx<'_>, now_ms: i64) -> Result<Option<Tz>, AppError> {
    Ok(read_rows(tx)?.map(|rows| resolve(&rows, now_ms).1.tz))
}

/// The business day in effect at `now_ms` (`None` before the branch exists).
pub fn current_business_day(
    store: &arena_store::Store,
    now_ms: i64,
) -> Result<Option<String>, AppError> {
    let Some(branch) = store.branch_locale()? else {
        return Ok(None);
    };
    let rows = locale_rows(&branch, &store.settings()?);
    Ok(Some(resolve(&rows, now_ms).0.to_string()))
}

/// The locale block of the Hub snapshot at `now_ms`: what is in effect now, and each tz,
/// day-start or financial-year change still waiting for its business day.
pub fn snapshot_locale(rows: &[LocaleSetting], now_ms: i64) -> SnapshotLocale {
    let (day, l) = resolve(rows, now_ms);
    let (exponent, symbol) = currency(&l.currency)
        .map(|c| (c.exponent, c.symbol.to_owned()))
        .unwrap_or((2, l.currency.clone()));
    let waiting = |key: &str| {
        rows.iter()
            .find(|r| r.key == key)
            .and_then(|r| r.waiting_on(day))
    };
    let number = |key: &str| {
        waiting(key).and_then(|(v, from)| {
            Some(PendingNumber {
                value: v.parse().ok()?,
                effective_from: from.to_string(),
            })
        })
    };
    let pending = LocalePending {
        tz: waiting(KEY_TZ).map(|(v, from)| PendingText {
            value: v.to_owned(),
            effective_from: from.to_string(),
        }),
        day_start_min: number(KEY_DAY_START_MIN),
        fy_start_month: number(KEY_FY_START_MONTH),
    };
    let any =
        pending.tz.is_some() || pending.day_start_min.is_some() || pending.fy_start_month.is_some();
    SnapshotLocale {
        currency: l.currency,
        exponent,
        symbol,
        symbol_position: l.symbol_position,
        number_format: l.number_format,
        tz: l.tz.name().to_owned(),
        day_start_min: l.day_start_min,
        fy_start_month: l.fy_start_month,
        pending: any.then_some(pending),
    }
}

/// `minor` units as the branch writes money at `now_ms`: the shared formatter of the UI
/// (`₹1,23,456.78`, no decimals for a zero exponent, the symbol after the number when the branch
/// puts it there).
pub fn format_money(tx: &StoreTx<'_>, now_ms: i64, minor: i64) -> Result<String, AppError> {
    let Some(rows) = read_rows(tx)? else {
        return Ok(minor.to_string());
    };
    let (_, l) = resolve(&rows, now_ms);
    let (exponent, symbol) = currency(&l.currency)
        .map(|c| (usize::from(c.exponent), c.symbol.to_owned()))
        .unwrap_or((2, l.currency.clone()));
    Ok(format_minor(
        minor,
        exponent,
        &symbol,
        l.symbol_position,
        l.number_format,
    ))
}

fn format_minor(
    minor: i64,
    exponent: usize,
    symbol: &str,
    position: SymbolPosition,
    format: NumberFormat,
) -> String {
    let digits = format!("{:0>width$}", minor.unsigned_abs(), width = exponent + 1);
    let (whole, fraction) = digits.split_at(digits.len() - exponent);
    let size = match format {
        NumberFormat::Indian => 2,
        NumberFormat::International => 3,
    };
    let grouped = if whole.len() <= 3 {
        whole.to_owned()
    } else {
        let (head, tail) = whole.split_at(whole.len() - 3);
        let mut groups: Vec<&str> = Vec::new();
        let mut end = head.len();
        while end > 0 {
            groups.push(&head[end.saturating_sub(size)..end]);
            end = end.saturating_sub(size);
        }
        groups.reverse();
        format!("{},{tail}", groups.join(","))
    };
    let number = if exponent > 0 {
        format!("{grouped}.{fraction}")
    } else {
        grouped
    };
    let amount = match position {
        SymbolPosition::After => format!("{number} {symbol}"),
        SymbolPosition::Before => format!("{symbol}{number}"),
    };
    if minor < 0 {
        format!("\u{2212}{amount}")
    } else {
        amount
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDateTime, TimeZone};

    fn d(s: &str) -> NaiveDate {
        parse_date(s).unwrap()
    }

    /// UTC ms of the wall-clock time `local` in `tz`.
    fn at(tz: Tz, local: &str) -> i64 {
        let naive = NaiveDateTime::parse_from_str(local, "%Y-%m-%d %H:%M").unwrap();
        tz.from_local_datetime(&naive).unwrap().timestamp_millis()
    }

    fn rows(tz: &str, day_start: u16) -> Vec<LocaleSetting> {
        vec![
            LocaleSetting::now(KEY_CURRENCY, "INR"),
            LocaleSetting::now(KEY_SYMBOL_POSITION, "before"),
            LocaleSetting::now(KEY_NUMBER_FORMAT, "indian"),
            LocaleSetting::now(KEY_TZ, tz),
            LocaleSetting::now(KEY_DAY_START_MIN, day_start.to_string()),
            LocaleSetting::now(KEY_FY_START_MONTH, "4"),
        ]
    }

    fn defer(rows: &mut [LocaleSetting], key: &str, value: &str, from: &str) {
        let r = rows.iter_mut().find(|r| r.key == key).unwrap();
        r.prior_value = Some(std::mem::replace(&mut r.value, value.to_owned()));
        r.effective_from = Some(d(from));
    }

    #[test]
    fn a_minute_before_day_start_is_the_previous_date() {
        let ist = Tz::Asia__Kolkata;
        assert_eq!(
            business_day_of(at(ist, "2027-01-02 05:59"), ist, 360),
            d("2027-01-01")
        );
        assert_eq!(
            business_day_of(at(ist, "2027-01-02 06:00"), ist, 360),
            d("2027-01-02")
        );
        assert_eq!(
            business_day_of(at(ist, "2027-01-02 00:00"), ist, 0),
            d("2027-01-02")
        );
        // The same instant is still the 1st in UTC.
        assert_eq!(
            business_day_of(at(ist, "2027-01-02 02:00"), Tz::UTC, 0),
            d("2027-01-01")
        );
    }

    #[test]
    fn financial_years() {
        assert_eq!(fy_of(d("2027-03-31"), 4), "FY2026-27");
        assert_eq!(fy_of(d("2027-04-01"), 4), "FY2027-28");
        assert_eq!(fy_of(d("2099-12-31"), 4), "FY2099-00");
        assert_eq!(fy_of(d("2027-06-30"), 7), "FY2026-27");
        assert_eq!(fy_of(d("2027-01-01"), 1), "FY2027");
        assert_eq!(fy_of(d("2027-11-30"), 12), "FY2026-27");
        assert_eq!(fy_of(d("2027-12-01"), 12), "FY2027-28");
    }

    #[test]
    fn a_tz_change_during_d_applies_from_d_plus_one() {
        let (ist, gst) = (Tz::Asia__Kolkata, Tz::Asia__Dubai);
        let mut r = rows("Asia/Kolkata", 360);
        // Saved at 14:00 on D = 2027-01-01, effective from D+1.
        defer(&mut r, KEY_TZ, "Asia/Dubai", "2027-01-02");
        // The rest of D: Kolkata rules (23:30 IST and 05:59 IST next morning are still D).
        for local in ["2027-01-01 14:00", "2027-01-01 23:30", "2027-01-02 05:59"] {
            let (day, l) = resolve(&r, at(ist, local));
            assert_eq!((day, l.tz), (d("2027-01-01"), ist), "{local}");
        }
        // 06:00 IST on D+1 (04:30 in Dubai, before Dubai's day start): D+1 under Dubai rules,
        // not back to D.
        let (day, l) = resolve(&r, at(ist, "2027-01-02 06:00"));
        assert_eq!((day, l.tz), (d("2027-01-02"), gst));
        // From then on Dubai rules decide: 05:59 Dubai on D+2 is still D+1.
        let (day, l) = resolve(&r, at(gst, "2027-01-03 05:59"));
        assert_eq!((day, l.tz), (d("2027-01-02"), gst));
        let (day, _) = resolve(&r, at(gst, "2027-01-03 06:00"));
        assert_eq!(day, d("2027-01-03"));
    }

    #[test]
    fn a_zone_ahead_waits_for_the_old_day_to_end() {
        let (ist, gst) = (Tz::Asia__Kolkata, Tz::Asia__Dubai);
        let mut r = rows("Asia/Dubai", 360);
        defer(&mut r, KEY_TZ, "Asia/Kolkata", "2027-01-02");
        // 06:30 IST is 05:00 in Dubai: still D under Dubai rules.
        let (day, l) = resolve(&r, at(ist, "2027-01-02 06:30"));
        assert_eq!((day, l.tz), (d("2027-01-01"), gst));
        let (day, l) = resolve(&r, at(gst, "2027-01-02 06:00"));
        assert_eq!((day, l.tz), (d("2027-01-02"), ist));
    }

    #[test]
    fn a_day_start_change_is_deferred_too() {
        let ist = Tz::Asia__Kolkata;
        let mut r = rows("Asia/Kolkata", 360);
        defer(&mut r, KEY_DAY_START_MIN, "0", "2027-01-02");
        // 03:00 on the 2nd: old rules (06:00 start) say the 1st.
        assert_eq!(resolve(&r, at(ist, "2027-01-02 03:00")).0, d("2027-01-01"));
        let (day, l) = resolve(&r, at(ist, "2027-01-02 06:00"));
        assert_eq!((day, l.day_start_min), (d("2027-01-02"), 0));
        assert_eq!(resolve(&r, at(ist, "2027-01-03 03:00")).0, d("2027-01-03"));
    }

    #[test]
    fn effective_locale_picks_prior_or_current() {
        let mut r = rows("Asia/Kolkata", 360);
        defer(&mut r, KEY_TZ, "Asia/Dubai", "2027-01-02");
        assert_eq!(effective_locale(&r, d("2027-01-01")).tz, Tz::Asia__Kolkata);
        assert_eq!(effective_locale(&r, d("2027-01-02")).tz, Tz::Asia__Dubai);
        let l = effective_locale(&r, d("2027-01-02"));
        assert_eq!(
            (l.currency.as_str(), l.symbol_position, l.number_format),
            ("INR", SymbolPosition::Before, NumberFormat::Indian)
        );
    }

    #[test]
    fn snapshot_locale_reports_a_pending_change_until_it_applies() {
        let ist = Tz::Asia__Kolkata;
        let mut r = rows("Asia/Kolkata", 360);
        defer(&mut r, KEY_TZ, "Asia/Dubai", "2027-01-02");
        let s = snapshot_locale(&r, at(ist, "2027-01-01 14:00"));
        assert_eq!(s.tz, "Asia/Kolkata");
        assert_eq!((s.exponent, s.symbol.as_str()), (2, "₹"));
        assert_eq!(
            s.pending,
            Some(LocalePending {
                tz: Some(PendingText {
                    value: "Asia/Dubai".into(),
                    effective_from: "2027-01-02".into(),
                }),
                day_start_min: None,
                fy_start_month: None,
            })
        );
        let s = snapshot_locale(&r, at(ist, "2027-01-02 07:00"));
        assert_eq!((s.tz.as_str(), s.pending), ("Asia/Dubai", None));
    }

    #[test]
    fn an_expired_deferral_does_not_steer_a_later_one() {
        let (ist, gst) = (Tz::Asia__Kolkata, Tz::Asia__Dubai);
        let mut r = rows("Asia/Kolkata", 360);
        // Kolkata → Dubai took effect on the 2nd; its prior is still stored.
        defer(&mut r, KEY_TZ, "Asia/Dubai", "2027-01-02");
        // On the 9th, under Dubai rules, a day start change waits for the 10th.
        defer(&mut r, KEY_DAY_START_MIN, "0", "2027-01-10");
        // 06:30 IST on the 10th is 05:00 in Dubai: still the 9th under Dubai's 06:00 start. The
        // stale Kolkata prior must not make the 10th (and the new day start) arrive early.
        let (day, l) = resolve(&r, at(ist, "2027-01-10 06:30"));
        assert_eq!((day, l.tz, l.day_start_min), (d("2027-01-09"), gst, 360));
        let (day, l) = resolve(&r, at(gst, "2027-01-10 06:00"));
        assert_eq!((day, l.day_start_min), (d("2027-01-10"), 0));
        // Its rollover is when the 10th starts under Dubai rules.
        assert_eq!(
            next_rollover(&r, at(gst, "2027-01-09 12:00")),
            Some(at(gst, "2027-01-10 06:00"))
        );
        assert_eq!(next_rollover(&r, at(gst, "2027-01-10 07:00")), None);
    }

    #[test]
    fn a_financial_year_change_is_deferred_and_reported_on_its_own_day() {
        let ist = Tz::Asia__Kolkata;
        let mut r = rows("Asia/Kolkata", 360);
        defer(&mut r, KEY_FY_START_MONTH, "1", "2027-04-02");
        let (day, l) = resolve(&r, at(ist, "2027-04-01 20:00"));
        assert_eq!((day, l.fy_start_month), (d("2027-04-01"), 4));
        defer(&mut r, KEY_TZ, "Asia/Dubai", "2027-04-03");
        let s = snapshot_locale(&r, at(ist, "2027-04-01 20:00"));
        let p = s.pending.unwrap();
        assert_eq!(
            p.fy_start_month,
            Some(PendingNumber {
                value: 1,
                effective_from: "2027-04-02".into()
            })
        );
        assert_eq!(p.tz.unwrap().effective_from, "2027-04-03");
        let (_, l) = resolve(&r, at(ist, "2027-04-02 07:00"));
        assert_eq!((l.fy_start_month, l.tz), (1, ist));
    }

    #[test]
    fn missing_rows_fall_back_to_the_branch_columns() {
        let branch = BranchLocale {
            currency: "USD".into(),
            tz: "America/New_York".into(),
            day_start_min: 120,
            fy_start_month: 1,
        };
        let r = locale_rows(&branch, &[]);
        let l = effective_locale(&r, d("2027-01-01"));
        assert_eq!(l.currency, "USD");
        assert_eq!(l.number_format, NumberFormat::International);
        assert_eq!(l.tz, Tz::America__New_York);
        assert_eq!((l.day_start_min, l.fy_start_month), (120, 1));
    }

    #[test]
    fn money_groups_like_the_ui_formatter() {
        let f = |m, e, p, n| format_minor(m, e, "₹", p, n);
        let (before, after) = (SymbolPosition::Before, SymbolPosition::After);
        let (indian, intl) = (NumberFormat::Indian, NumberFormat::International);
        assert_eq!(f(12_345_678, 2, before, indian), "₹1,23,456.78");
        assert_eq!(f(12_345_678, 2, after, intl), "123,456.78 ₹");
        assert_eq!(f(4_000, 2, before, indian), "₹40.00");
        assert_eq!(f(5, 2, before, indian), "₹0.05");
        assert_eq!(f(1234, 0, before, intl), "₹1,234");
        assert_eq!(f(-250, 2, before, indian), "\u{2212}₹2.50");
    }
}
