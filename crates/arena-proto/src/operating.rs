use chrono::{Datelike, Duration, NaiveDate, NaiveTime, TimeZone};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const KEY_OPENING_HOURS: &str = "branch.opening_hours";
pub const KEY_OPENING_HOURS_SET: &str = "branch.opening_hours_set";
pub const KEY_FULFILMENT: &str = "branch.fulfilment";
pub const KEY_KIOSK_VOLUME_DEFAULT: &str = "kiosk.volume_default";
pub const KEY_BACKUP_DIR: &str = "system.backup_dir";
pub const KEY_ADS_CONSENT: &str = "branch.ads_consent";
pub const KEY_ADS_RATING: &str = "branch.ads_rating";
pub const KEY_SETUP_LIVE: &str = "setup.live";
/// The game media cache cap in GiB (Story 6.2): an integer from 1 to 10000.
pub const KEY_MEDIA_CAP_GB: &str = "system.media_cap_gb";
pub const DEFAULT_MEDIA_CAP_GB: &str = "10";
pub const MEDIA_CAP_GB_RANGE: (i64, i64) = (1, 10_000);

pub const DEFAULT_OPENING_HOURS: &str = r#"[{"closed":true},{"closed":true},{"closed":true},{"closed":true},{"closed":true},{"closed":true},{"closed":true}]"#;
pub const DEFAULT_OPENING_HOURS_SET: &str = "false";
pub const DEFAULT_SETUP_LIVE: &str = "false";
pub const DEFAULT_FULFILMENT: &str = "counter";
pub const FULFILMENT_MODES: &[&str] = &["seat", "counter"];
pub const DEFAULT_KIOSK_VOLUME: &str = "50";
pub const KIOSK_VOLUME_MAX: i64 = 100;
pub const DEFAULT_BACKUP_DIR: &str = "";
pub const DEFAULT_ADS_CONSENT: &str =
    r#"{"given":false,"at":null,"staff_id":null,"policy_version":null}"#;
pub const ADS_POLICY_VERSION: u32 = 1;
pub const DEFAULT_ADS_RATING: &str = "PG";
/// The content ratings a branch may choose, loosest last. MA is never offered.
pub const ADS_RATINGS: &[&str] = &["G", "PG", "T"];
pub const BACKUP_PROBE_PREFIX: &str = ".arena360-write-test-";
pub const DAYS_PER_WEEK: usize = 7;
pub const LAST_MINUTE: u16 = 1439;

pub const KIOSK_OPERATING_KEYS: [&str; 2] = [KEY_KIOSK_VOLUME_DEFAULT, KEY_FULFILMENT];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpeningDay {
    Closed,
    Open { open_min: u16, close_min: u16 },
}

impl OpeningDay {
    pub fn wraps(self) -> bool {
        matches!(self, Self::Open { open_min, close_min } if close_min < open_min)
    }

    fn to_json(self) -> String {
        match self {
            Self::Closed => r#"{"closed":true}"#.to_owned(),
            Self::Open {
                open_min,
                close_min,
            } => format!(r#"{{"open_min":{open_min},"close_min":{close_min}}}"#),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpeningHoursError {
    NotAWeek,
    Day(usize),
}

impl OpeningHoursError {
    pub fn field(self) -> String {
        match self {
            Self::NotAWeek => KEY_OPENING_HOURS.to_owned(),
            Self::Day(i) => format!("{KEY_OPENING_HOURS}[{i}]"),
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::NotAWeek => "opening hours must be a JSON array of 7 days, Monday first",
            Self::Day(_) => {
                "each day is {\"closed\":true} or {\"open_min\",\"close_min\"} in minutes 0–1439, not equal"
            }
        }
    }
}

fn minute(v: Option<&serde_json::Value>) -> Option<u16> {
    v?.as_u64()
        .filter(|m| *m <= u64::from(LAST_MINUTE))
        .map(|m| m as u16)
}

fn parse_day(v: &serde_json::Value) -> Option<OpeningDay> {
    let obj = v.as_object()?;
    if obj.contains_key("closed") {
        return (obj.len() == 1 && obj["closed"] == serde_json::Value::Bool(true))
            .then_some(OpeningDay::Closed);
    }
    if obj.len() != 2 {
        return None;
    }
    let open_min = minute(obj.get("open_min"))?;
    let close_min = minute(obj.get("close_min"))?;
    (open_min != close_min).then_some(OpeningDay::Open {
        open_min,
        close_min,
    })
}

pub fn parse_opening_hours(value: &str) -> Result<[OpeningDay; DAYS_PER_WEEK], OpeningHoursError> {
    let parsed: serde_json::Value =
        serde_json::from_str(value).map_err(|_| OpeningHoursError::NotAWeek)?;
    let days = parsed.as_array().ok_or(OpeningHoursError::NotAWeek)?;
    if days.len() != DAYS_PER_WEEK {
        return Err(OpeningHoursError::NotAWeek);
    }
    let mut week = [OpeningDay::Closed; DAYS_PER_WEEK];
    for (i, day) in days.iter().enumerate() {
        week[i] = parse_day(day).ok_or(OpeningHoursError::Day(i))?;
    }
    Ok(week)
}

pub fn has_open_day(week: &[OpeningDay; DAYS_PER_WEEK]) -> bool {
    week.iter().any(|d| *d != OpeningDay::Closed)
}

pub fn is_absolute_path(value: &str) -> bool {
    let b = value.as_bytes();
    value.starts_with('/')
        || value.starts_with("\\\\")
        || (b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && matches!(b[2], b'\\' | b'/'))
}

/// The scheme of a folder the Android folder picker returned (a document tree URI).
pub const BACKUP_TREE_SCHEME: &str = "content://";

/// A folder chosen through the Android folder picker (`content://…/tree/…`), kept as the backup
/// location in place of a path.
pub fn is_backup_tree_uri(value: &str) -> bool {
    value.len() > BACKUP_TREE_SCHEME.len()
        && value.starts_with(BACKUP_TREE_SCHEME)
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

/// What `system.backup_dir` may hold when set: an absolute path, or an Android tree URI.
pub fn is_backup_location(value: &str) -> bool {
    is_absolute_path(value) || is_backup_tree_uri(value)
}

pub fn opening_hours_json(week: &[OpeningDay; DAYS_PER_WEEK]) -> String {
    let days: Vec<String> = week.iter().map(|d| d.to_json()).collect();
    format!("[{}]", days.join(","))
}

/// The start (Unix ms) of the opening window of `week` in `tz` that contains `now_ms`, or `None`
/// when the branch is closed then (Story 8.35). A window that wraps past midnight belongs to the
/// day it starts on, so the previous day's window counts after midnight. Where windows overlap,
/// the earliest start is answered.
pub fn opening_window_start(
    week: &[OpeningDay; DAYS_PER_WEEK],
    tz: Tz,
    now_ms: i64,
) -> Option<i64> {
    let today = tz.timestamp_millis_opt(now_ms).single()?.date_naive();
    let at = |date: NaiveDate, minute: u16| -> Option<i64> {
        let time = NaiveTime::from_hms_opt(u32::from(minute) / 60, u32::from(minute) % 60, 0)?;
        Some(
            tz.from_local_datetime(&date.and_time(time))
                .earliest()?
                .timestamp_millis(),
        )
    };
    (-1..=0)
        .filter_map(|d| {
            let date = today + Duration::days(d);
            let OpeningDay::Open {
                open_min,
                close_min,
            } = week[date.weekday().num_days_from_monday() as usize]
            else {
                return None;
            };
            let start = at(date, open_min)?;
            let end = at(
                date + Duration::days(i64::from(close_min < open_min)),
                close_min,
            )?;
            (start <= now_ms && now_ms < end).then_some(start)
        })
        .min()
}

/// What a paid Hub tells the cloud about when its branch is open (Story 8.35): the stored
/// `branch.opening_hours` JSON and the branch's time zone (IANA name).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BranchOpening {
    pub opening_hours: String,
    pub timezone: String,
}

/// The longest `opening_hours` JSON the cloud keeps, in bytes.
pub const BRANCH_OPENING_MAX_BYTES: usize = 1024;

impl BranchOpening {
    /// The week and zone when both parse and the JSON is within [`BRANCH_OPENING_MAX_BYTES`].
    pub fn parse(&self) -> Option<([OpeningDay; DAYS_PER_WEEK], Tz)> {
        if self.opening_hours.len() > BRANCH_OPENING_MAX_BYTES {
            return None;
        }
        let week = parse_opening_hours(&self.opening_hours).ok()?;
        let tz = self.timezone.parse::<Tz>().ok()?;
        Some((week, tz))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AdsConsent {
    pub given: bool,
    #[ts(type = "number | null")]
    pub at: Option<i64>,
    pub staff_id: Option<String>,
    #[serde(default)]
    pub policy_version: Option<u32>,
}

impl AdsConsent {
    pub fn parse(value: &str) -> Option<Self> {
        serde_json::from_str(value).ok()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| DEFAULT_ADS_CONSENT.to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AdsConsentSet {
    pub given: bool,
    pub policy_version: u32,
}

/// The maximum ad content rating a branch asks for (the stored `branch.ads_rating`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum AdRating {
    G,
    #[default]
    PG,
    T,
}

impl AdRating {
    /// The rating a stored value names. Anything else (MA, junk, empty) is PG, never a looser one.
    pub fn parse(value: &str) -> Self {
        match value {
            "G" => Self::G,
            "T" => Self::T,
            _ => Self::PG,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum AdConsentState {
    Given,
    NotNeeded,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupStatus {
    pub opening_hours_set: bool,
    pub backup_location_set: bool,
    pub ad_consent: AdConsentState,
    /// The branch's maximum ad content rating, for the Android ads plugin (Story 4.12).
    #[serde(default)]
    pub ad_rating: AdRating,
    pub printer: crate::PrinterState,
    #[serde(default)]
    pub products_count: u32,
    #[serde(default)]
    pub groups_without_rate: u32,
    #[serde(default)]
    pub manager_exists: bool,
    #[serde(default)]
    pub test_session: TestSessionState,
    #[serde(default)]
    pub live: bool,
    /// The daily auto export could not be saved (Story 9.5); the header says the backup folder is
    /// not available until an export succeeds.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub backup_export_failed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum TestSessionState {
    #[default]
    Unset,
    Running,
    Passed,
    Failed,
}

/// What the Hub's game media cache holds (Story 6.2): bytes of stored files, the cap, the
/// catalogue version it follows and how many games it knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct MediaCacheStatus {
    #[ts(type = "number")]
    pub used_bytes: u64,
    #[ts(type = "number")]
    pub cover_bytes: u64,
    #[ts(type = "number")]
    pub video_bytes: u64,
    #[ts(type = "number")]
    pub cap_bytes: u64,
    #[ts(type = "number")]
    pub catalog_version: u64,
    #[ts(type = "number")]
    pub games: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupDirProblem {
    NotFound,
    NotDir,
    NotWritable,
}

impl BackupDirProblem {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::NotDir => "not_dir",
            Self::NotWritable => "not_writable",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn week(days: &[&str]) -> String {
        format!("[{}]", days.join(","))
    }

    const CLOSED: &str = r#"{"closed":true}"#;

    /// Monday 09:00 to 23:00, Friday 18:00 to 02:00 (Saturday), the rest closed.
    fn shop() -> [OpeningDay; 7] {
        parse_opening_hours(&week(&[
            r#"{"open_min":540,"close_min":1380}"#,
            CLOSED,
            CLOSED,
            CLOSED,
            r#"{"open_min":1080,"close_min":120}"#,
            CLOSED,
            CLOSED,
        ]))
        .unwrap()
    }

    fn at(tz: Tz, (y, mo, d): (i32, u32, u32), (h, mi): (u32, u32)) -> i64 {
        tz.with_ymd_and_hms(y, mo, d, h, mi, 0)
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn the_window_start_is_when_the_open_day_began() {
        let tz = chrono_tz::Asia::Kolkata;
        let w = shop();
        // Monday 2026-10-26.
        let mon = (2026, 10, 26);
        let start = Some(at(tz, mon, (9, 0)));
        assert_eq!(opening_window_start(&w, tz, at(tz, mon, (9, 0))), start);
        assert_eq!(opening_window_start(&w, tz, at(tz, mon, (14, 5))), start);
        assert_eq!(opening_window_start(&w, tz, at(tz, mon, (22, 59))), start);
        assert_eq!(opening_window_start(&w, tz, at(tz, mon, (8, 59))), None);
        assert_eq!(opening_window_start(&w, tz, at(tz, mon, (23, 0))), None);
        assert_eq!(
            opening_window_start(&w, tz, at(tz, (2026, 10, 27), (10, 0))),
            None
        );
    }

    #[test]
    fn a_window_past_midnight_belongs_to_the_day_it_started_on() {
        let tz = chrono_tz::Asia::Kolkata;
        let w = shop();
        // Friday 2026-10-30 18:00 to Saturday 02:00.
        let start = Some(at(tz, (2026, 10, 30), (18, 0)));
        assert_eq!(
            opening_window_start(&w, tz, at(tz, (2026, 10, 30), (17, 59))),
            None
        );
        assert_eq!(
            opening_window_start(&w, tz, at(tz, (2026, 10, 30), (23, 30))),
            start
        );
        assert_eq!(
            opening_window_start(&w, tz, at(tz, (2026, 10, 31), (0, 30))),
            start
        );
        assert_eq!(
            opening_window_start(&w, tz, at(tz, (2026, 10, 31), (1, 59))),
            start
        );
        assert_eq!(
            opening_window_start(&w, tz, at(tz, (2026, 10, 31), (2, 0))),
            None
        );
        // Sunday has no window of its own and Saturday's does not exist.
        assert_eq!(
            opening_window_start(&w, tz, at(tz, (2026, 11, 1), (1, 0))),
            None
        );
    }

    #[test]
    fn the_zone_decides_the_day() {
        let w = shop();
        let utc_time = at(chrono_tz::UTC, (2026, 10, 26), (4, 0));
        // 04:00 UTC is 09:30 in Kolkata (open) and 23:00 the day before in New York.
        assert!(opening_window_start(&w, chrono_tz::Asia::Kolkata, utc_time).is_some());
        assert_eq!(
            opening_window_start(&w, chrono_tz::America::New_York, utc_time),
            None
        );
        assert_eq!(
            opening_window_start(&[OpeningDay::Closed; 7], chrono_tz::UTC, utc_time),
            None
        );
    }

    #[test]
    fn a_branch_opening_parses_only_a_good_week_and_zone_within_the_limit() {
        let ok = BranchOpening {
            opening_hours: DEFAULT_OPENING_HOURS.to_owned(),
            timezone: "Asia/Kolkata".to_owned(),
        };
        assert!(ok.parse().is_some());
        let bad_zone = BranchOpening {
            timezone: "Mars/Base".to_owned(),
            ..ok.clone()
        };
        assert!(bad_zone.parse().is_none());
        let bad_hours = BranchOpening {
            opening_hours: "[]".to_owned(),
            ..ok.clone()
        };
        assert!(bad_hours.parse().is_none());
        let long = BranchOpening {
            opening_hours: format!("{}{}", DEFAULT_OPENING_HOURS, " ".repeat(1024)),
            ..ok
        };
        assert!(long.parse().is_none());
    }

    #[test]
    fn the_default_week_is_closed_every_day() {
        let w = parse_opening_hours(DEFAULT_OPENING_HOURS).unwrap();
        assert_eq!(w, [OpeningDay::Closed; 7]);
        assert_eq!(opening_hours_json(&w), DEFAULT_OPENING_HOURS);
    }

    #[test]
    fn open_wrap_closed_and_bad_days() {
        let ok = week(&[
            r#"{"open_min":600,"close_min":1380}"#,
            CLOSED,
            CLOSED,
            CLOSED,
            r#"{"open_min":1080,"close_min":120}"#,
            CLOSED,
            CLOSED,
        ]);
        let w = parse_opening_hours(&ok).unwrap();
        assert_eq!(
            w[0],
            OpeningDay::Open {
                open_min: 600,
                close_min: 1380
            }
        );
        assert!(!w[0].wraps());
        assert!(w[4].wraps());
        assert_eq!(w[6], OpeningDay::Closed);
        for (bad, day) in [
            (r#"{"open_min":600,"close_min":600}"#, 0),
            (r#"{"open_min":600,"close_min":1440}"#, 2),
            (r#"{"open_min":-1,"close_min":10}"#, 3),
            (r#"{"open_min":1.5,"close_min":10}"#, 1),
            (r#"{"closed":false}"#, 5),
            (r#"{"closed":true,"open_min":1}"#, 6),
            (r#"{"open_min":1}"#, 4),
            (r#"{"open_min":1,"close_min":2,"x":3}"#, 4),
            ("null", 0),
        ] {
            let mut days = vec![CLOSED; 7];
            days[day] = bad;
            let e = parse_opening_hours(&week(&days)).unwrap_err();
            assert_eq!(e, OpeningHoursError::Day(day), "{bad}");
            assert_eq!(e.field(), format!("branch.opening_hours[{day}]"));
        }
        for bad in [
            week(&[CLOSED; 6]),
            week(&[CLOSED; 8]),
            "{}".into(),
            "x".into(),
        ] {
            assert_eq!(
                parse_opening_hours(&bad),
                Err(OpeningHoursError::NotAWeek),
                "{bad}"
            );
        }
        assert_eq!(OpeningHoursError::NotAWeek.field(), "branch.opening_hours");
    }

    #[test]
    fn consent_and_status_serialize() {
        let c = AdsConsent::parse(DEFAULT_ADS_CONSENT).unwrap();
        assert!(!c.given);
        let given = AdsConsent {
            given: true,
            at: Some(5),
            staff_id: Some("S".into()),
            policy_version: Some(ADS_POLICY_VERSION),
        };
        assert_eq!(AdsConsent::parse(&given.to_json()), Some(given));
        assert_eq!(
            serde_json::to_value(SetupStatus {
                opening_hours_set: false,
                backup_location_set: true,
                ad_consent: AdConsentState::NotNeeded,
                ad_rating: AdRating::PG,
                printer: crate::PrinterState::Skipped,
                products_count: 0,
                groups_without_rate: 2,
                manager_exists: true,
                test_session: TestSessionState::Passed,
                live: false,
                backup_export_failed: false,
            })
            .unwrap(),
            serde_json::json!({"opening_hours_set": false, "backup_location_set": true, "ad_consent": "not_needed", "ad_rating": "PG", "printer": "skipped", "products_count": 0, "groups_without_rate": 2, "manager_exists": true, "test_session": "passed", "live": false})
        );
        assert_eq!(BackupDirProblem::NotWritable.as_str(), "not_writable");
        assert_eq!(
            AdsConsent::parse(r#"{"given":true,"at":1,"staff_id":"S"}"#)
                .unwrap()
                .policy_version,
            None
        );
    }

    #[test]
    fn the_hub_ui_sends_the_same_ad_policy_version() {
        let ui = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../apps/hub/src/lib/operating.ts"
        ))
        .unwrap();
        assert!(
            ui.contains(&format!(
                "export const ADS_POLICY_VERSION = {ADS_POLICY_VERSION};"
            )),
            "apps/hub/src/lib/operating.ts must match ADS_POLICY_VERSION"
        );
    }

    #[test]
    fn absolute_paths_and_open_days() {
        for ok in ["/mnt/backup", r"D:\Backups", "e:/b", r"\\nas\share"] {
            assert!(is_absolute_path(ok), "{ok}");
        }
        for bad in ["", "backups", "./b", r"D:Backups", r"\single", "C:"] {
            assert!(!is_absolute_path(bad), "{bad}");
        }
        let tree = "content://com.android.externalstorage.documents/tree/1A2B-3C4D%3ABackups";
        assert!(is_backup_tree_uri(tree) && is_backup_location(tree));
        for bad in [
            "content://",
            "content:/x",
            "https://a/b",
            " content://a/b",
            "content://a\n",
        ] {
            assert!(!is_backup_tree_uri(bad), "{bad:?}");
        }
        assert!(!has_open_day(&[OpeningDay::Closed; 7]));
        let mut w = [OpeningDay::Closed; 7];
        w[6] = OpeningDay::Open {
            open_min: 1,
            close_min: 2,
        };
        assert!(has_open_day(&w));
    }
}
