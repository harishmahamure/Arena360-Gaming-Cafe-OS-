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
//!
//! The Account page (Story 6.34) adds two more, again naming no player:
//!
//! - `player.account_page{}` -> `player.account_page_result{name, username, phone_masked, ..}`.
//! - `player.password_change{current, new}` -> `player.password_change_result{outcome}`.
//!
//! Extend (Story 6.21) adds two for a prepaid player on a live session, again naming no player and
//! no session:
//!
//! - `player.extend_options{}` -> `player.extend_options_result{quotes, wallet_minor, packs, ..}`.
//! - `player.extend{minutes, source, pack_id?}` -> `player.extend_result{outcome}`.

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

/// One pack or pass on the Account page's Wallet & packs tab (Story 6.34), expired ones too.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AccountPackEntry {
    pub name: String,
    /// The names of the device groups it is valid for.
    pub groups: Vec<String>,
    /// Minutes left on a pack; `None` for a pass, which has a window and no hours. On an expired
    /// pack these are the unused minutes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "number | null", optional)]
    pub minutes_left: Option<i64>,
    /// A pass's daily window as minutes since midnight (start, end); `None` for a pack or an
    /// all-day pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "[number, number] | null", optional)]
    pub window_min: Option<(i64, i64)>,
    /// When it expires (epoch ms).
    #[ts(type = "number")]
    pub expires_at: i64,
    pub expired: bool,
}

/// One item of an order on the Account page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AccountOrderItem {
    pub name: String,
    #[ts(type = "number")]
    pub qty: i64,
}

/// One of the player's orders at this branch (Story 6.34).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AccountOrder {
    pub order_id: String,
    /// When it was placed (epoch ms).
    #[ts(type = "number")]
    pub at: i64,
    pub items: Vec<AccountOrderItem>,
    #[ts(type = "number")]
    pub total_minor: i64,
    /// `open` (not paid yet), `settled` or `void`.
    pub status: String,
    /// The tax invoice number, once one was issued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "string | null", optional)]
    pub invoice_no: Option<String>,
}

/// `player.account_page_result` (Hub -> kiosk): what the Account page shows for the profiled
/// player on this PC (Story 6.34). The phone is masked at the Hub: the kiosk never holds it whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerAccountPage {
    pub name: String,
    pub username: String,
    /// The phone with all but its last 4 digits masked.
    pub phone_masked: String,
    /// The wallet balance in minor units, less what running wallet sessions have set aside.
    #[ts(type = "number")]
    pub balance_minor: i64,
    /// Owned packs and passes, unexpired first (soonest expiry first), then expired ones.
    pub packs: Vec<AccountPackEntry>,
    /// The player's orders at this branch, newest first.
    pub orders: Vec<AccountOrder>,
}

/// How many orders the Account page lists.
pub const ACCOUNT_ORDER_LIMIT: usize = 50;

/// `player.password_change` (paired kiosk -> Hub): the current password and the new one. Never
/// printed.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerChangePassword {
    pub current: String,
    pub new: String,
}

impl std::fmt::Debug for PlayerChangePassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerChangePassword")
            .field("current", &"[redacted]")
            .field("new", &"[redacted]")
            .finish()
    }
}

impl PlayerChangePassword {
    /// The frame rules: both fields 1–1024 bytes. (The new password's 6–128 characters is an
    /// answer, `invalid_new`, not a bad frame.) A failure is `BAD_FRAME` naming the field, never
    /// echoing its value.
    pub fn validate(&self) -> Result<(), AppError> {
        for (field, value) in [("current", &self.current), ("new", &self.new)] {
            if value.is_empty() || value.len() > crate::MAX_LOGIN_PASSWORD_BYTES {
                return Err(AppError::with_field(
                    ErrorCode::BadFrame,
                    field,
                    "password must be 1–1024 bytes",
                ));
            }
        }
        Ok(())
    }
}

/// `player.password_change_result` (Hub -> kiosk): the typed outcome of a password change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "outcome", rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum PlayerPasswordChangeResult {
    /// The new password is stored.
    Ok,
    /// The current password is wrong (also: no profiled player on this PC's session).
    WrongCurrent,
    /// The new password is not 6 to 128 characters.
    InvalidNew,
    /// This PC's field is paused after too many wrong tries; no hash was checked.
    Locked { retry_s: u32 },
    /// `apply()` refused: its code and message.
    Refused { code: ErrorCode, message: String },
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

/// One priced step of `extend-options`: `minutes` more, quoted by the Hub at the rate that
/// applies now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ExtendQuote {
    #[ts(type = "number")]
    pub minutes: i64,
    /// What `minutes` cost at the live rate (wallet and credit), in minor units.
    #[ts(type = "number")]
    pub price_minor: i64,
}

/// A pack the player can pay an extend from: valid for this PC's group, unexpired, with time left.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ExtendPackSource {
    pub pack_id: String,
    pub name: String,
    #[ts(type = "number")]
    pub minutes_left: i64,
}

/// `player.extend_options_result` (Hub -> kiosk): what the player on this PC can add and pay with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerExtendOptions {
    /// +30 min, +1 h and +2 h, priced at the group's live rate.
    pub quotes: Vec<ExtendQuote>,
    /// The wallet balance less what running wallet sessions have set aside.
    #[ts(type = "number")]
    pub wallet_minor: i64,
    /// Packs valid for this PC's group, soonest expiry first.
    pub packs: Vec<ExtendPackSource>,
    /// What credit still allows (limit less what is owed); `None` when credit is not enabled for
    /// this player.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "number | null", optional)]
    pub credit_headroom_minor: Option<i64>,
    /// When the branch next closes, as the branch writes the time (`11:00 pm`); `None` when it
    /// keeps no opening hours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub closing_label: Option<String>,
}

/// `player.extend` (paired kiosk -> Hub): add `minutes` to the session on this PC, paid from
/// `source`. It names no session and no player; a pack source names the pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerExtend {
    #[ts(type = "number")]
    pub minutes: i64,
    pub source: crate::ExtendSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pack_id: Option<String>,
}

impl PlayerExtend {
    /// The frame rules: one of the offered steps, and a pack id exactly when the source is a
    /// pack. A failure is `BAD_FRAME` naming the field.
    pub fn validate(&self) -> Result<(), AppError> {
        if !crate::EXTEND_OPTIONS_MIN.contains(&self.minutes) {
            return Err(AppError::with_field(
                ErrorCode::BadFrame,
                "minutes",
                "extend by 30 min, 1 h or 2 h",
            ));
        }
        let pack = self.source == crate::ExtendSource::Pack;
        let named = self
            .pack_id
            .as_deref()
            .is_some_and(|p| !p.is_empty() && p.len() <= 64);
        if pack != named {
            return Err(AppError::with_field(
                ErrorCode::BadFrame,
                "pack_id",
                "only a pack source names a pack",
            ));
        }
        Ok(())
    }
}

/// `player.extend_result` (Hub -> kiosk): whether the Hub added the time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "outcome", rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum PlayerExtendResult {
    /// The time was added; the new end reaches the kiosk as the next session tick.
    Ok,
    /// `apply()` refused (`PAST_CLOSING`, `CREDIT_LIMIT`, `INSUFFICIENT_BALANCE`, ...): its code
    /// and message.
    Refused { code: ErrorCode, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{Frame, FrameBody, parse_frame};
    use serde_json::json;

    #[test]
    fn the_extend_frames_round_trip_and_name_no_session() {
        for (text, body) in [
            (
                r#"{"v":1,"kind":"player.extend_options","rid":"a"}"#,
                FrameBody::PlayerExtendOptions,
            ),
            (
                r#"{"v":1,"kind":"player.extend","rid":"b","minutes":60,"source":"wallet"}"#,
                FrameBody::PlayerExtend(PlayerExtend {
                    minutes: 60,
                    source: crate::ExtendSource::Wallet,
                    pack_id: None,
                }),
            ),
            (
                r#"{"v":1,"kind":"player.extend","rid":"c","minutes":30,"source":"pack","pack_id":"K1"}"#,
                FrameBody::PlayerExtend(PlayerExtend {
                    minutes: 30,
                    source: crate::ExtendSource::Pack,
                    pack_id: Some("K1".into()),
                }),
            ),
        ] {
            let f = parse_frame(text).unwrap();
            assert_eq!(f.body, body);
            assert_eq!(parse_frame(&f.to_text()).unwrap(), f);
        }
        let options = Frame::new(
            "a",
            FrameBody::PlayerExtendOptionsResult(PlayerExtendOptions {
                quotes: vec![ExtendQuote {
                    minutes: 30,
                    price_minor: 5_000,
                }],
                wallet_minor: 4_000,
                packs: vec![ExtendPackSource {
                    pack_id: "K1".into(),
                    name: "10 hours".into(),
                    minutes_left: 90,
                }],
                credit_headroom_minor: Some(20_000),
                closing_label: Some("11:00 pm".into()),
            }),
        );
        let v = serde_json::to_value(&options).unwrap();
        assert_eq!(v["kind"], "player.extend_options_result");
        assert_eq!(v["quotes"][0]["price_minor"], 5_000);
        assert_eq!(parse_frame(&options.to_text()).unwrap(), options);
        let refused = Frame::new(
            "b",
            FrameBody::PlayerExtendResult(PlayerExtendResult::Refused {
                code: ErrorCode::PastClosing,
                message: "x".into(),
            }),
        );
        assert_eq!(
            serde_json::to_value(&refused).unwrap()["kind"],
            "player.extend_result"
        );
        assert_eq!(parse_frame(&refused.to_text()).unwrap(), refused);
    }

    #[test]
    fn an_extend_names_a_step_and_a_pack_only_for_a_pack_source() {
        let extend = |minutes, source, pack: Option<&str>| PlayerExtend {
            minutes,
            source,
            pack_id: pack.map(str::to_owned),
        };
        use crate::ExtendSource::{Credit, Pack, Wallet};
        assert!(extend(30, Wallet, None).validate().is_ok());
        assert!(extend(120, Credit, None).validate().is_ok());
        assert!(extend(60, Pack, Some("K")).validate().is_ok());
        for bad in [
            extend(45, Wallet, None),
            extend(60, Pack, None),
            extend(60, Pack, Some("")),
            extend(60, Wallet, Some("K")),
            extend(60, Pack, Some(&"x".repeat(65))),
        ] {
            assert_eq!(bad.validate().unwrap_err().code, ErrorCode::BadFrame);
        }
    }

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
    fn the_account_page_frames_round_trip() {
        let req = parse_frame(r#"{"v":1,"kind":"player.account_page","rid":"a"}"#).unwrap();
        assert_eq!(req.body, FrameBody::PlayerAccountPageRequest);
        let page = Frame::new(
            "a",
            FrameBody::PlayerAccountPageResult(PlayerAccountPage {
                name: "Asha Rao".into(),
                username: "asha".into(),
                phone_masked: "+91••••••3210".into(),
                balance_minor: 25_000,
                packs: vec![AccountPackEntry {
                    name: "Night pass".into(),
                    groups: vec!["PC".into()],
                    minutes_left: None,
                    window_min: Some((1_320, 360)),
                    expires_at: 7,
                    expired: false,
                }],
                orders: vec![AccountOrder {
                    order_id: "O1".into(),
                    at: 5,
                    items: vec![AccountOrderItem {
                        name: "Burger".into(),
                        qty: 2,
                    }],
                    total_minor: 24_000,
                    status: "settled".into(),
                    invoice_no: Some("MUM/26-27/E1/1".into()),
                }],
            }),
        );
        let v = serde_json::to_value(&page).unwrap();
        assert_eq!(v["kind"], "player.account_page_result");
        assert_eq!(v["orders"][0]["invoice_no"], "MUM/26-27/E1/1");
        assert_eq!(parse_frame(&page.to_text()).unwrap(), page);
        let change = parse_frame(
            r#"{"v":1,"kind":"player.password_change","rid":"b","current":"old","new":"newpass"}"#,
        )
        .unwrap();
        assert_eq!(
            change.body,
            FrameBody::PlayerPasswordChange(PlayerChangePassword {
                current: "old".into(),
                new: "newpass".into(),
            })
        );
        for result in [
            PlayerPasswordChangeResult::Ok,
            PlayerPasswordChangeResult::WrongCurrent,
            PlayerPasswordChangeResult::InvalidNew,
            PlayerPasswordChangeResult::Locked { retry_s: 60 },
        ] {
            let f = Frame::new("r", FrameBody::PlayerPasswordChangeResult(result));
            assert_eq!(
                serde_json::to_value(&f).unwrap()["kind"],
                "player.password_change_result"
            );
            assert_eq!(parse_frame(&f.to_text()).unwrap(), f);
        }
    }

    #[test]
    fn a_password_change_is_bounded_and_never_printed() {
        let change = |c: &str, n: &str| PlayerChangePassword {
            current: c.to_owned(),
            new: n.to_owned(),
        };
        assert!(change("x", "y").validate().is_ok());
        for bad in [
            change("", "y"),
            change("x", ""),
            change("x", &"y".repeat(1025)),
        ] {
            assert_eq!(bad.validate().unwrap_err().code, ErrorCode::BadFrame);
        }
        let text = format!("{:?}", change("hunter2", "swordfish"));
        assert!(!text.contains("hunter2") && !text.contains("swordfish"));
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
