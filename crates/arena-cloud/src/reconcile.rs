//! The billing reconcile (Story 10.3): a webhook that never arrives must not leave a paid branch
//! unentitled or a renewal unnoticed. Every 10 minutes the jobs role reads each subscription in
//! `pending`, `renewal_needs_approval` or `grace` from Razorpay and applies what it finds through
//! the same code path as a webhook, recorded with source `reconcile`.

use std::ffi::OsString;
use std::time::Duration;

use arena_store::control::ControlDb;

use crate::backup::{AlertTarget, JobError, code, open, report_failure};
use crate::billing::BillingProvider;
use crate::config::{Database, JobConfig, Task};
use crate::webhook::{self, Handled, Source};

/// A run gives up (and alerts) after this long, inside the 10-minute schedule.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(8 * 60);

/// How many subscriptions one run reads.
const BATCH: u32 = 500;

/// What one run did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReconcileOutcome {
    /// Subscriptions read from the provider.
    pub checked: u64,
    /// Subscriptions that moved (a missed activation or renewal).
    pub applied: u64,
    /// Subscriptions the provider could not be read for; the next run tries again.
    pub failed: u64,
}

/// Reads every open, unsettled subscription from the provider and applies what it finds. A
/// `manual` provider reads nothing.
pub async fn reconcile(
    db: &ControlDb,
    provider: &BillingProvider,
    now: i64,
) -> Result<ReconcileOutcome, JobError> {
    let failed = |e: arena_store::control::ControlError| {
        JobError::new(code::BILLING_RECONCILE_FAILED, e.to_string())
    };
    let due = db.subscriptions_to_reconcile(BATCH).await.map_err(failed)?;
    let mut out = ReconcileOutcome::default();
    for sub in due {
        let Some(rid) = &sub.razorpay_subscription_id else {
            continue;
        };
        let remote = match provider.fetch_subscription(rid).await {
            Ok(Some(r)) => r,
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!(
                    subscription_id = %sub.id,
                    code = %e,
                    "could not read a subscription from the provider; retrying next run"
                );
                out.failed += 1;
                continue;
            }
        };
        out.checked += 1;
        let (event_id, event) = webhook::from_remote(rid, &remote);
        let hash = webhook::payload_hash(event_id.as_bytes());
        match webhook::handle(db, &event_id, &event, &hash, Source::Reconcile, now)
            .await
            .map_err(failed)?
        {
            Handled::Applied {
                subscription_id,
                state,
                review,
            } => {
                out.applied += 1;
                tracing::warn!(
                    %subscription_id,
                    state = state.as_str(),
                    "the reconcile applied a Razorpay event the webhook never delivered"
                );
                if review {
                    webhook::alert_ops(&subscription_id).await;
                }
            }
            Handled::Recorded | Handled::Unmatched | Handled::Duplicate => {}
        }
    }
    tracing::info!(
        task = Task::BillingReconcile.as_str(),
        checked = out.checked,
        applied = out.applied,
        failed = out.failed,
        "subscriptions reconciled"
    );
    Ok(out)
}

/// `--task billing-reconcile`: opens `gz_control` (no migration), resolves the billing provider
/// from the environment and reconciles once; a failure (including a timeout) goes through the
/// alert path. Returns whether it succeeded.
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
        let provider = crate::billing::provider_from_env(job.dev_data_dir.is_some(), env)
            .map_err(|e| JobError::new(code::CONFIG_INVALID, e))?;
        let db = open(&database)
            .await
            .map_err(|e| JobError::new(code::BILLING_RECONCILE_FAILED, e.to_string()))?;
        reconcile(&db, &provider, crate::now_ms()).await
    };
    let res = match tokio::time::timeout(RUN_TIMEOUT, run).await {
        Ok(r) => r,
        Err(_) => Err(JobError::new(
            code::BILLING_RECONCILE_TIMEOUT,
            format!(
                "the reconcile did not finish within {} s",
                RUN_TIMEOUT.as_secs()
            ),
        )),
    };
    match res {
        Ok(_) => true,
        Err(e) => {
            report_failure(Task::BillingReconcile, &e, target).await;
            false
        }
    }
}
