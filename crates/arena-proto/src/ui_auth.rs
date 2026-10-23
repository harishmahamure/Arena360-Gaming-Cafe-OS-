//! The local Hub UI's loopback auth API (`/ui/auth/*`, `/ui/me`, `/ui/intent`, `/ui/settings`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::ErrorCode;
use crate::intent::Intent;

/// `POST /ui/auth/sign-in`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SignInRequest {
    pub login: String,
    pub password: String,
}

/// The signed-in staff member, as `GET /ui/me` and a sign-in return it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionStaff {
    pub staff_id: String,
    pub display_name: String,
    pub role_name: String,
}

/// A successful sign-in: the bearer token for this Hub process and who it belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SignInOk {
    pub token: String,
    pub staff: SessionStaff,
}

/// The error body of every `/ui/*` auth and intent request. `retry_after_s` is set only on
/// `AUTH_LOCKED`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct UiError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub retry_after_s: Option<u32>,
}

/// `POST /ui/intent`: the session's staff actor replaces `intent.actor`, and any
/// `elevated_by` is dropped. A success answers [`IntentResult`](crate::IntentResult).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct UiIntentRequest {
    pub intent: Intent,
}

/// One branch setting, as `GET /ui/settings` lists them (the `rev` is the edit's `base_rev`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SettingEntry {
    pub key: String,
    pub value: String,
    #[ts(type = "number")]
    pub rev: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_error_omits_unset_fields() {
        let e = UiError {
            code: ErrorCode::AuthLocked,
            message: "m".into(),
            field: None,
            retry_after_s: Some(42),
        };
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"code":"AUTH_LOCKED","message":"m","retry_after_s":42}"#
        );
    }
}
