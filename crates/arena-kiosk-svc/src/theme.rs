//! This kiosk's high-contrast setting (Story 1.21): `display.hc.<own device_id>`, set by a
//! manager on the Hub, never by the player.
//!
//! - The Hub link reads it with `settings.get` after every hello and applies a pushed
//!   `settings.changed` live ([`apply`]); an older revision never replaces a newer one.
//! - Each value is cached in `queue.db` `kv` under its setting key, so a cold start offline
//!   still knows it ([`load`]). A kiosk that never heard from its Hub (or whose cache is
//!   damaged) is not high contrast.
//! - The value is published on [`Ctx::theme`](crate::runtime::Ctx); the app supervisor sends it
//!   to the app as IPC `theme{hc}` on every connection and on every change.

use arena_proto::{SettingEntry, hc_key};

use crate::procs::AppliedRevs;
use crate::queue::QueueDb;
use crate::runtime::Ctx;

/// The setting key of `device_id`'s high contrast.
pub fn key_for(device_id: &str) -> String {
    hc_key(device_id)
}

/// The value a stored text means: `true` or `false`, anything else is not a value.
fn parse(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// The cached high contrast of `device_id` (off when never received, unreadable or invalid).
pub fn load(queue: &QueueDb, device_id: Option<&str>) -> bool {
    let Some(id) = device_id else {
        return false;
    };
    match queue.kv_get(&key_for(id)) {
        Ok(Some((value, _))) => parse(&value).unwrap_or_else(|| {
            tracing::error!(value = %value, "the cached high-contrast value is invalid; off");
            false
        }),
        Ok(None) => false,
        Err(e) => {
            tracing::error!(error = %e, "the high-contrast cache cannot be read; off");
            false
        }
    }
}

/// Applies this device's high contrast from `values` (other keys are ignored): cached and
/// published when it is new. An older revision than the one applied is ignored, an invalid
/// value is refused and the previous one stays.
pub fn apply(ctx: &Ctx, device_id: &str, values: &[SettingEntry], revs: &mut AppliedRevs) {
    let key = key_for(device_id);
    for entry in values.iter().filter(|e| e.key == key) {
        if revs.get(&key).is_some_and(|r| entry.rev < r) {
            tracing::warn!(
                rev = entry.rev,
                applied = revs.get(&key),
                "ignoring an older high-contrast value"
            );
            continue;
        }
        let Some(hc) = parse(&entry.value) else {
            tracing::error!(rev = entry.rev, value = %entry.value, "the hub sent an invalid high-contrast value; keeping the previous one");
            continue;
        };
        revs.record(&key, entry.rev);
        if let Err(e) = ctx.queue.lock().unwrap_or_else(|p| p.into_inner()).kv_set(
            &key,
            &entry.value,
            entry.rev,
        ) {
            tracing::error!(error = %e, "could not cache the high-contrast value");
        }
        ctx.theme.send_if_modified(|current| {
            if *current == hc {
                return false;
            }
            tracing::info!(rev = entry.rev, hc, "high contrast updated");
            *current = hc;
            true
        });
    }
}
