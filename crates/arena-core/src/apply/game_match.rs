//! `game.matched` (Story 6.5, system actor): the catalogue matches the Hub found in a device's
//! stored scan.
//!
//! The matches replace the device's `game_matches` row (rev 1 on the first, then bumped). In the
//! same transaction the library entries' install paths on that device are brought up to date:
//!
//! - an entry linked to a catalogue game gets an install on the device when the scan found that
//!   game (`match_key` = `game:<catalogue id>`);
//! - an entry added from an unmatched proposal keeps the `exe:` key it was approved under, and
//!   gets an install on any device whose scan finds an executable with that key;
//! - an install whose recorded path the scan still finds keeps it (a second copy at another
//!   path is not a change); one the scan no longer finds is moved to the first path found.
//!
//! A path the allow-list would refuse (an unsigned path under a user-writable root), or an
//! install that would push `kiosk.library` past its size limit, is left out; the matches are
//! recorded all the same. `kiosk.process_allow` and `kiosk.library` follow the installs exactly
//! as for `library.upsert`.

use std::collections::HashSet;

use arena_proto::{
    AppError, ErrorCode, GameMatch, GameMatched, Intent, MAX_LIBRARY_VALUE_CHARS, ProcessList,
    ProcessRule, ScanEntry, proposal_key,
};
use arena_store::{
    ChangeOp, GameMatchesRow, InstallRow, KioskScanRow, RowKey, StoreTx, find_game_matches,
    find_kiosk_scan, insert_game_matches, replace_game_matches,
};

use super::Target;
use super::library::{self, LibraryTarget};
use super::mutate::Written;

/// The library, the device's stored scan and its stored matches.
pub(crate) struct MatchTarget {
    lib: LibraryTarget,
    scan: Option<KioskScanRow>,
    stored: Option<GameMatchesRow>,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>, p: &GameMatched) -> Result<Target, AppError> {
    Ok(Target::GameMatch(Box::new(MatchTarget {
        lib: library::load_with(tx, None)?,
        scan: find_kiosk_scan(tx, &p.device_id)?,
        stored: find_game_matches(tx, &p.device_id)?,
    })))
}

fn scan_entries(scan: &KioskScanRow) -> Vec<ScanEntry> {
    serde_json::from_str(&scan.entries).unwrap_or_default()
}

pub(super) fn validate(intent: &Intent, p: &GameMatched, t: &MatchTarget) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "game.matched takes no base_rev"));
    }
    p.validate()?;
    if !t.lib.has_device(&p.device_id) {
        return Err(invalid("device_id", "this is not a paired PC"));
    }
    let Some(scan) = &t.scan else {
        return Err(invalid("device_id", "this PC has no scan to match"));
    };
    if scan.scanned_at != p.scanned_at {
        return Err(invalid(
            "scanned_at",
            "the PC's scan is not the one these matches were made from",
        ));
    }
    let entries = scan_entries(scan);
    for (i, m) in p.matches.iter().enumerate() {
        if !entries.iter().any(|e| e.path.eq_ignore_ascii_case(&m.path)) {
            return Err(invalid(
                &format!("matches[{i}].path"),
                "the scan did not find this path",
            ));
        }
    }
    if let Some(stored) = &t.stored {
        let same = stored.scanned_at == p.scanned_at && stored.matches == matches_json(p)?;
        if stored.scanned_at > p.scanned_at || same {
            return Err(invalid(
                "matches",
                "these matches are already recorded for this scan",
            ));
        }
    }
    Ok(())
}

fn matches_json(p: &GameMatched) -> Result<String, AppError> {
    serde_json::to_string(&p.matches).map_err(|e| AppError::new(ErrorCode::Internal, e.to_string()))
}

/// Whether the allow-list would take `path` as an unsigned rule.
fn path_allowed(path: &str) -> bool {
    let rule = ProcessRule {
        path_glob: path.to_owned(),
        signer: None,
    };
    serde_json::to_string(&[rule])
        .ok()
        .is_some_and(|j| arena_proto::parse_process_rules(ProcessList::Allow, &j).is_ok())
}

/// The path a device should record for an install: the current one when the scan still finds
/// it, else the first path found.
fn pick<'a>(current: Option<&InstallRow>, found: &[&'a str]) -> Option<&'a str> {
    let first = *found.first()?;
    match current {
        Some(c) => Some(
            found
                .iter()
                .copied()
                .find(|p| p.eq_ignore_ascii_case(&c.path))
                .unwrap_or(first),
        ),
        None => Some(first),
    }
}

/// The installs `device_id` should have after the scan: `(entry id, path, key)` for each entry
/// the scan finds that differs from what is stored.
fn wanted(
    t: &MatchTarget,
    device_id: &str,
    matches: &[GameMatch],
    entries: &[ScanEntry],
) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for e in &t.lib.live {
        let current = t
            .lib
            .installs
            .iter()
            .find(|i| i.entry_id == e.id && i.device_id == device_id);
        // The keys this entry is known by: its catalogue game, or the `exe:` keys of its installs.
        let mut keys: Vec<String> = Vec::new();
        if let Some(c) = &e.catalog_id {
            keys.push(format!("game:{c}"));
        } else {
            for i in t.lib.installs.iter().filter(|i| i.entry_id == e.id) {
                if !i.match_key.is_empty() && !keys.contains(&i.match_key) {
                    keys.push(i.match_key.clone());
                }
            }
        }
        for key in keys {
            let found: Vec<&str> = if let Some(id) = key.strip_prefix("game:") {
                matches
                    .iter()
                    .filter(|m| m.catalog_id == id)
                    .map(|m| m.path.as_str())
                    .collect()
            } else {
                entries
                    .iter()
                    .filter(|s| {
                        !matches.iter().any(|m| m.path.eq_ignore_ascii_case(&s.path))
                            && proposal_key(None, &s.file_name, &s.product_name, &s.publisher)
                                == key
                    })
                    .map(|s| s.path.as_str())
                    .collect()
            };
            let Some(path) = pick(current, &found) else {
                continue;
            };
            if current.is_some_and(|c| c.path == path && c.match_key == key) {
                break;
            }
            out.push((e.id.clone(), path.to_owned(), key));
            break;
        }
    }
    out
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    p: &GameMatched,
    t: &MatchTarget,
) -> Result<Vec<Written>, AppError> {
    let json = matches_json(p)?;
    let op = if t.stored.is_some() {
        replace_game_matches(tx, &p.device_id, p.scanned_at, &json)?;
        ChangeOp::Update
    } else {
        insert_game_matches(tx, &p.device_id, p.scanned_at, &json)?;
        ChangeOp::Insert
    };
    let mut out = vec![Written {
        key: RowKey::GameMatches {
            device_id: p.device_id.clone(),
        },
        op,
        before: None,
    }];
    let entries = t.scan.as_ref().map(scan_entries).unwrap_or_default();
    let mut changes = wanted(t, &p.device_id, &p.matches, &entries);
    changes.retain(|(_, path, _)| path_allowed(path));
    if changes.is_empty() {
        return Ok(out);
    }
    // The installs after the change, to work out the two derived settings.
    let mut after: Vec<InstallRow> = t.lib.installs.clone();
    for (entry_id, path, key) in &changes {
        match after
            .iter_mut()
            .find(|i| i.entry_id == *entry_id && i.device_id == p.device_id)
        {
            Some(i) => {
                i.path.clone_from(path);
                i.match_key.clone_from(key);
            }
            None => after.push(InstallRow {
                entry_id: entry_id.clone(),
                device_id: p.device_id.clone(),
                path: path.clone(),
                match_key: key.clone(),
                rev: 0,
            }),
        }
    }
    let plan = library::plan(&t.lib, &t.lib.live, &after)?;
    if plan.library.as_ref().map_or(0, |v| v.chars().count()) > MAX_LIBRARY_VALUE_CHARS {
        return Ok(out);
    }
    if let Some(allow) = &plan.allow
        && arena_proto::parse_process_rules(ProcessList::Allow, allow).is_err()
    {
        return Ok(out);
    }
    let mut seen = HashSet::new();
    for (entry_id, path, key) in &changes {
        let old = t
            .lib
            .installs
            .iter()
            .find(|i| i.entry_id == *entry_id && i.device_id == p.device_id);
        if !seen.insert(entry_id.clone()) {
            continue;
        }
        if let Some(w) = library::write_install(tx, old, entry_id, &p.device_id, path, key)? {
            out.push(w);
        }
    }
    library::write_derived(tx, &t.lib, plan, &mut out)?;
    Ok(out)
}
