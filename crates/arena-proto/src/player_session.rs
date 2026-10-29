//! The plan picker (Story 6.9): the paired-only LAN frames a logged-in player's kiosk uses to see
//! the options valid for its PC and to start a prepaid session.
//!
//! - `player.plans{}` -> `player.plans_result{wallet, options[]}`.
//! - `player.start_session{choice}` -> `player.start_result{outcome}`.
//! - `player.logout{}` -> `ack`.
//!
//! None of them names a player: after a correct `player.authenticate` the Hub remembers who
//! logged in on that device for a short while and builds the intent itself, with the paired device
//! as actor and the player on the session row.
//!
//! The account menu (Story 6.17) adds three more, for the player on a live session; they name no
//! player either, the Hub reads it from the session on that device:
//!
//! - `player.account{}` -> `player.account_result{balance_minor, packs[]}`.
//! - `player.end_session{}` -> `player.end_result{outcome}` (Log out).
//! - `player.unlock{password}` -> `player.unlock_result{outcome}` (the player's own lock screen).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{AppError, ErrorCode};
use crate::snapshot::SnapshotActiveRule;

/// How long the Hub remembers a login on a device after its last `player.plans` /
/// `player.start_session`, in seconds.
pub const PLAYER_LOGIN_RECORD_SECONDS: u64 = 120;
/// The `error` message the Hub answers when a device has no (or an expired) login record. The
/// kiosk treats it as "log in again".
pub const PLAYER_LOGIN_REQUIRED: &str = "player login required";
/// Most options one `player.plans_result` carries.
pub const MAX_PLAN_OPTIONS: usize = 200;

/// What the player picked to pay for the session. No postpaid: the kiosk never starts one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "mode", rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum PlayerChoice {
    /// Minutes off an owned pack.
    Pack { pack_id: String },
    /// An owned pass, inside its window.
    Pass { pack_id: String },
    /// The wallet, at the group's hourly rate.
    Wallet,
    /// A block plan, paid from the wallet in the same intent.
    Block { plan_id: String },
}

/// Why an option cannot be chosen now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum PlanTag {
    /// A pack or pass past its expiry, or a pack with no time left.
    Expired,
    /// A pass or block whose play window is closed now.
    OutsideWindow,
}

/// One card of the plan picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlanOption {
    pub choice: PlayerChoice,
    /// The pack, pass or block name; empty for the wallet.
    pub name: String,
    pub selectable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tag: Option<PlanTag>,
    /// Minutes left on a pack (the unused minutes of an expired one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "number | null", optional)]
    pub minutes_left: Option<i64>,
    /// When a pack or pass expires (epoch ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "number | null", optional)]
    pub expires_at: Option<i64>,
    /// The play window of a pass or block, `22:00–06:00`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub window: Option<String>,
    /// The end of a block's window today (`06:00`), when the window is open now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ends_label: Option<String>,
    /// A block's price in minor units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "number | null", optional)]
    pub price_minor: Option<i64>,
}

/// The wallet card: what the player can spend and the hourly rate it buys time at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerWallet {
    /// The balance less what the player's running wallet sessions have set aside.
    #[ts(type = "number")]
    pub balance_minor: i64,
    /// This PC's group rate per hour now (the active rule's when one applies).
    #[ts(type = "number")]
    pub rate_minor: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub rule: Option<SnapshotActiveRule>,
}

/// `player.plans_result` (Hub -> kiosk): the options valid for this PC's group, in order packs,
/// passes, blocks, then the wallet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerPlansResult {
    /// `None` when this PC's group has no rate (no wallet option is offered).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub wallet: Option<PlayerWallet>,
    pub options: Vec<PlanOption>,
}

/// `player.start_session` (paired kiosk -> Hub): start the chosen option for the player who
/// logged in on this device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerStartSession {
    pub choice: PlayerChoice,
}

/// `player.start_result` (Hub -> kiosk): whether the Hub started the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "outcome", rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum PlayerStartResult {
    /// The session started; `session.started` follows on the same connection.
    Ok,
    /// `apply()` refused: its code (`INSUFFICIENT_BALANCE`, `PACK_EXPIRED`, ...) and message.
    Refused { code: ErrorCode, message: String },
}

/// One owned pack or pass the account menu lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AccountPack {
    pub name: String,
    /// Minutes left on a pack; `None` for a pass, which has a window and no hours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "number | null", optional)]
    pub minutes_left: Option<i64>,
    /// When it expires (epoch ms).
    #[ts(type = "number")]
    pub expires_at: i64,
}

/// `player.account_result` (Hub -> kiosk): what the profiled player on this PC holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerAccountResult {
    /// The wallet balance in minor units, less what running wallet sessions have set aside.
    #[ts(type = "number")]
    pub balance_minor: i64,
    /// Packs and passes valid for this PC's group and not expired, soonest expiry first.
    pub packs: Vec<AccountPack>,
}

/// `player.end_result` (Hub -> kiosk): whether the Hub ended the player's own session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "outcome", rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum PlayerEndResult {
    /// The session ended; the lock follows as a `pc.command` on the same connection.
    Ok,
    /// `apply()` refused: its code and message.
    Refused { code: ErrorCode, message: String },
}

/// `player.unlock` (paired kiosk -> Hub): the password a player typed on their own lock screen.
/// Never printed.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerUnlock {
    pub password: String,
}

impl std::fmt::Debug for PlayerUnlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerUnlock")
            .field("password", &"[redacted]")
            .finish()
    }
}

impl PlayerUnlock {
    /// The frame rules: a password of 1–1024 bytes. A failure is `BAD_FRAME` naming the field
    /// (never echoing its value).
    pub fn validate(&self) -> Result<(), AppError> {
        if self.password.is_empty() || self.password.len() > crate::MAX_LOGIN_PASSWORD_BYTES {
            return Err(AppError::with_field(
                ErrorCode::BadFrame,
                "password",
                "password must be 1–1024 bytes",
            ));
        }
        Ok(())
    }
}

/// `player.unlock_result` (Hub -> kiosk): the typed outcome of a `player.unlock`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "outcome", rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum PlayerUnlockResult {
    /// The player's own password; the kiosk lifts its lock.
    Ok,
    /// A wrong password (also: no profiled player on this PC's session).
    Rejected,
    /// This PC's field is paused after too many wrong tries; no hash was checked.
    Locked { retry_s: u32 },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{Frame, FrameBody, parse_frame};
    use serde_json::json;

    #[test]
    fn the_account_frames_round_trip() {
        for (text, body) in [
            (
                r#"{"v":1,"kind":"player.account","rid":"a"}"#,
                FrameBody::PlayerAccount,
            ),
            (
                r#"{"v":1,"kind":"player.end_session","rid":"b"}"#,
                FrameBody::PlayerEndSession,
            ),
            (
                r#"{"v":1,"kind":"player.unlock","rid":"c","password":"pw"}"#,
                FrameBody::PlayerUnlock(PlayerUnlock {
                    password: "pw".into(),
                }),
            ),
        ] {
            let f = parse_frame(text).unwrap();
            assert_eq!(f.body, body);
            assert_eq!(parse_frame(&f.to_text()).unwrap(), f);
        }
        let account = Frame::new(
            "a",
            FrameBody::PlayerAccountResult(PlayerAccountResult {
                balance_minor: 25_000,
                packs: vec![AccountPack {
                    name: "10 hours".into(),
                    minutes_left: Some(90),
                    expires_at: 7,
                }],
            }),
        );
        let v = serde_json::to_value(&account).unwrap();
        assert_eq!(v["kind"], "player.account_result");
        assert_eq!(v["packs"][0]["minutes_left"], 90);
        assert_eq!(parse_frame(&account.to_text()).unwrap(), account);
        for (kind, body) in [
            (
                "player.end_result",
                FrameBody::PlayerEndResult(PlayerEndResult::Refused {
                    code: ErrorCode::Stale,
                    message: "x".into(),
                }),
            ),
            (
                "player.unlock_result",
                FrameBody::PlayerUnlockResult(PlayerUnlockResult::Locked { retry_s: 60 }),
            ),
        ] {
            let f = Frame::new("r", body);
            assert_eq!(serde_json::to_value(&f).unwrap()["kind"], kind);
            assert_eq!(parse_frame(&f.to_text()).unwrap(), f);
        }
    }

    #[test]
    fn an_unlock_password_is_bounded_and_never_printed() {
        let unlock = |p: &str| PlayerUnlock {
            password: p.to_owned(),
        };
        assert!(unlock("x").validate().is_ok());
        for bad in ["", &"x".repeat(1025)] {
            assert_eq!(
                unlock(bad).validate().unwrap_err().code,
                ErrorCode::BadFrame
            );
        }
        assert!(!format!("{:?}", unlock("hunter2")).contains("hunter2"));
    }

    #[test]
    fn each_choice_is_a_flat_tagged_object() {
        let cases = [
            (
                PlayerChoice::Pack {
                    pack_id: "K1".into(),
                },
                json!({"mode": "pack", "pack_id": "K1"}),
            ),
            (
                PlayerChoice::Pass {
                    pack_id: "K2".into(),
                },
                json!({"mode": "pass", "pack_id": "K2"}),
            ),
            (PlayerChoice::Wallet, json!({"mode": "wallet"})),
            (
                PlayerChoice::Block {
                    plan_id: "B1".into(),
                },
                json!({"mode": "block", "plan_id": "B1"}),
            ),
        ];
        for (choice, want) in cases {
            assert_eq!(serde_json::to_value(&choice).unwrap(), want);
            assert_eq!(
                serde_json::from_value::<PlayerChoice>(want).unwrap(),
                choice
            );
        }
        // Postpaid is not a choice a kiosk can make.
        assert!(serde_json::from_value::<PlayerChoice>(json!({"mode": "postpaid"})).is_err());
    }

    #[test]
    fn the_request_frames_round_trip() {
        let text = r#"{"v":1,"kind":"player.plans","rid":"a"}"#;
        let f = parse_frame(text).unwrap();
        assert_eq!(f.body, FrameBody::PlayerPlans);
        assert_eq!(parse_frame(&f.to_text()).unwrap(), f);

        let text = r#"{"v":1,"kind":"player.start_session","rid":"b","choice":{"mode":"block","plan_id":"B1"}}"#;
        let f = parse_frame(text).unwrap();
        assert_eq!(
            f.body,
            FrameBody::PlayerStartSession(PlayerStartSession {
                choice: PlayerChoice::Block {
                    plan_id: "B1".into()
                }
            })
        );
        assert_eq!(parse_frame(&f.to_text()).unwrap(), f);

        let text = r#"{"v":1,"kind":"player.logout","rid":"c"}"#;
        let f = parse_frame(text).unwrap();
        assert_eq!(f.body, FrameBody::PlayerLogout);
        assert_eq!(parse_frame(&f.to_text()).unwrap(), f);
    }

    #[test]
    fn the_results_round_trip() {
        let result = PlayerPlansResult {
            wallet: Some(PlayerWallet {
                balance_minor: 4_000,
                rate_minor: 6_000,
                rule: None,
            }),
            options: vec![PlanOption {
                choice: PlayerChoice::Pack {
                    pack_id: "K1".into(),
                },
                name: "10 hours".into(),
                selectable: false,
                tag: Some(PlanTag::Expired),
                minutes_left: Some(45),
                expires_at: Some(1),
                window: None,
                ends_label: None,
                price_minor: None,
            }],
        };
        let frame = Frame::new("p", FrameBody::PlayerPlansResult(result));
        let v = serde_json::to_value(&frame).unwrap();
        assert_eq!(v["kind"], "player.plans_result");
        assert_eq!(v["options"][0]["tag"], "expired");
        assert_eq!(v["options"][0]["choice"]["mode"], "pack");
        assert_eq!(v["wallet"]["balance_minor"], 4_000);
        assert_eq!(parse_frame(&frame.to_text()).unwrap(), frame);

        for (outcome, want) in [
            (PlayerStartResult::Ok, json!({"outcome": "ok"})),
            (
                PlayerStartResult::Refused {
                    code: ErrorCode::InsufficientBalance,
                    message: "low".into(),
                },
                json!({"outcome": "refused", "code": "INSUFFICIENT_BALANCE", "message": "low"}),
            ),
        ] {
            let frame = Frame::new("s", FrameBody::PlayerStartResult(outcome));
            let v = serde_json::to_value(&frame).unwrap();
            assert_eq!(v["kind"], "player.start_result");
            for (k, w) in want.as_object().unwrap() {
                assert_eq!(&v[k], w, "{k}");
            }
            assert_eq!(parse_frame(&frame.to_text()).unwrap(), frame);
        }
    }
}
