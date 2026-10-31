//! What the standby knows about how far it has got (Story 9.20).
//!
//! The follower engine feeds [`StandbyProgress`] as frames arrive; the standby screen, `GET
//! /healthz` and the report sent to the main Hub read it with [`StandbyProgress::report`]. Lag in
//! rows is the main Hub's known log end minus the newest applied row; lag in seconds is how long
//! the standby has been continuously behind. A line that goes quiet counts from the last frame.

use std::sync::{Arc, Mutex};

use arena_core::{Clock, HubStatus};
use arena_proto::{StandbyReport, standby_behind};
use tokio::sync::watch;

/// The standby's progress, in the standby's own clock (ms).
#[derive(Debug, Clone)]
pub struct StandbyProgress {
    started_at: i64,
    schema_version: u32,
    applied: Option<(String, i64)>,
    /// The `seq` the main Hub's log is known to reach in the applied epoch.
    tip_seq: Option<i64>,
    lagging_since: Option<i64>,
    last_frame_at: Option<i64>,
    last_row_at: Option<i64>,
}

impl StandbyProgress {
    pub fn new(now_ms: i64, schema_version: u32) -> Self {
        Self {
            started_at: now_ms,
            schema_version,
            applied: None,
            tip_seq: None,
            lagging_since: None,
            last_frame_at: None,
            last_row_at: None,
        }
    }

    fn lag_rows(&self) -> i64 {
        match (&self.applied, self.tip_seq) {
            (Some((_, applied)), Some(tip)) => (tip - applied).max(0),
            _ => 0,
        }
    }

    fn settle_lag(&mut self, now_ms: i64) {
        self.lagging_since = if self.lag_rows() > 0 {
            Some(self.lagging_since.unwrap_or(now_ms))
        } else {
            None
        };
    }

    /// The database's own newest row, read when the engine starts or a snapshot has loaded.
    pub fn start_at(&mut self, applied: Option<(String, i64)>, now_ms: i64) {
        self.tip_seq = applied.as_ref().map(|(_, seq)| *seq);
        self.applied = applied;
        self.settle_lag(now_ms);
    }

    /// The main Hub said where its log ends.
    pub fn ready(&mut self, tip: Option<(String, i64)>, now_ms: i64) {
        self.last_frame_at = Some(now_ms);
        if let Some((epoch, seq)) = tip
            && self.applied.as_ref().is_none_or(|(e, _)| *e == epoch)
        {
            self.tip_seq = Some(seq);
        }
        self.settle_lag(now_ms);
    }

    /// A batch was applied. `live` says the main Hub has nothing more to send now.
    pub fn batch(&mut self, applied: Option<(String, i64)>, rows: usize, live: bool, now_ms: i64) {
        self.last_frame_at = Some(now_ms);
        if rows > 0 {
            self.last_row_at = Some(now_ms);
        }
        if applied.is_some() {
            self.applied = applied;
        }
        if let Some((_, seq)) = &self.applied
            && (live || self.tip_seq.is_none_or(|t| t < *seq))
        {
            self.tip_seq = Some(*seq);
        }
        self.settle_lag(now_ms);
    }

    /// Any other frame from the main Hub (a `ping`).
    pub fn heard(&mut self, now_ms: i64) {
        self.last_frame_at = Some(now_ms);
    }

    /// How long the line from the main Hub has been quiet at `now_ms`, counting from the last
    /// frame, or from when the standby started if it has heard none (Story 9.21). The main Hub
    /// pings at least every 10 s while it holds its lease, so this is how long the lease has not
    /// been renewed, as far as the standby can tell.
    pub fn silent_ms(&self, now_ms: i64) -> i64 {
        (now_ms - self.last_frame_at.unwrap_or(self.started_at)).max(0)
    }

    /// The standby's report at `now_ms`.
    pub fn report(&self, now_ms: i64) -> StandbyReport {
        let lag_rows = self.lag_rows();
        let lag_seconds = self
            .lagging_since
            .map_or(0, |since| (now_ms - since).max(0) / 1000);
        let silent_seconds = (now_ms - self.last_frame_at.unwrap_or(self.started_at)).max(0) / 1000;
        StandbyReport {
            applied_epoch: self.applied.as_ref().map(|(e, _)| e.clone()),
            applied_seq: self.applied.as_ref().map(|(_, s)| *s),
            lag_rows,
            lag_seconds,
            last_row_at: self.last_row_at,
            schema_version: self.schema_version,
            behind: standby_behind(lag_rows, lag_seconds, silent_seconds),
        }
    }
}

/// The progress the engine and the status publisher share.
pub type SharedProgress = Arc<Mutex<StandbyProgress>>;

/// Puts the standby's report into the Hub status (`following`), which `GET /healthz` and the
/// standby screen read. Call it about once a second while the standby runs.
pub fn publish_following(
    progress: &SharedProgress,
    clock: &dyn Clock,
    status: &watch::Sender<HubStatus>,
) {
    let report = progress
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .report(clock.now_ms());
    status.send_if_modified(|s| {
        if s.following.as_ref() == Some(&report) {
            return false;
        }
        s.following = Some(report);
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_proto::StandbyBehindCause;

    const T0: i64 = 1_800_000_000_000;

    fn at(epoch: &str, seq: i64) -> Option<(String, i64)> {
        Some((epoch.to_owned(), seq))
    }

    #[test]
    fn a_new_standby_reports_no_lag_and_no_warning() {
        let p = StandbyProgress::new(T0, 90);
        let r = p.report(T0 + 5_000);
        assert_eq!((r.lag_rows, r.lag_seconds, r.behind), (0, 0, None));
        assert_eq!(r.schema_version, 90);
    }

    #[test]
    fn lag_is_the_known_log_end_minus_the_applied_row_and_counts_seconds() {
        let mut p = StandbyProgress::new(T0, 90);
        p.start_at(at("1:a", 10), T0);
        p.ready(at("1:a", 40), T0 + 1_000);
        p.batch(at("1:a", 20), 10, false, T0 + 2_000);
        let r = p.report(T0 + 12_000);
        assert_eq!(r.lag_rows, 20);
        assert_eq!(r.lag_seconds, 11);
        assert_eq!(r.applied_seq, Some(20));
        assert_eq!(r.last_row_at, Some(T0 + 2_000));
        assert_eq!(r.behind, None);
    }

    #[test]
    fn rows_waiting_for_more_than_30_seconds_warn_and_catching_up_clears_it() {
        let mut p = StandbyProgress::new(T0, 90);
        p.start_at(at("1:a", 10), T0);
        p.ready(at("1:a", 40), T0);
        p.batch(at("1:a", 20), 10, false, T0 + 25_000);
        p.heard(T0 + 35_000);
        let r = p.report(T0 + 36_000);
        assert_eq!(r.behind, Some(StandbyBehindCause::Lagging));
        p.batch(at("1:a", 40), 20, true, T0 + 37_000);
        let r = p.report(T0 + 37_000);
        assert_eq!((r.lag_rows, r.lag_seconds, r.behind), (0, 0, None));
    }

    #[test]
    fn a_quiet_line_for_more_than_60_seconds_warns_with_no_stream() {
        let mut p = StandbyProgress::new(T0, 90);
        p.start_at(at("1:a", 10), T0);
        p.batch(None, 0, true, T0 + 1_000);
        assert_eq!(p.report(T0 + 61_000).behind, None);
        assert_eq!(
            p.report(T0 + 62_000).behind,
            Some(StandbyBehindCause::NoStream)
        );
        p.heard(T0 + 63_000);
        assert_eq!(p.report(T0 + 64_000).behind, None);
    }

    #[test]
    fn a_standby_that_never_connected_warns_after_60_seconds() {
        let p = StandbyProgress::new(T0, 90);
        assert_eq!(
            p.report(T0 + 61_000).behind,
            Some(StandbyBehindCause::NoStream)
        );
    }

    #[test]
    fn the_report_reaches_the_hub_status_once_per_change() {
        let (tx, rx) = arena_core::status_channel(HubStatus::opened(90, "01JSELF"));
        let progress: SharedProgress = Arc::new(Mutex::new(StandbyProgress::new(T0, 90)));
        let clock = arena_core::ManualClock::new(T0);
        publish_following(&progress, &clock, &tx);
        assert_eq!(rx.borrow().following.as_ref().map(|r| r.lag_rows), Some(0));
        let mut seen = rx.clone();
        seen.mark_unchanged();
        publish_following(&progress, &clock, &tx);
        assert!(!seen.has_changed().unwrap(), "same report, no wake-up");
    }
}
