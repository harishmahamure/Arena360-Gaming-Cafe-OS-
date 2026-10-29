use arena_proto::{
    BILLING_GRACE_RANGE, Booster, BoosterPriority, DisplayMode, FULFILMENT_MODES,
    KEY_BILLING_GRACE_MIN, KEY_BILLING_LOW_BALANCE, KEY_BILLING_TIME_UP_MODE,
    KEY_BILLING_WARN_MARKS, KEY_BOOSTER_CLOSE, KEY_BOOSTER_POWER_PLAN, KEY_BOOSTER_PRIORITY,
    KEY_DISPLAY_MODES, KEY_FULFILMENT, KEY_KIOSK_VOLUME_DEFAULT, KIOSK_BILLING_KEYS,
    KIOSK_BOOSTER_KEYS, KIOSK_OPERATING_KEYS, KIOSK_VOLUME_MAX, SESSION_WARNING_MARKS_MIN,
    SettingEntry, TimeUpMode, low_balance_from, parse_booster_programs, parse_display_modes,
    parse_low_balance, parse_warn_marks,
};

use crate::procs::AppliedRevs;
use crate::queue::QueueDb;
use crate::runtime::Ctx;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fulfilment {
    Seat,
    Counter,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KioskOperating {
    pub volume_default: Option<u8>,
    pub fulfilment: Option<Fulfilment>,
    pub grace_min: Option<u8>,
    pub time_up_mode: Option<TimeUpMode>,
    /// The minutes-left marks players are warned at, longest first (Story 6.20).
    pub warn_marks: Option<Vec<u8>>,
    /// The wallet amount (minor units) the low-balance warning fires at; `Some(0)` is off.
    pub low_balance_minor: Option<i64>,
    /// What a game launch frees up (Story 6.25): the branch's, or nothing, normal priority
    /// above normal and no power plan switch before the Hub ever sent it.
    pub booster: Booster,
    /// The display modes a player may pick (Story 6.35); empty (also before the Hub ever sent
    /// it) means each PC's own mode only.
    pub display_modes: Vec<DisplayMode>,
}

impl KioskOperating {
    /// The marks to warn at: the branch's, or 10, 5 and 1 before the Hub ever sent them.
    pub fn marks(&self) -> Vec<u8> {
        self.warn_marks
            .clone()
            .unwrap_or_else(|| SESSION_WARNING_MARKS_MIN.to_vec())
    }

    /// The wallet amount the low-balance warning fires at; `None` when it is off.
    pub fn low_balance(&self) -> Option<i64> {
        match self.low_balance_minor {
            Some(minor) => Some(minor).filter(|m| *m > 0),
            None => low_balance_from(None),
        }
    }
}

pub const KEYS: [&str; 10] = [
    KIOSK_OPERATING_KEYS[0],
    KIOSK_OPERATING_KEYS[1],
    KIOSK_BILLING_KEYS[0],
    KIOSK_BILLING_KEYS[1],
    KIOSK_BILLING_KEYS[2],
    KIOSK_BILLING_KEYS[3],
    KIOSK_BOOSTER_KEYS[0],
    KIOSK_BOOSTER_KEYS[1],
    KIOSK_BOOSTER_KEYS[2],
    KEY_DISPLAY_MODES,
];

fn valid(key: &str, value: &str) -> bool {
    match key {
        KEY_KIOSK_VOLUME_DEFAULT => parse_volume(value).is_some(),
        KEY_FULFILMENT => FULFILMENT_MODES.contains(&value),
        KEY_BILLING_GRACE_MIN => parse_grace(value).is_some(),
        KEY_BILLING_TIME_UP_MODE => TimeUpMode::parse(value).is_some(),
        KEY_BILLING_WARN_MARKS => parse_warn_marks(value).is_ok(),
        KEY_BILLING_LOW_BALANCE => parse_low_balance(value).is_some(),
        KEY_BOOSTER_CLOSE => parse_booster_programs(value).is_ok(),
        KEY_BOOSTER_PRIORITY => BoosterPriority::parse(value).is_some(),
        KEY_BOOSTER_POWER_PLAN => matches!(value, "true" | "false"),
        KEY_DISPLAY_MODES => parse_display_modes(value).is_ok(),
        _ => false,
    }
}

fn parse_volume(value: &str) -> Option<u8> {
    value
        .parse::<u8>()
        .ok()
        .filter(|v| i64::from(*v) <= KIOSK_VOLUME_MAX && !value.starts_with('+'))
}

fn parse_grace(value: &str) -> Option<u8> {
    value
        .parse::<u8>()
        .ok()
        .filter(|v| i64::from(*v) <= BILLING_GRACE_RANGE.1 && !value.starts_with('+'))
}

fn parse_fulfilment(value: &str) -> Option<Fulfilment> {
    match value {
        "seat" => Some(Fulfilment::Seat),
        "counter" => Some(Fulfilment::Counter),
        _ => None,
    }
}

pub fn load(queue: &QueueDb) -> KioskOperating {
    let read = |key: &str| match queue.kv_get(key) {
        Ok(Some((value, _))) => Some(value),
        Ok(None) => None,
        Err(e) => {
            tracing::error!(key, error = %e, "the operating setting cache cannot be read");
            None
        }
    };
    KioskOperating {
        volume_default: read(KEY_KIOSK_VOLUME_DEFAULT).and_then(|v| parse_volume(&v)),
        fulfilment: read(KEY_FULFILMENT).and_then(|v| parse_fulfilment(&v)),
        grace_min: read(KEY_BILLING_GRACE_MIN).and_then(|v| parse_grace(&v)),
        time_up_mode: read(KEY_BILLING_TIME_UP_MODE).and_then(|v| TimeUpMode::parse(&v)),
        warn_marks: read(KEY_BILLING_WARN_MARKS).and_then(|v| parse_warn_marks(&v).ok()),
        low_balance_minor: read(KEY_BILLING_LOW_BALANCE).and_then(|v| parse_low_balance(&v)),
        booster: Booster {
            close: read(KEY_BOOSTER_CLOSE)
                .and_then(|v| parse_booster_programs(&v).ok())
                .unwrap_or_default(),
            priority: read(KEY_BOOSTER_PRIORITY)
                .and_then(|v| BoosterPriority::parse(&v))
                .unwrap_or_default(),
            high_performance: read(KEY_BOOSTER_POWER_PLAN).is_some_and(|v| v == "true"),
        },
        display_modes: read(KEY_DISPLAY_MODES)
            .and_then(|v| parse_display_modes(&v).ok())
            .unwrap_or_default(),
    }
}

pub fn seed(queue: &QueueDb, revs: &mut AppliedRevs) {
    for key in KEYS {
        revs.seed(queue, key);
    }
}

pub fn apply(ctx: &Ctx, values: &[SettingEntry], revs: &mut AppliedRevs) {
    for entry in values.iter().filter(|e| KEYS.contains(&e.key.as_str())) {
        if revs.get(&entry.key).is_some_and(|r| entry.rev < r) {
            tracing::warn!(key = %entry.key, rev = entry.rev, applied = revs.get(&entry.key), "ignoring an older operating setting");
            continue;
        }
        if !valid(&entry.key, &entry.value) {
            tracing::error!(key = %entry.key, rev = entry.rev, value = %entry.value, "the hub sent an invalid operating setting; keeping the previous one");
            continue;
        }
        revs.record(&entry.key, entry.rev);
        if let Err(e) = ctx.queue.lock().unwrap_or_else(|p| p.into_inner()).kv_set(
            &entry.key,
            &entry.value,
            entry.rev,
        ) {
            tracing::error!(key = %entry.key, error = %e, "could not cache the operating setting");
            continue;
        }
        tracing::info!(key = %entry.key, rev = entry.rev, value = %entry.value, "operating setting stored");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_parse_only_in_range() {
        assert_eq!(parse_volume("0"), Some(0));
        assert_eq!(parse_volume("100"), Some(100));
        for bad in ["101", "255", "-1", "+5", "", "x"] {
            assert_eq!(parse_volume(bad), None, "{bad}");
        }
        assert_eq!(parse_fulfilment("seat"), Some(Fulfilment::Seat));
        assert_eq!(parse_fulfilment("counter"), Some(Fulfilment::Counter));
        assert_eq!(parse_fulfilment("Seat"), None);
        assert!(valid(KEY_FULFILMENT, "seat"));
        assert!(!valid("kiosk.process_mode", "seat"));
        assert_eq!(parse_grace("0"), Some(0));
        assert_eq!(parse_grace("10"), Some(10));
        for bad in ["11", "-1", "+5", "", "x", "1e1"] {
            assert_eq!(parse_grace(bad), None, "{bad}");
        }
        assert!(valid(KEY_BILLING_TIME_UP_MODE, "staff_handles"));
        assert!(!valid(KEY_BILLING_TIME_UP_MODE, "Clean up"));
        assert!(!valid(KEY_BILLING_GRACE_MIN, "11"));
    }

    #[test]
    fn a_cold_cache_knows_nothing_and_a_stored_value_is_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let q = QueueDb::open(&dir.path().join("queue.db")).unwrap();
        assert_eq!(load(&q), KioskOperating::default());
        q.kv_set(KEY_KIOSK_VOLUME_DEFAULT, "30", 2).unwrap();
        q.kv_set(KEY_FULFILMENT, "seat", 3).unwrap();
        assert_eq!(
            load(&q),
            KioskOperating {
                volume_default: Some(30),
                fulfilment: Some(Fulfilment::Seat),
                ..Default::default()
            }
        );
        q.kv_set(KEY_FULFILMENT, "junk", 4).unwrap();
        assert_eq!(load(&q).fulfilment, None);
        q.kv_set(KEY_BILLING_GRACE_MIN, "5", 5).unwrap();
        q.kv_set(KEY_BILLING_TIME_UP_MODE, "staff_handles", 5)
            .unwrap();
        let l = load(&q);
        assert_eq!(
            (l.grace_min, l.time_up_mode),
            (Some(5), Some(TimeUpMode::StaffHandles))
        );
        q.kv_set(KEY_BILLING_GRACE_MIN, "99", 6).unwrap();
        assert_eq!(load(&q).grace_min, None);
    }

    #[test]
    fn marks_and_low_balance_default_until_the_hub_sends_them() {
        let dir = tempfile::tempdir().unwrap();
        let q = QueueDb::open(&dir.path().join("queue.db")).unwrap();
        let cold = load(&q);
        assert_eq!(
            (cold.marks(), cold.low_balance()),
            (vec![10, 5, 1], Some(5000))
        );
        q.kv_set(KEY_BILLING_WARN_MARKS, "3,20", 7).unwrap();
        q.kv_set(KEY_BILLING_LOW_BALANCE, "12000", 7).unwrap();
        let l = load(&q);
        assert_eq!((l.marks(), l.low_balance()), (vec![20, 3], Some(12000)));
        q.kv_set(KEY_BILLING_LOW_BALANCE, "0", 8).unwrap();
        assert_eq!(load(&q).low_balance(), None);
        q.kv_set(KEY_BILLING_WARN_MARKS, "5,5", 8).unwrap();
        q.kv_set(KEY_BILLING_LOW_BALANCE, "-1", 9).unwrap();
        let bad = load(&q);
        assert_eq!(
            (bad.marks(), bad.low_balance()),
            (vec![10, 5, 1], Some(5000))
        );
        assert!(valid(KEY_BILLING_WARN_MARKS, "10, 5"));
        assert!(!valid(KEY_BILLING_WARN_MARKS, "10,10"));
        assert!(valid(KEY_BILLING_LOW_BALANCE, "0"));
        assert!(!valid(KEY_BILLING_LOW_BALANCE, "-1"));
    }

    #[test]
    fn the_display_modes_default_to_none_and_ignore_bad_lists() {
        let dir = tempfile::tempdir().unwrap();
        let q = QueueDb::open(&dir.path().join("queue.db")).unwrap();
        assert!(load(&q).display_modes.is_empty());
        q.kv_set(KEY_DISPLAY_MODES, "1920x1080@144, 1920x1080@60", 2)
            .unwrap();
        let modes: Vec<String> = load(&q)
            .display_modes
            .iter()
            .map(|m| m.to_string())
            .collect();
        assert_eq!(modes, ["1920x1080@144", "1920x1080@60"]);
        q.kv_set(KEY_DISPLAY_MODES, "1920x1080", 3).unwrap();
        assert!(load(&q).display_modes.is_empty());
        assert!(valid(KEY_DISPLAY_MODES, "") && valid(KEY_DISPLAY_MODES, "1920x1080@60"));
        assert!(!valid(KEY_DISPLAY_MODES, "big"));
    }

    #[test]
    fn the_booster_defaults_until_the_hub_sends_it_and_ignores_bad_values() {
        let dir = tempfile::tempdir().unwrap();
        let q = QueueDb::open(&dir.path().join("queue.db")).unwrap();
        let cold = load(&q).booster;
        assert_eq!(cold, Booster::default());
        assert_eq!(cold.priority, BoosterPriority::AboveNormal);
        assert!(cold.close.is_empty() && !cold.high_performance);
        q.kv_set(KEY_BOOSTER_CLOSE, "discord.exe, OneDrive.exe", 2)
            .unwrap();
        q.kv_set(KEY_BOOSTER_PRIORITY, "high", 2).unwrap();
        q.kv_set(KEY_BOOSTER_POWER_PLAN, "true", 2).unwrap();
        let b = load(&q).booster;
        assert_eq!(b.close, ["discord.exe", "OneDrive.exe"]);
        assert_eq!(b.priority, BoosterPriority::High);
        assert!(b.high_performance);
        q.kv_set(KEY_BOOSTER_CLOSE, "discord", 3).unwrap();
        q.kv_set(KEY_BOOSTER_PRIORITY, "realtime", 3).unwrap();
        q.kv_set(KEY_BOOSTER_POWER_PLAN, "yes", 3).unwrap();
        assert_eq!(load(&q).booster, Booster::default());
        assert!(valid(KEY_BOOSTER_CLOSE, ""));
        assert!(!valid(KEY_BOOSTER_CLOSE, "x"));
        assert!(valid(KEY_BOOSTER_PRIORITY, "normal") && !valid(KEY_BOOSTER_PRIORITY, "idle"));
        assert!(valid(KEY_BOOSTER_POWER_PLAN, "false") && !valid(KEY_BOOSTER_POWER_PLAN, "1"));
    }
}
