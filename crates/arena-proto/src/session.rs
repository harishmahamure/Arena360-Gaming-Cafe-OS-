use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum SessionMode {
    Postpaid,
    Pack,
    Wallet,
    Pass,
    Block,
}

impl SessionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Postpaid => "postpaid",
            Self::Pack => "pack",
            Self::Wallet => "wallet",
            Self::Pass => "pass",
            Self::Block => "block",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionStart {
    pub device_id: String,
    pub mode: SessionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub player_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionEnd {
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BillLine {
    pub device_id: String,
    #[ts(type = "number")]
    pub from_at: i64,
    #[ts(type = "number")]
    pub to_at: i64,
    #[ts(type = "number")]
    pub minutes: i64,
    #[ts(type = "number")]
    pub rate_minor_per_hour: i64,
    #[ts(type = "number")]
    pub amount_minor: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SnapshotBill {
    pub session_id: String,
    pub device_id: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub order_id: Option<String>,
    pub lines: Vec<BillLine>,
    #[ts(type = "number")]
    pub subtotal_minor: i64,
    #[ts(type = "number")]
    pub paid_minor: i64,
    #[ts(type = "number")]
    pub remaining_minor: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SnapshotSession {
    pub id: String,
    pub device_id: String,
    pub state: String,
    #[ts(type = "number")]
    pub started_at: i64,
    #[ts(type = "number")]
    pub rev: i64,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub player_id: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub username: Option<String>,
    #[serde(default)]
    #[ts(optional, type = "number | null")]
    pub guest_no: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionStartedEvent {
    pub session_id: String,
    pub device_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionScreen {
    pub session_id: String,
    pub device_name: String,
    #[ts(type = "number")]
    pub elapsed_ms: u64,
    #[ts(type = "number")]
    pub lock_after_ms: u64,
    pub time_up_mode: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_round_trips_and_guest_omits_player() {
        let p: SessionStart =
            serde_json::from_value(serde_json::json!({"device_id": "D", "mode": "postpaid"}))
                .unwrap();
        assert_eq!(p.player_id, None);
        assert_eq!(p.mode.as_str(), "postpaid");
        assert!(serde_json::to_value(&p).unwrap().get("player_id").is_none());
    }
}
