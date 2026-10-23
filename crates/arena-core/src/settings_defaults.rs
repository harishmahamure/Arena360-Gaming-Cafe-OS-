//! The settings every branch starts with, written by `branch.bootstrap`. Every value is TEXT.

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
}

/// The value rule for every known setting. A key not listed here is not a setting.
pub const SETTING_RULES: &[(&str, SettingRule)] = &[
    (
        "idle_lock_minutes",
        SettingRule::Integer { min: 1, max: 30 },
    ),
    ("hub.locale", SettingRule::Locale),
    ("receipt.footer", SettingRule::Text),
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_has_a_rule_and_satisfies_it() {
        assert_eq!(SETTING_RULES.len(), DEFAULT_SETTINGS.len());
        for (key, value) in DEFAULT_SETTINGS {
            let rule = setting_rule(key).unwrap_or_else(|| panic!("{key} has no rule"));
            assert!(rule.accepts(value), "{key}={value:?}");
        }
        assert_eq!(setting_rule("nope"), None);
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
