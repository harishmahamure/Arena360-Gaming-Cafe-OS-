//! The `.a360` export file (Story 9.4): the whole branch database in one file an owner keeps
//! on a folder or a USB drive.
//!
//! Layout: the magic `A360EXP1`, a big-endian `u32` header length, the header as JSON
//! ([`ExportHeader`]), then the body, a zstd-compressed SQLite database made with `VACUUM INTO`.
//! `sha256` is the hash of the compressed body exactly as stored, so a reader can check the file
//! without decompressing it. `epoch` and `last_seq` are those of the last change row the copy
//! holds, so they name the point in time the file stands for, not the moment the button was
//! pressed.

use std::io::Read;

use chrono::TimeZone;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The settings key `backup.export` writes: the last export that finished.
pub const KEY_BACKUP_LAST_EXPORT: &str = "backup.last_export";
/// Before any export.
pub const DEFAULT_BACKUP_LAST_EXPORT: &str = "";

/// The first bytes of every export file.
pub const EXPORT_MAGIC: &[u8; 8] = b"A360EXP1";
/// The header layout this build writes and reads.
pub const EXPORT_FORMAT_VERSION: u32 = 1;
/// The file extension, without the dot.
pub const EXPORT_EXTENSION: &str = "a360";
/// The longest header a reader accepts, in bytes.
pub const MAX_EXPORT_HEADER_BYTES: u32 = 64 * 1024;
/// The longest file name `backup.export` records.
pub const MAX_EXPORT_FILE_NAME_CHARS: usize = 100;

/// The header of an export file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ExportHeader {
    pub format_version: u32,
    pub branch_id: String,
    pub branch_code: String,
    /// The epoch of the last change row in the copy (empty when it holds none).
    pub epoch: String,
    /// That row's `seq`.
    #[ts(type = "number")]
    pub last_seq: i64,
    /// The newest database migration the copy has.
    pub schema_version: u32,
    /// When the export was made, UTC ms.
    #[ts(type = "number")]
    pub exported_at: i64,
    /// Lower-case hex SHA-256 of the compressed body.
    pub sha256: String,
}

/// Why bytes are not an export file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormatError {
    /// The file does not start with the magic.
    BadMagic,
    /// The file ends inside the preamble or the header.
    Truncated,
    /// The header length is zero or above [`MAX_EXPORT_HEADER_BYTES`].
    HeaderSize,
    /// The header is not the JSON of an [`ExportHeader`].
    BadHeader,
    /// The header is of a layout this build does not read.
    UnsupportedVersion(u32),
}

impl ExportFormatError {
    /// A short code for logs and messages.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BadMagic => "bad_magic",
            Self::Truncated => "truncated",
            Self::HeaderSize => "header_size",
            Self::BadHeader => "bad_header",
            Self::UnsupportedVersion(_) => "unsupported_version",
        }
    }
}

impl std::fmt::Display for ExportFormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedVersion(v) => write!(f, "unsupported_version({v})"),
            other => f.write_str(other.as_str()),
        }
    }
}

impl std::error::Error for ExportFormatError {}

impl ExportHeader {
    /// The bytes that precede the body: magic, header length, header JSON.
    pub fn encode(&self) -> Vec<u8> {
        let json = serde_json::to_vec(self).unwrap_or_default();
        let len = u32::try_from(json.len()).unwrap_or(u32::MAX);
        let mut out = Vec::with_capacity(EXPORT_MAGIC.len() + 4 + json.len());
        out.extend_from_slice(EXPORT_MAGIC);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&json);
        out
    }

    /// Reads the header from the start of `bytes`: the header and the offset of the body.
    pub fn decode(bytes: &[u8]) -> Result<(Self, usize), ExportFormatError> {
        let magic_len = EXPORT_MAGIC.len();
        let preamble = magic_len + 4;
        if bytes.len() < preamble {
            let n = bytes.len().min(magic_len);
            return Err(if bytes[..n] == EXPORT_MAGIC[..n] {
                ExportFormatError::Truncated
            } else {
                ExportFormatError::BadMagic
            });
        }
        let head = &bytes[..preamble];
        if &head[..magic_len] != EXPORT_MAGIC {
            return Err(ExportFormatError::BadMagic);
        }
        let len = u32::from_be_bytes([
            head[magic_len],
            head[magic_len + 1],
            head[magic_len + 2],
            head[magic_len + 3],
        ]);
        if len == 0 || len > MAX_EXPORT_HEADER_BYTES {
            return Err(ExportFormatError::HeaderSize);
        }
        let end = preamble + len as usize;
        let json = bytes
            .get(preamble..end)
            .ok_or(ExportFormatError::Truncated)?;
        let header: Self =
            serde_json::from_slice(json).map_err(|_| ExportFormatError::BadHeader)?;
        if header.format_version != EXPORT_FORMAT_VERSION {
            return Err(ExportFormatError::UnsupportedVersion(header.format_version));
        }
        Ok((header, end))
    }

    /// Reads the header from the start of `reader`, leaving it at the first byte of the body.
    pub fn read(reader: &mut impl Read) -> Result<Self, ExportFormatError> {
        let magic_len = EXPORT_MAGIC.len();
        let mut preamble = vec![0_u8; magic_len + 4];
        reader
            .read_exact(&mut preamble)
            .map_err(|_| ExportFormatError::Truncated)?;
        if &preamble[..magic_len] != EXPORT_MAGIC {
            return Err(ExportFormatError::BadMagic);
        }
        let len = u32::from_be_bytes([
            preamble[magic_len],
            preamble[magic_len + 1],
            preamble[magic_len + 2],
            preamble[magic_len + 3],
        ]);
        if len == 0 || len > MAX_EXPORT_HEADER_BYTES {
            return Err(ExportFormatError::HeaderSize);
        }
        preamble.resize(magic_len + 4 + len as usize, 0);
        reader
            .read_exact(&mut preamble[magic_len + 4..])
            .map_err(|_| ExportFormatError::Truncated)?;
        Self::decode(&preamble).map(|(header, _)| header)
    }
}

/// `<branch_code>-<yyyy-mm-dd-hhmm>.a360`, the time in the branch's zone (UTC for an unknown
/// zone). Characters of the code that are not letters or digits become `-`.
pub fn export_file_name(branch_code: &str, exported_at_ms: i64, tz: &str) -> String {
    stamped_name(branch_code, "", exported_at_ms, tz)
}

fn file_code(branch_code: &str) -> String {
    branch_code
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn stamped_name(branch_code: &str, infix: &str, exported_at_ms: i64, tz: &str) -> String {
    format!(
        "{}-{infix}{}.{EXPORT_EXTENSION}",
        file_code(branch_code),
        stamp_of(exported_at_ms, tz)
    )
}

/// How many daily auto exports the Hub keeps (Story 9.5); older auto files are deleted.
pub const AUTO_EXPORT_KEEP: usize = 7;
/// The `actor.id` of the system actor that records an auto export.
pub const AUTO_EXPORT_ACTOR: &str = "backup";
/// How many exports `backup.last_export` lists in all: the last one and the ones before it.
pub const EXPORT_HISTORY: usize = 7;

/// `<branch_code>-auto-<yyyy-mm-dd-hhmm>.a360`: the name of a daily auto export (Story 9.5), the
/// manual name with `auto-` after the code. Only files with this shape are ever pruned.
pub fn auto_export_file_name(branch_code: &str, exported_at_ms: i64, tz: &str) -> String {
    stamped_name(branch_code, "auto-", exported_at_ms, tz)
}

/// Whether `name` ends like an auto export of any branch: `-auto-<yyyy-mm-dd-hhmm>.a360`.
pub fn is_auto_export_file(name: &str) -> bool {
    let Some((_, rest)) = name.rsplit_once("-auto-") else {
        return false;
    };
    rest.strip_suffix(&format!(".{EXPORT_EXTENSION}"))
        .is_some_and(|stamp| {
            stamp.len() == 15
                && stamp.bytes().enumerate().all(|(i, b)| match i {
                    4 | 7 | 10 => b == b'-',
                    _ => b.is_ascii_digit(),
                })
        })
}

/// Whether `name` is an auto export of the branch with `branch_code`.
pub fn is_auto_export_name(name: &str, branch_code: &str) -> bool {
    let prefix = format!("{}-auto-", file_code(branch_code));
    name.starts_with(&prefix) && is_auto_export_file(name) && name.len() == prefix.len() + 15 + 5
}

/// One line of the export list: a file, how big, when, and who (`None` for a daily auto export).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ExportEntry {
    pub file_name: String,
    #[ts(type = "number")]
    pub bytes: i64,
    #[ts(type = "number")]
    pub exported_at: i64,
    /// The display name of the staff member who tapped Export; `None` for an auto export.
    pub by: Option<String>,
}

/// What an export wrote, and what `backup.export` records: the answer of `POST /ui/backup/export`
/// and (as JSON) the value of `backup.last_export`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BackupExport {
    pub file_name: String,
    /// The size of the file.
    #[ts(type = "number")]
    pub bytes: i64,
    /// The epoch of the last change row the file holds (empty when it holds none).
    pub epoch: String,
    /// That row's `seq`.
    #[ts(type = "number")]
    pub last_seq: i64,
    /// When the file was made, UTC ms.
    #[ts(type = "number")]
    pub exported_at: i64,
    /// Who made it (Story 9.5): the staff member's display name, `None` for an auto export. Set
    /// by the Hub when it applies the record; whatever a sender puts here is replaced.
    #[serde(default)]
    pub by: Option<String>,
    /// The exports before this one, newest first, fewer than [`EXPORT_HISTORY`]. Set by the Hub
    /// when it applies the record.
    #[serde(default)]
    pub recent: Vec<ExportEntry>,
}

/// Where the format reference lives for the owner (the help centre); the `README.txt` of an
/// "Export everything" folder links it.
pub const EXPORT_FORMAT_REFERENCE_URL: &str = "https://arena360.cloud/help/export-format";

/// `<branch_code>-everything-<from>_<to>-<yyyy-mm-dd-hhmm>`: the folder "Export everything"
/// writes inside the backup location (Story 9.6), the time in the branch's zone.
pub fn everything_folder_name(
    branch_code: &str,
    from: &str,
    to: &str,
    exported_at_ms: i64,
    tz: &str,
) -> String {
    format!(
        "{}-everything-{from}_{to}-{}",
        file_code(branch_code),
        stamp_of(exported_at_ms, tz)
    )
}

fn stamp_of(exported_at_ms: i64, tz: &str) -> String {
    let zone = tz.parse::<chrono_tz::Tz>().unwrap_or(chrono_tz::UTC);
    let when = zone
        .timestamp_millis_opt(exported_at_ms)
        .single()
        .unwrap_or_else(|| chrono_tz::UTC.timestamp_opt(0, 0).unwrap());
    when.format("%Y-%m-%d-%H%M").to_string()
}

/// The payload of `POST /ui/backup/export-everything` (Story 9.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ExportEverythingRequest {
    /// First business day (`YYYY-MM-DD`) the reports cover.
    pub from: String,
    /// Last business day (`YYYY-MM-DD`) the reports cover.
    pub to: String,
    /// A manager's PIN ticket for `backup.export`, when the caller lacks the permission.
    #[serde(default)]
    #[ts(optional)]
    pub elevation_ticket: Option<String>,
}

/// What "Export everything" wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct EverythingExported {
    /// The folder, inside the backup location.
    pub folder_name: String,
    /// The database export inside it, as `backup.last_export` records it.
    pub export: BackupExport,
    /// The report CSV files written, by name.
    pub reports: Vec<String>,
    /// The reports that could not be written, as the report ids of `EXPORT_REPORTS`.
    pub skipped: Vec<String>,
}

fn plain_name(name: &str) -> bool {
    name.chars().count() <= MAX_EXPORT_FILE_NAME_CHARS
        && name
            .strip_suffix(&format!(".{EXPORT_EXTENSION}"))
            .is_some_and(|stem| {
                !stem.is_empty()
                    && stem
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            })
}

impl BackupExport {
    /// This export and the ones before it, newest first.
    pub fn entries(&self) -> Vec<ExportEntry> {
        let mut all = vec![ExportEntry {
            file_name: self.file_name.clone(),
            bytes: self.bytes,
            exported_at: self.exported_at,
            by: self.by.clone(),
        }];
        all.extend(self.recent.iter().cloned());
        all
    }

    /// Whether the facts could describe a file this build writes.
    pub fn is_consistent(&self) -> bool {
        let by_ok = |b: &String| {
            !b.is_empty() && b.chars().count() <= 100 && !b.chars().any(char::is_control)
        };
        plain_name(&self.file_name)
            && self.by.as_ref().is_none_or(by_ok)
            && self.recent.len() < EXPORT_HISTORY
            && self.recent.iter().all(|e| {
                plain_name(&e.file_name)
                    && e.bytes > 0
                    && e.exported_at > 0
                    && e.by.as_ref().is_none_or(by_ok)
            })
            && self.bytes > 0
            && self.last_seq >= 0
            && self.exported_at > 0
            && self.epoch.chars().count() <= 100
            && !self.epoch.chars().any(char::is_control)
    }

    /// The value of `backup.last_export`, or `None` when it is empty or malformed.
    pub fn parse(value: &str) -> Option<Self> {
        let parsed: Self = serde_json::from_str(value).ok()?;
        parsed.is_consistent().then_some(parsed)
    }

    /// The JSON stored in `backup.last_export`.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_everything_folder_is_named_for_the_range_and_the_branch_time() {
        // 2027-01-15 08:00 UTC is 13:30 in Kolkata.
        assert_eq!(
            everything_folder_name(
                "LV 01",
                "2027-01-01",
                "2027-01-15",
                1_800_000_000_000,
                "Asia/Kolkata"
            ),
            "LV-01-everything-2027-01-01_2027-01-15-2027-01-15-1330"
        );
    }

    fn header() -> ExportHeader {
        ExportHeader {
            format_version: EXPORT_FORMAT_VERSION,
            branch_id: "01JBRANCH".into(),
            branch_code: "LVL01".into(),
            epoch: "2:abc".into(),
            last_seq: 41,
            schema_version: 85,
            exported_at: 1_800_000_000_000,
            sha256: "ab".repeat(32),
        }
    }

    #[test]
    fn a_header_round_trips_through_bytes_and_a_reader() {
        let mut file = header().encode();
        let body_at = file.len();
        file.extend_from_slice(b"BODY");
        assert_eq!(&file[..8], EXPORT_MAGIC);
        let len = u32::from_be_bytes(file[8..12].try_into().unwrap()) as usize;
        assert_eq!(body_at, 12 + len);
        let (parsed, offset) = ExportHeader::decode(&file).unwrap();
        assert_eq!((parsed, offset), (header(), body_at));
        let mut cursor = std::io::Cursor::new(file);
        assert_eq!(ExportHeader::read(&mut cursor).unwrap(), header());
        let mut rest = Vec::new();
        cursor.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, b"BODY");
    }

    #[test]
    fn bytes_that_are_not_an_export_are_refused_with_a_reason() {
        let good = header().encode();
        assert_eq!(
            ExportHeader::decode(b"SQLite format 3\0........").unwrap_err(),
            ExportFormatError::BadMagic
        );
        assert_eq!(
            ExportHeader::decode(b"A360").unwrap_err(),
            ExportFormatError::Truncated
        );
        assert_eq!(
            ExportHeader::decode(b"nope").unwrap_err(),
            ExportFormatError::BadMagic
        );
        assert_eq!(
            ExportHeader::decode(&good[..good.len() - 1]).unwrap_err(),
            ExportFormatError::Truncated
        );
        let mut huge = EXPORT_MAGIC.to_vec();
        huge.extend_from_slice(&(MAX_EXPORT_HEADER_BYTES + 1).to_be_bytes());
        assert_eq!(
            ExportHeader::decode(&huge).unwrap_err(),
            ExportFormatError::HeaderSize
        );
        let mut empty = EXPORT_MAGIC.to_vec();
        empty.extend_from_slice(&0_u32.to_be_bytes());
        assert_eq!(
            ExportHeader::decode(&empty).unwrap_err(),
            ExportFormatError::HeaderSize
        );
        let junk = b"{not json}";
        let mut bad = EXPORT_MAGIC.to_vec();
        bad.extend_from_slice(&(junk.len() as u32).to_be_bytes());
        bad.extend_from_slice(junk);
        assert_eq!(
            ExportHeader::decode(&bad).unwrap_err(),
            ExportFormatError::BadHeader
        );
        let newer = ExportHeader {
            format_version: 2,
            ..header()
        };
        assert_eq!(
            ExportHeader::decode(&newer.encode()).unwrap_err(),
            ExportFormatError::UnsupportedVersion(2)
        );
        let mut short = std::io::Cursor::new(good[..5].to_vec());
        assert_eq!(
            ExportHeader::read(&mut short).unwrap_err(),
            ExportFormatError::Truncated
        );
    }

    #[test]
    fn the_file_name_is_the_code_and_the_branch_local_minute() {
        // 2026-10-30 18:42 UTC is 00:12 the next day in Kolkata.
        let at = chrono::Utc
            .with_ymd_and_hms(2026, 10, 30, 18, 42, 59)
            .unwrap()
            .timestamp_millis();
        assert_eq!(
            export_file_name("LVL01", at, "Asia/Kolkata"),
            "LVL01-2026-10-31-0012.a360"
        );
        assert_eq!(
            export_file_name("LVL01", at, "UTC"),
            "LVL01-2026-10-30-1842.a360"
        );
        assert_eq!(
            export_file_name("LVL01", at, "Not/AZone"),
            "LVL01-2026-10-30-1842.a360"
        );
        assert_eq!(
            export_file_name("A/B..", at, "UTC"),
            "A-B---2026-10-30-1842.a360"
        );
    }

    #[test]
    fn an_auto_export_has_its_own_name_and_only_that_name_is_auto() {
        let at = chrono::Utc
            .with_ymd_and_hms(2026, 10, 30, 18, 42, 59)
            .unwrap()
            .timestamp_millis();
        let auto = auto_export_file_name("LVL01", at, "Asia/Kolkata");
        assert_eq!(auto, "LVL01-auto-2026-10-31-0012.a360");
        assert!(is_auto_export_name(&auto, "LVL01"));
        assert!(!is_auto_export_name(&auto, "LVL02"));
        assert!(!is_auto_export_name("LVL01-2026-10-31-0012.a360", "LVL01"));
        assert!(!is_auto_export_name(
            "LVL01-auto-2026-10-31-0012.a360.part",
            "LVL01"
        ));
        assert!(!is_auto_export_name("LVL01-auto-notes.a360", "LVL01"));
        assert!(!is_auto_export_name(
            "x/LVL01-auto-2026-10-31-0012.a360",
            "LVL01"
        ));
        assert!(is_auto_export_name(
            "A-B---auto-2026-10-30-1842.a360",
            "A/B.."
        ));
    }

    #[test]
    fn the_history_is_bounded_and_listed_newest_first() {
        let entry = |n: i64| ExportEntry {
            file_name: format!("X-{n}.a360"),
            bytes: 10,
            exported_at: n,
            by: None,
        };
        let facts = BackupExport {
            file_name: "X-9.a360".into(),
            bytes: 5,
            exported_at: 9,
            by: Some("Asha".into()),
            recent: (1..=6).map(entry).collect(),
            ..Default::default()
        };
        assert!(facts.is_consistent());
        assert_eq!(facts.entries().len(), 7);
        assert_eq!(facts.entries()[0].by.as_deref(), Some("Asha"));
        assert_eq!(BackupExport::parse(&facts.to_json()), Some(facts.clone()));
        let long = BackupExport {
            recent: (1..=7).map(entry).collect(),
            ..facts
        };
        assert!(!long.is_consistent());
    }

    #[test]
    fn the_recorded_facts_round_trip_and_reject_a_path() {
        let facts = BackupExport {
            file_name: "LVL01-2026-10-30-1842.a360".into(),
            bytes: 1234,
            epoch: "2:abc".into(),
            last_seq: 41,
            exported_at: 1_800_000_000_000,
            ..Default::default()
        };
        assert!(facts.is_consistent());
        assert_eq!(BackupExport::parse(&facts.to_json()), Some(facts.clone()));
        assert_eq!(BackupExport::parse(""), None);
        for bad in [
            BackupExport {
                file_name: "../x.a360".into(),
                ..facts.clone()
            },
            BackupExport {
                file_name: "C:\\x.a360".into(),
                ..facts.clone()
            },
            BackupExport {
                file_name: "x.db".into(),
                ..facts.clone()
            },
            BackupExport {
                file_name: ".a360".into(),
                ..facts.clone()
            },
            BackupExport {
                bytes: 0,
                ..facts.clone()
            },
            BackupExport {
                last_seq: -1,
                ..facts.clone()
            },
            BackupExport {
                exported_at: 0,
                ..facts.clone()
            },
        ] {
            assert!(!bad.is_consistent(), "{bad:?}");
            assert_eq!(BackupExport::parse(&bad.to_json()), None);
        }
    }
}
