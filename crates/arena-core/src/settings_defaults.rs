//! The settings every branch starts with, written by `branch.bootstrap`. Every value is TEXT.

use arena_proto::{
    BACKFILL_PROCESS_MODE, BOOTSTRAP_PROCESS_MODE, BranchBootstrap, DEFAULT_ADS_CONSENT,
    DEFAULT_BACKUP_DIR, DEFAULT_FULFILMENT, DEFAULT_KIOSK_VOLUME, DEFAULT_OPENING_HOURS,
    DEFAULT_OPENING_HOURS_SET, DEFAULT_PROCESS_ALLOW, DEFAULT_PROCESS_BLOCK, FULFILMENT_MODES,
    HC_DEFAULT, KEY_ADS_CONSENT, KEY_BACKUP_DIR, KEY_FULFILMENT, KEY_HC_HUB,
    KEY_KIOSK_VOLUME_DEFAULT, KEY_OPENING_HOURS, KEY_OPENING_HOURS_SET, KEY_PROCESS_ALLOW,
    KEY_PROCESS_BLOCK, KEY_PROCESS_MODE, KIOSK_VOLUME_MAX, ProcessList, ProcessMode, parse_hc_key,
};
use arena_proto::{
    BILLING_GRACE_RANGE, BILLING_MINIMUM_RANGE, BILLING_ROUNDING_RANGE,
    DEFAULT_BILLING_CREDIT_AUTO_PAUSE, DEFAULT_BILLING_GRACE_MIN, DEFAULT_BILLING_MINIMUM_MIN,
    DEFAULT_BILLING_ROUNDING_MIN, DEFAULT_BILLING_TIME_UP_MODE, KEY_BILLING_CREDIT_AUTO_PAUSE,
    KEY_BILLING_GRACE_MIN, KEY_BILLING_MINIMUM_MIN, KEY_BILLING_ROUNDING_MIN,
    KEY_BILLING_TIME_UP_MODE, TIME_UP_MODES,
};
use arena_proto::{
    DEFAULT_KITCHEN_KOT_MODE, DEFAULT_KITCHEN_KOT_PAPER_MM, DEFAULT_KITCHEN_KOT_PRINTER,
    DEFAULT_LICENCE_FLAGS, KEY_KITCHEN_KOT_MODE, KEY_KITCHEN_KOT_PAPER_MM, KEY_KITCHEN_KOT_PRINTER,
    KEY_LICENCE_FLAGS, KITCHEN_KOT_MODES,
};
use arena_proto::{
    DEFAULT_PRINTER_PAPER_MM, DEFAULT_PRINTER_SELECTED, DEFAULT_PRINTER_STATUS,
    KEY_PRINTER_PAPER_MM, KEY_PRINTER_SELECTED, KEY_PRINTER_STATUS, PRINTER_PAPER_WIDTHS_MM,
};
use arena_proto::{DEFAULT_SETUP_LIVE, DEFAULT_TEST_SESSION, KEY_SETUP_LIVE, KEY_TEST_SESSION};
use arena_proto::{
    DEFAULT_TAX, KEY_BRANCH_CODE, KEY_BRANCH_CODE_LOCKED_AT, KEY_BRANCH_NAME, KEY_TAX, TaxSettings,
};

/// When staff last stopped the Android Hub service (epoch ms as text, empty until then). Staff
/// set it through `settings.set`, so the audit log records the stop with both actors.
pub const KEY_HUB_STOPPED_AT: &str = "hub.stopped_at";

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

/// `(key, value)` pairs inserted at bootstrap, in this order. A branch bootstrapped before a
/// key existed gets it from `branch.seed_defaults` when the Hub becomes primary.
pub const DEFAULT_SETTINGS: &[(&str, &str)] = &[
    ("idle_lock_minutes", "5"),
    ("hub.locale", "en-IN"),
    ("receipt.footer", ""),
    (KEY_HUB_STOPPED_AT, ""),
    // Story 1.18: the kiosk process lists (JSON arrays of `{path_glob, signer?}`).
    (KEY_PROCESS_ALLOW, DEFAULT_PROCESS_ALLOW),
    (KEY_PROCESS_BLOCK, DEFAULT_PROCESS_BLOCK),
    // A new branch enforces; an upgraded one is backfilled in report mode ([`backfill_value`]).
    (KEY_PROCESS_MODE, BOOTSTRAP_PROCESS_MODE),
    // Story 1.21: high contrast for the Hub UI. Each paired device's `display.hc.<id>` row is
    // created by `device.register` (and backfilled by `branch.seed_defaults`).
    (KEY_HC_HUB, HC_DEFAULT),
    (KEY_OPENING_HOURS, DEFAULT_OPENING_HOURS),
    (KEY_OPENING_HOURS_SET, DEFAULT_OPENING_HOURS_SET),
    (KEY_FULFILMENT, DEFAULT_FULFILMENT),
    (KEY_KIOSK_VOLUME_DEFAULT, DEFAULT_KIOSK_VOLUME),
    (KEY_BACKUP_DIR, DEFAULT_BACKUP_DIR),
    (KEY_ADS_CONSENT, DEFAULT_ADS_CONSENT),
    (KEY_PRINTER_SELECTED, DEFAULT_PRINTER_SELECTED),
    (KEY_PRINTER_PAPER_MM, DEFAULT_PRINTER_PAPER_MM),
    (KEY_PRINTER_STATUS, DEFAULT_PRINTER_STATUS),
    (KEY_LICENCE_FLAGS, DEFAULT_LICENCE_FLAGS),
    (KEY_KITCHEN_KOT_MODE, DEFAULT_KITCHEN_KOT_MODE),
    (KEY_KITCHEN_KOT_PRINTER, DEFAULT_KITCHEN_KOT_PRINTER),
    (KEY_KITCHEN_KOT_PAPER_MM, DEFAULT_KITCHEN_KOT_PAPER_MM),
    (KEY_TAX, DEFAULT_TAX),
    (KEY_TEST_SESSION, DEFAULT_TEST_SESSION),
    (KEY_SETUP_LIVE, DEFAULT_SETUP_LIVE),
    (KEY_BILLING_ROUNDING_MIN, DEFAULT_BILLING_ROUNDING_MIN),
    (KEY_BILLING_MINIMUM_MIN, DEFAULT_BILLING_MINIMUM_MIN),
    (KEY_BILLING_GRACE_MIN, DEFAULT_BILLING_GRACE_MIN),
    (KEY_BILLING_TIME_UP_MODE, DEFAULT_BILLING_TIME_UP_MODE),
    (
        KEY_BILLING_CREDIT_AUTO_PAUSE,
        DEFAULT_BILLING_CREDIT_AUTO_PAUSE,
    ),
];

pub const SYSTEM_MANAGED_KEYS: [&str; 6] = [
    KEY_BRANCH_CODE_LOCKED_AT,
    KEY_OPENING_HOURS_SET,
    KEY_ADS_CONSENT,
    KEY_PRINTER_STATUS,
    KEY_LICENCE_FLAGS,
    KEY_TEST_SESSION,
];

pub const KEYED_FIELD_KEYS: [&str; 18] = [
    KEY_KITCHEN_KOT_MODE,
    KEY_KITCHEN_KOT_PRINTER,
    KEY_KITCHEN_KOT_PAPER_MM,
    KEY_TAX,
    KEY_OPENING_HOURS,
    KEY_OPENING_HOURS_SET,
    KEY_FULFILMENT,
    KEY_KIOSK_VOLUME_DEFAULT,
    KEY_BACKUP_DIR,
    KEY_ADS_CONSENT,
    KEY_PRINTER_SELECTED,
    KEY_PRINTER_PAPER_MM,
    KEY_PRINTER_STATUS,
    KEY_BILLING_ROUNDING_MIN,
    KEY_BILLING_MINIMUM_MIN,
    KEY_BILLING_GRACE_MIN,
    KEY_BILLING_TIME_UP_MODE,
    KEY_BILLING_CREDIT_AUTO_PAUSE,
];

/// The value `branch.seed_defaults` inserts for a default setting an existing branch lacks:
/// the bootstrap default, except `kiosk.process_mode`, which is backfilled as `report` so an
/// upgraded café's games are never killed before an operator has checked the reports and
/// switched it to `enforce`.
pub fn backfill_value<'a>(key: &str, bootstrap_default: &'a str) -> &'a str {
    if key == KEY_PROCESS_MODE {
        BACKFILL_PROCESS_MODE
    } else {
        bootstrap_default
    }
}

/// The shape a setting's TEXT value must have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingRule {
    /// A base-10 integer (ASCII digits only) within the inclusive range.
    Integer {
        min: i64,
        max: i64,
    },
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
    /// A kiosk process list: a JSON array of at most 500 `{path_glob, signer?}` rules
    /// (`arena_proto::parse_process_rules`), up to `MAX_PROCESS_RULES_CHARS` long.
    ProcessRules(ProcessList),
    /// `"true"` or `"false"`.
    Bool,
    OpeningHours,
    Path,
    AdsConsent,
    PrinterSelected,
    PrinterStatus,
    TestSession,
    Tax,
    BranchCode,
    BranchName,
}

/// The value rule for every known setting. A key not listed here is not a setting.
pub const SETTING_RULES: &[(&str, SettingRule)] = &[
    (
        "idle_lock_minutes",
        SettingRule::Integer { min: 1, max: 30 },
    ),
    ("hub.locale", SettingRule::Locale),
    ("receipt.footer", SettingRule::Text),
    (KEY_HUB_STOPPED_AT, SettingRule::Text),
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
    (
        KEY_PROCESS_ALLOW,
        SettingRule::ProcessRules(ProcessList::Allow),
    ),
    (
        KEY_PROCESS_BLOCK,
        SettingRule::ProcessRules(ProcessList::Block),
    ),
    (KEY_PROCESS_MODE, SettingRule::OneOf(ProcessMode::VALUES)),
    (KEY_OPENING_HOURS, SettingRule::OpeningHours),
    (KEY_OPENING_HOURS_SET, SettingRule::Bool),
    (KEY_FULFILMENT, SettingRule::OneOf(FULFILMENT_MODES)),
    (
        KEY_KIOSK_VOLUME_DEFAULT,
        SettingRule::Integer {
            min: 0,
            max: KIOSK_VOLUME_MAX,
        },
    ),
    (KEY_BACKUP_DIR, SettingRule::Path),
    (KEY_ADS_CONSENT, SettingRule::AdsConsent),
    (KEY_PRINTER_SELECTED, SettingRule::PrinterSelected),
    (
        KEY_PRINTER_PAPER_MM,
        SettingRule::OneOf(PRINTER_PAPER_WIDTHS_MM),
    ),
    (KEY_PRINTER_STATUS, SettingRule::PrinterStatus),
    (KEY_LICENCE_FLAGS, SettingRule::Text),
    (KEY_KITCHEN_KOT_MODE, SettingRule::OneOf(KITCHEN_KOT_MODES)),
    (KEY_KITCHEN_KOT_PRINTER, SettingRule::PrinterSelected),
    (
        KEY_KITCHEN_KOT_PAPER_MM,
        SettingRule::OneOf(PRINTER_PAPER_WIDTHS_MM),
    ),
    (KEY_TAX, SettingRule::Tax),
    (KEY_TEST_SESSION, SettingRule::TestSession),
    (KEY_SETUP_LIVE, SettingRule::Bool),
    (
        KEY_BILLING_ROUNDING_MIN,
        SettingRule::Integer {
            min: BILLING_ROUNDING_RANGE.0,
            max: BILLING_ROUNDING_RANGE.1,
        },
    ),
    (
        KEY_BILLING_MINIMUM_MIN,
        SettingRule::Integer {
            min: BILLING_MINIMUM_RANGE.0,
            max: BILLING_MINIMUM_RANGE.1,
        },
    ),
    (
        KEY_BILLING_GRACE_MIN,
        SettingRule::Integer {
            min: BILLING_GRACE_RANGE.0,
            max: BILLING_GRACE_RANGE.1,
        },
    ),
    (KEY_BILLING_TIME_UP_MODE, SettingRule::OneOf(TIME_UP_MODES)),
    (KEY_BILLING_CREDIT_AUTO_PAUSE, SettingRule::Bool),
    (KEY_BRANCH_CODE, SettingRule::BranchCode),
    (KEY_BRANCH_NAME, SettingRule::BranchName),
    (KEY_BRANCH_CODE_LOCKED_AT, SettingRule::Text),
];

/// The rule for every key that starts with `display.hc.` and names `hub` or a device id
/// (`arena_proto::parse_hc_key`). `display.hc.hub` is also in [`DEFAULT_SETTINGS`]; the
/// per-device rows are created with their device.
pub const HC_RULE: SettingRule = SettingRule::Bool;

/// The rule for `key`, if `key` is a known setting: an exact [`SETTING_RULES`] entry, else
/// [`HC_RULE`] for a high-contrast key.
pub fn setting_rule(key: &str) -> Option<SettingRule> {
    SETTING_RULES
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, rule)| *rule)
        .or_else(|| parse_hc_key(key).map(|_| HC_RULE))
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
            Self::ProcessRules(list) => arena_proto::parse_process_rules(list, value).is_ok(),
            Self::Bool => matches!(value, "true" | "false"),
            Self::OpeningHours => arena_proto::parse_opening_hours(value).is_ok(),
            Self::Path => {
                value.is_empty()
                    || (arena_proto::is_absolute_path(value)
                        && value.trim() == value
                        && !value.chars().any(char::is_control))
            }
            Self::AdsConsent => arena_proto::AdsConsent::parse(value).is_some(),
            Self::PrinterSelected => arena_proto::PrinterSelected::parse(value).is_ok(),
            Self::PrinterStatus => arena_proto::PrinterStatus::parse(value).is_some(),
            Self::TestSession => arena_proto::TestSessionStatus::parse(value).is_some(),
            Self::Tax => TaxSettings::parse(value).is_some_and(|t| t.check().is_ok()),
            Self::BranchCode => arena_proto::valid_branch_code(value),
            Self::BranchName => {
                let n = value.chars().count();
                value == value.trim()
                    && (1..=80).contains(&n)
                    && !value.chars().any(char::is_control)
            }
        }
    }

    /// The longest value this rule takes, in characters.
    pub fn max_chars(self) -> usize {
        match self {
            Self::ProcessRules(_) => arena_proto::MAX_PROCESS_RULES_CHARS,
            _ => crate::apply::MAX_SETTING_VALUE_CHARS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_has_a_rule_and_satisfies_it() {
        // Every exact rule is a default or a locale key, except `display.hc.hub`, a default
        // covered by the `display.hc.` prefix rule.
        assert_eq!(
            SETTING_RULES.len() + 1,
            DEFAULT_SETTINGS.len() + LOCALE_KEYS.len() + 3
        );
        for (key, _) in SETTING_RULES {
            assert!(
                DEFAULT_SETTINGS.iter().any(|(k, _)| k == key)
                    || LOCALE_KEYS.contains(key)
                    || [KEY_BRANCH_CODE, KEY_BRANCH_CODE_LOCKED_AT, KEY_BRANCH_NAME].contains(key),
                "{key} has a rule but no default"
            );
        }
        for (key, value) in DEFAULT_SETTINGS {
            let rule = setting_rule(key).unwrap_or_else(|| panic!("{key} has no rule"));
            assert!(rule.accepts(value), "{key}={value:?}");
        }
        assert!(setting_rule(KEY_BRANCH_CODE).unwrap().accepts("LVL01"));
        for bad in ["", "lv", "lvl", "LV", "TOOLONG1", "AB-12"] {
            assert!(
                !setting_rule(KEY_BRANCH_CODE).unwrap().accepts(bad),
                "{bad}"
            );
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
            onboarding: None,
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
    fn process_list_rules() {
        let allow = setting_rule("kiosk.process_allow").unwrap();
        let block = setting_rule("kiosk.process_block").unwrap();
        assert!(allow.accepts(r#"[{"path_glob": "C:\\Games\\*.exe"}]"#));
        assert!(
            !allow.accepts(r#"[{"path_glob": "x.exe"}]"#),
            "a name alone"
        );
        assert!(block.accepts(r#"[{"path_glob": "x.exe"}]"#));
        assert!(!block.accepts("not json"));
        assert!(!block.accepts("{}"));
        let mode = setting_rule("kiosk.process_mode").unwrap();
        assert!(mode.accepts("enforce") && mode.accepts("report"));
        assert!(!mode.accepts("off") && !mode.accepts("Report"));
        assert_eq!(backfill_value("kiosk.process_mode", "enforce"), "report");
        assert_eq!(backfill_value("receipt.footer", ""), "");
        assert_eq!(allow.max_chars(), arena_proto::MAX_PROCESS_RULES_CHARS);
        assert_eq!(
            setting_rule("receipt.footer").unwrap().max_chars(),
            crate::apply::MAX_SETTING_VALUE_CHARS
        );
    }

    #[test]
    fn operating_rules() {
        let rule = |k| setting_rule(k).unwrap();
        assert!(rule("branch.fulfilment").accepts("seat"));
        assert!(rule("branch.fulfilment").accepts("counter"));
        for bad in ["", "Seat", "table", " seat"] {
            assert!(!rule("branch.fulfilment").accepts(bad), "{bad:?}");
        }
        let volume = rule("kiosk.volume_default");
        for ok in ["0", "50", "100"] {
            assert!(volume.accepts(ok), "{ok}");
        }
        for bad in ["101", "-1", "", "5.5", "loud"] {
            assert!(!volume.accepts(bad), "{bad:?}");
        }
        let hours = rule("branch.opening_hours");
        assert!(hours.accepts(DEFAULT_OPENING_HOURS));
        assert!(!hours.accepts("[]"));
        let dir = rule("system.backup_dir");
        assert!(dir.accepts("") && dir.accepts("/mnt/backup") && dir.accepts(r"D:\Backups"));
        assert!(!dir.accepts("/mnt/x\n") && !dir.accepts(" /mnt") && !dir.accepts("/mnt\u{7}"));
        assert!(!dir.accepts("backups") && !dir.accepts("./b"));
        let consent = rule("branch.ads_consent");
        assert!(consent.accepts(DEFAULT_ADS_CONSENT));
        assert!(!consent.accepts("yes"));
        assert!(rule("branch.opening_hours_set").accepts("true"));
    }

    #[test]
    fn billing_rules() {
        let rule = |k| setting_rule(k).unwrap();
        for (key, ok, bad) in [
            (
                KEY_BILLING_ROUNDING_MIN,
                &["1", "60"][..],
                &["", "0", "61", "abc", "1e1", "+5", "-1", "12.5"][..],
            ),
            (
                KEY_BILLING_MINIMUM_MIN,
                &["0", "240"][..],
                &["", "241", "abc", "1e1", "+5", "-1", "12.5"][..],
            ),
            (
                KEY_BILLING_GRACE_MIN,
                &["0", "10"][..],
                &["", "11", "abc", "1e1", "+5", "-1", "12.5"][..],
            ),
            (
                KEY_BILLING_TIME_UP_MODE,
                &["clean_up", "staff_handles"][..],
                &["", "Clean up", "Clean_up", " clean_up"][..],
            ),
        ] {
            for v in ok {
                assert!(rule(key).accepts(v), "{key}={v}");
            }
            for v in bad {
                assert!(!rule(key).accepts(v), "{key}={v:?}");
            }
        }
    }

    #[test]
    fn printer_rules() {
        let rule = |k| setting_rule(k).unwrap();
        let paper = rule("printer.paper_mm");
        assert!(paper.accepts("58") && paper.accepts("80"));
        for bad in ["", "57", "80mm", "112"] {
            assert!(!paper.accepts(bad), "{bad:?}");
        }
        let selected = rule("printer.selected");
        assert!(selected.accepts(""));
        assert!(selected.accepts(r#"{"kind":"lan","id_or_addr":"192.168.1.50","name":"P"}"#));
        assert!(selected.accepts(r#"{"kind":"usb","id_or_addr":"EPSON","name":"EPSON"}"#));
        assert!(!selected.accepts(r#"{"kind":"lan","id_or_addr":"abc","name":"P"}"#));
        assert!(!selected.accepts("printer"));
        let status = rule("printer.status");
        assert!(status.accepts(DEFAULT_PRINTER_STATUS));
        assert!(!status.accepts(r#"{"state":"ok"}"#));
        assert!(SYSTEM_MANAGED_KEYS.contains(&"printer.status"));
    }

    #[test]
    fn hc_keys_take_the_bool_rule() {
        let device = "display.hc.01JABCDEFGHJKMNPQRSTVWXYZ0";
        for key in ["display.hc.hub", device] {
            let rule = setting_rule(key).unwrap_or_else(|| panic!("{key}"));
            assert_eq!(rule, SettingRule::Bool);
            assert!(rule.accepts("true") && rule.accepts("false"));
            for bad in ["", "TRUE", "1", "yes", " true"] {
                assert!(!rule.accepts(bad), "{bad:?}");
            }
        }
        for not_a_setting in ["display.hc.", "display.hc.D1", "display.hc.hubx"] {
            assert_eq!(setting_rule(not_a_setting), None, "{not_a_setting}");
        }
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
            (KEY_HUB_STOPPED_AT, ""),
            ("kiosk.process_allow", DEFAULT_PROCESS_ALLOW),
            ("kiosk.process_block", DEFAULT_PROCESS_BLOCK),
            ("kiosk.process_mode", "enforce"),
            ("display.hc.hub", "false"),
            ("branch.opening_hours_set", "false"),
            ("branch.fulfilment", "counter"),
            ("kiosk.volume_default", "50"),
            ("system.backup_dir", ""),
            ("printer.selected", ""),
            ("printer.paper_mm", "80"),
            ("printer.status", DEFAULT_PRINTER_STATUS),
            ("branch.tax", DEFAULT_TAX),
            ("setup.test_session", DEFAULT_TEST_SESSION),
            ("billing.rounding_min", "15"),
            ("billing.minimum_min", "30"),
            ("billing.grace_min", "2"),
            ("billing.time_up_mode", "clean_up"),
        ] {
            assert!(DEFAULT_SETTINGS.contains(&(k, v)), "{k}");
        }
    }
}
