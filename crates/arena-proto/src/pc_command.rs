use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{AppError, ErrorCode};

pub const PC_MESSAGE_MAX: usize = 140;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum PcAction {
    Lock,
    Unlock,
    Message,
    Restart,
    Shutdown,
    /// Run the session-end cleanup again (Story 6.11); sent from the tile sheet while a
    /// `check_pc` alert is open.
    CleanupRetry,
    /// Staff checked the PC by hand: the kiosk releases its "getting ready" hold (Story 6.11).
    CleanupChecked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum LockReason {
    PayAtCounter,
    EndedByStaff,
    TimesUp,
    /// Time ran out and staff end the session: the lock says to see the counter.
    TimesUpSeeCounter,
    LockedByStaff,
    Paused,
    /// The kiosk is cleaning the PC after a session, or holds after a cleanup step failed.
    /// Kiosk-local: the Hub never sends it (Story 6.11).
    PreparingPc,
}

impl PcAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lock => "lock",
            Self::Unlock => "unlock",
            Self::Message => "message",
            Self::Restart => "restart",
            Self::Shutdown => "shutdown",
            Self::CleanupRetry => "cleanup_retry",
            Self::CleanupChecked => "cleanup_checked",
        }
    }

    pub fn is_power(self) -> bool {
        matches!(self, Self::Restart | Self::Shutdown)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DeviceScreen {
    pub device_id: String,
    pub action: PcAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DevicePower {
    pub device_id: String,
    pub action: PcAction,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

/// Whether `text`, trimmed, is a staff message: 1 to [`PC_MESSAGE_MAX`] characters, none a
/// control character.
pub fn valid_message_text(text: &str) -> bool {
    let t = text.trim();
    (1..=PC_MESSAGE_MAX).contains(&t.chars().count()) && !t.chars().any(char::is_control)
}

impl DeviceScreen {
    pub fn validate(&self) -> Result<(), AppError> {
        match self.action {
            PcAction::Message => Err(invalid("action", "a message is sent with device.message")),
            a if a.is_power() => Err(invalid("action", "lock, unlock or a cleanup command only")),
            _ => Ok(()),
        }
    }
}

/// Who a staff message goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum MessageScope {
    /// One PC, named by `device_id`.
    Seat,
    /// Every online PC that has a live session; names no device.
    All,
}

/// The payload of `device.message` (Story 6.16): staff, and later the owner (Story 14.32),
/// message one PC or every PC with a session. The kiosk shows it as an info toast and keeps it
/// in the messages bell until the session ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DeviceMessage {
    pub scope: MessageScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub device_id: Option<String>,
    pub text: String,
}

impl DeviceMessage {
    pub fn validate(&self) -> Result<(), AppError> {
        match (self.scope, &self.device_id) {
            (MessageScope::Seat, Some(id)) if !id.is_empty() => {}
            (MessageScope::Seat, _) => {
                return Err(invalid("device_id", "a seat message names a PC"));
            }
            (MessageScope::All, None) => {}
            (MessageScope::All, Some(_)) => {
                return Err(invalid("device_id", "a message to all PCs names no PC"));
            }
        }
        if valid_message_text(&self.text) {
            Ok(())
        } else {
            Err(invalid("text", "the message must be 1 to 140 characters"))
        }
    }
}

impl DevicePower {
    pub fn validate(&self) -> Result<(), AppError> {
        if self.action.is_power() {
            Ok(())
        } else {
            Err(invalid("action", "restart or shutdown only"))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PcAck {
    pub cmd_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PcAckEvent {
    pub cmd_id: String,
    pub device_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(action: PcAction) -> DeviceScreen {
        DeviceScreen {
            device_id: "D".into(),
            action,
        }
    }

    #[test]
    fn screen_takes_lock_unlock_and_the_cleanup_commands_but_no_message() {
        assert!(screen(PcAction::Lock).validate().is_ok());
        assert!(screen(PcAction::Unlock).validate().is_ok());
        assert!(screen(PcAction::Message).validate().is_err());
        assert!(screen(PcAction::Restart).validate().is_err());
    }

    fn message(scope: MessageScope, device: Option<&str>, text: &str) -> DeviceMessage {
        DeviceMessage {
            scope,
            device_id: device.map(Into::into),
            text: text.into(),
        }
    }

    #[test]
    fn a_message_is_one_to_140_chars_without_control_characters() {
        let seat = |t: &str| message(MessageScope::Seat, Some("D"), t).validate();
        assert!(seat("Closing at 11").is_ok());
        assert!(seat("").is_err());
        assert!(seat("  ").is_err());
        assert!(seat("a\nb").is_err());
        assert!(seat(&"é".repeat(140)).is_ok());
        assert!(seat(&"é".repeat(141)).is_err());
        assert!(seat(&format!("  {}  ", "x".repeat(140))).is_ok());
    }

    #[test]
    fn the_scope_decides_whether_a_pc_is_named() {
        assert!(message(MessageScope::Seat, None, "hi").validate().is_err());
        assert!(
            message(MessageScope::Seat, Some(""), "hi")
                .validate()
                .is_err()
        );
        assert!(
            message(MessageScope::All, Some("D"), "hi")
                .validate()
                .is_err()
        );
        assert!(message(MessageScope::All, None, "hi").validate().is_ok());
        let json = serde_json::to_value(message(MessageScope::All, None, "hi")).unwrap();
        assert_eq!(json, serde_json::json!({"scope": "all", "text": "hi"}));
    }

    #[test]
    fn the_cleanup_commands_are_screen_commands_with_no_text() {
        for action in [PcAction::CleanupRetry, PcAction::CleanupChecked] {
            assert!(!action.is_power());
            assert!(screen(action).validate().is_ok());
            assert_eq!(
                serde_json::to_string(&action).unwrap(),
                format!("\"{}\"", action.as_str())
            );
        }
        assert_eq!(PcAction::CleanupRetry.as_str(), "cleanup_retry");
        assert_eq!(PcAction::CleanupChecked.as_str(), "cleanup_checked");
        assert_eq!(
            serde_json::to_string(&LockReason::PreparingPc).unwrap(),
            "\"preparing_pc\""
        );
    }

    #[test]
    fn power_takes_restart_or_shutdown() {
        let p = |action| DevicePower {
            device_id: "D".into(),
            action,
        };
        assert!(p(PcAction::Restart).validate().is_ok());
        assert!(p(PcAction::Shutdown).validate().is_ok());
        assert!(p(PcAction::Lock).validate().is_err());
    }
}
