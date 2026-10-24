use arena_proto::{
    FULFILMENT_MODES, KEY_FULFILMENT, KEY_KIOSK_VOLUME_DEFAULT, KIOSK_OPERATING_KEYS,
    KIOSK_VOLUME_MAX, SettingEntry,
};

use crate::procs::AppliedRevs;
use crate::queue::QueueDb;
use crate::runtime::Ctx;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fulfilment {
    Seat,
    Counter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KioskOperating {
    pub volume_default: Option<u8>,
    pub fulfilment: Option<Fulfilment>,
}

pub const KEYS: [&str; 2] = KIOSK_OPERATING_KEYS;

fn valid(key: &str, value: &str) -> bool {
    match key {
        KEY_KIOSK_VOLUME_DEFAULT => parse_volume(value).is_some(),
        KEY_FULFILMENT => FULFILMENT_MODES.contains(&value),
        _ => false,
    }
}

fn parse_volume(value: &str) -> Option<u8> {
    value
        .parse::<u8>()
        .ok()
        .filter(|v| i64::from(*v) <= KIOSK_VOLUME_MAX && !value.starts_with('+'))
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
            }
        );
        q.kv_set(KEY_FULFILMENT, "junk", 4).unwrap();
        assert_eq!(load(&q).fulfilment, None);
    }
}
