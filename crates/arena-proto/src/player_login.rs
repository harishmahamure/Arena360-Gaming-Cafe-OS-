//! Player login at the kiosk (Story 6.8): the paired-only LAN frame `player.authenticate` and its
//! answer `player.auth_result`.
//!
//! Authentication is a request/response frame, not an intent: a kiosk must not be able to assert
//! "player X is here", and the verdict (throttle, hash) is not a branch fact. The credentials
//! travel only over the pinned TLS link, are checked with Argon2id and never appear in a log or a
//! `Debug` print.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{AppError, ErrorCode};

/// Longest username a `player.authenticate` may carry, in bytes (a normalised username is at most
/// 32; the rest is slack for what a player types before it is normalised).
pub const MAX_LOGIN_USERNAME_BYTES: usize = 128;
/// Longest password a `player.authenticate` may carry, in bytes (bounds the Argon2id input).
pub const MAX_LOGIN_PASSWORD_BYTES: usize = 1024;
/// Consecutive wrong logins from one kiosk before it is paused.
pub const LOGIN_MAX_FAILURES: u32 = 5;
/// How long a kiosk is paused after [`LOGIN_MAX_FAILURES`], in seconds.
pub const LOGIN_LOCK_SECONDS: u32 = 60;
/// The `kiosk.log` code of a correct player login (detail `player <player_id>`).
pub const PLAYER_LOGIN_CODE: &str = "kiosk.player_login";

/// `player.authenticate` (paired kiosk → Hub): a username and password a player typed at the PC.
/// Never printed.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerAuthenticate {
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for PlayerAuthenticate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerAuthenticate")
            .field("username", &"[redacted]")
            .field("password", &"[redacted]")
            .finish()
    }
}

impl PlayerAuthenticate {
    /// The frame rules: both fields present and within their byte limits. A failure is
    /// `BAD_FRAME` naming the field (never echoing its value).
    pub fn validate(&self) -> Result<(), AppError> {
        let bad = |field: &str, message: &str| {
            Err(AppError::with_field(ErrorCode::BadFrame, field, message))
        };
        if self.username.is_empty() || self.username.len() > MAX_LOGIN_USERNAME_BYTES {
            return bad("username", "username must be 1–128 bytes");
        }
        if self.password.is_empty() || self.password.len() > MAX_LOGIN_PASSWORD_BYTES {
            return bad("password", "password must be 1–1024 bytes");
        }
        Ok(())
    }
}

/// Who logged in: the player id and the name the player gave (shown as "Logged in as <name>").
/// The kiosk holds this in memory only; it is not a session and grants nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerContext {
    pub player_id: String,
    pub name: String,
}

/// `player.auth_result` (Hub → kiosk): the typed outcome of a `player.authenticate`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "outcome", rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum PlayerAuthResult {
    /// The credentials are right.
    Ok { player: PlayerContext },
    /// Unknown username, deleted player, imported player with no password, or wrong password: one
    /// answer for all of them.
    Rejected,
    /// This kiosk is paused after too many wrong tries; no hash was checked.
    Locked { retry_s: u32 },
    /// The player already has a session on another PC (its label).
    Busy { device_name: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{Frame, FrameBody, parse_frame};
    use serde_json::json;

    fn auth(username: &str, password: &str) -> PlayerAuthenticate {
        PlayerAuthenticate {
            username: username.into(),
            password: password.into(),
        }
    }

    #[test]
    fn the_credentials_never_print() {
        let a = auth("asha", "hunter2-secret");
        let shown = format!("{a:?}");
        assert!(
            !shown.contains("hunter2") && !shown.contains("asha"),
            "{shown}"
        );
        let frame = Frame::new("r", FrameBody::PlayerAuthenticate(a));
        let shown = format!("{frame:?}");
        assert!(
            !shown.contains("hunter2") && !shown.contains("asha"),
            "{shown}"
        );
    }

    #[test]
    fn the_request_round_trips_flat() {
        let text =
            r#"{"v":1,"kind":"player.authenticate","rid":"a","username":"asha","password":"pw"}"#;
        let f = parse_frame(text).unwrap();
        assert_eq!(f.body, FrameBody::PlayerAuthenticate(auth("asha", "pw")));
        assert_eq!(parse_frame(&f.to_text()).unwrap(), f);
    }

    #[test]
    fn each_outcome_is_a_flat_tagged_frame() {
        let cases = [
            (
                PlayerAuthResult::Ok {
                    player: PlayerContext {
                        player_id: "P1".into(),
                        name: "Asha".into(),
                    },
                },
                json!({"outcome": "ok", "player": {"player_id": "P1", "name": "Asha"}}),
            ),
            (PlayerAuthResult::Rejected, json!({"outcome": "rejected"})),
            (
                PlayerAuthResult::Locked { retry_s: 60 },
                json!({"outcome": "locked", "retry_s": 60}),
            ),
            (
                PlayerAuthResult::Busy {
                    device_name: "PC 3".into(),
                },
                json!({"outcome": "busy", "device_name": "PC 3"}),
            ),
        ];
        for (result, body) in cases {
            let frame = Frame::new("a", FrameBody::PlayerAuthResult(result));
            let v = serde_json::to_value(&frame).unwrap();
            assert_eq!(v["kind"], "player.auth_result");
            assert_eq!(v["rid"], "a");
            for (k, want) in body.as_object().unwrap() {
                assert_eq!(&v[k], want, "{k}");
            }
            assert_eq!(parse_frame(&frame.to_text()).unwrap(), frame);
        }
    }

    #[test]
    fn validation_bounds_both_fields() {
        assert!(auth("asha", "pw").validate().is_ok());
        for (u, p, field) in [
            ("", "pw", "username"),
            ("asha", "", "password"),
            (&"u".repeat(MAX_LOGIN_USERNAME_BYTES + 1), "pw", "username"),
            (
                "asha",
                &"p".repeat(MAX_LOGIN_PASSWORD_BYTES + 1),
                "password",
            ),
        ] {
            let e = auth(u, p).validate().unwrap_err();
            assert_eq!(e.code, ErrorCode::BadFrame);
            assert_eq!(e.field.as_deref(), Some(field));
            assert!(!e.message.contains("pw"), "{}", e.message);
        }
        assert!(
            auth("a", &"p".repeat(MAX_LOGIN_PASSWORD_BYTES))
                .validate()
                .is_ok()
        );
    }
}
