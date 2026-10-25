use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{AppError, ErrorCode};

pub const PAIR_CODE_DIGITS: usize = 6;
pub const PAIR_CODE_TTL_MS: i64 = 10 * 60 * 1000;
pub const PAIR_PIN_MAX_FAILURES: u32 = 5;
pub const PAIR_PIN_WINDOW_MS: i64 = 10 * 60 * 1000;
pub const PAIR_PIN_PAUSE_S: u32 = 60;
pub const MAX_PC_NAME_CHARS: usize = 63;
pub const PAIR_PATH: &str = "/pair";
pub const PAIR_SYSTEM_ACTOR: &str = "hub-pair";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum DeviceKind {
    #[default]
    Pc,
    Console,
    Vr,
    Sim,
}

impl DeviceKind {
    pub const ALL: [DeviceKind; 4] = [Self::Pc, Self::Console, Self::Vr, Self::Sim];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pc => "pc",
            Self::Console => "console",
            Self::Vr => "vr",
            Self::Sim => "sim",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PairRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pin: Option<String>,
    pub pc_name: String,
    pub version: String,
}

impl std::fmt::Debug for PairRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairRequest")
            .field("code", &self.code.as_ref().map(|_| "<redacted>"))
            .field("pin", &self.pin.as_ref().map(|_| "<redacted>"))
            .field("pc_name", &self.pc_name)
            .field("version", &self.version)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairSecret<'a> {
    Code(&'a str),
    Pin(&'a str),
}

impl PairRequest {
    pub fn secret(&self) -> Result<PairSecret<'_>, AppError> {
        match (self.code.as_deref(), self.pin.as_deref()) {
            (Some(code), None) => Ok(PairSecret::Code(code)),
            (None, Some(pin)) => Ok(PairSecret::Pin(pin)),
            _ => Err(AppError::new(
                ErrorCode::BadFrame,
                "send exactly one of code or pin",
            )),
        }
    }

    pub fn validate(&self) -> Result<PairSecret<'_>, AppError> {
        let secret = self.secret()?;
        if !is_pc_name(&self.pc_name) {
            return Err(AppError::with_field(
                ErrorCode::ValidationFailed,
                "pc_name",
                "pc_name must be 1–63 printable characters",
            ));
        }
        if !is_semver(&self.version) {
            return Err(AppError::with_field(
                ErrorCode::ValidationFailed,
                "version",
                "version must be x.y.z",
            ));
        }
        Ok(secret)
    }
}

pub fn is_pair_code(code: &str) -> bool {
    code.len() == PAIR_CODE_DIGITS && code.bytes().all(|b| b.is_ascii_digit())
}

pub fn is_pc_name(name: &str) -> bool {
    let trimmed = name.trim();
    let len = trimmed.chars().count();
    trimmed == name && (1..=MAX_PC_NAME_CHARS).contains(&len) && !name.chars().any(char::is_control)
}

pub fn sanitize_pc_name(raw: &str) -> String {
    let printable: String = raw.chars().filter(|c| !c.is_control()).collect();
    printable
        .trim()
        .chars()
        .take(MAX_PC_NAME_CHARS)
        .collect::<String>()
        .trim()
        .to_owned()
}

fn is_semver(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit()))
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PairResponse {
    pub device_id: String,
    pub token: String,
    pub hub_fingerprint: String,
}

impl std::fmt::Debug for PairResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairResponse")
            .field("device_id", &self.device_id)
            .field("token", &"<redacted>")
            .field("hub_fingerprint", &self.hub_fingerprint)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KioskPairingCode {
    pub code: String,
    #[ts(type = "number")]
    pub expires_at: i64,
}

impl std::fmt::Debug for KioskPairingCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KioskPairingCode")
            .field("code", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KioskPairInfo {
    pub pc_name: String,
    pub hub: Option<crate::SelectedHub>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KioskPaired {
    pub device_id: String,
    pub branch_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(code: Option<&str>, pin: Option<&str>) -> PairRequest {
        PairRequest {
            code: code.map(str::to_owned),
            pin: pin.map(str::to_owned),
            pc_name: "PC-01".into(),
            version: "0.1.0".into(),
        }
    }

    #[test]
    fn exactly_one_secret_and_a_valid_name_and_version() {
        assert_eq!(
            req(Some("123456"), None).validate().unwrap(),
            PairSecret::Code("123456")
        );
        assert_eq!(
            req(None, Some("1111")).validate().unwrap(),
            PairSecret::Pin("1111")
        );
        assert_eq!(
            req(Some("1"), Some("2")).validate().unwrap_err().code,
            ErrorCode::BadFrame
        );
        assert_eq!(
            req(None, None).validate().unwrap_err().code,
            ErrorCode::BadFrame
        );
        for bad in ["", " PC", "a\u{7}b", &"x".repeat(64)] {
            let mut r = req(Some("123456"), None);
            r.pc_name = bad.into();
            assert_eq!(r.validate().unwrap_err().field.as_deref(), Some("pc_name"));
        }
        let mut r = req(Some("123456"), None);
        r.version = "1.2".into();
        assert_eq!(r.validate().unwrap_err().field.as_deref(), Some("version"));
    }

    #[test]
    fn codes_and_names() {
        assert!(is_pair_code("012345"));
        for bad in ["12345", "1234567", "12345a", ""] {
            assert!(!is_pair_code(bad), "{bad}");
        }
        assert_eq!(sanitize_pc_name("  desk\u{0}-1  "), "desk-1");
        assert_eq!(sanitize_pc_name(&"y".repeat(80)).chars().count(), 63);
        assert!(is_pc_name(&sanitize_pc_name("  PC 7 ")));
    }

    #[test]
    fn secrets_never_reach_debug_output() {
        let r = format!("{:?}", req(Some("987654"), Some("4321")));
        assert!(!r.contains("987654") && !r.contains("4321"));
        let p = format!(
            "{:?}",
            PairResponse {
                device_id: "D".into(),
                token: "tok-secret".into(),
                hub_fingerprint: "f".into(),
            }
        );
        assert!(!p.contains("tok-secret"));
        let c = format!(
            "{:?}",
            KioskPairingCode {
                code: "135791".into(),
                expires_at: 1
            }
        );
        assert!(!c.contains("135791"));
        assert_eq!(serde_json::to_string(&DeviceKind::Vr).unwrap(), "\"vr\"");
        assert_eq!(DeviceKind::parse("sim"), Some(DeviceKind::Sim));
    }
}
