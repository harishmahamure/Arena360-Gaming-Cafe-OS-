//! Player sign-up at the kiosk (Story 6.10): the paired-only LAN frame `player.signup` and its
//! answer `player.signup_result`.
//!
//! The Hub runs the Story 3.6 checks, hashes and seals exactly as the counter does, applies
//! `player.create` as the paired device and records the login, so the kiosk goes straight to the
//! plan picker. The kiosk never names a player id or a policy version. Credentials and personal
//! data travel only over the pinned TLS link and never appear in a log or a `Debug` print.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{AppError, ErrorCode};
use crate::player_login::PlayerContext;

/// The consent policy version the Hub stamps on a kiosk sign-up (the value of the
/// `player.policy_version` catalogue string). The kiosk never names it.
pub const PLAYER_POLICY_VERSION: &str = "1";

/// Frame limits, in bytes: bound what is read before the Story 3.6 checks run (the checks
/// themselves are in characters).
pub const MAX_SIGNUP_USERNAME_BYTES: usize = 128;
pub const MAX_SIGNUP_PASSWORD_BYTES: usize = 1024;
pub const MAX_SIGNUP_PHONE_BYTES: usize = 64;
pub const MAX_SIGNUP_NAME_BYTES: usize = 512;

/// `player.signup` (paired kiosk -> Hub): the fields a new player typed at the PC and the one
/// consent box. Never printed.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerSignup {
    pub username: String,
    pub password: String,
    pub phone: String,
    pub name: String,
    pub consent: bool,
}

impl std::fmt::Debug for PlayerSignup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerSignup")
            .field("username", &"[redacted]")
            .field("password", &"[redacted]")
            .field("phone", &"[redacted]")
            .field("name", &"[redacted]")
            .field("consent", &self.consent)
            .finish()
    }
}

impl PlayerSignup {
    /// The frame rules: every field within its byte limit. A failure is `BAD_FRAME` naming the
    /// field (never echoing its value). The field rules themselves are `PlayerForm::check`.
    pub fn validate(&self) -> Result<(), AppError> {
        for (field, value, max) in [
            ("username", &self.username, MAX_SIGNUP_USERNAME_BYTES),
            ("password", &self.password, MAX_SIGNUP_PASSWORD_BYTES),
            ("phone", &self.phone, MAX_SIGNUP_PHONE_BYTES),
            ("name", &self.name, MAX_SIGNUP_NAME_BYTES),
        ] {
            if value.len() > max {
                return Err(AppError::with_field(
                    ErrorCode::BadFrame,
                    field,
                    format!("{field} is too long"),
                ));
            }
        }
        Ok(())
    }
}

/// `player.signup_result` (Hub -> kiosk): the typed outcome of a `player.signup`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "outcome", rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum PlayerSignupResult {
    /// The player exists and is logged in on this PC.
    Ok { player: PlayerContext },
    /// Nothing was written: the code (`USERNAME_TAKEN`, `PHONE_TAKEN`, `VALIDATION_FAILED`, ...)
    /// and the field it concerns.
    Refused {
        code: ErrorCode,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        field: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{Frame, FrameBody, parse_frame};
    use serde_json::json;

    fn signup() -> PlayerSignup {
        PlayerSignup {
            username: "asha_9".into(),
            password: "hunter2-secret".into(),
            phone: "+919876543210".into(),
            name: "Asha Rao".into(),
            consent: true,
        }
    }

    #[test]
    fn personal_data_never_prints() {
        let shown = format!("{:?}", signup());
        for secret in ["asha_9", "hunter2", "9876543210", "Asha"] {
            assert!(!shown.contains(secret), "{shown}");
        }
        let frame = Frame::new("r", FrameBody::PlayerSignup(signup()));
        let shown = format!("{frame:?}");
        for secret in ["asha_9", "hunter2", "9876543210", "Asha"] {
            assert!(!shown.contains(secret), "{shown}");
        }
    }

    #[test]
    fn the_request_round_trips_flat() {
        let text = r#"{"v":1,"kind":"player.signup","rid":"a","username":"asha_9","password":"hunter2-secret","phone":"+919876543210","name":"Asha Rao","consent":true}"#;
        let f = parse_frame(text).unwrap();
        assert_eq!(f.body, FrameBody::PlayerSignup(signup()));
        assert_eq!(parse_frame(&f.to_text()).unwrap(), f);
    }

    #[test]
    fn each_outcome_is_a_flat_tagged_frame() {
        let cases = [
            (
                PlayerSignupResult::Ok {
                    player: PlayerContext {
                        player_id: "P1".into(),
                        name: "Asha".into(),
                    },
                },
                json!({"outcome": "ok", "player": {"player_id": "P1", "name": "Asha"}}),
            ),
            (
                PlayerSignupResult::Refused {
                    code: ErrorCode::UsernameTaken,
                    field: Some("username".into()),
                },
                json!({"outcome": "refused", "code": "USERNAME_TAKEN", "field": "username"}),
            ),
            (
                PlayerSignupResult::Refused {
                    code: ErrorCode::PermissionDenied,
                    field: None,
                },
                json!({"outcome": "refused", "code": "PERMISSION_DENIED"}),
            ),
        ];
        for (result, body) in cases {
            let frame = Frame::new("a", FrameBody::PlayerSignupResult(result));
            let v = serde_json::to_value(&frame).unwrap();
            assert_eq!(v["kind"], "player.signup_result");
            assert_eq!(v["rid"], "a");
            for (k, want) in body.as_object().unwrap() {
                assert_eq!(&v[k], want, "{k}");
            }
            assert_eq!(parse_frame(&frame.to_text()).unwrap(), frame);
        }
    }

    #[test]
    fn the_frame_bounds_every_field() {
        assert!(signup().validate().is_ok());
        let long = |n: usize| "x".repeat(n + 1);
        for (field, s) in [
            (
                "username",
                PlayerSignup {
                    username: long(MAX_SIGNUP_USERNAME_BYTES),
                    ..signup()
                },
            ),
            (
                "password",
                PlayerSignup {
                    password: long(MAX_SIGNUP_PASSWORD_BYTES),
                    ..signup()
                },
            ),
            (
                "phone",
                PlayerSignup {
                    phone: long(MAX_SIGNUP_PHONE_BYTES),
                    ..signup()
                },
            ),
            (
                "name",
                PlayerSignup {
                    name: long(MAX_SIGNUP_NAME_BYTES),
                    ..signup()
                },
            ),
        ] {
            let e = s.validate().unwrap_err();
            assert_eq!(e.code, ErrorCode::BadFrame);
            assert_eq!(e.field.as_deref(), Some(field));
        }
    }
}
