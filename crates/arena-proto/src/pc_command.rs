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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum LockReason {
    PayAtCounter,
    EndedByStaff,
    TimesUp,
    LockedByStaff,
    Paused,
}

impl PcAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lock => "lock",
            Self::Unlock => "unlock",
            Self::Message => "message",
            Self::Restart => "restart",
            Self::Shutdown => "shutdown",
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub text: Option<String>,
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

impl DeviceScreen {
    pub fn validate(&self) -> Result<(), AppError> {
        if self.action.is_power() {
            return Err(invalid("action", "lock, unlock or message only"));
        }
        let text = self.text.as_deref().map(str::trim);
        match (self.action, text) {
            (PcAction::Message, Some(t))
                if (1..=PC_MESSAGE_MAX).contains(&t.chars().count())
                    && !t.chars().any(char::is_control) =>
            {
                Ok(())
            }
            (PcAction::Message, _) => {
                Err(invalid("text", "the message must be 1 to 140 characters"))
            }
            (_, None) => Ok(()),
            _ => Err(invalid("text", "only a message carries text")),
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

    fn screen(action: PcAction, text: Option<&str>) -> DeviceScreen {
        DeviceScreen {
            device_id: "D".into(),
            action,
            text: text.map(Into::into),
        }
    }

    #[test]
    fn screen_text_is_one_to_140_chars_and_only_for_message() {
        assert!(screen(PcAction::Lock, None).validate().is_ok());
        assert!(screen(PcAction::Lock, Some("x")).validate().is_err());
        assert!(screen(PcAction::Message, None).validate().is_err());
        assert!(screen(PcAction::Message, Some("  ")).validate().is_err());
        assert!(screen(PcAction::Message, Some("a\nb")).validate().is_err());
        assert!(
            screen(PcAction::Message, Some(&"é".repeat(140)))
                .validate()
                .is_ok()
        );
        assert!(
            screen(PcAction::Message, Some(&"é".repeat(141)))
                .validate()
                .is_err()
        );
        assert!(screen(PcAction::Restart, None).validate().is_err());
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
