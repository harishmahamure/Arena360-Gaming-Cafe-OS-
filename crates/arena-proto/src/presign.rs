//! `POST /v1/presign` (Story 8.1): a paid Hub asks the cloud for short-lived upload links under
//! its own branch prefix of the data bucket. The tenant and branch are never in the request;
//! the cloud takes them from the device token.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How long a presigned PUT URL stays valid, in seconds.
pub const PRESIGN_TTL_S: u64 = 600;

/// The `kind` of a request for a daily rollup.
pub const PRESIGN_KIND_ROLLUP: &str = "rollup";
/// The `kind` of a request for a daily snapshot.
pub const PRESIGN_KIND_SNAPSHOT: &str = "snapshot";

/// A change-log batch: the first and last sequence numbers it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PresignBatch {
    #[ts(type = "number")]
    pub first: i64,
    #[ts(type = "number")]
    pub last: i64,
}

/// What the Hub wants links for: a batch and/or the branch summary (`epoch`, `batch`,
/// `summary`), or one dated file (`epoch`, `kind`, `date`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PresignRequest {
    /// The Hub's epoch, `<counter>:<instance>`.
    #[serde(default)]
    pub epoch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub batch: Option<PresignBatch>,
    #[serde(default)]
    pub summary: bool,
    /// `rollup` or `snapshot` (with `date`), or `keys` (without one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub kind: Option<String>,
    /// `yyyy-mm-dd`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub date: Option<String>,
}

/// The links issued. Each is valid for [`PRESIGN_TTL_S`] seconds.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PresignResponse {
    /// For the batch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub batch_url: Option<String>,
    /// For `state/latest.json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub summary_url: Option<String>,
    /// For the rollup or snapshot asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub url: Option<String>,
    /// The request also asked for the summary, but the Hub's epoch is below the branch fence, so
    /// no summary link was issued (the batch link was). Absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub summary_fenced: Option<bool>,
}
