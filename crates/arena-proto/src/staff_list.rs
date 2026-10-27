//! The Settings → Staff screen's wire types: `GET /ui/staff` and `POST /ui/staff/deactivate`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One staff member as Settings → Staff lists them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StaffView {
    pub id: String,
    pub name: String,
    pub login: String,
    /// The role's name (a built-in or a custom one).
    pub role: String,
    /// `active` (has a password) or `invited` (waiting to use the setup code).
    pub status: StaffStatus,
    #[ts(type = "number")]
    pub rev: i64,
}

/// Whether a listed member has signed up yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum StaffStatus {
    Active,
    Invited,
}

/// `GET /ui/staff`: the active members (the first `used` of them count against `limit`) and
/// the licence's `users` limit. Deactivated members are not listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StaffList {
    pub staff: Vec<StaffView>,
    #[ts(type = "number")]
    pub used: i64,
    #[ts(type = "number")]
    pub limit: i64,
}

/// The body of `POST /ui/staff/deactivate`: the Owner confirms with their own PIN.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct UiStaffDeactivateRequest {
    pub staff_id: String,
    pub pin: String,
    pub idempotency_key: String,
}

impl std::fmt::Debug for UiStaffDeactivateRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiStaffDeactivateRequest")
            .field("staff_id", &self.staff_id)
            .finish_non_exhaustive()
    }
}
