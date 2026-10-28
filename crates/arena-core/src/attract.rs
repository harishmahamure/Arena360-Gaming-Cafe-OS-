//! The kiosk attract view (Story 6.7): what each idle PC shows, computed from the branch store.
//!
//! One [`AttractView`] per device: the branch name, the device's group and its hourly rate with
//! the pricing rule in effect ([`crate::pricing::resolve`], the same one the snapshot and the
//! session engine use), the branch locale, and the curated videos of the library.
//!
//! A PC that is unassigned, in maintenance, or whose group has no active rate gets
//! [`AttractState::AskCounter`]: no rate and no rule.

use arena_proto::{
    AppError, AttractMedia, AttractState, AttractView, MAX_ATTRACT_MEDIA, SnapshotActiveRule,
};
use arena_store::Store;

use crate::locale::{locale_rows, snapshot_locale};

/// The stored media of one catalogue game: its video and its cover, as content hashes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameMediaHashes {
    pub video: Option<String>,
    pub cover: Option<String>,
}

/// Looks up the stored media of a catalogue game id (an empty answer when it has none).
pub type MediaLookup<'a> = &'a dyn Fn(&str) -> GameMediaHashes;

/// The library's curated videos: enabled entries linked to the catalogue that have a stored
/// video, in name order, at most [`MAX_ATTRACT_MEDIA`]. The same video of two entries counts
/// once.
fn media_of(store: &Store, lookup: MediaLookup<'_>) -> Result<Vec<AttractMedia>, AppError> {
    let mut out: Vec<AttractMedia> = Vec::new();
    for entry in store.library_entries()? {
        if out.len() == MAX_ATTRACT_MEDIA {
            break;
        }
        let (true, Some(game)) = (
            entry.enabled && entry.deleted_at.is_none(),
            &entry.catalog_id,
        ) else {
            continue;
        };
        let found = lookup(game);
        let Some(video) = found.video else {
            continue;
        };
        if out.iter().any(|m| m.video == video) {
            continue;
        }
        out.push(AttractMedia {
            video,
            cover: found.cover,
        });
    }
    Ok(out)
}

/// The attract view of each of `device_ids` at `now_ms`, in the order given. Nothing is
/// returned before the branch has a locale (not bootstrapped).
pub fn attract_views(
    store: &Store,
    device_ids: &[String],
    now_ms: i64,
    lookup: MediaLookup<'_>,
) -> Result<Vec<(String, AttractView)>, AppError> {
    let (branch, meta, groups, rates, rules, locale_branch, settings) =
        store.read_consistent(|s| {
            Ok((
                s.branch_summary()?,
                s.devices_meta()?,
                s.groups()?,
                s.rates()?,
                s.pricing_rules()?,
                s.branch_locale()?,
                s.settings()?,
            ))
        })?;
    let (Some(branch), Some(locale_branch)) = (branch, locale_branch) else {
        return Ok(Vec::new());
    };
    let locale = snapshot_locale(&locale_rows(&locale_branch, &settings), now_ms);
    let tz = locale.tz.parse::<chrono_tz::Tz>().ok();
    let media = media_of(store, lookup)?;
    Ok(device_ids
        .iter()
        .map(|id| {
            let group = meta
                .iter()
                .find(|m| &m.device_id == id && m.deleted_at.is_none() && m.state != "maintenance")
                .and_then(|m| m.group_id.as_deref())
                .and_then(|gid| groups.iter().find(|g| g.group_id == gid));
            let base = group.and_then(|g| rates.iter().find(|r| r.group_id == g.group_id));
            let (state, rate_minor, rule) = match (group, base, tz) {
                (Some(g), Some(base), Some(tz)) => {
                    let resolved = crate::pricing::resolve(
                        &rules,
                        &g.group_id,
                        base.price_per_hour_minor,
                        tz,
                        now_ms,
                    );
                    let rule = resolved
                        .rule_id
                        .and_then(|rid| rules.iter().find(|r| r.rule_id == rid))
                        .map(|r| SnapshotActiveRule {
                            rule_id: r.rule_id.clone(),
                            name: r.name.clone(),
                            adjust_kind: r.adjust_kind.clone(),
                            adjust_value: r.adjust_value,
                            rate_minor: resolved.rate_minor,
                            until: resolved.until,
                        });
                    (AttractState::Ready, Some(resolved.rate_minor), rule)
                }
                _ => (AttractState::AskCounter, None, None),
            };
            let view = AttractView {
                branch_name: branch.name.clone(),
                state,
                rate_minor,
                group_name: group
                    .filter(|_| state == AttractState::Ready)
                    .map(|g| g.name.clone()),
                rule,
                locale: locale.clone(),
                media: media.clone(),
            };
            (id.clone(), view)
        })
        .collect())
}
