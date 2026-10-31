//! What the main Hub knows about its linked standby (Story 9.20).
//!
//! The replication stream tells the observer when the standby connects, reports and leaves; the
//! System -> Standby panel (`GET /ui/standby`) and `GET /healthz` read it. The warning rule is the
//! shared [`standby_behind`], with the silence measured here on the main Hub's own clock, so a
//! standby that stops reporting is flagged even though it can no longer say so itself.

use std::sync::{Arc, Mutex};

use arena_proto::{StandbyLine, StandbyPanel, StandbyReport, standby_behind};
use arena_store::LinkedStandby;

#[derive(Debug, Default)]
struct Seen {
    device_id: String,
    connected: bool,
    last_contact_at: Option<i64>,
    schema_version: Option<u32>,
    report: Option<StandbyReport>,
}

/// A shared handle on what the stream saw of the standby.
#[derive(Debug, Clone, Default)]
pub struct StandbyObserver(Arc<Mutex<Seen>>);

impl StandbyObserver {
    fn with<R>(&self, f: impl FnOnce(&mut Seen) -> R) -> R {
        f(&mut self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner))
    }

    /// The standby `device_id` opened the stream.
    pub fn connected(&self, device_id: &str, schema_version: u32, now_ms: i64) {
        self.with(|s| {
            if s.device_id != device_id {
                *s = Seen {
                    device_id: device_id.to_owned(),
                    ..Seen::default()
                };
            }
            s.connected = true;
            s.last_contact_at = Some(now_ms);
            s.schema_version = Some(schema_version);
        });
    }

    /// The standby reported how far it has got.
    pub fn reported(&self, device_id: &str, report: StandbyReport, now_ms: i64) {
        self.with(|s| {
            if s.device_id != device_id {
                return;
            }
            s.last_contact_at = Some(now_ms);
            s.schema_version = Some(report.schema_version);
            s.report = Some(report);
        });
    }

    /// The stream of `device_id` ended.
    pub fn closed(&self, device_id: &str) {
        self.with(|s| {
            if s.device_id == device_id {
                s.connected = false;
            }
        });
    }

    fn view(&self, linked_at: Option<i64>, now_ms: i64) -> (StandbyLine, Seen) {
        self.with(|s| {
            let (lag_rows, lag_seconds) = s
                .report
                .as_ref()
                .map_or((0, 0), |r| (r.lag_rows, r.lag_seconds));
            let since = s.last_contact_at.or(linked_at).unwrap_or(now_ms);
            let silent = (now_ms - since).max(0) / 1000;
            let line = StandbyLine {
                lag_rows,
                lag_seconds,
                behind: standby_behind(lag_rows, lag_seconds, silent),
            };
            let seen = Seen {
                device_id: s.device_id.clone(),
                connected: s.connected,
                last_contact_at: s.last_contact_at,
                schema_version: s.schema_version,
                report: s.report.clone(),
            };
            (line, seen)
        })
    }

    /// The standby's lag for `GET /healthz`; `None` before any standby has connected.
    pub fn line(&self, now_ms: i64) -> Option<StandbyLine> {
        let (line, seen) = self.view(None, now_ms);
        seen.last_contact_at.map(|_| line)
    }

    /// The panel for the linked standby `linked`.
    pub fn panel(&self, linked: &LinkedStandby, now_ms: i64) -> StandbyPanel {
        let (line, seen) = self.view(Some(linked.linked_at), now_ms);
        // What was seen belongs to whichever device connected last; ignore it for another one.
        let ours = seen.device_id == linked.device_id;
        let report = seen.report.filter(|_| ours);
        StandbyPanel {
            device_id: linked.device_id.clone(),
            name: linked.label.clone(),
            linked_at: linked.linked_at,
            connected: ours && seen.connected,
            last_contact_at: seen.last_contact_at.filter(|_| ours),
            version: linked.reported_version.clone(),
            schema_version: seen.schema_version.filter(|_| ours),
            lag_rows: if ours { line.lag_rows } else { 0 },
            lag_seconds: if ours { line.lag_seconds } else { 0 },
            last_row_at: report.and_then(|r| r.last_row_at),
            behind: if ours {
                line.behind
            } else {
                standby_behind(0, 0, (now_ms - linked.linked_at).max(0) / 1000)
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_proto::StandbyBehindCause;

    const T0: i64 = 1_800_000_000_000;

    fn linked() -> LinkedStandby {
        LinkedStandby {
            device_id: "dev-s".into(),
            label: "Back office PC".into(),
            linked_at: T0 - 600_000,
            reported_version: Some("0.9.1".into()),
        }
    }

    fn report(lag_rows: i64, lag_seconds: i64) -> StandbyReport {
        StandbyReport {
            applied_epoch: Some("1:a".into()),
            applied_seq: Some(40),
            lag_rows,
            lag_seconds,
            last_row_at: Some(T0 - 2_000),
            schema_version: 90,
            behind: None,
        }
    }

    #[test]
    fn a_linked_standby_that_never_connected_shows_no_contact() {
        let o = StandbyObserver::default();
        assert_eq!(o.line(T0), None);
        let p = o.panel(&linked(), T0 - 590_000);
        assert!(!p.connected);
        assert_eq!((p.last_contact_at, p.schema_version), (None, None));
        assert_eq!(p.name, "Back office PC");
        assert_eq!(p.behind, None);
        // Linked for over a minute and never heard from: "Standby behind", no stream.
        assert_eq!(
            o.panel(&linked(), T0).behind,
            Some(StandbyBehindCause::NoStream)
        );
    }

    #[test]
    fn the_panel_shows_the_standbys_own_report_and_the_contact_time() {
        let o = StandbyObserver::default();
        o.connected("dev-s", 90, T0 - 5_000);
        o.reported("dev-s", report(0, 0), T0 - 1_000);
        let p = o.panel(&linked(), T0);
        assert!(p.connected);
        assert_eq!(p.last_contact_at, Some(T0 - 1_000));
        assert_eq!((p.lag_rows, p.lag_seconds, p.behind), (0, 0, None));
        assert_eq!(p.last_row_at, Some(T0 - 2_000));
        assert_eq!(p.version.as_deref(), Some("0.9.1"));
        assert_eq!(o.line(T0).unwrap().lag_rows, 0);
    }

    #[test]
    fn lag_over_30_seconds_and_a_silent_line_over_60_seconds_both_warn() {
        let o = StandbyObserver::default();
        o.connected("dev-s", 90, T0);
        o.reported("dev-s", report(7, 45), T0);
        assert_eq!(
            o.panel(&linked(), T0 + 1_000).behind,
            Some(StandbyBehindCause::Lagging)
        );
        o.reported("dev-s", report(0, 0), T0 + 2_000);
        assert_eq!(o.panel(&linked(), T0 + 3_000).behind, None);
        // The standby stops reporting; the main Hub's own clock notices.
        o.closed("dev-s");
        let p = o.panel(&linked(), T0 + 70_000);
        assert!(!p.connected);
        assert_eq!(p.behind, Some(StandbyBehindCause::NoStream));
        assert_eq!(o.line(T0 + 70_000).unwrap().behind, p.behind);
    }

    #[test]
    fn another_device_does_not_borrow_the_linked_standbys_numbers() {
        let o = StandbyObserver::default();
        o.connected("other", 90, T0);
        o.reported("other", report(99, 99), T0);
        let p = o.panel(&linked(), T0 + 1_000);
        assert!(!p.connected);
        assert_eq!((p.lag_rows, p.last_contact_at), (0, None));
        // A report from a device that did not connect first is ignored.
        o.reported("dev-s", report(5, 5), T0);
        assert_eq!(o.panel(&linked(), T0 + 1_000).lag_rows, 0);
    }
}
