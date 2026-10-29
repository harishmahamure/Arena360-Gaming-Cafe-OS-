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

/// Longest game id of a `game.launched`, in characters (a library entry id).
pub const MAX_GAME_ID_LEN: usize = 64;

/// The launchers the Games tab opens (Story 6.26): the `launcher` of a `launcher.opened`.
pub const OPENABLE_LAUNCHERS: [&str; 3] = ["steam", "epic", "riot"];

/// The payload of `launcher.opened` (Story 6.26): a player opened Steam, Epic or Riot from the
/// Games tab. As with `game.launched`, the Hub names the player and session itself from the
/// device's live session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KioskLauncherOpened {
    /// `steam`, `epic` or `riot`.
    pub launcher: String,
}

impl KioskLauncherOpened {
    /// `launcher` must be one of [`OPENABLE_LAUNCHERS`]; a failure is `VALIDATION_FAILED` naming
    /// the field.
    pub fn validate(&self) -> Result<(), crate::AppError> {
        use crate::{AppError, ErrorCode};
        if OPENABLE_LAUNCHERS.contains(&self.launcher.as_str()) {
            return Ok(());
        }
        Err(AppError::with_field(
            ErrorCode::ValidationFailed,
            "launcher",
            "launcher must be steam, epic or riot",
        ))
    }
}

/// The payload of `game.launched` (Story 6.23): a player started a library game at this PC. The
/// Hub names the player and session itself from the device's live session; the kiosk never does.
/// `name` is the library name at the time, so the log reads the same after a rename.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KioskGameLaunched {
    pub game_id: String,
    pub name: String,
    /// Story 6.25: the milliseconds from the click to the game's first window, or `null` when it
    /// showed none within [`LAUNCH_WINDOW_TIMEOUT_MS`](crate::LAUNCH_WINDOW_TIMEOUT_MS) (or
    /// exited first). The record is sent once this is known, with `issued_at` still the click.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub launch_ms: Option<u32>,
}

impl KioskGameLaunched {
    /// `game_id` is 1-64 of `A-Z a-z 0-9 _ -`; `name` is 1-80 characters with no control
    /// character; `launch_ms` is at most the window timeout. A failure is `VALIDATION_FAILED`
    /// naming the field.
    pub fn validate(&self) -> Result<(), crate::AppError> {
        use crate::{AppError, ErrorCode};
        if self
            .launch_ms
            .is_some_and(|ms| ms > crate::LAUNCH_WINDOW_TIMEOUT_MS)
        {
            return Err(AppError::with_field(
                ErrorCode::ValidationFailed,
                "launch_ms",
                "launch_ms must be within the 3 minute window",
            ));
        }
        let id_ok = (1..=MAX_GAME_ID_LEN).contains(&self.game_id.len())
            && self
                .game_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if !id_ok {
            return Err(AppError::with_field(
                ErrorCode::ValidationFailed,
                "game_id",
                "game_id must be 1-64 characters of a-z, A-Z, 0-9, '_' or '-'",
            ));
        }
        let len = self.name.chars().count();
        if len == 0 || len > crate::LIBRARY_NAME_MAX || self.name.chars().any(char::is_control) {
            return Err(AppError::with_field(
                ErrorCode::ValidationFailed,
                "name",
                "name must be 1-80 characters with no control characters",
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
    Unpaired,
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
    fn a_game_launch_names_a_game_and_a_name() {
        let ok = KioskGameLaunched {
            game_id: "01JABC-def_9".into(),
            name: "Valorant".into(),
            launch_ms: None,
        };
        assert!(ok.validate().is_ok());
        for (id, name, field) in [
            ("", "X", "game_id"),
            ("a b", "X", "game_id"),
            ("a/b", "X", "game_id"),
            ("a", "", "name"),
            ("a", "bad\nname", "name"),
        ] {
            let bad = KioskGameLaunched {
                game_id: id.into(),
                name: name.into(),
                launch_ms: None,
            };
            assert_eq!(bad.validate().unwrap_err().field.as_deref(), Some(field));
        }
        let long = KioskGameLaunched {
            game_id: "a".repeat(MAX_GAME_ID_LEN + 1),
            name: "X".into(),
            launch_ms: None,
        };
        assert!(long.validate().is_err());
    }

    #[test]
    fn a_launcher_open_names_steam_epic_or_riot() {
        for ok in OPENABLE_LAUNCHERS {
            assert!(
                KioskLauncherOpened {
                    launcher: ok.into()
                }
                .validate()
                .is_ok()
            );
        }
        for bad in ["", "Steam", "battlenet", "none", "steam ", "../steam"] {
            let err = KioskLauncherOpened {
                launcher: bad.into(),
            }
            .validate()
            .unwrap_err();
            assert_eq!(err.field.as_deref(), Some("launcher"), "{bad:?}");
        }
    }

    #[test]
    fn a_launch_time_is_within_the_three_minute_window_and_optional_on_the_wire() {
        let timed = |launch_ms| KioskGameLaunched {
            game_id: "g".into(),
            name: "X".into(),
            launch_ms,
        };
        assert!(timed(Some(14_200)).validate().is_ok());
        assert!(
            timed(Some(crate::LAUNCH_WINDOW_TIMEOUT_MS))
                .validate()
                .is_ok()
        );
        let late = timed(Some(crate::LAUNCH_WINDOW_TIMEOUT_MS + 1))
            .validate()
            .unwrap_err();
        assert_eq!(late.field.as_deref(), Some("launch_ms"));
        // A record from before Story 6.25 has no launch time.
        let old: KioskGameLaunched =
            serde_json::from_value(serde_json::json!({"game_id": "g", "name": "X"})).unwrap();
        assert_eq!(old.launch_ms, None);
    }

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
