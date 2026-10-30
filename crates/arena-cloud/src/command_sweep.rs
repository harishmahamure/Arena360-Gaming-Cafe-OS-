//! The command mailbox sweeper (Story 8.20): `--role jobs --task command-sweep`, run every minute.
//!
//! It returns `fetched` commands with no answer for 10 minutes to `pending` (the next fetch
//! delivers them again; the Hub's `applied_commands` dedupe answers without re-applying), expires
//! `pending` commands older than 7 days with `COMMAND_EXPIRED`, and logs the command backlog per
//! branch (pending, fetched, oldest age). The OpenTelemetry export of that metric is deferred with
//! the Story 2.3 exporter, as for the other cloud metrics.

use std::ffi::OsString;
use std::time::Duration;

use arena_store::control::{CommandBacklog, ControlDb};

use crate::backup::{AlertTarget, JobError, code, open, report_failure};
use crate::config::{Database, JobConfig, Task};

/// A sweep gives up (and alerts) after this long, well inside the one-minute schedule's overlap
/// guard.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(50);

/// What one run did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepOutcome {
    pub expired: u64,
    pub requeued: u64,
    pub revoked: u64,
    pub backlog: Vec<CommandBacklog>,
}

/// One sweep at `now`, then the backlog that remains.
pub async fn sweep_once(db: &ControlDb, now: i64) -> Result<SweepOutcome, JobError> {
    let report = db
        .sweep_commands(now)
        .await
        .map_err(|e| JobError::new(code::SWEEP_FAILED, e.to_string()))?;
    let backlog = db
        .command_backlog()
        .await
        .map_err(|e| JobError::new(code::SWEEP_FAILED, e.to_string()))?;
    for b in &backlog {
        tracing::info!(
            branch_id = %b.branch_id,
            command_backlog_pending = b.pending,
            command_backlog_fetched = b.fetched,
            command_backlog_oldest_age_s = u64::try_from((now - b.oldest_created_at) / 1000)
                .unwrap_or(0),
            "command backlog"
        );
    }
    tracing::info!(
        task = Task::CommandSweep.as_str(),
        expired = report.expired,
        requeued = report.requeued,
        revoked = report.revoked,
        branches_waiting = backlog.len(),
        "commands swept"
    );
    Ok(SweepOutcome {
        expired: report.expired,
        requeued: report.requeued,
        revoked: report.revoked,
        backlog,
    })
}

/// `--task command-sweep`: opens `gz_control` (no migration), sweeps once and reports a failure
/// (including a timeout) through the alert path. Returns whether it succeeded.
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
        let db = open(&database)
            .await
            .map_err(|e| JobError::new(code::SWEEP_FAILED, e.to_string()))?;
        sweep_once(&db, crate::now_ms()).await
    };
    let res = match tokio::time::timeout(RUN_TIMEOUT, run).await {
        Ok(r) => r,
        Err(_) => Err(JobError::new(
            code::SWEEP_TIMEOUT,
            format!(
                "the sweep did not finish within {} s",
                RUN_TIMEOUT.as_secs()
            ),
        )),
    };
    match res {
        Ok(_) => true,
        Err(e) => {
            report_failure(Task::CommandSweep, &e, target).await;
            false
        }
    }
}
