//! The cloud backup status the Hub header and System -> Backup show (Story 9.16, FR175).
//!
//! `GET /ui/backup/status` answers it from the Hub's own bookkeeping (`upload_cursor`,
//! `snapshot_log`, `backup_state`, `gst_archive_log`), so it reads the same whether or not the
//! cloud can be reached now. A Free Hub uploads nothing: `cloud` is false and every other field is
//! empty.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Which upload the last error belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
#[serde(rename_all = "snake_case")]
pub enum BackupErrorSource {
    /// The change-log batches, the summary, the rollups, the snapshot or the GST archive.
    Upload,
    /// The player key backup.
    Keys,
    /// The full snapshot an upgrade from Free stores before the first batch.
    FirstSnapshot,
}

/// The last thing that went wrong with a backup: a short code, never player data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BackupError {
    pub source: BackupErrorSource,
    pub code: String,
    /// When (Hub clock, ms) it happened; `null` when the Hub did not note the time.
    #[ts(type = "number | null")]
    pub at: Option<i64>,
}

/// How far the branch's cloud backup has got.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BackupStatus {
    /// This Hub is on a paid tier, so it backs up to the cloud. False on Free.
    pub cloud: bool,
    /// The Hub clock when this was read (ms).
    #[ts(type = "number")]
    pub as_of: i64,
    /// The tier's upload interval in seconds; 0 when `cloud` is false.
    pub upload_interval_s: u32,
    /// When the cloud last accepted a batch (Hub clock, ms).
    #[ts(type = "number | null")]
    pub last_upload_at: Option<i64>,
    /// When the branch summary was last published.
    #[ts(type = "number | null")]
    pub last_summary_at: Option<i64>,
    /// When the last daily snapshot was stored, and the business day it is for.
    #[ts(type = "number | null")]
    pub last_snapshot_at: Option<i64>,
    pub last_snapshot_date: Option<String>,
    /// When the player key backup last succeeded.
    #[ts(type = "number | null")]
    pub last_keys_at: Option<i64>,
    /// When the newest yearly GST archive was stored.
    #[ts(type = "number | null")]
    pub last_gst_archive_at: Option<i64>,
    /// Change rows not yet in an accepted batch.
    #[ts(type = "number")]
    pub pending_rows: i64,
    /// Those rows as batches (the most one batch holds is fixed).
    #[ts(type = "number")]
    pub pending_batches: i64,
    /// The branch is paid but its first full snapshot is not stored yet, so no batch goes up.
    pub first_backup_pending: bool,
    /// The most recent failure, if any backup step failed since it last worked.
    pub error: Option<BackupError>,
}

impl BackupStatus {
    /// What a Free Hub (or one that cannot tell) answers: no cloud backup.
    pub fn local_only(as_of: i64) -> Self {
        Self {
            cloud: false,
            as_of,
            upload_interval_s: 0,
            last_upload_at: None,
            last_summary_at: None,
            last_snapshot_at: None,
            last_snapshot_date: None,
            last_keys_at: None,
            last_gst_archive_at: None,
            pending_rows: 0,
            pending_batches: 0,
            first_backup_pending: false,
            error: None,
        }
    }

    /// The last upload is older than twice the interval: "Backup behind" (a Hub that has never
    /// uploaded is "first backup pending", not behind).
    pub fn behind(&self, now: i64) -> bool {
        let Some(at) = self.last_upload_at else {
            return false;
        };
        self.cloud
            && self.upload_interval_s > 0
            && now.saturating_sub(at) > 2 * i64::from(self.upload_interval_s) * 1000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paid(last: Option<i64>) -> BackupStatus {
        BackupStatus {
            cloud: true,
            upload_interval_s: 900,
            last_upload_at: last,
            ..BackupStatus::local_only(0)
        }
    }

    #[test]
    fn behind_is_more_than_twice_the_interval_after_the_last_upload() {
        let at = 1_000_000;
        assert!(
            !paid(Some(at)).behind(at + 1_800_000),
            "exactly twice is not behind"
        );
        assert!(paid(Some(at)).behind(at + 1_800_001));
        assert!(
            !paid(None).behind(i64::MAX),
            "never uploaded is first backup pending"
        );
        assert!(
            !BackupStatus::local_only(0).behind(i64::MAX),
            "Free is never behind"
        );
    }
}
