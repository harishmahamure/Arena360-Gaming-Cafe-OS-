use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const KEY_OPENING_HOURS: &str = "branch.opening_hours";
pub const KEY_OPENING_HOURS_SET: &str = "branch.opening_hours_set";
pub const KEY_FULFILMENT: &str = "branch.fulfilment";
pub const KEY_KIOSK_VOLUME_DEFAULT: &str = "kiosk.volume_default";
pub const KEY_BACKUP_DIR: &str = "system.backup_dir";
pub const KEY_ADS_CONSENT: &str = "branch.ads_consent";

pub const DEFAULT_OPENING_HOURS: &str = r#"[{"closed":true},{"closed":true},{"closed":true},{"closed":true},{"closed":true},{"closed":true},{"closed":true}]"#;
pub const DEFAULT_OPENING_HOURS_SET: &str = "false";
pub const DEFAULT_FULFILMENT: &str = "counter";
pub const FULFILMENT_MODES: &[&str] = &["seat", "counter"];
pub const DEFAULT_KIOSK_VOLUME: &str = "50";
pub const KIOSK_VOLUME_MAX: i64 = 100;
pub const DEFAULT_BACKUP_DIR: &str = "";
pub const DEFAULT_ADS_CONSENT: &str =
    r#"{"given":false,"at":null,"staff_id":null,"policy_version":null}"#;
pub const ADS_POLICY_VERSION: u32 = 1;
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

pub fn opening_hours_json(week: &[OpeningDay; DAYS_PER_WEEK]) -> String {
    let days: Vec<String> = week.iter().map(|d| d.to_json()).collect();
    format!("[{}]", days.join(","))
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
    pub printer: crate::PrinterState,
    #[serde(default)]
    pub products_count: u32,
    #[serde(default)]
    pub groups_without_rate: u32,
    #[serde(default)]
    pub manager_exists: bool,
    #[serde(default)]
    pub test_session: TestSessionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum TestSessionState {
    #[default]
    Unset,
    Passed,
    Failed,
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
                printer: crate::PrinterState::Skipped,
                products_count: 0,
                groups_without_rate: 2,
                manager_exists: true,
                test_session: TestSessionState::Passed,
            })
            .unwrap(),
            serde_json::json!({"opening_hours_set": false, "backup_location_set": true, "ad_consent": "not_needed", "printer": "skipped", "products_count": 0, "groups_without_rate": 2, "manager_exists": true, "test_session": "passed"})
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
        assert!(!has_open_day(&[OpeningDay::Closed; 7]));
        let mut w = [OpeningDay::Closed; 7];
        w[6] = OpeningDay::Open {
            open_min: 1,
            close_min: 2,
        };
        assert!(has_open_day(&w));
    }
}
