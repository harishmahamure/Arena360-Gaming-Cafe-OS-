use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum InviteRole {
    Manager,
    Counter,
    Kitchen,
}

impl InviteRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manager => "manager",
            Self::Counter => "counter",
            Self::Kitchen => "kitchen",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Manager, Self::Counter, Self::Kitchen]
            .into_iter()
            .find(|r| r.as_str() == s)
    }

    pub fn hub_role(self) -> &'static str {
        match self {
            Self::Manager => "Manager",
            Self::Counter => "Counter",
            Self::Kitchen => "Kitchen",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StaffInvite {
    pub name: String,
    pub phone: String,
    pub role: InviteRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub branch_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum InviteStatus {
    Waiting,
    Applied,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StaffInviteView {
    pub id: String,
    pub name: String,
    pub phone: String,
    pub role: InviteRole,
    pub status: InviteStatus,
    pub error: Option<AppError>,
    #[ts(type = "number")]
    pub setup_expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StaffInvites {
    pub invites: Vec<StaffInviteView>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StaffInvited {
    pub invite: StaffInviteView,
    pub code: String,
}

impl std::fmt::Debug for StaffInvited {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaffInvited")
            .field("invite", &self.invite)
            .field("code", &"[redacted]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_round_trip_and_map_to_hub_roles() {
        for r in [
            InviteRole::Manager,
            InviteRole::Counter,
            InviteRole::Kitchen,
        ] {
            assert_eq!(InviteRole::parse(r.as_str()), Some(r));
        }
        assert_eq!(InviteRole::parse("owner"), None);
        assert_eq!(InviteRole::Counter.hub_role(), "Counter");
    }

    #[test]
    fn the_code_never_prints() {
        let invited = StaffInvited {
            invite: StaffInviteView {
                id: "i".into(),
                name: "Neha".into(),
                phone: "+919876543210".into(),
                role: InviteRole::Manager,
                status: InviteStatus::Waiting,
                error: None,
                setup_expires_at: 1,
            },
            code: "K7Q49MZD".into(),
        };
        assert!(!format!("{invited:?}").contains("K7Q49MZD"));
    }
}
