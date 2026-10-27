pub const KEY_BILLING_ROUNDING_MIN: &str = "billing.rounding_min";
pub const KEY_BILLING_MINIMUM_MIN: &str = "billing.minimum_min";
pub const KEY_BILLING_GRACE_MIN: &str = "billing.grace_min";
pub const KEY_BILLING_TIME_UP_MODE: &str = "billing.time_up_mode";
pub const KEY_BILLING_CREDIT_AUTO_PAUSE: &str = "billing.credit_auto_pause";

pub const DEFAULT_BILLING_ROUNDING_MIN: &str = "15";
pub const DEFAULT_BILLING_MINIMUM_MIN: &str = "30";
pub const DEFAULT_BILLING_GRACE_MIN: &str = "2";
pub const DEFAULT_BILLING_TIME_UP_MODE: &str = "clean_up";
pub const DEFAULT_BILLING_CREDIT_AUTO_PAUSE: &str = "true";

pub const BILLING_ROUNDING_RANGE: (i64, i64) = (1, 60);
pub const BILLING_MINIMUM_RANGE: (i64, i64) = (0, 240);
pub const BILLING_GRACE_RANGE: (i64, i64) = (0, 10);
pub const TIME_UP_MODES: &[&str] = &["clean_up", "staff_handles"];

pub const KIOSK_BILLING_KEYS: [&str; 2] = [KEY_BILLING_GRACE_MIN, KEY_BILLING_TIME_UP_MODE];

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
    fn modes_round_trip() {
        for m in TIME_UP_MODES {
            assert_eq!(TimeUpMode::parse(m).unwrap().as_str(), *m);
        }
        assert_eq!(TimeUpMode::parse("Clean up"), None);
    }
}
