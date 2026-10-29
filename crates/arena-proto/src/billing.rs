pub const KEY_BILLING_ROUNDING_MIN: &str = "billing.rounding_min";
pub const KEY_BILLING_MINIMUM_MIN: &str = "billing.minimum_min";
pub const KEY_BILLING_GRACE_MIN: &str = "billing.grace_min";
pub const KEY_BILLING_TIME_UP_MODE: &str = "billing.time_up_mode";
pub const KEY_BILLING_CREDIT_AUTO_PAUSE: &str = "billing.credit_auto_pause";
/// The minutes-left marks players are warned at: one to three distinct whole minutes, each 1–30,
/// as a comma-separated list (`10,5,1`).
pub const KEY_BILLING_WARN_MARKS: &str = "billing.warn_marks";
/// The wallet amount (minor units) at or below which a wallet-paid player is warned; 0 is off.
pub const KEY_BILLING_LOW_BALANCE: &str = "billing.low_balance_minor";

pub const DEFAULT_BILLING_ROUNDING_MIN: &str = "15";
pub const DEFAULT_BILLING_MINIMUM_MIN: &str = "30";
pub const DEFAULT_BILLING_GRACE_MIN: &str = "2";
pub const DEFAULT_BILLING_TIME_UP_MODE: &str = "clean_up";
pub const DEFAULT_BILLING_CREDIT_AUTO_PAUSE: &str = "true";
pub const DEFAULT_BILLING_WARN_MARKS: &str = "10,5,1";
pub const DEFAULT_BILLING_LOW_BALANCE: &str = "5000";

pub const BILLING_ROUNDING_RANGE: (i64, i64) = (1, 60);
pub const BILLING_MINIMUM_RANGE: (i64, i64) = (0, 240);
pub const BILLING_GRACE_RANGE: (i64, i64) = (0, 10);
pub const BILLING_WARN_MARK_RANGE: (u8, u8) = (1, 30);
pub const BILLING_WARN_MARKS_MAX: usize = 3;
pub const BILLING_LOW_BALANCE_MAX_MINOR: i64 = 100_000_000;
pub const TIME_UP_MODES: &[&str] = &["clean_up", "staff_handles"];

pub const KIOSK_BILLING_KEYS: [&str; 4] = [
    KEY_BILLING_GRACE_MIN,
    KEY_BILLING_TIME_UP_MODE,
    KEY_BILLING_WARN_MARKS,
    KEY_BILLING_LOW_BALANCE,
];

/// Why a `billing.warn_marks` value is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarnMarksProblem {
    /// Not one to three whole numbers separated by commas.
    Shape,
    /// A mark outside 1–30 minutes.
    Range,
    /// The same mark twice.
    Duplicate,
}

/// Reads a warning-marks value into its marks, longest first (the order the timer reaches
/// them). Spaces around a mark are tolerated; a mark is plain digits, 1–30, and no mark repeats.
pub fn parse_warn_marks(value: &str) -> Result<Vec<u8>, WarnMarksProblem> {
    let parts: Vec<&str> = value.split(',').map(str::trim).collect();
    if parts.len() > BILLING_WARN_MARKS_MAX
        || parts
            .iter()
            .any(|p| p.is_empty() || p.len() > 3 || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(WarnMarksProblem::Shape);
    }
    let (lo, hi) = BILLING_WARN_MARK_RANGE;
    let mut marks = Vec::with_capacity(parts.len());
    for p in parts {
        let mark = p.parse::<u8>().map_err(|_| WarnMarksProblem::Range)?;
        if !(lo..=hi).contains(&mark) {
            return Err(WarnMarksProblem::Range);
        }
        if marks.contains(&mark) {
            return Err(WarnMarksProblem::Duplicate);
        }
        marks.push(mark);
    }
    marks.sort_unstable_by(|a, b| b.cmp(a));
    Ok(marks)
}

/// The marks a branch warns at: its setting, or the default 10, 5 and 1 when it is missing or
/// unreadable.
pub fn warn_marks_from(value: Option<&str>) -> Vec<u8> {
    value
        .and_then(|v| parse_warn_marks(v).ok())
        .unwrap_or_else(|| crate::SESSION_WARNING_MARKS_MIN.to_vec())
}

/// A low-balance setting's amount in minor units; 0 means off.
pub fn parse_low_balance(value: &str) -> Option<i64> {
    (value.len() <= 18 && value.bytes().all(|b| b.is_ascii_digit()))
        .then(|| value.parse::<i64>().ok())
        .flatten()
        .filter(|n| (0..=BILLING_LOW_BALANCE_MAX_MINOR).contains(n))
}

/// The low-balance threshold in minor units: `None` when the warning is off (0) or the stored
/// value is unreadable, the default when the setting is missing.
pub fn low_balance_from(value: Option<&str>) -> Option<i64> {
    parse_low_balance(value.unwrap_or(DEFAULT_BILLING_LOW_BALANCE)).filter(|m| *m > 0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeUpMode {
    CleanUp,
    StaffHandles,
}

impl TimeUpMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CleanUp => "clean_up",
            Self::StaffHandles => "staff_handles",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [Self::CleanUp, Self::StaffHandles]
            .into_iter()
            .find(|m| m.as_str() == value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BillingRules {
    pub rounding_min: u32,
    pub minimum_min: u32,
    pub grace_min: u32,
    pub time_up_mode: TimeUpMode,
}

fn minutes(value: Option<String>, (min, max): (i64, i64), default: u32) -> u32 {
    value
        .filter(|v| v.len() <= 18 && v.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|n| (min..=max).contains(n))
        .map_or(default, |n| n as u32)
}

impl BillingRules {
    pub fn defaults() -> Self {
        Self::from_lookup(|_| None)
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        Self {
            rounding_min: minutes(get(KEY_BILLING_ROUNDING_MIN), BILLING_ROUNDING_RANGE, 15),
            minimum_min: minutes(get(KEY_BILLING_MINIMUM_MIN), BILLING_MINIMUM_RANGE, 30),
            grace_min: minutes(get(KEY_BILLING_GRACE_MIN), BILLING_GRACE_RANGE, 2),
            time_up_mode: get(KEY_BILLING_TIME_UP_MODE)
                .and_then(|v| TimeUpMode::parse(&v))
                .unwrap_or(TimeUpMode::CleanUp),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(pairs: &[(&str, &str)]) -> BillingRules {
        BillingRules::from_lookup(|k| {
            pairs
                .iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| (*v).to_owned())
        })
    }

    #[test]
    fn defaults_match_the_default_strings() {
        let d = BillingRules::defaults();
        assert_eq!(
            (d.rounding_min, d.minimum_min, d.grace_min, d.time_up_mode),
            (15, 30, 2, TimeUpMode::CleanUp)
        );
        assert_eq!(
            rules(&[
                (KEY_BILLING_ROUNDING_MIN, DEFAULT_BILLING_ROUNDING_MIN),
                (KEY_BILLING_MINIMUM_MIN, DEFAULT_BILLING_MINIMUM_MIN),
                (KEY_BILLING_GRACE_MIN, DEFAULT_BILLING_GRACE_MIN),
                (KEY_BILLING_TIME_UP_MODE, DEFAULT_BILLING_TIME_UP_MODE),
            ]),
            d
        );
    }

    #[test]
    fn stored_values_are_read_and_bad_ones_fall_back_per_key() {
        let r = rules(&[
            (KEY_BILLING_ROUNDING_MIN, "60"),
            (KEY_BILLING_MINIMUM_MIN, "0"),
            (KEY_BILLING_GRACE_MIN, "10"),
            (KEY_BILLING_TIME_UP_MODE, "staff_handles"),
        ]);
        assert_eq!(
            (r.rounding_min, r.minimum_min, r.grace_min, r.time_up_mode),
            (60, 0, 10, TimeUpMode::StaffHandles)
        );
        for bad in ["", "0", "61", "+5", "-1", "1e1", "12.5", " 5", "abc"] {
            assert_eq!(
                rules(&[(KEY_BILLING_ROUNDING_MIN, bad)]).rounding_min,
                15,
                "{bad:?}"
            );
        }
        assert_eq!(rules(&[(KEY_BILLING_MINIMUM_MIN, "241")]).minimum_min, 30);
        assert_eq!(rules(&[(KEY_BILLING_GRACE_MIN, "11")]).grace_min, 2);
        let mixed = rules(&[
            (KEY_BILLING_ROUNDING_MIN, "10"),
            (KEY_BILLING_TIME_UP_MODE, "Clean up"),
        ]);
        assert_eq!(
            (mixed.rounding_min, mixed.time_up_mode),
            (10, TimeUpMode::CleanUp)
        );
    }

    #[test]
    fn warn_marks_are_one_to_three_distinct_minutes_longest_first() {
        assert_eq!(parse_warn_marks("10,5,1"), Ok(vec![10, 5, 1]));
        assert_eq!(parse_warn_marks("1, 30 ,7"), Ok(vec![30, 7, 1]));
        assert_eq!(parse_warn_marks("15"), Ok(vec![15]));
        for bad in [
            "", ",", "10,", "10,,5", "1,2,3,4", "a", "-5", "+5", "1.5", "1e1",
        ] {
            assert_eq!(
                parse_warn_marks(bad),
                Err(WarnMarksProblem::Shape),
                "{bad:?}"
            );
        }
        for bad in ["0", "31", "10,0", "300", "256"] {
            assert_eq!(
                parse_warn_marks(bad),
                Err(WarnMarksProblem::Range),
                "{bad:?}"
            );
        }
        assert_eq!(parse_warn_marks("5,10,5"), Err(WarnMarksProblem::Duplicate));
        assert_eq!(
            warn_marks_from(Some("nope")),
            crate::SESSION_WARNING_MARKS_MIN
        );
        assert_eq!(warn_marks_from(None), [10, 5, 1]);
        assert_eq!(warn_marks_from(Some("20,3")), [20, 3]);
        assert_eq!(
            parse_warn_marks(DEFAULT_BILLING_WARN_MARKS),
            Ok(crate::SESSION_WARNING_MARKS_MIN.to_vec())
        );
    }

    #[test]
    fn low_balance_is_an_amount_where_zero_is_off() {
        assert_eq!(low_balance_from(None), Some(5000));
        assert_eq!(low_balance_from(Some("0")), None);
        assert_eq!(low_balance_from(Some("12000")), Some(12000));
        assert_eq!(low_balance_from(Some("100000000")), Some(100_000_000));
        for bad in ["", "-1", "+5", "1.5", "x", "100000001"] {
            assert_eq!(parse_low_balance(bad), None, "{bad:?}");
            assert_eq!(low_balance_from(Some(bad)), None, "{bad:?}");
        }
    }

    #[test]
    fn modes_round_trip() {
        for m in TIME_UP_MODES {
            assert_eq!(TimeUpMode::parse(m).unwrap().as_str(), *m);
        }
        assert_eq!(TimeUpMode::parse("Clean up"), None);
    }
}
