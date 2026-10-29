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

/// Longest single play a `game.play_recorded` may carry, in minutes (7 days: the most-played
/// window, so one record can never outweigh it).
pub const MAX_PLAY_MINUTES: i64 = 7 * 24 * 60;

/// How many games the most-played list holds (Story 6.27).
pub const MOST_PLAYED_LIMIT: usize = 10;

/// How many games' media `games.most_played_result` carries (Story 6.29).
pub const MAX_GAME_MEDIA: usize = 64;

/// The most-played window, in days (Story 6.27).
pub const MOST_PLAYED_DAYS: i64 = 7;

/// The payload of `game.play_recorded` (Story 6.27): a game started at this PC stopped running
/// (its last process exited, or the session ended). As with `game.launched`, the Hub names the
/// player and session itself; the kiosk never does. Times are Unix milliseconds on the kiosk's
/// clock; `minutes` is the played time in whole minutes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KioskGamePlayRecorded {
    /// The library entry id of the game.
    pub game_id: String,
    /// When the game was started.
    #[ts(type = "number")]
    pub started_at: i64,
    /// When it stopped running.
    #[ts(type = "number")]
    pub ended_at: i64,
    /// The played time in whole minutes (rounded to the nearest).
    #[ts(type = "number")]
    pub minutes: i64,
}

impl KioskGamePlayRecorded {
    /// `game_id` is 1-64 of `A-Z a-z 0-9 _ -`; `started_at` is positive and not after `ended_at`;
    /// `minutes` is within [`MAX_PLAY_MINUTES`] and agrees with the two times to within a minute.
    /// A failure is `VALIDATION_FAILED` naming the field.
    pub fn validate(&self) -> Result<(), crate::AppError> {
        use crate::{AppError, ErrorCode};
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
        if self.started_at <= 0 {
            return Err(AppError::with_field(
                ErrorCode::ValidationFailed,
                "started_at",
                "started_at must be a positive time",
            ));
        }
        if self.ended_at < self.started_at {
            return Err(AppError::with_field(
                ErrorCode::ValidationFailed,
                "ended_at",
                "ended_at must not be before started_at",
            ));
        }
        if !(0..=MAX_PLAY_MINUTES).contains(&self.minutes) {
            return Err(AppError::with_field(
                ErrorCode::ValidationFailed,
                "minutes",
                "minutes must be within 0 and 7 days",
            ));
        }
        let span_ms = self.ended_at - self.started_at;
        if (self.minutes * 60_000 - span_ms).abs() > 60_000 {
            return Err(AppError::with_field(
                ErrorCode::ValidationFailed,
                "minutes",
                "minutes must match the time between started_at and ended_at",
            ));
        }
        Ok(())
    }
}

/// One game of the most-played list (Story 6.27): the branch's minutes played in the window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct MostPlayedGame {
    pub game_id: String,
    #[ts(type = "number")]
    pub minutes: i64,
}

/// When the live player last played a game (Story 6.27), in any window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LastPlayed {
    pub game_id: String,
    /// When that play ended, in Unix milliseconds.
    #[ts(type = "number")]
    pub at: i64,
}

/// `games.most_played_result` (Hub -> kiosk, Story 6.27): the branch's top games by minutes
/// played over the last 7 days and the live player's own last-played times. It names no other
/// player.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct MostPlayed {
    /// At most [`MOST_PLAYED_LIMIT`] games with minutes above zero, most minutes first (ties by
    /// game id).
    pub top: Vec<MostPlayedGame>,
    /// The live player's last play of each game, newest first; empty for a guest or no session.
    pub last_played: Vec<LastPlayed>,
    /// The stored video and cover of the library games that have either (Story 6.29), for the
    /// Home hero and featured row; at most [`MAX_GAME_MEDIA`]. Empty when the Hub holds none.
    #[serde(default)]
    pub media: Vec<GameMediaRef>,
}

/// The stored media of one library game (Story 6.29): content hashes the kiosk fetches from the
/// Hub as `/media/<sha256>`. `game_id` is the library entry's id, as in [`MostPlayedGame`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GameMediaRef {
    pub game_id: String,
    pub video: Option<String>,
    pub cover: Option<String>,
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
    fn a_play_record_names_a_game_and_agrees_with_its_times() {
        let play = |game_id: &str, started_at, ended_at, minutes| KioskGamePlayRecorded {
            game_id: game_id.into(),
            started_at,
            ended_at,
            minutes,
        };
        assert!(
            play("01JABC-def_9", 1_000, 1_000 + 25 * 60_000, 25)
                .validate()
                .is_ok()
        );
        // Rounding either way is accepted; a zero-minute play is a play.
        assert!(
            play("g", 1_000, 1_000 + 25 * 60_000 + 20_000, 25)
                .validate()
                .is_ok()
        );
        assert!(
            play("g", 1_000, 1_000 + 25 * 60_000 - 20_000, 25)
                .validate()
                .is_ok()
        );
        assert!(play("g", 1_000, 1_000 + 20_000, 0).validate().is_ok());
        let too_long = play(
            "g",
            1_000,
            1_000 + (MAX_PLAY_MINUTES + 1) * 60_000,
            MAX_PLAY_MINUTES + 1,
        );
        for (p, field) in [
            (play("", 1_000, 61_000, 1), "game_id"),
            (play("a b", 1_000, 61_000, 1), "game_id"),
            (play(&"g".repeat(65), 1_000, 61_000, 1), "game_id"),
            (play("g", 0, 61_000, 1), "started_at"),
            (play("g", 2_000, 1_000, 0), "ended_at"),
            (play("g", 1_000, 61_000, -1), "minutes"),
            (play("g", 1_000, 61_000, 5), "minutes"),
            (too_long, "minutes"),
        ] {
            let err = p.validate().unwrap_err();
            assert_eq!(err.field.as_deref(), Some(field), "{p:?}");
        }
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

    #[test]
    fn the_most_played_frames_round_trip_and_name_no_other_player() {
        use crate::frame::{Frame, FrameBody, parse_frame};
        let ask = parse_frame(r#"{"v":1,"kind":"games.most_played","rid":"m"}"#).unwrap();
        assert_eq!(ask.body, FrameBody::GamesMostPlayed);
        let reply = Frame::new(
            "m",
            FrameBody::GamesMostPlayedResult(MostPlayed {
                top: vec![MostPlayedGame {
                    game_id: "G1".into(),
                    minutes: 90,
                }],
                last_played: vec![LastPlayed {
                    game_id: "G1".into(),
                    at: 5,
                }],
                media: vec![GameMediaRef {
                    game_id: "G1".into(),
                    video: None,
                    cover: Some("ab".repeat(32)),
                }],
            }),
        );
        let v: serde_json::Value = serde_json::from_str(&reply.to_text()).unwrap();
        assert_eq!(v["kind"], "games.most_played_result");
        assert_eq!(
            v["top"][0],
            serde_json::json!({"game_id": "G1", "minutes": 90})
        );
        assert_eq!(
            v["last_played"][0],
            serde_json::json!({"game_id": "G1", "at": 5})
        );
        assert_eq!(
            v["media"][0],
            serde_json::json!({"game_id": "G1", "video": null, "cover": "ab".repeat(32)})
        );
        assert_eq!(parse_frame(&reply.to_text()).unwrap(), reply);
        let empty = serde_json::to_value(MostPlayed::default()).unwrap();
        assert_eq!(
            empty,
            serde_json::json!({"top": [], "last_played": [], "media": []})
        );
        // A Hub that predates Story 6.29 sends no media.
        let old: MostPlayed = serde_json::from_str(r#"{"top":[],"last_played":[]}"#).unwrap();
        assert!(old.media.is_empty());
    }
}
