//! The local Hub UI's loopback auth API (`/ui/auth/*`, `/ui/me`, `/ui/intent`, `/ui/settings`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::ErrorCode;
use crate::intent::{Intent, IntentKind};
use crate::permission::Permission;

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
    /// What the member's role allows now (the screens use it to show read-only controls; the
    /// Hub still checks every intent).
    pub permissions: Vec<Permission>,
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
/// client-supplied `elevated_by` is dropped. `elevation_ticket` (from `POST /ui/elevate`) is
/// redeemed for one use: the Hub then sets `elevated_by` itself. A success answers
/// [`IntentResult`](crate::IntentResult).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct UiIntentRequest {
    pub intent: Intent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub elevation_ticket: Option<String>,
}

/// Most intents one elevation ticket may cover.
pub const MAX_TICKET_USES: usize = 10;

/// One intent an elevation approves: its kind and, for `settings.set`, the setting key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ElevationItem {
    pub intent_kind: IntentKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub key: Option<String>,
}

/// `POST /ui/elevate`: a manager's PIN, typed on this screen, approving exactly `items`
/// (1–[`MAX_TICKET_USES`] intents; each is redeemed once). The PIN goes nowhere else, and
/// `Debug` never prints it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ElevateRequest {
    pub items: Vec<ElevationItem>,
    pub pin: String,
}

impl std::fmt::Debug for ElevateRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElevateRequest")
            .field("items", &self.items)
            .field("pin", &"<redacted>")
            .finish()
    }
}

/// A granted elevation: a single-session ticket for the approved `items`, valid for
/// `expires_in_s`. It never says whose PIN it was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ElevateOk {
    pub ticket: String,
    pub items: Vec<ElevationItem>,
    pub expires_in_s: u32,
}

/// `POST /ui/elevate/drop`: gives up a ticket's unused approvals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ElevateDrop {
    pub ticket: String,
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
    fn elevate_request_never_prints_its_pin() {
        let req: ElevateRequest = serde_json::from_str(
            r#"{"items":[{"intent_kind":"settings.set","key":"k"}],"pin":"2468"}"#,
        )
        .unwrap();
        assert_eq!(req.items[0].key.as_deref(), Some("k"));
        assert!(!format!("{req:?}").contains("2468"));
    }

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
