//! Hub alerts: time-critical events that interrupt staff until one acknowledges them.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What an alert is about. Each kind has a catalogue label (`hub.alerts.kind.<kind>`) and a tile tag
/// ([`AlertKind::tag`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum AlertKind {
    /// A stand-in that tests and demos raise.
    Fixture,
    /// A session reached its end time (raised by Story 3.26).
    TimeUp,
    /// Staff marked a console's controllers missing after a session (raised by `controller.check`).
    ControllerMissing,
    /// A Bluetooth printer dropped a job (raised by the Hub's printer dispatch; Story 4.6).
    PrinterOffline,
    /// A lot with stock left expires within the warning window or already has (raised by
    /// `stock.expiry_check` and `stock.in`; Story 5.7). A branch-level alert.
    StockExpiring,
    /// A tracked product's quantity on hand fell to or under its low-stock threshold (raised by
    /// any stock decrement and by `stock.threshold`; Story 5.8). One open per product; it records
    /// the product, the quantity and the threshold.
    StockLow,
    /// A kiosk's player login was paused after five wrong tries in a row (raised by the Hub's
    /// LAN layer, once while open; Story 6.8). A device alert: the tile shows `locked-out`.
    LoginLockout,
    /// A kiosk's session-end cleanup step failed twice (raised by `kiosk.cleanup_failed`; Story
    /// 6.11). A device alert, one open per device: the tile shows `check-pc` and the kiosk holds a
    /// locked screen until staff pick "Retry cleanup" or "Checked".
    CheckPc,
    /// A player pressed "Call staff" at a kiosk (raised by the Hub on `device.call_staff`; Story
    /// 6.15). A device alert, one open per device: the tile shows `call`.
    CallStaff,
    /// A player placed an order from a kiosk (raised by the Hub on `shop.place`; Story 6.31). A
    /// device alert, one open per device: it sounds, the tile shows `order`, and it closes by
    /// itself once every line of the session's bill is served (or staff acknowledge it).
    KioskOrder,
    /// A kitchen ticket reached Ready (raised by `kitchen.advance`; Story 7.4). A device ticket
    /// raises one per device; a walk-up ticket raises a branch-level alert that records its ticket.
    /// It sounds once and closes when staff acknowledge it or the Ready is undone.
    OrderReady,
    /// The daily snapshot was skipped because the disk has less than twice the database's size
    /// free (raised by the Hub's snapshot task; Story 8.8). A branch-level alert, one open at a
    /// time; it does not sound.
    BackupDisk,
    /// A paid licence moved the branch to another plan (raised by `licence.apply`; Story 10.6). A
    /// branch-level notice, one open at a time; it does not sound and clears when staff
    /// acknowledge it. Its text names the plan the Hub runs now.
    PlanUpdated,
}

impl AlertKind {
    /// Every kind.
    pub const ALL: [AlertKind; 13] = [
        Self::Fixture,
        Self::TimeUp,
        Self::ControllerMissing,
        Self::PrinterOffline,
        Self::StockExpiring,
        Self::StockLow,
        Self::LoginLockout,
        Self::CheckPc,
        Self::CallStaff,
        Self::KioskOrder,
        Self::OrderReady,
        Self::BackupDisk,
        Self::PlanUpdated,
    ];

    /// The stored and wire form, e.g. `time_up`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fixture => "fixture",
            Self::TimeUp => "time_up",
            Self::ControllerMissing => "controller_missing",
            Self::PrinterOffline => "printer_offline",
            Self::StockExpiring => "stock_expiring",
            Self::StockLow => "stock_low",
            Self::LoginLockout => "login_lockout",
            Self::CheckPc => "check_pc",
            Self::CallStaff => "call_staff",
            Self::KioskOrder => "kiosk_order",
            Self::OrderReady => "order_ready",
            Self::BackupDisk => "backup_disk",
            Self::PlanUpdated => "plan_updated",
        }
    }

    /// The kind with this stored form, if any.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// The short text the tile's `alert-badge` shows.
    pub fn tag(self) -> &'static str {
        match self {
            Self::Fixture => "new",
            Self::TimeUp => "time-up",
            Self::ControllerMissing => "controller",
            Self::PrinterOffline => "printer",
            Self::StockExpiring => "expiring",
            Self::StockLow => "low-stock",
            Self::LoginLockout => "locked-out",
            Self::CheckPc => "check-pc",
            Self::CallStaff => "call",
            Self::KioskOrder => "order",
            Self::OrderReady => "ready",
            Self::BackupDisk => "backup",
            Self::PlanUpdated => "plan",
        }
    }
}

/// The payload of `alert.raise` (system only): an alert of `alert_kind` (an [`AlertKind`] wire
/// form; any other is `VALIDATION_FAILED`; not `kind`, which tags the payload), for `device_id`
/// when it is about one device. Raising one that is already open is `STALE` and writes nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AlertRaise {
    pub alert_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub device_id: Option<String>,
}

impl AlertRaise {
    /// A raise of `alert_kind`, for `device_id` when given.
    pub fn new(kind: AlertKind, device_id: Option<String>) -> Self {
        Self {
            alert_kind: kind.as_str().to_owned(),
            device_id,
        }
    }
}

/// The payload of `alert.ack`: staff acknowledge the open alert `alert_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AlertAck {
    pub alert_id: String,
}

/// One open alert in the Hub snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SnapshotAlert {
    pub id: String,
    pub kind: AlertKind,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub device_id: Option<String>,
    #[ts(type = "number")]
    pub raised_at: i64,
    /// The product a low-stock alert is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub product_id: Option<String>,
    /// That product's name now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub product: Option<String>,
    /// The quantity on hand a low-stock alert last saw.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "number", optional)]
    pub qty: Option<i64>,
    /// The threshold a low-stock alert crossed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "number", optional)]
    pub threshold: Option<i64>,
    /// The walk-up kitchen ticket an order-ready alert is about (Story 7.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ticket_id: Option<String>,
    /// That ticket's number (`K-<n>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "number", optional)]
    pub ticket_number: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_payloads_round_trip_inside_an_intent_payload() {
        let raise =
            crate::IntentPayload::AlertRaise(AlertRaise::new(AlertKind::TimeUp, Some("D1".into())));
        let json = serde_json::to_value(&raise).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"kind": "alert.raise", "alert_kind": "time_up", "device_id": "D1"})
        );
        assert_eq!(
            serde_json::from_value::<crate::IntentPayload>(json).unwrap(),
            raise
        );
        let ack = crate::IntentPayload::AlertAck(AlertAck {
            alert_id: "A1".into(),
        });
        let json = serde_json::to_value(&ack).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"kind": "alert.ack", "alert_id": "A1"})
        );
        assert_eq!(
            serde_json::from_value::<crate::IntentPayload>(json).unwrap(),
            ack
        );
    }

    #[test]
    fn every_kind_round_trips_and_has_a_tag() {
        for kind in AlertKind::ALL {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{}\"", kind.as_str()));
            assert_eq!(AlertKind::parse(kind.as_str()), Some(kind));
            assert!(!kind.tag().is_empty());
        }
        assert_eq!(AlertKind::parse("nope"), None);
        assert_eq!(AlertKind::Fixture.tag(), "new");
        assert_eq!(AlertKind::TimeUp.tag(), "time-up");
        assert_eq!(AlertKind::LoginLockout.as_str(), "login_lockout");
        assert_eq!(AlertKind::LoginLockout.tag(), "locked-out");
        assert_eq!(AlertKind::CheckPc.as_str(), "check_pc");
        assert_eq!(AlertKind::CheckPc.tag(), "check-pc");
        assert_eq!(AlertKind::CallStaff.as_str(), "call_staff");
        assert_eq!(AlertKind::CallStaff.tag(), "call");
        assert_eq!(AlertKind::KioskOrder.as_str(), "kiosk_order");
        assert_eq!(AlertKind::KioskOrder.tag(), "order");
        assert_eq!(AlertKind::OrderReady.as_str(), "order_ready");
        assert_eq!(AlertKind::OrderReady.tag(), "ready");
    }
}
