//! Fetching a paid branch's cloud backup onto a replacement Hub (Story 9.12, AD-36, AD-4).
//!
//! `POST /v1/restore/plan` (Hub token) lists the branch's own prefix of the data bucket and
//! answers with read URLs valid for [`crate::PRESIGN_TTL_S`] seconds. The cloud keeps no manifest
//! and a snapshot's last change is inside the file, so the Hub asks twice: first without `after`
//! (the snapshot and the key backup, `batches_pending`), then with `after` set to the position it
//! read from the downloaded snapshot (the batches after it). The same call re-signs expired URLs.
//!
//! The Hub stages the objects on disk and shows [`RestoreFetchView`] through the loopback
//! `/ui/restore/cloud` routes. Nothing is replayed here (Story 9.13).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A point in the change log: an epoch and a `seq` in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RestorePosition {
    pub epoch: String,
    #[ts(type = "number")]
    pub seq: i64,
}

/// The payload of `POST /v1/restore/plan`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RestorePlanRequest {
    /// Must be the calling Hub's own instance.
    pub hub_instance_id: String,
    /// The last change the downloaded snapshot holds; batches after it are listed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub after: Option<RestorePosition>,
}

/// The newest snapshot and a link to read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RestorePlanSnapshot {
    /// The business day it was taken for (`yyyy-mm-dd`).
    pub date: String,
    pub url: String,
}

/// One change-log batch and a link to read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RestorePlanBatch {
    pub epoch: String,
    #[ts(type = "number")]
    pub first: i64,
    #[ts(type = "number")]
    pub last: i64,
    pub url: String,
}

/// What the cloud holds for the branch and how to read it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RestorePlan {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub snapshot: Option<RestorePlanSnapshot>,
    /// `keys/player-keys.bin`, when the branch has stored one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub keys_url: Option<String>,
    /// In log order: by epoch counter, then `first`.
    #[serde(default)]
    pub batches: Vec<RestorePlanBatch>,
    /// A snapshot exists and the request named no `after`: ask again with the snapshot's position.
    #[serde(default)]
    pub batches_pending: bool,
    /// How long the links stay valid, in seconds.
    pub expires_in_s: u64,
}

/// Where the staging of a cloud backup stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
#[serde(rename_all = "snake_case")]
pub enum RestoreFetchPhase {
    /// Objects are being downloaded and checked.
    Downloading,
    /// A batch is missing or damaged; waiting for the Owner to choose.
    Gap,
    /// Everything chosen is staged (Story 9.13 replays it).
    Ready,
    /// The cloud refused the plan and retrying will not help (see `error`).
    Failed,
}

/// The first bad range of a backup and what stopping before it costs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RestoreGapView {
    pub epoch: String,
    /// The first seq that is missing or damaged.
    #[ts(type = "number")]
    pub from_seq: i64,
    /// The last seq of the bad range.
    #[ts(type = "number")]
    pub to_seq: i64,
    /// The last seq that is fine before the range (the snapshot's when no batch precedes it).
    #[ts(type = "number")]
    pub last_good_seq: i64,
    /// Changes that stopping at `last_good_seq` would lose.
    #[ts(type = "number")]
    pub lost: i64,
}

/// A fully staged backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RestoreReadyView {
    /// The snapshot's date, when the backup has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub snapshot_date: Option<String>,
    pub batches: u32,
    #[ts(type = "number")]
    pub last_seq: i64,
    /// The Owner chose to stop before a gap.
    pub partial: bool,
}

/// The progress panel of "Restore from cloud backup" (`GET /ui/restore/cloud`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RestoreFetchView {
    pub phase: RestoreFetchPhase,
    /// Objects finished, and in all (snapshot, key backup and batches).
    pub done: u32,
    pub total: u32,
    #[ts(type = "number")]
    pub bytes: u64,
    /// `done` of `total`, 0 to 100.
    pub percent: u8,
    /// The last failed attempt, as a code (never a URL); the download keeps retrying.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub retrying: Option<String>,
    /// The error code that ended the attempt (`phase` `failed`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub gap: Option<RestoreGapView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ready: Option<RestoreReadyView>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_request_is_strict_and_after_is_optional() {
        let r: RestorePlanRequest = serde_json::from_str(r#"{"hub_instance_id":"H"}"#).unwrap();
        assert_eq!(r.after, None);
        assert!(
            serde_json::from_str::<RestorePlanRequest>(r#"{"hub_instance_id":"H","tenant":"x"}"#)
                .is_err()
        );
        let r: RestorePlanRequest =
            serde_json::from_str(r#"{"hub_instance_id":"H","after":{"epoch":"2:H","seq":9}}"#)
                .unwrap();
        assert_eq!(r.after.unwrap().seq, 9);
    }

    #[test]
    fn the_phase_is_snake_case_on_the_wire() {
        assert_eq!(
            serde_json::to_string(&RestoreFetchPhase::Downloading).unwrap(),
            "\"downloading\""
        );
    }
}
