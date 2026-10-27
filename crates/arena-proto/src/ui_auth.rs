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

pub const MIN_STAFF_PASSWORD_CHARS: usize = 10;
pub const MAX_STAFF_PASSWORD_CHARS: usize = 128;

/// `POST /ui/auth/setup-code`: a first-time member's login, one-time code and the password and
/// PIN they chose. It goes nowhere else, and `Debug` never prints the secrets.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupCodeRequest {
    pub login: String,
    pub code: String,
    pub password: String,
    pub pin: String,
}

impl std::fmt::Debug for SetupCodeRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetupCodeRequest")
            .field("login", &self.login)
            .finish_non_exhaustive()
    }
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

/// `GET /ui/me`: the signed-in staff member and whether the session is idle-locked (a
/// reloaded view shows the lock again).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct UiMe {
    #[serde(flatten)]
    pub staff: SessionStaff,
    pub locked: bool,
    /// Whether the member has a PIN (without one only signing out leaves the lock screen).
    pub has_pin: bool,
}

/// `POST /ui/unlock`: a PIN typed on the lock screen. It goes nowhere else, and `Debug` never
/// prints it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct UnlockRequest {
    pub pin: String,
}

impl std::fmt::Debug for UnlockRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnlockRequest")
            .field("pin", &"<redacted>")
            .finish()
    }
}

/// An unlocked session: who now acts in it. `switched` is set when the PIN was another staff
/// member's, so the session now belongs to them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct UnlockOk {
    pub staff: SessionStaff,
    pub switched: bool,
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

/// The reply to a successful `POST /ui/intent`: the applied rows and, for `session.settle`, when
/// it settled and until when it can be undone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct UiIntentResult {
    pub applied: crate::frame::AppliedSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub settled: Option<crate::session::SettleOutcome>,
}

/// Most intents one elevation ticket may cover.
pub const MAX_TICKET_USES: usize = 10;

/// One thing an elevation approves: an intent (`intent_kind` and, for `settings.set`, the
/// setting `key`), redeemed once; or a read permission (`permission`, one of
/// [`Permission::READ_ELEVATION`], alone in its request), which grants unlimited reads for a
/// while. Exactly one of `intent_kind` and `permission` is set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ElevationItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub intent_kind: Option<IntentKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub permission: Option<Permission>,
}

impl ElevationItem {
    /// An item approving one intent of `kind` (with the setting `key` of a `settings.set`).
    pub fn intent(kind: IntentKind, key: Option<String>) -> Self {
        Self {
            intent_kind: Some(kind),
            key,
            permission: None,
        }
    }

    /// An item asking for a read elevation of `permission`.
    pub fn read(permission: Permission) -> Self {
        Self {
            intent_kind: None,
            key: None,
            permission: Some(permission),
        }
    }
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
        assert_eq!(req.items[0].intent_kind, Some(IntentKind::SettingsSet));
        assert!(!format!("{req:?}").contains("2468"));
    }

    #[test]
    fn a_read_item_names_only_its_permission() {
        let item: ElevationItem = serde_json::from_str(r#"{"permission":"audit.view"}"#).unwrap();
        assert_eq!(item, ElevationItem::read(Permission::AuditView));
        assert_eq!(
            serde_json::to_string(&item).unwrap(),
            r#"{"permission":"audit.view"}"#
        );
        assert_eq!(
            serde_json::to_string(&ElevationItem::intent(IntentKind::StaffUpsert, None)).unwrap(),
            r#"{"intent_kind":"staff.upsert"}"#
        );
    }

    #[test]
    fn unlock_request_never_prints_its_pin() {
        let req: UnlockRequest = serde_json::from_str(r#"{"pin":"2468"}"#).unwrap();
        assert!(!format!("{req:?}").contains("2468"));
    }

    #[test]
    fn me_flattens_the_staff_block() {
        let me = UiMe {
            staff: SessionStaff {
                staff_id: "S".into(),
                display_name: "Counter".into(),
                role_name: "Counter".into(),
                permissions: vec![],
            },
            locked: true,
            has_pin: true,
        };
        assert_eq!(
            serde_json::to_value(&me).unwrap(),
            serde_json::json!({
                "staff_id": "S",
                "display_name": "Counter",
                "role_name": "Counter",
                "permissions": [],
                "locked": true,
                "has_pin": true
            })
        );
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
