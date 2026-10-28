//! What this kiosk's idle screen shows (Story 6.7): the last attract view the Hub pushed and
//! the branch logo, cached in `queue.db` so a cold start with the Hub unreachable still paints
//! branding, rates and rule at once.
//!
//! - **View.** The Hub pushes one `attract` event per device on its 5 s tick. The service keeps
//!   the last one in `kv` under [`KEY_VIEW`]; a view equal to the current one is dropped (the
//!   app is told only when something changed, and the cache is written only then). A view that
//!   is not sane (a media hash that is not a SHA-256, too many items) is refused.
//! - **Logo.** The `kiosk.logo` setting is fetched after every hello and applied when the Hub
//!   pushes `settings.changed`, newest revision first (like the library: an older revision never
//!   replaces a newer one, and a value that fails the logo rules is kept out of the cache). An
//!   empty value means no logo.
//! - Both are published on [`Ctx::attract`](crate::runtime::Ctx); the app supervisor sends them
//!   to the app as IPC `attract{view?, logo?}`: both first on every connection, then only what
//!   changed.

use arena_proto::{AttractView, KEY_KIOSK_LOGO, SettingEntry, logo_problem};

use crate::procs::AppliedRevs;
use crate::queue::QueueDb;
use crate::runtime::Ctx;

/// The `kv` key of the cached attract view (JSON).
pub const KEY_VIEW: &str = "attract.view";
/// The settings the logo cache holds.
pub const KEYS: [&str; 1] = [KEY_KIOSK_LOGO];

/// What the idle screen shows: what the service last learned.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shown {
    pub view: Option<AttractView>,
    /// The logo as a `data:` URI (`None`: none set, so the screen shows the Arena 360 mark).
    pub logo: Option<String>,
}

fn logo_of(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

/// The cached view and logo (each `None` when never received, unreadable or not valid).
pub fn load(queue: &QueueDb) -> Shown {
    let view = match queue.kv_get(KEY_VIEW) {
        Ok(Some((json, _))) => match serde_json::from_str::<AttractView>(&json) {
            Ok(v) if v.is_sane() => Some(v),
            _ => {
                tracing::error!("the cached attract view is not valid; ignoring it");
                None
            }
        },
        Ok(None) => None,
        Err(e) => {
            tracing::error!(error = %e, "the attract cache cannot be read");
            None
        }
    };
    let logo = match queue.kv_get(KEY_KIOSK_LOGO) {
        Ok(Some((value, _))) if logo_problem(&value).is_none() => logo_of(&value),
        Ok(Some(_)) => {
            tracing::error!("the cached logo is not valid; showing the default mark");
            None
        }
        Ok(None) => None,
        Err(e) => {
            tracing::error!(error = %e, "the logo cache cannot be read");
            None
        }
    };
    Shown { view, logo }
}

/// Seeds the applied revision from the cache.
pub fn seed(queue: &QueueDb, revs: &mut AppliedRevs) {
    for key in KEYS {
        revs.seed(queue, key);
    }
}

/// Takes a pushed view: refused when it is not sane, dropped when it equals the current one,
/// otherwise cached and published.
pub fn push(ctx: &Ctx, view: AttractView) {
    if !view.is_sane() {
        tracing::warn!("the hub sent an invalid attract view; keeping the previous one");
        return;
    }
    let changed = ctx.attract.send_if_modified(|shown| {
        if shown.view.as_ref() == Some(&view) {
            return false;
        }
        shown.view = Some(view.clone());
        true
    });
    if !changed {
        return;
    }
    let json = match serde_json::to_string(&view) {
        Ok(j) => j,
        Err(e) => {
            tracing::error!(error = %e, "could not encode the attract view for the cache");
            return;
        }
    };
    if let Err(e) = ctx.queue.lock().unwrap_or_else(|p| p.into_inner()).kv_set(
        KEY_VIEW,
        &json,
        ctx.clock.now_ms(),
    ) {
        tracing::error!(error = %e, "could not cache the attract view");
    }
}

/// Caches and publishes the logo in `values` when it is newer than the applied one and valid.
pub fn apply(ctx: &Ctx, values: &[SettingEntry], revs: &mut AppliedRevs) {
    for entry in values.iter().filter(|e| KEYS.contains(&e.key.as_str())) {
        if revs.get(&entry.key).is_some_and(|r| entry.rev < r) {
            tracing::warn!(
                rev = entry.rev,
                applied = revs.get(&entry.key),
                "ignoring an older logo"
            );
            continue;
        }
        if let Some(problem) = logo_problem(&entry.value) {
            tracing::error!(
                rev = entry.rev,
                problem,
                "the hub sent an invalid logo; keeping the previous one"
            );
            continue;
        }
        revs.record(&entry.key, entry.rev);
        if let Err(e) = ctx.queue.lock().unwrap_or_else(|p| p.into_inner()).kv_set(
            &entry.key,
            &entry.value,
            entry.rev,
        ) {
            tracing::error!(error = %e, "could not cache the logo");
            continue;
        }
        let logo = logo_of(&entry.value);
        ctx.attract.send_if_modified(|shown| {
            if shown.logo == logo {
                return false;
            }
            tracing::info!(rev = entry.rev, set = logo.is_some(), "logo updated");
            shown.logo = logo;
            true
        });
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use arena_proto::{AttractMedia, AttractState, NumberFormat, SnapshotLocale, SymbolPosition};

    pub(crate) fn view(rate: i64) -> AttractView {
        AttractView {
            branch_name: "Level Up".into(),
            state: AttractState::Ready,
            rate_minor: Some(rate),
            group_name: Some("PC".into()),
            rule: None,
            locale: SnapshotLocale {
                currency: "INR".into(),
                exponent: 2,
                symbol: "₹".into(),
                symbol_position: SymbolPosition::Before,
                number_format: NumberFormat::Indian,
                tz: "Asia/Kolkata".into(),
                day_start_min: 360,
                fy_start_month: 4,
                pending: None,
            },
            media: vec![AttractMedia {
                video: "ab".repeat(32),
                cover: None,
            }],
        }
    }

    const SVG: &str =
        "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciLz4=";

    #[test]
    fn a_cold_cache_shows_nothing_and_a_stored_view_and_logo_are_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let q = QueueDb::open(&dir.path().join("queue.db")).unwrap();
        assert_eq!(load(&q), Shown::default());
        q.kv_set(KEY_VIEW, &serde_json::to_string(&view(12_000)).unwrap(), 1)
            .unwrap();
        q.kv_set(KEY_KIOSK_LOGO, SVG, 4).unwrap();
        let shown = load(&q);
        assert_eq!(shown.view.as_ref().unwrap().rate_minor, Some(12_000));
        assert_eq!(shown.logo.as_deref(), Some(SVG));
        // It survives a restart (a new connection to the same file).
        drop(q);
        let q = QueueDb::open(&dir.path().join("queue.db")).unwrap();
        assert_eq!(load(&q), shown);
        // An empty logo is none.
        q.kv_set(KEY_KIOSK_LOGO, "", 5).unwrap();
        assert_eq!(load(&q).logo, None);
    }

    #[test]
    fn a_damaged_cache_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let q = QueueDb::open(&dir.path().join("queue.db")).unwrap();
        q.kv_set(KEY_VIEW, "nope", 1).unwrap();
        q.kv_set(KEY_KIOSK_LOGO, "data:image/png;base64,AAAA", 1)
            .unwrap();
        assert_eq!(load(&q), Shown::default());
        let mut bad = view(1);
        bad.media[0].video = "nope".into();
        q.kv_set(KEY_VIEW, &serde_json::to_string(&bad).unwrap(), 2)
            .unwrap();
        assert_eq!(load(&q).view, None);
    }
}
