//! Kiosk types: the `kiosk.log` intent payload and the Hub-link status the Kiosk service
//! reports to its unprivileged UI (the UI receives it as the Tauri event `hub-status`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How serious a kiosk event is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum KioskLogLevel {
    Info,
    Warn,
    Error,
}

impl KioskLogLevel {
    /// The stored form, e.g. `warn`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// The payload of `kiosk.log`: one event a kiosk recorded, queued while offline and replayed in
/// order. `code` is a short machine code (`app_restarted`), `detail` free text without PII.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KioskLog {
    pub level: KioskLogLevel,
    pub code: String,
    #[serde(default)]
    pub detail: String,
}

/// Longest `kiosk.log` `code`, in bytes (`[a-z0-9_.-]`).
pub const MAX_KIOSK_LOG_CODE_LEN: usize = 64;
/// Longest `kiosk.log` `detail`, in characters.
pub const MAX_KIOSK_LOG_DETAIL_CHARS: usize = 500;

impl KioskLog {
    /// The payload rules the Hub applies (and the kiosk checks before queueing): `code` is
    /// 1–64 of `a–z 0–9 _ . -`, `detail` at most 500 characters with no control characters
    /// other than newline and tab. A failure is `VALIDATION_FAILED` naming the field.
    pub fn validate(&self) -> Result<(), crate::AppError> {
        use crate::{AppError, ErrorCode};
        let code_ok = (1..=MAX_KIOSK_LOG_CODE_LEN).contains(&self.code.len())
            && self
                .code
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b));
        if !code_ok {
            return Err(AppError::with_field(
                ErrorCode::ValidationFailed,
                "code",
                "code must be 1–64 characters of a–z, 0–9, '_', '.' or '-'",
            ));
        }
        if self.detail.chars().count() > MAX_KIOSK_LOG_DETAIL_CHARS
            || self
                .detail
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err(AppError::with_field(
                ErrorCode::ValidationFailed,
                "detail",
                "detail must be at most 500 characters with no control characters",
            ));
        }
        Ok(())
    }
}

/// The state of the Kiosk service's link to the Hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum KioskHubState {
    /// `hello.ok` received; the link is up.
    Connected,
    /// The Hub is unreachable (or refused the pin); the service retries with back-off.
    Reconnecting,
    /// The Hub refused the device token (`TOKEN_REVOKED`/`TOKEN_INVALID`): the device must be
    /// provisioned again. The service retries only slowly.
    TokenRejected,
}

/// The `hub-status` event the Kiosk UI receives from its Tauri shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KioskHubStatus {
    pub state: KioskHubState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_forms() {
        assert_eq!(
            serde_json::to_value(KioskHubStatus {
                state: KioskHubState::TokenRejected
            })
            .unwrap(),
            serde_json::json!({"state": "token_rejected"})
        );
        for l in [
            KioskLogLevel::Info,
            KioskLogLevel::Warn,
            KioskLogLevel::Error,
        ] {
            assert_eq!(
                serde_json::to_string(&l).unwrap(),
                format!("\"{}\"", l.as_str())
            );
        }
        let log: KioskLog =
            serde_json::from_value(serde_json::json!({"level": "warn", "code": "x"})).unwrap();
        assert_eq!(log.detail, "");
    }
}
