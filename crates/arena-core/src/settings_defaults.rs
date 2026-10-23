//! The settings every branch starts with, written by `branch.bootstrap`. Every value is TEXT.

use arena_proto::BranchBootstrap;

/// The branch locale keys. They are authoritative for the locale; the `branch` row's columns
/// only hold the bootstrap values.
pub const KEY_CURRENCY: &str = "branch.currency";
pub const KEY_SYMBOL_POSITION: &str = "branch.symbol_position";
pub const KEY_NUMBER_FORMAT: &str = "branch.number_format";
pub const KEY_TZ: &str = "branch.tz";
pub const KEY_DAY_START_MIN: &str = "branch.day_start_min";
pub const KEY_FY_START_MONTH: &str = "branch.fy_start_month";

/// Every locale key, in the order bootstrap inserts them (after [`DEFAULT_SETTINGS`]).
pub const LOCALE_KEYS: [&str; 6] = [
    KEY_CURRENCY,
    KEY_SYMBOL_POSITION,
    KEY_NUMBER_FORMAT,
    KEY_TZ,
    KEY_DAY_START_MIN,
    KEY_FY_START_MONTH,
];

/// The keys whose change waits for the next business day: they move the business day itself,
/// or relabel it (one business day never gets two FY labels).
pub const DEFERRED_KEYS: [&str; 3] = [KEY_TZ, KEY_DAY_START_MIN, KEY_FY_START_MONTH];

/// The locale settings `branch.bootstrap` seeds from its payload: the symbol goes before the
/// amount, and digits are grouped the Indian way for INR and internationally otherwise.
pub fn bootstrap_locale_settings(b: &BranchBootstrap) -> [(&'static str, String); 6] {
    let number_format = if b.currency == "INR" {
        "indian"
    } else {
        "international"
    };
    [
        (KEY_CURRENCY, b.currency.clone()),
        (KEY_SYMBOL_POSITION, "before".to_owned()),
        (KEY_NUMBER_FORMAT, number_format.to_owned()),
        (KEY_TZ, b.tz.clone()),
        (KEY_DAY_START_MIN, b.day_start_min.to_string()),
        (KEY_FY_START_MONTH, b.fy_start_month.to_string()),
    ]
}

/// `(key, value)` pairs inserted at bootstrap, in this order.
pub const DEFAULT_SETTINGS: &[(&str, &str)] = &[
    ("idle_lock_minutes", "5"),
    ("hub.locale", "en-IN"),
    ("receipt.footer", ""),
];

/// The shape a setting's TEXT value must have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingRule {
    /// A base-10 integer (ASCII digits only) within the inclusive range.
    Integer { min: i64, max: i64 },
    /// A locale tag: `[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})*`.
    Locale,
    /// Any text (bounded only by the global length limit).
    Text,
    /// An ISO 4217 code from `arena_proto::currency::CURRENCIES`.
    Currency,
    /// An exact IANA zone name from the bundled tz database (`Asia/Kolkata`, `UTC`).
    TimeZone,
    /// One of a fixed set of words.
    OneOf(&'static [&'static str]),
}

/// The value rule for every known setting. A key not listed here is not a setting.
pub const SETTING_RULES: &[(&str, SettingRule)] = &[
    (
        "idle_lock_minutes",
        SettingRule::Integer { min: 1, max: 30 },
    ),
    ("hub.locale", SettingRule::Locale),
    ("receipt.footer", SettingRule::Text),
    (KEY_CURRENCY, SettingRule::Currency),
    (
        KEY_SYMBOL_POSITION,
        SettingRule::OneOf(&["before", "after"]),
    ),
    (
        KEY_NUMBER_FORMAT,
        SettingRule::OneOf(&["indian", "international"]),
    ),
    (KEY_TZ, SettingRule::TimeZone),
    (
        KEY_DAY_START_MIN,
        SettingRule::Integer { min: 0, max: 1439 },
    ),
    (KEY_FY_START_MONTH, SettingRule::Integer { min: 1, max: 12 }),
];

/// The rule for `key`, if `key` is a known setting.
pub fn setting_rule(key: &str) -> Option<SettingRule> {
    SETTING_RULES
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, rule)| *rule)
}

fn is_locale(value: &str) -> bool {
    let mut parts = value.split('-');
    let lang = parts.next().unwrap_or("");
    (2..=3).contains(&lang.len())
        && lang.bytes().all(|b| b.is_ascii_alphabetic())
        && parts.all(|p| (2..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

impl SettingRule {
    /// Whether `value` has this rule's shape.
    pub fn accepts(self, value: &str) -> bool {
        match self {
            Self::Integer { min, max } => {
                !value.is_empty()
                    && value.len() <= 18
                    && value.bytes().all(|b| b.is_ascii_digit())
                    && value.parse::<i64>().is_ok_and(|n| (min..=max).contains(&n))
            }
            Self::Locale => is_locale(value),
            Self::Text => true,
            Self::Currency => arena_proto::currency::is_known_currency(value),
            Self::TimeZone => value.parse::<chrono_tz::Tz>().is_ok(),
            Self::OneOf(words) => words.contains(&value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_has_a_rule_and_satisfies_it() {
        assert_eq!(
            SETTING_RULES.len(),
            DEFAULT_SETTINGS.len() + LOCALE_KEYS.len()
        );
        for (key, value) in DEFAULT_SETTINGS {
            let rule = setting_rule(key).unwrap_or_else(|| panic!("{key} has no rule"));
            assert!(rule.accepts(value), "{key}={value:?}");
        }
        assert_eq!(setting_rule("nope"), None);
    }

    fn boot(currency: &str) -> BranchBootstrap {
        BranchBootstrap {
            name: "n".into(),
            branch_code: "c".into(),
            currency: currency.into(),
            tz: "Asia/Kolkata".into(),
            day_start_min: 360,
            fy_start_month: 4,
        }
    }

    #[test]
    fn bootstrap_seeds_every_locale_key_from_the_payload() {
        let inr = bootstrap_locale_settings(&boot("INR"));
        assert_eq!(
            inr.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
            LOCALE_KEYS.to_vec()
        );
        for (key, value) in &inr {
            assert!(setting_rule(key).unwrap().accepts(value), "{key}={value}");
        }
        let get = |list: &[(&str, String)], k: &str| {
            list.iter().find(|(key, _)| *key == k).unwrap().1.clone()
        };
        assert_eq!(get(&inr, KEY_SYMBOL_POSITION), "before");
        assert_eq!(get(&inr, KEY_NUMBER_FORMAT), "indian");
        assert_eq!(get(&inr, KEY_DAY_START_MIN), "360");
        assert_eq!(get(&inr, KEY_FY_START_MONTH), "4");
        let usd = bootstrap_locale_settings(&boot("USD"));
        assert_eq!(get(&usd, KEY_NUMBER_FORMAT), "international");
        assert_eq!(get(&usd, KEY_CURRENCY), "USD");
    }

    #[test]
    fn locale_rules() {
        let rule = |k| setting_rule(k).unwrap();
        assert!(rule(KEY_CURRENCY).accepts("KWD"));
        assert!(!rule(KEY_CURRENCY).accepts("XYZ"));
        assert!(!rule(KEY_CURRENCY).accepts("inr"));
        assert!(rule(KEY_TZ).accepts("Asia/Dubai"));
        assert!(!rule(KEY_TZ).accepts("Mars/Base"));
        assert!(rule(KEY_SYMBOL_POSITION).accepts("after"));
        assert!(!rule(KEY_SYMBOL_POSITION).accepts("Before"));
        assert!(rule(KEY_NUMBER_FORMAT).accepts("international"));
        assert!(!rule(KEY_NUMBER_FORMAT).accepts("western"));
        assert!(rule(KEY_DAY_START_MIN).accepts("0"));
        assert!(rule(KEY_DAY_START_MIN).accepts("1439"));
        assert!(!rule(KEY_DAY_START_MIN).accepts("1440"));
        assert!(rule(KEY_FY_START_MONTH).accepts("12"));
        assert!(!rule(KEY_FY_START_MONTH).accepts("0"));
        assert!(!rule(KEY_FY_START_MONTH).accepts("13"));
    }

    #[test]
    fn value_rules() {
        let minutes = setting_rule("idle_lock_minutes").unwrap();
        for ok in ["1", "5", "30", "07"] {
            assert!(minutes.accepts(ok), "{ok}");
        }
        for bad in [
            "",
            "0",
            "31",
            "-1",
            "+5",
            " 5",
            "5.0",
            "abc",
            "99999999999999999999",
        ] {
            assert!(!minutes.accepts(bad), "{bad}");
        }
        let locale = setting_rule("hub.locale").unwrap();
        for ok in ["en", "en-IN", "fil", "zh-Hant-TW", "de-CH-1996"] {
            assert!(locale.accepts(ok), "{ok}");
        }
        for bad in [
            "",
            "e",
            "engl",
            "en_IN",
            "en-",
            "en-I",
            "en-toolongxx",
            "é",
            "-en",
        ] {
            assert!(!locale.accepts(bad), "{bad}");
        }
        assert!(setting_rule("receipt.footer").unwrap().accepts(""));
    }

    #[test]
    fn keys_are_unique_and_required_ones_present() {
        let mut keys: Vec<_> = DEFAULT_SETTINGS.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), DEFAULT_SETTINGS.len());
        for (k, v) in [
            ("idle_lock_minutes", "5"),
            ("hub.locale", "en-IN"),
            ("receipt.footer", ""),
        ] {
            assert!(DEFAULT_SETTINGS.contains(&(k, v)), "{k}");
        }
    }
}
