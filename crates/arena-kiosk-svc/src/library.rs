//! The branch game and app library as this kiosk knows it (Story 6.3): the Hub's derived
//! `kiosk.library` setting, cached in `queue.db` so a kiosk that cold-starts with the Hub
//! unreachable still has the last library it received.
//!
//! The value is fetched with the other settings after every hello and applied when the Hub
//! pushes `settings.changed`, newest revision first (like the operating settings: an older
//! revision never replaces a newer one, and a value that does not parse is kept out of the
//! cache). Listing and launching the entries is Story 6.23.

use arena_proto::{KEY_KIOSK_LIBRARY, KioskLibraryEntry, SettingEntry, parse_kiosk_library};

use crate::procs::AppliedRevs;
use crate::queue::QueueDb;
use crate::runtime::Ctx;

/// The settings the library cache holds.
pub const KEYS: [&str; 1] = [KEY_KIOSK_LIBRARY];

/// The last library received: empty when none ever was, or when the cached value cannot be read
/// or parsed (logged).
pub fn load(queue: &QueueDb) -> Vec<KioskLibraryEntry> {
    match queue.kv_get(KEY_KIOSK_LIBRARY) {
        Ok(Some((value, _))) => parse_kiosk_library(&value).unwrap_or_else(|| {
            tracing::error!("the cached game library is not valid; using an empty library");
            Vec::new()
        }),
        Ok(None) => Vec::new(),
        Err(e) => {
            tracing::error!(error = %e, "the game library cache cannot be read");
            Vec::new()
        }
    }
}

/// Seeds the applied revision from the cache.
pub fn seed(queue: &QueueDb, revs: &mut AppliedRevs) {
    for key in KEYS {
        revs.seed(queue, key);
    }
}

/// Caches the library in `values` when it is newer than the applied one and valid.
pub fn apply(ctx: &Ctx, values: &[SettingEntry], revs: &mut AppliedRevs) {
    for entry in values.iter().filter(|e| KEYS.contains(&e.key.as_str())) {
        if revs.get(&entry.key).is_some_and(|r| entry.rev < r) {
            tracing::warn!(key = %entry.key, rev = entry.rev, applied = revs.get(&entry.key), "ignoring an older game library");
            continue;
        }
        let Some(list) = parse_kiosk_library(&entry.value) else {
            tracing::error!(key = %entry.key, rev = entry.rev, "the hub sent an invalid game library; keeping the previous one");
            continue;
        };
        revs.record(&entry.key, entry.rev);
        if let Err(e) = ctx.queue.lock().unwrap_or_else(|p| p.into_inner()).kv_set(
            &entry.key,
            &entry.value,
            entry.rev,
        ) {
            tracing::error!(key = %entry.key, error = %e, "could not cache the game library");
            continue;
        }
        tracing::info!(rev = entry.rev, entries = list.len(), "game library stored");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: &str = r#"[{"id":"I","kind":"game","name":"CS2","launcher":"steam","executable_path":"C:\\Games\\cs2.exe","launch_uri":"","launch_args":"","category":"","tags":["fps"],"age_rating":"18+","catalog_id":null}]"#;

    #[test]
    fn a_cold_cache_is_empty_and_a_stored_library_is_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let q = QueueDb::open(&dir.path().join("queue.db")).unwrap();
        assert!(load(&q).is_empty());
        q.kv_set(KEY_KIOSK_LIBRARY, ONE, 4).unwrap();
        let list = load(&q);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "CS2");
        assert_eq!(list[0].tags, ["fps"]);
        // It survives a restart (a new connection to the same file).
        drop(q);
        let q = QueueDb::open(&dir.path().join("queue.db")).unwrap();
        assert_eq!(load(&q)[0].executable_path, r"C:\Games\cs2.exe");
    }

    #[test]
    fn an_unparsable_cache_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let q = QueueDb::open(&dir.path().join("queue.db")).unwrap();
        for bad in ["nope", "{}", r#"[{"id":"I"}]"#, ""] {
            q.kv_set(KEY_KIOSK_LIBRARY, bad, 5).unwrap();
            assert!(load(&q).is_empty(), "{bad:?}");
        }
    }
}
