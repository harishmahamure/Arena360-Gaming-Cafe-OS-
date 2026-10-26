use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::operating::TestSessionState;

pub const KEY_TEST_SESSION: &str = "setup.test_session";
pub const DEFAULT_TEST_SESSION: &str = r#"{"state":"unset"}"#;
pub const TEST_SESSION_MS: u64 = 120_000;
pub const TEST_SESSION_CONFIRM_MS: u64 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct TestSessionStatus {
    pub state: TestSessionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub at: Option<i64>,
}

impl TestSessionStatus {
    pub fn unset() -> Self {
        Self {
            state: TestSessionState::Unset,
            device_id: None,
            at: None,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        let s: Self = serde_json::from_str(value).ok()?;
        let bare = s.device_id.is_none() && s.at.is_none();
        let full = s.device_id.is_some() && s.at.is_some();
        match s.state {
            TestSessionState::Unset => bare,
            _ => full,
        }
        .then_some(s)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| DEFAULT_TEST_SESSION.to_owned())
    }

    pub fn active(&self, now: i64) -> bool {
        self.state == TestSessionState::Running
            && self
                .at
                .is_some_and(|at| now.saturating_sub(at) < Self::WINDOW_MS)
    }

    pub const WINDOW_MS: i64 = (TEST_SESSION_MS + TEST_SESSION_CONFIRM_MS) as i64;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DeviceTestSessionStart {
    pub device_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum TestSessionOutcome {
    Locked,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DeviceTestSessionCompleted {
    pub device_id: String,
    pub outcome: TestSessionOutcome,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_round_trips_and_rejects_inconsistent_rows() {
        assert_eq!(
            TestSessionStatus::parse(DEFAULT_TEST_SESSION),
            Some(TestSessionStatus::unset())
        );
        assert_eq!(TestSessionStatus::unset().to_json(), DEFAULT_TEST_SESSION);
        let running = TestSessionStatus {
            state: TestSessionState::Running,
            device_id: Some("D".into()),
            at: Some(5),
        };
        assert_eq!(
            TestSessionStatus::parse(&running.to_json()),
            Some(running.clone())
        );
        for bad in [
            r#"{"state":"running"}"#,
            r#"{"state":"passed","device_id":"D"}"#,
            r#"{"state":"unset","device_id":"D","at":1}"#,
            "{}",
        ] {
            assert_eq!(TestSessionStatus::parse(bad), None, "{bad}");
        }
        assert!(running.active(5 + 129_999));
        assert!(!running.active(5 + 130_000));
    }
}
