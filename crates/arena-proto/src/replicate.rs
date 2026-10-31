//! The standby replication protocol (Story 9.19, AD-4): what a linked standby Hub and the main
//! Hub say to each other on `wss://<main>:7400/v1/replicate`, and the snapshot a standby loads
//! first.
//!
//! The first frame is the standby's `open` (its device token, schema version and last applied
//! position). The main Hub answers `ready`, then `batch` frames in log order: closed history
//! first, then live rows as they commit. A refusal is one `refused` frame and a close.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::ErrorCode;

/// The WebSocket path on `:7400` (and `:7401`, over TLS).
pub const REPLICATE_PATH: &str = "/v1/replicate";
/// `GET`, with `Authorization: Bearer <device token>`: the snapshot a standby loads first.
pub const REPLICATE_SNAPSHOT_PATH: &str = "/v1/replicate/snapshot";
/// The snapshot's position, as response headers.
pub const SNAPSHOT_EPOCH_HEADER: &str = "X-Arena-Epoch";
pub const SNAPSHOT_SEQ_HEADER: &str = "X-Arena-Seq";
/// Rows in one `batch` frame at most.
pub const REPLICATE_BATCH_ROWS: usize = 500;
/// The standby's reconnect backoff, doubled from the first to the last and jittered.
pub const REPLICATE_BACKOFF_MIN_MS: u64 = 2_000;
pub const REPLICATE_BACKOFF_MAX_MS: u64 = 30_000;

/// A place in the change log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicatePosition {
    pub epoch: String,
    pub seq: i64,
}

/// One `changes` row, carried verbatim: the follower writes it with the same epoch and `seq`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicatedChange {
    pub epoch: String,
    pub seq: i64,
    pub tbl: String,
    pub row_id: String,
    /// `insert`, `update` or `delete`.
    pub op: String,
    /// The MessagePack row, base64 on the wire.
    #[serde(with = "b64")]
    pub row: Vec<u8>,
    pub intent_id: String,
    pub intent_kind: String,
    pub actor_kind: String,
    pub actor_id: String,
    pub elevated_by_kind: Option<String>,
    pub elevated_by_id: Option<String>,
    pub elevated_by_mode: Option<String>,
    pub elevated_by_verifier_version: Option<i64>,
    pub at: i64,
    pub business_day: Option<String>,
    pub fy: Option<String>,
    pub reason: Option<String>,
}

/// A player key as the main Hub holds it when the batch is sent: stored on the standby when
/// `dek_wrapped` is present, deleted when it is not (erasure).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicatedKey {
    pub player_id: String,
    pub dek_wrapped: Option<String>,
}

/// A frame of the replication stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReplicateFrame {
    /// Standby to main: always the first frame.
    Open {
        token: String,
        schema_version: u32,
        /// The last change the standby holds; `None` for a snapshot of an empty log.
        from: Option<ReplicatePosition>,
    },
    /// Main to standby: the stream starts after `from`; `tip` is where the main log ends now.
    Ready { tip: Option<ReplicatePosition> },
    /// Main to standby: rows in log order, with the key state of every player they touch.
    /// `live` is false while the closed history is still being sent.
    Batch {
        changes: Vec<ReplicatedChange>,
        keys: Vec<ReplicatedKey>,
        live: bool,
    },
    /// Main to standby: nothing to send now; keeps the line alive.
    Ping,
    /// Standby to main: how far the standby has got, after each batch and each `ping`
    /// (Story 9.20).
    Report { report: crate::StandbyReport },
    /// Main to standby: the stream is refused and closed.
    Refused { code: ErrorCode, message: String },
}

/// The jittered delay before reconnect attempt `attempt` (0 = the first retry): doubles from
/// [`REPLICATE_BACKOFF_MIN_MS`] up to [`REPLICATE_BACKOFF_MAX_MS`], then `jitter` (0.0..=1.0)
/// takes it between 75 % and 100 % of that, never below the minimum.
pub fn replicate_backoff_ms(attempt: u32, jitter: f64) -> u64 {
    let base = REPLICATE_BACKOFF_MIN_MS
        .saturating_mul(1u64 << attempt.min(10))
        .min(REPLICATE_BACKOFF_MAX_MS);
    let scaled = (base as f64 * (0.75 + 0.25 * jitter.clamp(0.0, 1.0))) as u64;
    scaled.clamp(REPLICATE_BACKOFF_MIN_MS, REPLICATE_BACKOFF_MAX_MS)
}

mod b64 {
    use super::*;

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        STANDARD.decode(text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change() -> ReplicatedChange {
        ReplicatedChange {
            epoch: "1:hub".into(),
            seq: 7,
            tbl: "settings".into(),
            row_id: "k".into(),
            op: "update".into(),
            row: vec![0, 1, 2, 255],
            intent_id: "i".into(),
            intent_kind: "k".into(),
            actor_kind: "staff".into(),
            actor_id: "s".into(),
            elevated_by_kind: None,
            elevated_by_id: None,
            elevated_by_mode: None,
            elevated_by_verifier_version: None,
            at: 9,
            business_day: Some("2026-10-31".into()),
            fy: Some("FY2026-27".into()),
            reason: None,
        }
    }

    #[test]
    fn frames_round_trip_and_rows_stay_bytes() {
        let frames = [
            ReplicateFrame::Open {
                token: "t".into(),
                schema_version: 89,
                from: Some(ReplicatePosition {
                    epoch: "1:hub".into(),
                    seq: 6,
                }),
            },
            ReplicateFrame::Ready { tip: None },
            ReplicateFrame::Batch {
                changes: vec![change()],
                keys: vec![
                    ReplicatedKey {
                        player_id: "p1".into(),
                        dek_wrapped: Some("w".into()),
                    },
                    ReplicatedKey {
                        player_id: "p2".into(),
                        dek_wrapped: None,
                    },
                ],
                live: true,
            },
            ReplicateFrame::Ping,
            ReplicateFrame::Report {
                report: crate::StandbyReport {
                    applied_epoch: Some("1:hub".into()),
                    applied_seq: Some(6),
                    lag_rows: 2,
                    lag_seconds: 4,
                    last_row_at: Some(1_800_000_000_000),
                    schema_version: 89,
                    behind: Some(crate::StandbyBehindCause::Lagging),
                },
            },
            ReplicateFrame::Refused {
                code: ErrorCode::SchemaBehind,
                message: "update".into(),
            },
        ];
        for f in frames {
            let json = serde_json::to_string(&f).unwrap();
            assert_eq!(serde_json::from_str::<ReplicateFrame>(&json).unwrap(), f);
        }
        let json = serde_json::to_string(&ReplicateFrame::Refused {
            code: ErrorCode::ReplicateGap,
            message: String::new(),
        })
        .unwrap();
        assert!(json.contains("\"REPLICATE_GAP\""), "{json}");
    }

    #[test]
    fn backoff_doubles_inside_two_to_thirty_seconds() {
        for attempt in 0..40 {
            for jitter in [0.0, 0.5, 1.0] {
                let ms = replicate_backoff_ms(attempt, jitter);
                assert!(
                    (REPLICATE_BACKOFF_MIN_MS..=REPLICATE_BACKOFF_MAX_MS).contains(&ms),
                    "{attempt} {jitter} {ms}"
                );
            }
        }
        assert_eq!(replicate_backoff_ms(0, 1.0), 2_000);
        assert_eq!(replicate_backoff_ms(1, 1.0), 4_000);
        assert_eq!(replicate_backoff_ms(30, 1.0), 30_000);
        assert!(replicate_backoff_ms(3, 0.0) < replicate_backoff_ms(3, 1.0));
    }
}
