//! Matching scanned executables to the catalogue and proposing them for the library (Story 6.5):
//! the payloads of `game.matched`, `library.dismiss` and `library.dismiss_clear`, the
//! proposal key and the views behind Settings → Games → "Found on PCs".
//!
//! After a scan report is stored the Hub matches each executable against the cached catalogue
//! signatures and records the matches per device with `game.matched`. A proposal is a group of
//! found executables with the same [`proposal_key`]: a matched game by its catalogue id, any
//! other executable by file name, product name and publisher (without case).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{AppError, ErrorCode};

/// Most matches one `game.matched` carries (a scan holds at most 500 entries).
pub const MAX_MATCHES: usize = 500;
/// Most proposals one `library.dismiss` names.
pub const MAX_DISMISS_KEYS: usize = 500;
/// Longest proposal key, in characters.
pub const MAX_PROPOSAL_KEY_CHARS: usize = 400;
/// Most dismissed proposals a branch keeps.
pub const MAX_DISMISSED: usize = 5_000;

/// One executable the Hub matched to a catalogue game on a device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GameMatch {
    /// The global catalogue id.
    pub catalog_id: String,
    /// The full path of the executable on the device.
    pub path: String,
}

/// The payload of `game.matched` (system actor, no `base_rev`): the catalogue matches found in a
/// device's scan. It replaces the device's stored matches and brings the install paths of the
/// library entries up to date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GameMatched {
    pub device_id: String,
    /// The `scanned_at` of the stored scan these matches were made from.
    #[ts(type = "number")]
    pub scanned_at: i64,
    pub matches: Vec<GameMatch>,
}

/// The payload of `library.dismiss`: proposals the Manager does not want to see again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LibraryDismiss {
    pub keys: Vec<String>,
}

/// The payload of `library.dismiss_clear`: forget every dismissed proposal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LibraryDismissClear {}

/// One PC where a proposal was found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ProposalDevice {
    pub device_id: String,
    pub label: String,
    pub path: String,
}

/// One proposal of "Found on PCs": a game or app found on at least one PC that is not in the
/// library and not dismissed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ProposalView {
    /// Identifies the proposal (see [`proposal_key`]).
    pub key: String,
    /// The catalogue name when matched, else the product name, else the file name.
    pub name: String,
    pub file_name: String,
    pub product_name: String,
    pub publisher: String,
    /// The catalogue id when the executable matched a catalogue game.
    pub catalog_id: Option<String>,
    /// The catalogue cover's SHA-256 when matched and the Hub holds a cover.
    pub cover_sha256: Option<String>,
    /// `steam`, `epic`, `riot`, `battlenet` or `none`.
    pub launcher: String,
    /// The catalogue category when matched, else empty.
    pub category: String,
    /// The catalogue age rating when matched, else empty.
    pub age_rating: String,
    /// How many PCs it was found on.
    pub pc_count: u32,
    pub devices: Vec<ProposalDevice>,
}

/// The answer of `GET /ui/library/proposals`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ProposalsView {
    pub proposals: Vec<ProposalView>,
    /// How many proposals are dismissed.
    pub dismissed: u32,
}

/// The key of a found executable: `game:<catalogue id>` when it matched a catalogue game, else
/// `exe:<file name>|<product name>|<publisher>` in lower case.
pub fn proposal_key(
    catalog_id: Option<&str>,
    file_name: &str,
    product_name: &str,
    publisher: &str,
) -> String {
    match catalog_id {
        Some(id) => format!("game:{id}"),
        None => format!(
            "exe:{}|{}|{}",
            file_name.trim().to_lowercase(),
            product_name.trim().to_lowercase(),
            publisher.trim().to_lowercase()
        ),
    }
}

/// Whether `key` has the shape [`proposal_key`] makes: a `game:` or `exe:` prefix, something
/// after it, within the length limit and with no control character.
pub fn is_proposal_key(key: &str) -> bool {
    key.chars().count() <= MAX_PROPOSAL_KEY_CHARS
        && !key.chars().any(char::is_control)
        && (key.strip_prefix("game:").is_some_and(|r| !r.is_empty())
            || key.strip_prefix("exe:").is_some_and(|r| !r.is_empty()))
}

impl GameMatched {
    /// The payload rules: a device id, `scanned_at` above zero, at most 500 matches each with a
    /// catalogue id and a path, none repeated.
    pub fn validate(&self) -> Result<(), AppError> {
        let bad = |field: &str, message: &str| {
            Err(AppError::with_field(
                ErrorCode::ValidationFailed,
                field,
                message,
            ))
        };
        if self.device_id.trim().is_empty() {
            return bad("device_id", "a device is required");
        }
        if self.scanned_at <= 0 {
            return bad("scanned_at", "scanned_at must be after 0");
        }
        if self.matches.len() > MAX_MATCHES {
            return bad("matches", "too many matches");
        }
        for (i, m) in self.matches.iter().enumerate() {
            if m.catalog_id.trim().is_empty()
                || m.catalog_id.chars().count() > crate::LIBRARY_CATALOG_ID_MAX
                || m.catalog_id.chars().any(char::is_control)
            {
                return bad(&format!("matches[{i}].catalog_id"), "not a catalogue id");
            }
            if !crate::is_library_path(&m.path) || m.path.chars().count() > crate::LIBRARY_PATH_MAX
            {
                return bad(&format!("matches[{i}].path"), "not a full Windows path");
            }
            if self.matches[..i]
                .iter()
                .any(|o| o.path.eq_ignore_ascii_case(&m.path))
            {
                return bad(&format!("matches[{i}].path"), "listed twice");
            }
        }
        Ok(())
    }
}

impl LibraryDismiss {
    /// At least one and at most 500 well-formed keys.
    pub fn validate(&self) -> Result<(), AppError> {
        if self.keys.is_empty() || self.keys.len() > MAX_DISMISS_KEYS {
            return Err(AppError::with_field(
                ErrorCode::ValidationFailed,
                "keys",
                "name between 1 and 500 proposals",
            ));
        }
        if let Some(i) = self.keys.iter().position(|k| !is_proposal_key(k)) {
            return Err(AppError::with_field(
                ErrorCode::ValidationFailed,
                format!("keys[{i}]"),
                "not a proposal key",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_matched_executable_is_keyed_by_its_game_and_others_by_their_names_without_case() {
        assert_eq!(proposal_key(Some("G1"), "x.exe", "P", "Q"), "game:G1");
        assert_eq!(
            proposal_key(None, " Game.EXE ", "My Game", "ACME Inc"),
            "exe:game.exe|my game|acme inc"
        );
        assert_eq!(
            proposal_key(None, "a.exe", "", ""),
            proposal_key(None, "A.EXE", "", "")
        );
    }

    #[test]
    fn keys_have_a_known_prefix_and_something_after_it() {
        for ok in ["game:G1", "exe:a.exe||"] {
            assert!(is_proposal_key(ok), "{ok}");
        }
        for bad in ["", "game:", "exe:", "x:y", "game:a\nb"] {
            assert!(!is_proposal_key(bad), "{bad:?}");
        }
        assert!(!is_proposal_key(&format!("exe:{}", "a".repeat(400))));
    }

    #[test]
    fn matches_are_checked() {
        let m = |p: &str| GameMatch {
            catalog_id: "G1".into(),
            path: p.into(),
        };
        let mk = |matches| GameMatched {
            device_id: "D".into(),
            scanned_at: 5,
            matches,
        };
        assert!(mk(vec![m(r"C:\Games\a.exe")]).validate().is_ok());
        assert!(mk(vec![]).validate().is_ok());
        let dup = mk(vec![m(r"C:\Games\a.exe"), m(r"c:\games\A.EXE")]);
        assert_eq!(
            dup.validate().unwrap_err().field.as_deref(),
            Some("matches[1].path")
        );
        assert_eq!(
            mk(vec![m("a.exe")])
                .validate()
                .unwrap_err()
                .field
                .as_deref(),
            Some("matches[0].path")
        );
        let mut late = mk(vec![]);
        late.scanned_at = 0;
        assert_eq!(
            late.validate().unwrap_err().field.as_deref(),
            Some("scanned_at")
        );
        let mut empty_id = mk(vec![m(r"C:\a.exe")]);
        empty_id.matches[0].catalog_id = " ".into();
        assert_eq!(
            empty_id.validate().unwrap_err().field.as_deref(),
            Some("matches[0].catalog_id")
        );
    }

    #[test]
    fn dismissals_need_well_formed_keys() {
        assert!(
            LibraryDismiss {
                keys: vec!["game:G".into()]
            }
            .validate()
            .is_ok()
        );
        assert!(LibraryDismiss { keys: vec![] }.validate().is_err());
        assert_eq!(
            LibraryDismiss {
                keys: vec!["game:G".into(), "nope".into()]
            }
            .validate()
            .unwrap_err()
            .field
            .as_deref(),
            Some("keys[1]")
        );
    }
}
