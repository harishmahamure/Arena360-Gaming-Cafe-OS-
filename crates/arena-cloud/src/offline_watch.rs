//! The offline watch (Story 8.35): `--role jobs --task offline-watch`, run every 5 minutes.
//!
//! For each branch on a paid tier with a live Hub it raises an owner alert (Story 8.33) when the
//! branch is silent, or reporting but not backing up, while its cached opening hours say it should
//! be open, and a push-only entry when the heartbeat returns. An alert is once per episode: the
//! episode's key (the Hub's `last_seen_at`, or the newest upload time) makes the alert id
//! deterministic, so a repeat run, or a failed state write, raises nothing twice.
//!
//! When many branches would alert at once the run holds all of them and pages the on-call
//! instead: that is the cloud or the network, not a hundred cafés.

use std::ffi::OsString;
use std::sync::Arc;
use std::time::Duration;

use arena_proto::{BranchOpening, OwnerAlertKind, opening_window_start};
use arena_store::control::{WatchRow, WatchState};

use crate::alert;
use crate::auth::AuthState;
use crate::backup::{AlertTarget, JobError, code, open, report_failure};
use crate::config::{Database, JobConfig, Task};
use crate::owner_alerts::{self, Raise};

/// A run gives up (and alerts) after this long, inside the five-minute schedule.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(240);
/// How long a branch is silent, or stops uploading, before the owner is told (30 minutes).
pub const SILENCE_MS: i64 = 30 * 60 * 1000;
/// The fewest branches that can make a mass hold.
pub const HOLD_MIN_BRANCHES: usize = 5;
/// A mass hold needs more than this percent of the paid branches with a live Hub.
pub const HOLD_PERCENT: usize = 20;

/// What a run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchOutcome {
    /// Paid branches with a live Hub.
    pub watched: usize,
    /// `branch_offline` alerts raised.
    pub offline: usize,
    /// `branch_not_uploading` alerts raised.
    pub not_uploading: usize,
    /// `branch_online` entries added.
    pub online: usize,
    /// Raises that failed (logged; their keys are not recorded, so the next run tries again).
    pub failed: usize,
    /// The number of alerts held back, when there were too many at once.
    pub held: Option<usize>,
}

/// An alert a branch's state calls for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pending {
    Offline { key: i64, minutes: i64 },
    NotUploading { key: i64, minutes: i64 },
}

impl Pending {
    fn kind(self) -> OwnerAlertKind {
        match self {
            Self::Offline { .. } => OwnerAlertKind::BranchOffline,
            Self::NotUploading { .. } => OwnerAlertKind::BranchNotUploading,
        }
    }

    fn id(self, branch_id: &str) -> String {
        match self {
            Self::Offline { key, .. } => format!("off-{branch_id}-{key}"),
            Self::NotUploading { key, .. } => format!("up-{branch_id}-{key}"),
        }
    }

    fn minutes(self) -> i64 {
        match self {
            Self::Offline { minutes, .. } | Self::NotUploading { minutes, .. } => minutes,
        }
    }

    /// `state` with this episode recorded as alerted.
    fn recorded(self, mut state: WatchState) -> WatchState {
        match self {
            Self::Offline { key, .. } => state.offline_key = Some(key),
            Self::NotUploading { key, .. } => state.upload_key = Some(key),
        }
        state
    }
}

/// What `row` calls for at `now`: the state with a fresh upload's key cleared, and the alert
/// not yet raised for the episode, if any. A branch whose hours are not cached, or do not parse,
/// never alerts.
pub fn evaluate(row: &WatchRow, state: WatchState, now: i64) -> (WatchState, Option<Pending>) {
    let Some(seen) = row.last_seen_at else {
        return (state, None);
    };
    let window = row
        .opening
        .as_ref()
        .and_then(|(hours, zone)| {
            BranchOpening {
                opening_hours: hours.clone(),
                timezone: zone.clone(),
            }
            .parse()
        })
        .and_then(|(week, tz)| opening_window_start(&week, tz, now));
    // How long `since` has gone without news while the branch should be open: from the later of
    // `since` and the start of today's opening window, when that is at least the limit.
    let stalled = |since: i64| -> Option<i64> {
        let from = since.max(window?);
        (now - from >= SILENCE_MS).then(|| (now - from) / 60_000)
    };
    if now - seen >= SILENCE_MS {
        let pending = stalled(seen)
            .filter(|_| state.offline_key != Some(seen))
            .map(|minutes| Pending::Offline { key: seen, minutes });
        return (state, pending);
    }
    let mut state = state;
    let Some(newest) = row.newest_upload() else {
        return (state, None);
    };
    if now - newest < SILENCE_MS {
        state.upload_key = None;
        return (state, None);
    }
    let pending = stalled(newest)
        .filter(|_| state.upload_key != Some(newest))
        .map(|minutes| Pending::NotUploading {
            key: newest,
            minutes,
        });
    (state, pending)
}

/// Whether `would_alert` of `watched` branches is a mass outage: at least
/// [`HOLD_MIN_BRANCHES`] and more than [`HOLD_PERCENT`] percent.
pub fn is_mass_outage(would_alert: usize, watched: usize) -> bool {
    would_alert >= HOLD_MIN_BRANCHES && would_alert * 100 > watched * HOLD_PERCENT
}

/// The page the on-call gets: environment, task and count only.
pub fn held_text(env_name: &str, count: usize) -> String {
    format!(
        "arena-cloud {env_name}: task {} held {count} branch alerts because many branches look offline at once. See the job's logs.",
        Task::OfflineWatch.as_str()
    )
}

fn params(minutes: i64) -> std::collections::BTreeMap<String, String> {
    std::collections::BTreeMap::from([("minutes".to_owned(), minutes.to_string())])
}

fn failed(e: &crate::api::ApiError) -> String {
    format!("{:?}", e.body().code)
}

/// One run at the cloud's current time: back-online entries first, then the offline and
/// not-uploading alerts unless there are too many, in which case the on-call is paged.
pub async fn watch_once(st: &AuthState, target: &AlertTarget) -> Result<WatchOutcome, JobError> {
    let now = st.now();
    let rows = st
        .db
        .watch_rows(now)
        .await
        .map_err(|e| JobError::new(code::WATCH_FAILED, e.to_string()))?;
    let mut out = WatchOutcome {
        watched: rows.len(),
        ..WatchOutcome::default()
    };

    // The state each branch is in once its back-online entry is added.
    let mut states = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut state = row.state;
        if let (Some(key), Some(seen)) = (state.offline_key, row.last_seen_at)
            && seen > key
        {
            let minutes = ((seen - key) / 60_000).max(1);
            match raise(
                st,
                &row.branch_id,
                &format!("on-{}-{key}", row.branch_id),
                OwnerAlertKind::BranchOnline,
                minutes,
                false,
            )
            .await
            {
                Ok(()) => {
                    out.online += 1;
                    state.offline_key = None;
                    save(st, &row.branch_id, row.state, state).await;
                }
                Err(()) => out.failed += 1,
            }
        }
        states.push(state);
    }

    // What each branch calls for now. A branch with nothing to alert has its state saved at once
    // (a fresh upload clears its key); one with an alert waits for the mass-outage check.
    let mut calls = Vec::new();
    for (row, state) in rows.iter().zip(states) {
        let (after, pending) = evaluate(row, state, now);
        match pending {
            Some(p) => calls.push((row, after, p)),
            None => save(st, &row.branch_id, row.state, after).await,
        }
    }

    if is_mass_outage(calls.len(), out.watched) {
        out.held = Some(calls.len());
        tracing::warn!(
            task = Task::OfflineWatch.as_str(),
            held = calls.len(),
            watched = out.watched,
            "many branches would alert at once: held, paging the on-call"
        );
        page(target, calls.len()).await;
        return Ok(out);
    }

    for (row, after, pending) in calls {
        let id = pending.id(&row.branch_id);
        match raise(
            st,
            &row.branch_id,
            &id,
            pending.kind(),
            pending.minutes(),
            true,
        )
        .await
        {
            Ok(()) => {
                match pending {
                    Pending::Offline { .. } => out.offline += 1,
                    Pending::NotUploading { .. } => out.not_uploading += 1,
                }
                save(st, &row.branch_id, row.state, pending.recorded(after)).await;
            }
            Err(()) => out.failed += 1,
        }
    }
    tracing::info!(
        task = Task::OfflineWatch.as_str(),
        watched = out.watched,
        offline = out.offline,
        not_uploading = out.not_uploading,
        online = out.online,
        failed = out.failed,
        "offline watch done"
    );
    Ok(out)
}

/// Raises one alert for `branch_id`; a failure is logged and answered `Err`.
async fn raise(
    st: &AuthState,
    branch_id: &str,
    id: &str,
    kind: OwnerAlertKind,
    minutes: i64,
    whatsapp: bool,
) -> Result<(), ()> {
    let branch = match st.db.branch(branch_id).await {
        Ok(Some(b)) => b,
        Ok(None) => {
            tracing::error!(branch_id, "offline watch: branch not found");
            return Err(());
        }
        Err(e) => {
            tracing::error!(branch_id, error = %e, "offline watch: branch unreadable");
            return Err(());
        }
    };
    let params = params(minutes);
    match owner_alerts::raise(
        st,
        Raise {
            id: Some(id),
            branch: &branch,
            kind,
            params: &params,
            whatsapp,
        },
    )
    .await
    {
        Ok(_) => Ok(()),
        Err(e) => {
            tracing::error!(branch_id, alert_id = id, code = %failed(&e), "offline watch: raise failed");
            Err(())
        }
    }
}

/// Stores `after` when it differs from `before`; a failure is logged (the alert ids make the
/// next run safe).
async fn save(st: &AuthState, branch_id: &str, before: WatchState, after: WatchState) {
    if before == after {
        return;
    }
    if let Err(e) = st.db.save_branch_watch(branch_id, after).await {
        tracing::error!(branch_id, error = %e, "offline watch: state not saved");
    }
}

/// POSTs the held-alerts page to the webhook, when one is set. A failed POST is logged.
async fn page(target: &AlertTarget, count: usize) {
    let Some(url) = &target.webhook else {
        tracing::warn!(
            "no {} set; the held alerts are only in this log",
            alert::WEBHOOK_ENV
        );
        return;
    };
    match alert::post(
        url,
        &held_text(&target.env_name, count),
        alert::ALERT_TIMEOUT,
    )
    .await
    {
        Ok(()) => tracing::info!(task = Task::OfflineWatch.as_str(), "on-call paged"),
        Err(e) => tracing::error!(
            task = Task::OfflineWatch.as_str(),
            detail = %e,
            "the on-call page failed"
        ),
    }
}

/// `--task offline-watch`: opens `gz_control` (no migration), builds the same [`AuthState`] as
/// the web role, runs once and reports a failure (including a timeout) through the alert path.
/// Returns whether it succeeded; a mass hold is a success.
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
        let cfg = crate::config::auth_from_env(job.dev_data_dir.is_some(), env)
            .map_err(|e| JobError::new(code::CONFIG_INVALID, e))?;
        let db = open(&database)
            .await
            .map_err(|e| JobError::new(code::WATCH_FAILED, e.to_string()))?;
        let st = crate::auth_state(Arc::new(db), &cfg, crate::system_clock())
            .map_err(|e| JobError::new(code::CONFIG_INVALID, e))?;
        watch_once(&st, target).await
    };
    let res = match tokio::time::timeout(RUN_TIMEOUT, run).await {
        Ok(r) => r,
        Err(_) => Err(JobError::new(
            code::WATCH_TIMEOUT,
            format!(
                "the watch did not finish within {} s",
                RUN_TIMEOUT.as_secs()
            ),
        )),
    };
    match res {
        Ok(_) => true,
        Err(e) => {
            report_failure(Task::OfflineWatch, &e, target).await;
            false
        }
    }
}
