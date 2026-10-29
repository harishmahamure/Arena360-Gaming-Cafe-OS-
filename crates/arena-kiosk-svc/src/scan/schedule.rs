//! When the scan runs, and what happens to its result (Story 6.4).
//!
//! - **First run:** once, after the service's first successful Hub connection, when no scan is
//!   recorded.
//! - **Daily:** when the branch-time 05:00 after the last scan has passed. The branch time zone
//!   is `branch.tz`, read from the Hub and cached in `queue.db` (unknown means UTC).
//! - **Never during a session:** a run that is due while a session is on the PC waits for the
//!   next moment with none (the scheduler checks every [`Timings::scan_tick`]), and a running
//!   scan waits between folders while one is active ([`SessionPause`]).
//! - **Result:** queued in `queue.db` as a `kiosk.scan_reported` intent with its own
//!   idempotency key, so a Hub that is down never loses it, and the scan time is recorded under
//!   `scan.last_at`.
//!
//! [`Timings::scan_tick`]: crate::runtime::Timings

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use arena_proto::{KEY_BRANCH_TZ, KioskHubState, KioskLogLevel, KioskScanReported, SettingEntry};
use chrono::{LocalResult, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use tokio::sync::watch;

use super::{Cancelled, Pause, ScanHost, scan};
use crate::procs::AppliedRevs;
use crate::queue::QueueDb;
use crate::runtime::Ctx;

/// The `queue.db` key that holds the time (ms) the last scan finished.
pub const LAST_SCAN_KEY: &str = "scan.last_at";
/// The settings the schedule caches.
pub const KEYS: [&str; 1] = [KEY_BRANCH_TZ];
/// The local hour of the daily scan.
pub const DAILY_HOUR: u32 = 5;
/// The longest report the service sends, in bytes of JSON: below the Hub's frame limit (1 MiB)
/// with room for the envelope. A (very unlikely) larger one loses entries from its end.
pub const MAX_REPORT_BYTES: usize = 800_000;
/// How long a failed run waits before the next try.
pub const RETRY_AFTER_MS: i64 = 60 * 60 * 1000;

/// The branch time zone cached in `queue.db`; UTC when none is cached or it is unknown.
pub fn load_tz(queue: &QueueDb) -> Tz {
    match queue.kv_get(KEY_BRANCH_TZ) {
        Ok(Some((value, _))) => value.parse().unwrap_or_else(|_| {
            tracing::warn!(tz = %value, "the cached branch time zone is unknown; using UTC");
            Tz::UTC
        }),
        Ok(None) => Tz::UTC,
        Err(e) => {
            tracing::error!(error = %e, "the branch time zone cache cannot be read; using UTC");
            Tz::UTC
        }
    }
}

/// Seeds the applied revision of the zone from the cache.
pub fn seed(queue: &QueueDb, revs: &mut AppliedRevs) {
    for key in KEYS {
        revs.seed(queue, key);
    }
}

/// Caches the branch time zone in `values` when it is newer than the applied one and valid.
pub fn apply(ctx: &Ctx, values: &[SettingEntry], revs: &mut AppliedRevs) {
    for entry in values.iter().filter(|e| KEYS.contains(&e.key.as_str())) {
        if revs.get(&entry.key).is_some_and(|r| entry.rev < r) {
            tracing::warn!(key = %entry.key, rev = entry.rev, "ignoring an older branch time zone");
            continue;
        }
        if entry.value.parse::<Tz>().is_err() {
            tracing::error!(key = %entry.key, tz = %entry.value, "the hub sent an unknown time zone; keeping the previous one");
            continue;
        }
        revs.record(&entry.key, entry.rev);
        match ctx.queue.lock().unwrap_or_else(|p| p.into_inner()).kv_set(
            &entry.key,
            &entry.value,
            entry.rev,
        ) {
            Ok(()) => tracing::info!(tz = %entry.value, "branch time zone stored"),
            Err(e) => tracing::error!(error = %e, "could not cache the branch time zone"),
        }
    }
}

/// The 05:00 on `date` in `tz` (the earlier one when the clocks repeat it; the first valid
/// minute after it when the clocks skip it).
fn daily_at(tz: Tz, date: NaiveDate) -> Option<i64> {
    for minute in 0..=180 {
        let naive = date.and_hms_opt(DAILY_HOUR, minute, 0)?;
        match tz.from_local_datetime(&naive) {
            LocalResult::Single(t) => return Some(t.timestamp_millis()),
            LocalResult::Ambiguous(first, _) => return Some(first.timestamp_millis()),
            LocalResult::None => {}
        }
    }
    None
}

/// The first 05:00 in `tz` strictly after `last_ms`, in milliseconds since the epoch.
pub fn next_daily_after(last_ms: i64, tz: Tz) -> i64 {
    let Some(last) = Utc.timestamp_millis_opt(last_ms).single() else {
        return last_ms.saturating_add(24 * 60 * 60 * 1000);
    };
    let mut date = last.with_timezone(&tz).date_naive();
    for _ in 0..4 {
        if let Some(t) = daily_at(tz, date)
            && t > last_ms
        {
            return t;
        }
        let Some(next) = date.succ_opt() else { break };
        date = next;
    }
    last_ms.saturating_add(24 * 60 * 60 * 1000)
}

/// Whether a scan is due. With no recorded scan it is due once the service has connected to
/// the Hub; with one, once the daily time after it has passed.
pub fn is_due(last_scan: Option<i64>, connected_once: bool, now_ms: i64, tz: Tz) -> bool {
    // A recorded time in the future (the clock was ahead) counts as no record.
    match last_scan.filter(|l| *l <= now_ms) {
        None => connected_once,
        Some(last) => now_ms >= next_daily_after(last, tz),
    }
}

/// The recorded time of the last scan.
pub fn last_scan(queue: &QueueDb) -> Option<i64> {
    match queue.kv_get(LAST_SCAN_KEY) {
        Ok(Some((value, _))) => value.parse().ok(),
        Ok(None) => None,
        Err(e) => {
            tracing::error!(error = %e, "the last scan time cannot be read");
            None
        }
    }
}

/// Whether a session is on this PC (a session screen the Hub sent that has not ended).
pub fn session_active(ctx: &Ctx) -> bool {
    ctx.pc.screen.borrow().session.is_some()
}

/// Waits between folders while a session is on the PC; cancelled when the service stops.
pub struct SessionPause {
    ctx: Arc<Ctx>,
    cancel: Arc<AtomicBool>,
    poll: Duration,
}

impl Pause for SessionPause {
    fn checkpoint(&self) -> Result<(), Cancelled> {
        let mut logged = false;
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(Cancelled);
            }
            if !session_active(&self.ctx) {
                if logged {
                    tracing::info!("scan: the session ended; resuming");
                }
                return Ok(());
            }
            if !logged {
                tracing::info!("scan: a session is on this PC; paused until it ends");
                logged = true;
            }
            std::thread::sleep(self.poll);
        }
    }
}

/// The intent bytes of `report` as the Hub would receive them.
fn encoded_len(intent: &arena_proto::Intent) -> usize {
    serde_json::to_vec(intent).map_or(usize::MAX, |b| b.len())
}

/// Queues `report` (finished at `now`), trimmed to fit a Hub frame, and records the scan time.
/// Returns the number of entries kept.
fn queue_report(
    ctx: &Ctx,
    device_id: &str,
    now: i64,
    mut report: KioskScanReported,
) -> Result<usize, String> {
    let mut intent = crate::intents::kiosk_scan_reported(device_id, now, report.clone());
    while encoded_len(&intent) > MAX_REPORT_BYTES && !report.entries.is_empty() {
        let keep = report.entries.len() * 3 / 4;
        report.entries.truncate(keep);
        intent = crate::intents::kiosk_scan_reported(device_id, now, report.clone());
    }
    report
        .validate(now)
        .map_err(|e| format!("the scan report is invalid: {e:?}"))?;
    let queue = ctx.queue.lock().unwrap_or_else(|p| p.into_inner());
    queue
        .push(&intent)
        .map_err(|e| format!("the scan report cannot be queued: {e}"))?;
    queue
        .kv_set(LAST_SCAN_KEY, &now.to_string(), 0)
        .map_err(|e| format!("the scan time cannot be recorded: {e}"))?;
    // Story 6.26: the launcher cards open the launchers this scan found.
    let launchers = serde_json::to_string(&report.launchers)
        .map_err(|e| format!("the scan launchers cannot be recorded: {e}"))?;
    queue
        .kv_set(crate::launchers::SCAN_KEY, &launchers, 0)
        .map_err(|e| format!("the scan launchers cannot be recorded: {e}"))?;
    drop(queue);
    ctx.queued.notify_one();
    Ok(report.entries.len())
}

/// Scans on a blocking thread and queues the report. `Ok(None)` when the service stopped first.
async fn run_once(
    ctx: &Arc<Ctx>,
    host: &Arc<dyn ScanHost>,
    device_id: &str,
    stop: &mut watch::Receiver<bool>,
) -> Result<Option<usize>, String> {
    let cancel = Arc::new(AtomicBool::new(false));
    let pause = SessionPause {
        ctx: Arc::clone(ctx),
        cancel: Arc::clone(&cancel),
        poll: ctx
            .timings
            .scan_tick
            .clamp(Duration::from_millis(5), Duration::from_millis(500)),
    };
    let host = Arc::clone(host);
    let mut task = tokio::task::spawn_blocking(move || scan(host.as_ref(), &pause));
    let result = tokio::select! {
        r = &mut task => r,
        _ = stop.changed() => {
            cancel.store(true, Ordering::Relaxed);
            let _ = task.await;
            return Ok(None);
        }
    };
    match result {
        Ok(Ok(found)) => {
            let now = ctx.clock.now_ms();
            let (entries, skipped) = (found.entries.len(), found.skipped);
            let launchers = found.launchers.len();
            let kept = queue_report(ctx, device_id, now, found.into_report(now))?;
            tracing::info!(
                entries,
                kept,
                launchers,
                skipped,
                "scan finished and queued"
            );
            Ok(Some(kept))
        }
        Ok(Err(Cancelled)) => Ok(None),
        Err(e) => Err(format!("the scan thread failed: {e}")),
    }
}

/// Runs the schedule until `stop` is set.
pub(crate) async fn run(ctx: Arc<Ctx>, host: Arc<dyn ScanHost>, mut stop: watch::Receiver<bool>) {
    let mut state = ctx.hub_state.subscribe();
    let mut connected_once = *state.borrow() == KioskHubState::Connected;
    let mut retry_at: Option<i64> = None;
    let mut tick = tokio::time::interval(ctx.timings.scan_tick.max(Duration::from_millis(1)));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            changed = state.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            _ = stop.changed() => return,
        }
        if *state.borrow_and_update() == KioskHubState::Connected {
            connected_once = true;
        }
        let Some(cfg) = ctx.config() else { continue };
        let now = ctx.clock.now_ms();
        if retry_at.is_some_and(|t| now < t) {
            continue;
        }
        let (last, tz) = {
            let q = ctx.queue.lock().unwrap_or_else(|p| p.into_inner());
            (last_scan(&q), load_tz(&q))
        };
        if !is_due(last, connected_once, now, tz) || session_active(&ctx) {
            continue;
        }
        tracing::info!(first = last.is_none(), tz = %tz, "scan starting");
        match run_once(&ctx, &host, &cfg.device_id, &mut stop).await {
            Ok(Some(_)) => retry_at = None,
            Ok(None) => return,
            Err(e) => {
                tracing::error!(error = %e, "scan failed; trying again in an hour");
                let _ = ctx.log_event(
                    KioskLogLevel::Warn,
                    "scan_failed",
                    &e.chars().take(200).collect::<String>(),
                );
                retry_at = Some(ctx.clock.now_ms().saturating_add(RETRY_AFTER_MS));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(tz: Tz, y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        tz.with_ymd_and_hms(y, mo, d, h, mi, 0)
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn the_next_daily_run_is_the_first_five_am_after_the_last_scan() {
        let kolkata: Tz = "Asia/Kolkata".parse().unwrap();
        // Scanned at 04:00: today's 05:00. At 05:00 sharp or later: tomorrow's.
        assert_eq!(
            next_daily_after(ms(kolkata, 2026, 10, 29, 4, 0), kolkata),
            ms(kolkata, 2026, 10, 29, 5, 0)
        );
        for (h, mi) in [(5, 0), (5, 1), (13, 30), (23, 59)] {
            assert_eq!(
                next_daily_after(ms(kolkata, 2026, 10, 29, h, mi), kolkata),
                ms(kolkata, 2026, 10, 30, 5, 0),
                "{h}:{mi}"
            );
        }
        // The same instant is 05:00 only in its own zone.
        let last = ms(kolkata, 2026, 10, 29, 4, 30);
        assert_ne!(
            next_daily_after(last, Tz::UTC),
            next_daily_after(last, kolkata)
        );
        assert_eq!(
            next_daily_after(last, Tz::UTC),
            ms(Tz::UTC, 2026, 10, 29, 5, 0)
        );
    }

    #[test]
    fn a_daylight_saving_change_moves_the_run_with_the_clocks() {
        let ny: Tz = "America/New_York".parse().unwrap();
        // Clocks go back on 2026-11-01 (02:00 EDT becomes 01:00 EST): 05:00 that day is EST.
        let before = ms(ny, 2026, 10, 31, 6, 0);
        let next = next_daily_after(before, ny);
        assert_eq!(next, ms(ny, 2026, 11, 1, 5, 0));
        // 06:00 EDT to 05:00 EST is 23 local hours but 24 real ones.
        assert_eq!(next - before, 24 * 3_600_000);
        let after_change = ms(ny, 2026, 11, 1, 6, 0);
        assert_eq!(
            next_daily_after(after_change, ny),
            ms(ny, 2026, 11, 2, 5, 0)
        );
    }

    #[test]
    fn five_am_resolves_on_a_day_the_clocks_change() {
        let tz: Tz = "Europe/London".parse().unwrap();
        let d = NaiveDate::from_ymd_opt(2026, 3, 29).unwrap();
        assert_eq!(daily_at(tz, d), Some(ms(tz, 2026, 3, 29, 5, 0)));
    }

    #[test]
    fn a_first_scan_waits_for_a_hub_connection_and_a_daily_one_does_not() {
        let tz = Tz::UTC;
        let now = ms(tz, 2026, 10, 29, 12, 0);
        assert!(!is_due(None, false, now, tz));
        assert!(is_due(None, true, now, tz));
        let last = ms(tz, 2026, 10, 29, 5, 30);
        assert!(!is_due(Some(last), true, now, tz), "scanned today already");
        assert!(!is_due(Some(last), true, ms(tz, 2026, 10, 30, 4, 59), tz));
        assert!(is_due(Some(last), false, ms(tz, 2026, 10, 30, 5, 0), tz));
        assert!(is_due(Some(last), false, ms(tz, 2026, 11, 3, 0, 0), tz));
        assert!(
            is_due(Some(now + 3_600_000), true, now, tz),
            "a future record counts as none"
        );
    }

    #[test]
    fn the_zone_is_cached_and_unknown_means_utc() {
        let dir = tempfile::tempdir().unwrap();
        let q = QueueDb::open(&dir.path().join("queue.db")).unwrap();
        assert_eq!(load_tz(&q), Tz::UTC);
        q.kv_set(KEY_BRANCH_TZ, "Asia/Kolkata", 3).unwrap();
        assert_eq!(load_tz(&q).name(), "Asia/Kolkata");
        q.kv_set(KEY_BRANCH_TZ, "Mars/Base", 4).unwrap();
        assert_eq!(load_tz(&q), Tz::UTC);
        assert_eq!(last_scan(&q), None);
        q.kv_set(LAST_SCAN_KEY, "123", 0).unwrap();
        assert_eq!(last_scan(&q), Some(123));
    }
}
