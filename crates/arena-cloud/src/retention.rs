//! The cloud backup retention sweep (Story 9.15): `--role jobs --task retention-sweep`, run daily.
//!
//! For each branch it lists the objects under the branch prefix of the data bucket and deletes,
//! and only deletes, from four prefixes:
//!
//! - `changes/`, `snapshots/` and `rollups/` once older than the branch's retention (the licence's
//!   `retention_days`: Basic 365, Premium 1,095). A `changes/` object's age is the bucket's
//!   last-modified time; a snapshot's or rollup's is the end of the day in its key. After a move to
//!   a shorter plan the longer retention stays for [`GRACE_MS`] (30 days); a lapse to Free keeps
//!   the last paid tier's retention. The newest snapshot taken before the oldest retained batch is
//!   always kept, so every retained batch has a base to restore from.
//! - `gst-archive/` once 8 years have passed since the end of the financial year, on every tier and
//!   after a lapse.
//!
//! `keys/`, `state/` and anything else are never deleted. The retention a branch last had is kept
//! in `gz_control.branch_retention` so a downgrade is seen once and graced.

use std::ffi::OsString;
use std::time::Duration;

use arena_proto::{Tier, valid_fy};
use arena_store::control::{ControlDb, RetentionState};
use chrono::NaiveDate;

use crate::backup::{AlertTarget, JobError, code, open, report_failure};
use crate::config::{Database, JobConfig, Task};
use crate::presign::{Presigner, branch_prefix};

/// A sweep gives up (and alerts) after this long.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(300);
/// How long the previous, longer retention stays after a move to a shorter plan.
pub const GRACE_MS: i64 = 30 * DAY_MS;
/// Years a GST archive is kept after its financial year ends.
pub const GST_KEEP_YEARS: i32 = 8;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// The retention (days) to apply to a branch now, and the state to save when it changed.
/// `None` retention: the branch never had a paid tier, so only its GST archives are considered.
pub fn observe(
    tier: Tier,
    state: Option<RetentionState>,
    now: i64,
) -> (Option<u32>, Option<RetentionState>) {
    let current = tier.defaults().retention_days;
    let effective = |s: &RetentionState| {
        if s.prev_days > s.paid_days && now - s.changed_at < GRACE_MS {
            s.prev_days
        } else {
            s.paid_days
        }
    };
    match (current, state) {
        (0, None) => (None, None),
        // Lapsed: the last paid tier's retention stands.
        (0, Some(s)) => (Some(effective(&s)), None),
        (c, None) => {
            let s = RetentionState {
                paid_days: c,
                prev_days: c,
                changed_at: now,
            };
            (Some(c), Some(s))
        }
        (c, Some(s)) if s.paid_days == c => (Some(effective(&s)), None),
        (c, Some(s)) => {
            let next = RetentionState {
                paid_days: c,
                prev_days: s.paid_days,
                changed_at: now,
            };
            (Some(effective(&next)), Some(next))
        }
    }
}

/// The end of the UTC day `yyyy-mm-dd` at the start of `name` (ms), or none when it has no date.
fn day_end_ms(name: &str) -> Option<i64> {
    let day = NaiveDate::parse_from_str(name.get(..10)?, "%Y-%m-%d").ok()?;
    Some(
        day.succ_opt()?
            .and_hms_opt(0, 0, 0)?
            .and_utc()
            .timestamp_millis(),
    )
}

/// When a GST archive of `fy` may be deleted (ms): 8 years after the latest the year can end.
/// The label does not say which month the year starts in, so `FY2026-27` is taken to end on 31
/// December 2027 at the latest and `FY2027` on 31 December 2027: the archive can be kept a few
/// months too long, never too short.
fn gst_deletable_from_ms(fy: &str) -> Option<i64> {
    if !valid_fy(fy) {
        return None;
    }
    let rest = fy.strip_prefix("FY")?;
    let start: i32 = rest.get(..4)?.parse().ok()?;
    let end_year = if rest.contains('-') { start + 1 } else { start };
    let day = NaiveDate::from_ymd_opt(end_year + GST_KEEP_YEARS + 1, 1, 1)?;
    Some(day.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis())
}

/// The keys to delete among `objects` (full key, last-modified ms) under `prefix`.
pub fn plan_branch(
    prefix: &str,
    objects: &[(String, i64)],
    retention_days: Option<u32>,
    now: i64,
) -> Vec<String> {
    let mut doomed = Vec::new();
    let mut changes: Vec<(&str, i64)> = Vec::new();
    let mut snapshots: Vec<(&str, i64)> = Vec::new();
    for (key, modified) in objects {
        let Some(rel) = key.strip_prefix(prefix) else {
            continue;
        };
        if let Some(name) = rel.strip_prefix("gst-archive/") {
            if let Some(fy) = name.strip_suffix(".zst")
                && gst_deletable_from_ms(fy).is_some_and(|from| now >= from)
            {
                doomed.push(key.clone());
            }
        } else if rel.starts_with("changes/") {
            changes.push((key, *modified));
        } else if let Some(name) = rel.strip_prefix("snapshots/") {
            if let Some(end) = day_end_ms(name) {
                snapshots.push((key, end));
            }
        } else if let Some(name) = rel.strip_prefix("rollups/")
            && let (Some(days), Some(end)) = (retention_days, day_end_ms(name))
            && end < now - i64::from(days) * DAY_MS
        {
            doomed.push(key.clone());
        }
    }
    let Some(days) = retention_days else {
        return doomed;
    };
    let cutoff = now - i64::from(days) * DAY_MS;
    let oldest_retained = changes
        .iter()
        .filter(|(_, m)| *m >= cutoff)
        .map(|(_, m)| *m)
        .min();
    doomed.extend(
        changes
            .iter()
            .filter(|(_, m)| *m < cutoff)
            .map(|(k, _)| (*k).to_owned()),
    );
    // The snapshot a retained batch range restores from: the newest one that ends before the
    // oldest retained batch was written (the newest of all when no batch is retained).
    let base = snapshots
        .iter()
        .filter(|(_, end)| oldest_retained.is_none_or(|t| *end <= t))
        .max_by_key(|(_, end)| *end)
        .map(|(k, _)| *k);
    doomed.extend(
        snapshots
            .iter()
            .filter(|(k, end)| *end < cutoff && Some(*k) != base)
            .map(|(k, _)| (*k).to_owned()),
    );
    doomed
}

/// What one run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SweepOutcome {
    pub branches: u64,
    pub deleted: u64,
    pub failed: u64,
}

/// One sweep of every branch at `now`. A branch that fails is logged and counted; the rest run.
pub async fn sweep_once(
    db: &ControlDb,
    store: &Presigner,
    now: i64,
) -> Result<SweepOutcome, JobError> {
    let branches = db
        .retention_branches(now)
        .await
        .map_err(|e| JobError::new(code::RETENTION_SWEEP_FAILED, e.to_string()))?;
    let mut out = SweepOutcome::default();
    for b in &branches {
        out.branches += 1;
        let (days, save) = observe(b.tier, b.state, now);
        let result = async {
            if let Some(state) = save {
                db.save_branch_retention(&b.branch_id, state)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            let prefix = branch_prefix(&b.tenant_id, &b.branch_id);
            let objects = store.list_modified(&prefix).await?;
            let doomed = plan_branch(&prefix, &objects, days, now);
            for key in &doomed {
                store.delete(key).await?;
            }
            Ok::<_, String>(doomed.len() as u64)
        }
        .await;
        match result {
            Ok(n) => {
                out.deleted += n;
                if n > 0 {
                    tracing::info!(
                        task = Task::RetentionSweep.as_str(),
                        branch_id = %b.branch_id,
                        retention_days = days,
                        deleted = n,
                        "backup objects past retention deleted"
                    );
                }
            }
            Err(detail) => {
                out.failed += 1;
                tracing::error!(
                    task = Task::RetentionSweep.as_str(),
                    branch_id = %b.branch_id,
                    code = code::RETENTION_SWEEP_FAILED,
                    detail = %detail,
                    "retention sweep failed for a branch"
                );
            }
        }
    }
    tracing::info!(
        task = Task::RetentionSweep.as_str(),
        branches = out.branches,
        deleted = out.deleted,
        failed = out.failed,
        "retention swept"
    );
    if out.failed > 0 {
        return Err(JobError::new(
            code::RETENTION_SWEEP_FAILED,
            format!("{} of {} branches failed", out.failed, out.branches),
        ));
    }
    Ok(out)
}

/// `--task retention-sweep`: opens `gz_control` (no migration) and the data bucket, sweeps once
/// and reports a failure (including a timeout) through the alert path. Returns whether it
/// succeeded.
pub async fn run_job(
    job: &JobConfig,
    env: &impl Fn(&str) -> Option<OsString>,
    target: &AlertTarget,
) -> bool {
    let run = async {
        let database = match &job.dev_data_dir {
            Some(dir) => Database::Local {
                data_dir: dir.clone(),
            },
            None => crate::config::remote_database(env)
                .map_err(|e| JobError::new(code::CONFIG_INVALID, e))?,
        };
        let store = Presigner::from_env(job.dev_data_dir.is_some(), env)
            .map_err(|e| JobError::new(code::CONFIG_INVALID, e))?
            .ok_or_else(|| {
                JobError::new(code::CONFIG_INVALID, "ARENA_DATA_S3_* is not configured")
            })?;
        let db = open(&database)
            .await
            .map_err(|e| JobError::new(code::RETENTION_SWEEP_FAILED, e.to_string()))?;
        sweep_once(&db, &store, crate::now_ms()).await
    };
    let res = match tokio::time::timeout(RUN_TIMEOUT, run).await {
        Ok(r) => r,
        Err(_) => Err(JobError::new(
            code::RETENTION_SWEEP_TIMEOUT,
            format!(
                "the sweep did not finish within {} s",
                RUN_TIMEOUT.as_secs()
            ),
        )),
    };
    match res {
        Ok(_) => true,
        Err(e) => {
            report_failure(Task::RetentionSweep, &e, target).await;
            false
        }
    }
}
