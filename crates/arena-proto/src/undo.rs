use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum EffectKind {
    ReceiptPrinted,
    DrawerKicked,
    InvoiceNumberAssigned,
    NotificationSent,
}

impl EffectKind {
    pub const ALL: [EffectKind; 4] = [
        Self::ReceiptPrinted,
        Self::DrawerKicked,
        Self::InvoiceNumberAssigned,
        Self::NotificationSent,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReceiptPrinted => "receipt_printed",
            Self::DrawerKicked => "drawer_kicked",
            Self::InvoiceNumberAssigned => "invoice_number_assigned",
            Self::NotificationSent => "notification_sent",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct EffectRecordedEvent {
    pub kind: EffectKind,
    pub order_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_effect_kind_round_trips_its_wire_form() {
        for kind in EffectKind::ALL {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{}\"", kind.as_str()));
            assert_eq!(EffectKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(EffectKind::parse("nope"), None);
    }
}
