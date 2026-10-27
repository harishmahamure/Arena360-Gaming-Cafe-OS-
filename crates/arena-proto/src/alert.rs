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
}

impl AlertKind {
    /// Every kind.
    pub const ALL: [AlertKind; 2] = [Self::Fixture, Self::TimeUp];

    /// The stored and wire form, e.g. `time_up`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fixture => "fixture",
            Self::TimeUp => "time_up",
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
    }
}
