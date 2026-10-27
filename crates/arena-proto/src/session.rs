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
pub struct SessionEndReverted {
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionSettleReverted {
    pub order_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum PaymentMethod {
    Cash,
    Upi,
    Card,
}

impl PaymentMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cash => "cash",
            Self::Upi => "upi",
            Self::Card => "card",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PaymentRow {
    pub method: PaymentMethod,
    #[ts(type = "number")]
    pub amount_minor: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionSettle {
    pub order_id: String,
    pub payments: Vec<PaymentRow>,
}

pub const UNDO_WINDOW_MS: i64 = 8_000;
pub const CARD_REFERENCE_MAX: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SettleOutcome {
    pub order_id: String,
    #[ts(type = "number")]
    pub settled_at: i64,
    #[ts(type = "number")]
    pub undo_until: i64,
    #[ts(type = "number")]
    pub order_rev: i64,
    #[ts(type = "number")]
    pub total_minor: i64,
    pub irreversible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct EndOutcome {
    pub session_id: String,
    pub order_id: String,
    #[ts(type = "number")]
    pub ended_at: i64,
    #[ts(type = "number")]
    pub undo_until: i64,
    #[ts(type = "number")]
    pub session_rev: i64,
    #[ts(type = "number")]
    pub total_minor: i64,
    pub irreversible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BillSettledEvent {
    pub order_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BillRevertedEvent {
    pub order_id: String,
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
    #[serde(default)]
    #[ts(optional, type = "number | null")]
    pub order_rev: Option<i64>,
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
    fn settle_round_trips_and_cash_omits_its_reference() {
        let s: SessionSettle = serde_json::from_value(serde_json::json!({
            "order_id": "O",
            "payments": [{"method": "cash", "amount_minor": 100},
                         {"method": "upi", "amount_minor": 50, "reference": "1234"}]
        }))
        .unwrap();
        assert_eq!(s.payments[0].reference, None);
        assert_eq!(s.payments[1].method.as_str(), "upi");
        assert!(
            serde_json::to_value(&s).unwrap()["payments"][0]
                .get("reference")
                .is_none()
        );
    }

    #[test]
    fn reverts_round_trip() {
        let e: SessionEndReverted =
            serde_json::from_value(serde_json::json!({"session_id": "S"})).unwrap();
        assert_eq!(e.session_id, "S");
        let s: SessionSettleReverted =
            serde_json::from_value(serde_json::json!({"order_id": "O"})).unwrap();
        assert_eq!(s.order_id, "O");
    }

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
