//! The kiosk's installed-software scan (Story 6.4): the payload of `kiosk.scan_reported` and
//! the view the Hub UI reads (`GET /ui/library/scans`).
//!
//! A paired kiosk looks at its own PC (Steam, Epic, Riot and the common game folders) and
//! reports what it found. The Hub keeps the latest report per device in `kiosk_scans`; matching
//! it to the catalogue is Story 6.5.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{AppError, ErrorCode};

/// The branch setting that holds the branch time zone (an IANA name). Paired kiosks may read
/// it: the daily scan runs at 05:00 in that zone.
pub const KEY_BRANCH_TZ: &str = "branch.tz";

/// The launchers a scan reports as installed.
pub const SCAN_LAUNCHERS: [&str; 3] = ["steam", "epic", "riot"];
/// The launchers an entry may be tagged with.
pub const SCAN_ENTRY_LAUNCHERS: [&str; 5] = ["steam", "epic", "riot", "battlenet", "none"];

/// Most entries one report carries.
pub const MAX_SCAN_ENTRIES: usize = 500;
/// Longest path (an executable's, or a launcher's install folder), in characters.
pub const MAX_SCAN_PATH_CHARS: usize = 260;
/// Longest executable file name, in characters.
pub const MAX_SCAN_FILE_NAME_CHARS: usize = 128;
/// Longest product name and publisher, in characters.
pub const MAX_SCAN_TEXT_CHARS: usize = 128;

/// One launcher found on the PC and where it is installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ScanLauncher {
    /// `steam`, `epic` or `riot`.
    pub launcher: String,
    /// The launcher's install folder.
    pub path: String,
}

/// One executable found on the PC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ScanEntry {
    /// The full path of the executable.
    pub path: String,
    pub file_name: String,
    /// The `ProductName` of the executable's version info (empty when it has none).
    #[serde(default)]
    pub product_name: String,
    /// The `CompanyName` of the executable's version info (empty when it has none).
    #[serde(default)]
    pub publisher: String,
    /// The launcher whose source found it: `steam`, `epic`, `riot`, `battlenet` or `none`.
    pub launcher: String,
}

/// The payload of `kiosk.scan_reported` (device actor, no `base_rev`, no reason).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KioskScanReported {
    /// When the scan finished, in milliseconds since the epoch (not after the intent's
    /// `issued_at`).
    #[ts(type = "number")]
    pub scanned_at: i64,
    pub entries: Vec<ScanEntry>,
    /// Which of Steam, Epic and Riot are installed, one each.
    pub launchers: Vec<ScanLauncher>,
    /// How many folders or files the scan could not read.
    pub skipped: u32,
}

fn invalid(field: impl Into<String>, message: impl Into<String>) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field.into(), message.into())
}

/// Checks free text: at most `max` characters (at least `min`), no control character.
fn text(field: &str, v: &str, min: usize, max: usize) -> Result<(), AppError> {
    let n = v.chars().count();
    if n < min || n > max || v.chars().any(char::is_control) {
        let low = if min == 0 {
            "at most".to_owned()
        } else {
            format!("{min}–")
        };
        return Err(invalid(
            field,
            format!("{field} must be {low}{max} characters with no control characters"),
        ));
    }
    Ok(())
}

impl KioskScanReported {
    /// The payload rules the Hub applies (and the kiosk checks before queueing): `scanned_at`
    /// above zero and not after `issued_at`; at most 500 entries, each with a 1–260 character
    /// path, a 1–128 character file name, a product name and publisher of at most 128, and a
    /// known launcher; at most three launchers, one each of `steam`, `epic` and `riot`, with a
    /// 1–260 character path; no control characters in any text. A failure is
    /// `VALIDATION_FAILED` naming the field.
    pub fn validate(&self, issued_at: i64) -> Result<(), AppError> {
        if self.scanned_at <= 0 || self.scanned_at > issued_at {
            return Err(invalid(
                "scanned_at",
                "scanned_at must be after 0 and not after the time the report was issued",
            ));
        }
        if self.entries.len() > MAX_SCAN_ENTRIES {
            return Err(invalid(
                "entries",
                format!("a scan report holds at most {MAX_SCAN_ENTRIES} entries"),
            ));
        }
        if self.launchers.len() > SCAN_LAUNCHERS.len() {
            return Err(invalid("launchers", "at most three launchers"));
        }
        for (i, l) in self.launchers.iter().enumerate() {
            if !SCAN_LAUNCHERS.contains(&l.launcher.as_str())
                || self.launchers[..i].iter().any(|o| o.launcher == l.launcher)
            {
                return Err(invalid(
                    format!("launchers[{i}].launcher"),
                    "a launcher is steam, epic or riot, listed once",
                ));
            }
            text(
                &format!("launchers[{i}].path"),
                &l.path,
                1,
                MAX_SCAN_PATH_CHARS,
            )?;
        }
        for (i, e) in self.entries.iter().enumerate() {
            text(
                &format!("entries[{i}].path"),
                &e.path,
                1,
                MAX_SCAN_PATH_CHARS,
            )?;
            text(
                &format!("entries[{i}].file_name"),
                &e.file_name,
                1,
                MAX_SCAN_FILE_NAME_CHARS,
            )?;
            text(
                &format!("entries[{i}].product_name"),
                &e.product_name,
                0,
                MAX_SCAN_TEXT_CHARS,
            )?;
            text(
                &format!("entries[{i}].publisher"),
                &e.publisher,
                0,
                MAX_SCAN_TEXT_CHARS,
            )?;
            if !SCAN_ENTRY_LAUNCHERS.contains(&e.launcher.as_str()) {
                return Err(invalid(
                    format!("entries[{i}].launcher"),
                    "launcher must be steam, epic, riot, battlenet or none",
                ));
            }
        }
        Ok(())
    }
}

/// One PC's last scan as the Hub UI reads it. The scan fields are `null` when the PC was never
/// scanned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DeviceScanView {
    pub device_id: String,
    /// The PC's label.
    pub name: String,
    #[ts(type = "number | null")]
    pub scanned_at: Option<i64>,
    #[ts(type = "number | null")]
    pub entry_count: Option<i64>,
    #[ts(type = "number | null")]
    pub skipped: Option<i64>,
    pub launchers: Option<Vec<ScanLauncher>>,
}

/// The answer of `GET /ui/library/scans`: every live paired PC, ordered by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LibraryScansView {
    pub devices: Vec<DeviceScanView>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    fn entry(path: &str) -> ScanEntry {
        ScanEntry {
            path: path.into(),
            file_name: "cs2.exe".into(),
            product_name: "Counter-Strike 2".into(),
            publisher: "Valve".into(),
            launcher: "steam".into(),
        }
    }

    fn report() -> KioskScanReported {
        KioskScanReported {
            scanned_at: NOW - 1000,
            entries: vec![entry(r"C:\Games\cs2.exe")],
            launchers: vec![ScanLauncher {
                launcher: "steam".into(),
                path: r"C:\Program Files (x86)\Steam".into(),
            }],
            skipped: 2,
        }
    }

    fn field(r: &KioskScanReported) -> String {
        let e = r.validate(NOW).unwrap_err();
        assert_eq!(e.code, ErrorCode::ValidationFailed);
        e.field.unwrap_or_default()
    }

    #[test]
    fn a_good_report_passes_and_round_trips() {
        let r = report();
        r.validate(NOW).unwrap();
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(
            serde_json::from_value::<KioskScanReported>(json).unwrap(),
            r
        );
        let min: ScanEntry = serde_json::from_value(
            serde_json::json!({"path": "C:\\a.exe", "file_name": "a.exe", "launcher": "none"}),
        )
        .unwrap();
        assert_eq!(min.product_name, "");
        let empty = KioskScanReported {
            entries: vec![],
            launchers: vec![],
            skipped: 0,
            ..report()
        };
        empty.validate(NOW).unwrap();
        // Issued at the very moment it finished is fine.
        KioskScanReported {
            scanned_at: NOW,
            ..report()
        }
        .validate(NOW)
        .unwrap();
    }

    #[test]
    fn the_scan_time_is_positive_and_not_in_the_future() {
        for at in [0, -5, NOW + 1] {
            let r = KioskScanReported {
                scanned_at: at,
                ..report()
            };
            assert_eq!(field(&r), "scanned_at", "{at}");
        }
    }

    #[test]
    fn five_hundred_entries_pass_and_501_fail() {
        let mut r = report();
        r.entries = (0..MAX_SCAN_ENTRIES)
            .map(|i| entry(&format!(r"C:\Games\{i}.exe")))
            .collect();
        r.validate(NOW).unwrap();
        r.entries.push(entry(r"C:\Games\extra.exe"));
        assert_eq!(field(&r), "entries");
    }

    #[test]
    fn entry_text_is_bounded_and_has_no_control_characters() {
        let long = |n| "x".repeat(n);
        type Edit = fn(&mut ScanEntry, String);
        let bad: [(&str, Edit); 12] = [
            ("path", |e, _| e.path = String::new()),
            ("path", |e, l| e.path = l[..261].to_owned()),
            ("path", |e, _| e.path = "C:/a\u{0}.exe".into()),
            ("file_name", |e, _| e.file_name = String::new()),
            ("file_name", |e, l| e.file_name = l[..129].to_owned()),
            ("file_name", |e, _| e.file_name = "a\n.exe".into()),
            ("product_name", |e, l| e.product_name = l[..129].to_owned()),
            ("product_name", |e, _| e.product_name = "a\tb".into()),
            ("publisher", |e, l| e.publisher = l[..129].to_owned()),
            ("publisher", |e, _| e.publisher = "a\u{7f}".into()),
            ("launcher", |e, _| e.launcher = "gog".into()),
            ("launcher", |e, _| e.launcher = String::new()),
        ];
        for (name, edit) in bad {
            let mut r = report();
            edit(&mut r.entries[0], long(300));
            assert_eq!(field(&r), format!("entries[0].{name}"));
        }
        let mut ok = report();
        ok.entries[0].path = long(260);
        ok.entries[0].file_name = long(128);
        ok.entries[0].product_name = long(128);
        ok.entries[0].publisher = String::new();
        ok.entries[0].launcher = "battlenet".into();
        ok.validate(NOW).unwrap();
    }

    #[test]
    fn launchers_are_steam_epic_riot_once_each_with_a_path() {
        let l = |name: &str, path: &str| ScanLauncher {
            launcher: name.into(),
            path: path.into(),
        };
        let mut r = report();
        r.launchers = vec![l("steam", "C:\\S"), l("epic", "C:\\E"), l("riot", "C:\\R")];
        r.validate(NOW).unwrap();
        r.launchers.push(l("steam", "C:\\S2"));
        assert_eq!(field(&r), "launchers");
        r.launchers = vec![l("steam", "C:\\S"), l("steam", "C:\\S2")];
        assert_eq!(field(&r), "launchers[1].launcher");
        r.launchers = vec![l("battlenet", "C:\\B")];
        assert_eq!(field(&r), "launchers[0].launcher");
        r.launchers = vec![l("steam", "")];
        assert_eq!(field(&r), "launchers[0].path");
        r.launchers = vec![l("steam", &"p".repeat(261))];
        assert_eq!(field(&r), "launchers[0].path");
        r.launchers = vec![l("riot", "C:\\R\u{1b}")];
        assert_eq!(field(&r), "launchers[0].path");
    }
}
