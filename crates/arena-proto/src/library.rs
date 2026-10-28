//! The branch game and app library (Story 6.3): the payloads of `library.upsert` and
//! `library.delete`, the views the Hub UI reads, and the derived `kiosk.library` setting every
//! paired kiosk caches.
//!
//! An entry is a game or an app on the branch's PCs. The Hub edits it; `apply()` rewrites two
//! derived settings in the same transaction: `kiosk.library` (the enabled entries, see
//! [`KioskLibraryEntry`]) and the library's rules inside `kiosk.process_allow` (see
//! [`library_rules`]).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::process::{ProcessRule, is_full_path_glob};

/// The derived setting that carries the enabled entries to the kiosks (a JSON array of
/// [`KioskLibraryEntry`]). Only `library.upsert` / `library.delete` write it.
pub const KEY_KIOSK_LIBRARY: &str = "kiosk.library";
/// `kiosk.library` before any entry exists.
pub const DEFAULT_KIOSK_LIBRARY: &str = "[]";

/// The kinds an entry takes.
pub const LIBRARY_KINDS: [&str; 2] = ["game", "app"];
/// The launchers an entry takes.
pub const LIBRARY_LAUNCHERS: [&str; 5] = ["steam", "epic", "riot", "battlenet", "none"];

/// Longest entry name, in characters.
pub const LIBRARY_NAME_MAX: usize = 80;
/// Longest executable path, in characters (the longest allow-rule glob).
pub const LIBRARY_PATH_MAX: usize = 520;
/// Longest launch URI and launch arguments, in characters.
pub const LIBRARY_URI_MAX: usize = 512;
pub const LIBRARY_ARGS_MAX: usize = 512;
/// Longest category, in characters.
pub const LIBRARY_CATEGORY_MAX: usize = 40;
/// Most tags an entry may carry, and the longest one, in characters.
pub const LIBRARY_TAGS_MAX: usize = 10;
pub const LIBRARY_TAG_MAX: usize = 32;
/// Longest age rating, in characters.
pub const LIBRARY_AGE_RATING_MAX: usize = 16;
/// Most install paths one entry records (one per device).
pub const MAX_LIBRARY_INSTALLS: usize = 200;
/// Longest catalogue id, in characters.
pub const LIBRARY_CATALOG_ID_MAX: usize = 64;
/// Most live entries a branch's library holds: `kiosk.library` must fit one Hub frame.
pub const MAX_LIBRARY_ENTRIES: usize = 300;
/// Longest `kiosk.library` value, in characters.
pub const MAX_LIBRARY_VALUE_CHARS: usize = 900_000;

/// The install roots of each launcher's games, as allow-rule globs (unsigned: the launchers and
/// the games they install are not Arena-signed). `none` has none.
pub fn launcher_install_globs(launcher: &str) -> &'static [&'static str] {
    match launcher {
        "steam" => &[r"C:\Program Files (x86)\Steam\**.exe"],
        "epic" => &[
            r"C:\Program Files (x86)\Epic Games\**.exe",
            r"C:\Program Files\Epic Games\**.exe",
        ],
        "riot" => &[r"C:\Riot Games\**.exe"],
        "battlenet" => &[r"C:\Program Files (x86)\Battle.net\**.exe"],
        _ => &[],
    }
}

/// `rules(entry)`: the entry's own executable path (when it has one) as an unsigned allow rule,
/// then its launcher's install rules.
pub fn library_rules(executable_path: &str, launcher: &str) -> Vec<ProcessRule> {
    library_rules_with_installs(executable_path, launcher, &[])
}

/// [`library_rules`] plus one unsigned allow rule for each per-device install path that is not
/// the executable path already (Story 6.5: the same game may sit at a different path on each PC).
pub fn library_rules_with_installs(
    executable_path: &str,
    launcher: &str,
    install_paths: &[&str],
) -> Vec<ProcessRule> {
    let mut rules = Vec::new();
    if !executable_path.is_empty() {
        rules.push(ProcessRule {
            path_glob: executable_path.to_owned(),
            signer: None,
        });
    }
    for path in install_paths {
        if !path.is_empty() && !rules.iter().any(|r| r.path_glob.eq_ignore_ascii_case(path)) {
            rules.push(ProcessRule {
                path_glob: (*path).to_owned(),
                signer: None,
            });
        }
    }
    rules.extend(
        launcher_install_globs(launcher)
            .iter()
            .map(|g| ProcessRule {
                path_glob: (*g).to_owned(),
                signer: None,
            }),
    );
    rules
}

/// The launcher a catalogue game's free-text launcher names: `steam`, `epic`, `riot` or `battlenet`
/// (any case, with or without spaces, dots or a trailing "games"), else `none`.
pub fn launcher_from_catalogue(launcher: Option<&str>) -> &'static str {
    let key: String = launcher
        .unwrap_or("")
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_lowercase();
    match key.as_str() {
        "steam" => "steam",
        "epic" | "epicgames" | "epicgamesstore" => "epic",
        "riot" | "riotgames" | "riotclient" => "riot",
        "battlenet" | "battle" | "blizzard" => "battlenet",
        _ => "none",
    }
}

/// Whether `path` is a full Windows path (`X:\…`) with no wildcard or placeholder character and
/// no control character.
pub fn is_library_path(path: &str) -> bool {
    is_full_path_glob(path)
        && path
            .as_bytes()
            .get(..3)
            .is_some_and(|b| b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\')
        && !path.contains(['*', '?', '{', '}'])
        && !path.chars().any(char::is_control)
}

/// Whether `uri` starts with a scheme of at least two characters (`steam://…`) and has
/// something after it, with no control character.
pub fn is_launch_uri(uri: &str) -> bool {
    let Some((scheme, rest)) = uri.split_once(':') else {
        return false;
    };
    scheme.len() >= 2
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
        && !rest.is_empty()
        && !uri.chars().any(char::is_control)
}

/// Where one device keeps an entry (Story 6.5): its full executable path on that PC, and the
/// key of the proposal it was found as (see `proposal_key`; empty when added by hand), which is
/// how a later scan that finds the same game at a new path updates the path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LibraryInstall {
    pub device_id: String,
    pub path: String,
    #[serde(default)]
    pub match_key: String,
}

/// Where a kiosk finds an entry: the install path on one device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KioskInstall {
    pub device_id: String,
    pub path: String,
}

/// The payload of `library.upsert`. Without `id` it adds an entry (the writer mints the id);
/// with one it edits that entry (the intent carries `base_rev`).
///
/// With `catalog_id` set, `name`, `launcher` and `age_rating` are the catalogue's: the API layer
/// rewrites them from the cached catalogue before the intent is applied, and `apply()` refuses
/// to change them (or `catalog_id`) on an entry that is already linked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LibraryUpsert {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<String>,
    /// `game` or `app`. Named `entry_kind` because `kind` is the intent payload's tag.
    pub entry_kind: String,
    pub name: String,
    /// `steam`, `epic`, `riot`, `battlenet` or `none`.
    pub launcher: String,
    /// A full Windows path; empty only when `launch_uri` is set.
    #[serde(default)]
    pub executable_path: String,
    #[serde(default)]
    pub launch_uri: String,
    #[serde(default)]
    pub launch_args: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub age_rating: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub catalog_id: Option<String>,
    /// Per-device install paths to record with the entry (Story 6.5, approving proposals).
    /// Listed devices get this path; devices not listed keep theirs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub installs: Vec<LibraryInstall>,
}

/// The payload of `library.delete` (a soft delete).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LibraryDelete {
    pub id: String,
}

/// One library entry as the Hub UI reads it (`GET /ui/library`). For a linked entry the name,
/// launcher and age rating are the catalogue's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LibraryEntryView {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub launcher: String,
    pub executable_path: String,
    pub launch_uri: String,
    pub launch_args: String,
    pub category: String,
    pub tags: Vec<String>,
    pub age_rating: String,
    pub enabled: bool,
    pub catalog_id: Option<String>,
    /// The PCs where an install path is known (Story 6.5).
    pub installs: Vec<LibraryInstall>,
    #[ts(type = "number")]
    pub rev: i64,
}

/// The answer of `GET /ui/library`: every live entry (enabled or not), ordered by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LibraryView {
    pub entries: Vec<LibraryEntryView>,
}

/// One entry of the `kiosk.library` setting: what a kiosk needs to list and launch a game or
/// app (no `enabled` flag, no `rev`: only enabled entries are sent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KioskLibraryEntry {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub launcher: String,
    pub executable_path: String,
    pub launch_uri: String,
    pub launch_args: String,
    pub category: String,
    pub tags: Vec<String>,
    pub age_rating: String,
    #[serde(default)]
    pub catalog_id: Option<String>,
    /// Where each device keeps it (Story 6.5). A kiosk lists the entry only when its own device
    /// is here or its `executable_path` exists on it.
    #[serde(default)]
    pub installs: Vec<KioskInstall>,
}

/// A tag list as the JSON array its column holds.
pub fn tags_to_json(tags: &[String]) -> String {
    serde_json::to_string(tags).unwrap_or_else(|_| "[]".to_owned())
}

/// The tags a column holds (empty when it is not a JSON array of text).
pub fn tags_from_json(value: &str) -> Vec<String> {
    serde_json::from_str(value).unwrap_or_default()
}

/// Parses a `kiosk.library` value: a JSON array of [`KioskLibraryEntry`]. `None` when it is
/// anything else.
pub fn parse_kiosk_library(value: &str) -> Option<Vec<KioskLibraryEntry>> {
    if value.chars().count() > MAX_LIBRARY_VALUE_CHARS {
        return None;
    }
    serde_json::from_str(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_full_windows_paths_without_wildcards() {
        assert!(is_library_path(r"C:\Games\X\x.exe"));
        assert!(is_library_path(r"d:\g.exe"));
        for bad in [
            "",
            "x.exe",
            r"C:\",
            r"C:\Games\*.exe",
            r"C:\Games\x?.exe",
            r"C:\{install}\x.exe",
            r"{install}\x.exe",
            r"{system}\x.exe",
            "C:\\Games\\x\n.exe",
        ] {
            assert!(!is_library_path(bad), "{bad:?}");
        }
    }

    #[test]
    fn uris_need_a_scheme() {
        assert!(is_launch_uri("steam://rungameid/730"));
        assert!(is_launch_uri("com.epicgames.launcher://apps/x"));
        for bad in [
            "",
            "steam",
            "steam:",
            "C:\\x.exe",
            "1x://a",
            "://a",
            "a b://c\n",
        ] {
            assert!(!is_launch_uri(bad), "{bad:?}");
        }
    }

    #[test]
    fn an_entrys_rules_are_its_path_then_its_launchers_roots() {
        let rules = library_rules(r"C:\Games\x.exe", "epic");
        assert_eq!(
            rules
                .iter()
                .map(|r| r.path_glob.as_str())
                .collect::<Vec<_>>(),
            [
                r"C:\Games\x.exe",
                r"C:\Program Files (x86)\Epic Games\**.exe",
                r"C:\Program Files\Epic Games\**.exe"
            ]
        );
        assert!(rules.iter().all(|r| r.signer.is_none()));
        assert!(library_rules("", "none").is_empty());
        assert_eq!(library_rules("", "riot").len(), 1);
        assert_eq!(library_rules("", "steam").len(), 1);
        assert_eq!(library_rules("", "battlenet").len(), 1);
    }

    #[test]
    fn a_catalogue_launcher_maps_to_one_of_ours_or_none() {
        for (given, want) in [
            (Some("Steam"), "steam"),
            (Some("Epic Games"), "epic"),
            (Some("epic"), "epic"),
            (Some("Riot Games"), "riot"),
            (Some("Battle.net"), "battlenet"),
            (Some("battlenet"), "battlenet"),
            (Some("GOG"), "none"),
            (Some(""), "none"),
            (None, "none"),
        ] {
            assert_eq!(launcher_from_catalogue(given), want, "{given:?}");
            assert!(LIBRARY_LAUNCHERS.contains(&launcher_from_catalogue(given)));
        }
    }

    #[test]
    fn the_kiosk_library_parses_only_an_array_of_entries() {
        assert_eq!(parse_kiosk_library("[]"), Some(Vec::new()));
        let one = r#"[{"id":"I","kind":"game","name":"X","launcher":"steam","executable_path":"C:\\x.exe","launch_uri":"","launch_args":"","category":"","tags":["a"],"age_rating":"","catalog_id":null}]"#;
        let list = parse_kiosk_library(one).unwrap();
        assert_eq!(list[0].tags, ["a"]);
        assert_eq!(list[0].catalog_id, None);
        assert_eq!(parse_kiosk_library("{}"), None);
        assert_eq!(parse_kiosk_library("nope"), None);
        assert_eq!(parse_kiosk_library(r#"[{"id":"I"}]"#), None);
    }
}
