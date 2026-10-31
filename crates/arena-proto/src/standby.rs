//! How far behind a standby Hub is, and when to say so (Story 9.20, AD-4).
//!
//! The standby counts its own lag and reports it to the main Hub on the replication stream; both
//! Hubs show it on their standby screens and in `GET /healthz`. The warning rule is here once, so
//! the two screens cannot disagree.

use serde::{Deserialize, Serialize};

/// Lag above 0 rows for longer than this is "Standby behind".
pub const STANDBY_LAG_WARN_AFTER_S: i64 = 30;
/// No frame from the main Hub for longer than this is "Standby behind". The main Hub pings every
/// 10 s, so a healthy stream never stays quiet this long.
pub const STANDBY_SILENT_WARN_AFTER_S: i64 = 60;
/// `POST`, with `Authorization: Bearer <hub token>`: a standby Hub refreshes its own licence.
pub const STANDBY_LICENCE_PATH: &str = "/standby/licence";

/// Why a standby shows "Standby behind".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandbyBehindCause {
    /// Rows are waiting on the main Hub and have been for more than 30 s.
    Lagging,
    /// Nothing has arrived from the main Hub for more than 60 s.
    NoStream,
}

impl StandbyBehindCause {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lagging => "lagging",
            Self::NoStream => "no_stream",
        }
    }
}

/// The warning for a standby `lag_rows` behind for `lag_seconds`, whose last frame from the main
/// Hub came `silent_seconds` ago. A silent line wins: it explains the lag too.
pub fn standby_behind(
    lag_rows: i64,
    lag_seconds: i64,
    silent_seconds: i64,
) -> Option<StandbyBehindCause> {
    if silent_seconds > STANDBY_SILENT_WARN_AFTER_S {
        Some(StandbyBehindCause::NoStream)
    } else if lag_rows > 0 && lag_seconds > STANDBY_LAG_WARN_AFTER_S {
        Some(StandbyBehindCause::Lagging)
    } else {
        None
    }
}

/// What a standby knows about itself, at one moment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandbyReport {
    /// The newest change applied: its epoch and `seq`.
    pub applied_epoch: Option<String>,
    pub applied_seq: Option<i64>,
    /// Rows the main Hub holds that this standby does not.
    pub lag_rows: i64,
    /// How long it has been behind (0 when `lag_rows` is 0).
    pub lag_seconds: i64,
    /// When the last row was received (standby clock, ms).
    pub last_row_at: Option<i64>,
    pub schema_version: u32,
    pub behind: Option<StandbyBehindCause>,
}

/// What the main Hub shows for its linked standby, in Settings -> System -> Standby and in
/// `GET /ui/standby` (Story 9.20).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandbyPanel {
    pub device_id: String,
    /// The standby PC's name.
    pub name: String,
    pub linked_at: i64,
    /// The replication stream is open now.
    pub connected: bool,
    /// When the main Hub last heard from the standby (main Hub clock, ms).
    pub last_contact_at: Option<i64>,
    /// The app version the standby reported.
    pub version: Option<String>,
    pub schema_version: Option<u32>,
    pub lag_rows: i64,
    pub lag_seconds: i64,
    pub last_row_at: Option<i64>,
    pub behind: Option<StandbyBehindCause>,
}

/// The standby PC's own screen (Story 9.20): "Standby — following <main Hub name>", the lag, the
/// last row received, the schema version and the licence state. The floor is read-only there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandbyScreen {
    /// The main Hub's name, from the replicated branch.
    pub following: Option<String>,
    pub report: StandbyReport,
    /// The standby's own licence standing (`verified`, `expired`, `unverified`), once it has one.
    pub licence: Option<String>,
}

/// `GET /ui/standby`: the standby screen on a standby Hub, the System -> Standby panel on the main
/// Hub that has a linked standby, and neither otherwise.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandbyView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen: Option<StandbyScreen>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel: Option<StandbyPanel>,
}

/// The standby's lag as `GET /healthz` states it, on either Hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandbyLine {
    pub lag_rows: i64,
    pub lag_seconds: i64,
    pub behind: Option<StandbyBehindCause>,
}

/// The body of `POST /v1/standby/licence`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandbyLicenceRequest {
    pub version: String,
    pub schema_version: u32,
}

/// The answer: the branch's signed licence, as the main Hub's heartbeat gets it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandbyLicenceResponse {
    pub licence: String,
    pub cloud_time: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_caught_up_standby_with_a_live_line_is_not_behind() {
        assert_eq!(standby_behind(0, 0, 3), None);
        assert_eq!(standby_behind(0, 500, 60), None);
    }

    #[test]
    fn rows_waiting_for_more_than_30_seconds_are_lagging() {
        assert_eq!(standby_behind(12, 30, 1), None);
        assert_eq!(standby_behind(12, 31, 1), Some(StandbyBehindCause::Lagging));
        assert_eq!(standby_behind(0, 31, 1), None);
    }

    #[test]
    fn a_line_quiet_for_more_than_60_seconds_is_no_stream() {
        assert_eq!(standby_behind(0, 0, 61), Some(StandbyBehindCause::NoStream));
        assert_eq!(
            standby_behind(5, 99, 61),
            Some(StandbyBehindCause::NoStream)
        );
    }
}
