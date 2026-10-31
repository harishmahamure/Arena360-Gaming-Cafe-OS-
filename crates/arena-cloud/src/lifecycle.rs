//! Renewals that need approval, grace and lapse (Story 10.11, AD-20, AD-29): `--role jobs --task
//! subscription-lifecycle`, run every 5 minutes on cloud time.
//!
//! A paid subscription whose renewal needs the bank's approval (`renewal_needs_approval`), or
//! whose renewal charge failed, moves to `grace` when its paid period ends: the branch keeps its
//! paid licence for 7 more days. When the grace period ends, or Razorpay halts the subscription,
//! it lapses: the entitlement becomes the Free tier and the next heartbeat delivers a Free licence.
//! A cancelled plan (Story 10.14) ends, with no grace, when its paid period ends: the subscription
//! becomes `ended` and the entitlement the Free tier.
//!
//! A renewal charge that succeeds at any point returns the subscription to `active` through the
//! webhook, with the next period counted from the original renewal date (see [`crate::webhook`]).
//!
//! The owner is told when a renewal needs approval, and on days 1, 5 and 7 of grace. Every alert
//! has a deterministic id, so a run that repeats, or one that follows the webhook, sends nothing
//! twice. The hub is told nothing here: it learns of a lapse only from its licence.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::time::Duration;

use arena_proto::{OWNER_ALERT_PARAM_MAX_CHARS, OwnerAlertKind, SubscriptionState};
use arena_store::control::{Branch, ControlError, Lifecycle, Subscription, SubscriptionChange};
use chrono::TimeZone;

use crate::auth::AuthState;
use crate::backup::{AlertTarget, JobError, code, open, report_failure};
use crate::config::{Database, JobConfig, Task};
use crate::owner_alerts::{self, Raise};

/// A run gives up (and alerts) after this long, inside the 5-minute schedule.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(240);

/// How long a branch keeps its paid licence after a renewal went unpaid.
pub const GRACE_DAYS: i64 = 7;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// The days of grace on which the owner is warned.
pub const GRACE_ALERT_DAYS: [i64; 3] = [1, 5, 7];

/// How many subscriptions one run moves.
const BATCH: u32 = 500;

/// When the grace period of a renewal due at `period_end` ends.
pub fn grace_until(period_end: i64) -> i64 {
    period_end + GRACE_DAYS * DAY_MS
}

/// The day of the grace period (1 to 7) that `now` falls on, for a period ending at `grace_until`.
pub fn grace_day(grace_until: i64, now: i64) -> i64 {
    let start = grace_until - GRACE_DAYS * DAY_MS;
    ((now - start).div_euclid(DAY_MS) + 1).clamp(1, GRACE_DAYS)
}

/// The warning day that is due on grace day `day`: the latest of days 1, 5 and 7 reached.
pub fn warning_day(day: i64) -> Option<i64> {
    GRACE_ALERT_DAYS.into_iter().rev().find(|d| *d <= day)
}

/// What a subscription's renewal life does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The paid period has ended unpaid: grace until the given time.
    Grace(i64),
    /// The grace period has ended: the branch drops to Free.
    Lapse,
    /// The owner cancelled and the paid period has ended: no grace, the branch drops to Free
    /// (Story 10.14).
    End,
}

/// The next step of `sub` at `now`, or `None` when it has nothing due. A renewal that waits for
/// approval, or whose charge failed, goes to grace when the paid period ends; grace ends at
/// `grace_until`.
pub fn next_step(sub: &Subscription, failed_renewal: bool, now: i64) -> Option<Step> {
    match sub.state {
        SubscriptionState::Active if sub.cancel_at_period_end => sub
            .current_period_end
            .filter(|end| *end <= now)
            .map(|_| Step::End),
        SubscriptionState::RenewalNeedsApproval => sub
            .current_period_end
            .filter(|end| *end <= now)
            .map(|end| Step::Grace(grace_until(end))),
        SubscriptionState::Active if failed_renewal => sub
            .current_period_end
            .filter(|end| *end <= now)
            .map(|end| Step::Grace(grace_until(end))),
        SubscriptionState::Grace => sub
            .grace_until
            .filter(|until| *until <= now)
            .map(|_| Step::Lapse),
        _ => None,
    }
}

/// The alert a subscription calls for at `now`, as `(id, kind, parameters)`; `None` when it is
/// neither waiting for approval nor in grace. A date reads in `timezone`.
pub fn notice_for(
    sub: &Subscription,
    branch_name: &str,
    timezone: &str,
    now: i64,
) -> Option<(String, OwnerAlertKind, BTreeMap<String, String>)> {
    let name: String = branch_name
        .chars()
        .filter(|c| !c.is_control())
        .take(OWNER_ALERT_PARAM_MAX_CHARS)
        .collect();
    let name = if name.trim().is_empty() {
        "branch".to_owned()
    } else {
        name
    };
    match sub.state {
        SubscriptionState::RenewalNeedsApproval => Some((
            format!("ra-{}-{}", sub.id, sub.current_period_end.unwrap_or(0)),
            OwnerAlertKind::RenewalNeedsApproval,
            BTreeMap::from([("branch".to_owned(), name)]),
        )),
        SubscriptionState::Grace => {
            let until = sub.grace_until?;
            let day = warning_day(grace_day(until, now))?;
            Some((
                format!("gr-{}-{day}-{until}", sub.id),
                OwnerAlertKind::RenewalGrace,
                BTreeMap::from([
                    ("branch".to_owned(), name),
                    ("day".to_owned(), day.to_string()),
                    ("date".to_owned(), date_text(until, timezone)),
                ]),
            ))
        }
        _ => None,
    }
}

/// `ms` as `12 Nov 2026` in `timezone` (UTC when the zone is not known).
pub fn date_text(ms: i64, timezone: &str) -> String {
    let tz: chrono_tz::Tz = timezone.parse().unwrap_or(chrono_tz::UTC);
    tz.timestamp_millis_opt(ms)
        .single()
        .map_or_else(String::new, |t| t.format("%-d %b %Y").to_string())
}

/// Raises the alert `sub` calls for, once per alert id. `Ok(false)` when it calls for none.
pub async fn notify(
    st: &AuthState,
    sub: &Subscription,
    branch: &Branch,
    now: i64,
) -> Result<bool, crate::api::ApiError> {
    let Some((id, kind, params)) = notice_for(sub, &branch.name, &branch.timezone, now) else {
        return Ok(false);
    };
    owner_alerts::raise(
        st,
        Raise {
            id: Some(&id),
            branch,
            kind,
            params: &params,
            whatsapp: true,
        },
    )
    .await?;
    Ok(true)
}

/// Tells the owner of subscription `subscription_id` what its state calls for. Best effort: the
/// lifecycle sweep raises the same alert (same id) if this fails.
pub async fn notify_subscription(st: &AuthState, subscription_id: &str) {
    let now = st.now();
    let sub = match st.db.subscription(subscription_id).await {
        Ok(Some(s)) => s,
        Ok(None) => return,
        Err(e) => {
            tracing::error!(%subscription_id, error = %e, "renewal alert: subscription unreadable");
            return;
        }
    };
    let branch = match st.db.branch(&sub.branch_id).await {
        Ok(Some(b)) => b,
        Ok(None) => return,
        Err(e) => {
            tracing::error!(%subscription_id, error = %e, "renewal alert: branch unreadable");
            return;
        }
    };
    if let Err(e) = notify(st, &sub, &branch, now).await {
        tracing::warn!(%subscription_id, code = ?e.body().code, "renewal alert not raised; the sweep retries");
    }
}

/// What one run did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LifecycleOutcome {
    /// Subscriptions moved to `grace`.
    pub graced: u64,
    /// Subscriptions moved to `lapsed`.
    pub lapsed: u64,
    /// Cancelled subscriptions whose paid period ended: moved to `ended`.
    pub ended: u64,
    /// Owner alerts raised or confirmed (an alert already sent is not sent again).
    pub alerted: u64,
    /// Alerts or moves that failed; the next run tries again.
    pub failed: u64,
}

/// Applies `step` to `sub`. `Ok(true)` when the subscription moved; `Ok(false)` when it changed
/// under the sweep and is read again next run.
async fn apply(
    st: &AuthState,
    sub: &Subscription,
    step: Step,
    now: i64,
) -> Result<bool, ControlError> {
    let history_id = ulid::Ulid::generate().to_string();
    let (new_state, cause, lifecycle) = match step {
        Step::Grace(until) => (
            SubscriptionState::Grace,
            "job:renewal-unpaid",
            Lifecycle {
                grace_until: Some(until),
                ..Lifecycle::default()
            },
        ),
        Step::Lapse => (
            SubscriptionState::Lapsed,
            "job:grace-ended",
            Lifecycle {
                lapse: true,
                ..Lifecycle::default()
            },
        ),
        Step::End => (
            SubscriptionState::Ended,
            "job:cancelled-period-ended",
            Lifecycle {
                lapse: true,
                ..Lifecycle::default()
            },
        ),
    };
    st.db
        .apply_subscription_change(
            &SubscriptionChange {
                subscription_id: &sub.id,
                expect_rev: sub.rev,
                old_state: sub.state,
                new_state,
                period_end: None,
                first_paid_at: None,
                payment_country: None,
                mandate_method: None,
                mandate_detail: None,
                entitlement: None,
                history_id: &history_id,
                cause,
                ops_task: None,
                audit: None,
                close_task_kind: None,
                plan_change: None,
                lifecycle,
            },
            now,
        )
        .await
}

/// One run at the cloud's current time: moves what is due, then warns the owners.
pub async fn sweep_once(st: &AuthState) -> Result<LifecycleOutcome, JobError> {
    let failed =
        |e: ControlError| JobError::new(code::SUBSCRIPTION_LIFECYCLE_FAILED, e.to_string());
    let now = st.now();
    let mut out = LifecycleOutcome::default();

    for due in st.db.lifecycle_due(now, BATCH).await.map_err(failed)? {
        let mut sub = due;
        // A period that ended long ago can pass through grace and lapse in one run.
        for _ in 0..2 {
            let marked = match st.db.renewal_failed_at(&sub.id).await {
                Ok(mark) => mark.is_some(),
                Err(e) => {
                    tracing::error!(subscription_id = %sub.id, error = %e, "could not read the renewal mark");
                    out.failed += 1;
                    break;
                }
            };
            let Some(step) = next_step(&sub, marked, now) else {
                break;
            };
            match apply(st, &sub, step, now).await {
                Ok(true) => {
                    match step {
                        Step::Grace(_) => out.graced += 1,
                        Step::Lapse => out.lapsed += 1,
                        Step::End => out.ended += 1,
                    }
                    tracing::info!(
                        subscription_id = %sub.id,
                        branch_id = %sub.branch_id,
                        step = ?step,
                        "subscription moved on its renewal life"
                    );
                    match st.db.subscription(&sub.id).await.map_err(failed)? {
                        Some(next) => sub = next,
                        None => break,
                    }
                }
                Ok(false) => break,
                Err(e) => {
                    tracing::error!(subscription_id = %sub.id, error = %e, "could not move the subscription");
                    out.failed += 1;
                    break;
                }
            }
        }
    }

    for sub in st
        .db
        .subscriptions_awaiting_payment()
        .await
        .map_err(failed)?
    {
        let branch = match st.db.branch(&sub.branch_id).await {
            Ok(Some(branch)) => branch,
            Ok(None) => continue,
            Err(e) => {
                tracing::error!(subscription_id = %sub.id, error = %e, "could not read the branch");
                out.failed += 1;
                continue;
            }
        };
        match notify(st, &sub, &branch, now).await {
            Ok(true) => out.alerted += 1,
            Ok(false) => {}
            Err(e) => {
                tracing::warn!(
                    subscription_id = %sub.id,
                    code = ?e.body().code,
                    "the owner was not warned of the unpaid renewal; retrying next run"
                );
                out.failed += 1;
            }
        }
    }

    // Every state is logged: one that has emptied reads 0, so a gauge can fall.
    let counts = st.db.subscription_state_counts().await.map_err(failed)?;
    for state in SubscriptionState::ALL {
        let count = counts
            .iter()
            .find(|(name, _)| name == state.as_str())
            .map_or(0, |(_, n)| *n);
        tracing::info!(
            task = Task::SubscriptionLifecycle.as_str(),
            state = state.as_str(),
            subscriptions_per_state = count,
            "subscriptions in a state"
        );
    }
    tracing::info!(
        task = Task::SubscriptionLifecycle.as_str(),
        graced = out.graced,
        lapsed = out.lapsed,
        ended = out.ended,
        alerted = out.alerted,
        failed = out.failed,
        "subscription lifecycle done"
    );
    Ok(out)
}

/// `--task subscription-lifecycle`: opens `gz_control` (no migration), builds the same
/// [`AuthState`] as the web role, runs once and reports a failure (including a timeout) through
/// the alert path. Returns whether it succeeded.
pub async fn run_job(
    job: &JobConfig,
    env: &impl Fn(&str) -> Option<OsString>,
    target: &AlertTarget,
) -> bool {
    let run = async {
        let dev = job.dev_data_dir.is_some();
        let database = match &job.dev_data_dir {
            Some(dir) => Database::Local {
                data_dir: dir.clone(),
            },
            None => crate::config::remote_database(env)
                .map_err(|e| JobError::new(code::CONFIG_INVALID, e))?,
        };
        let cfg = crate::config::auth_from_env(dev, env)
            .map_err(|e| JobError::new(code::CONFIG_INVALID, e))?;
        let db = open(&database)
            .await
            .map_err(|e| JobError::new(code::SUBSCRIPTION_LIFECYCLE_FAILED, e.to_string()))?;
        let st = crate::auth_state(std::sync::Arc::new(db), &cfg, crate::system_clock())
            .map_err(|e| JobError::new(code::CONFIG_INVALID, e))?;
        sweep_once(&st).await
    };
    let res = match tokio::time::timeout(RUN_TIMEOUT, run).await {
        Ok(r) => r,
        Err(_) => Err(JobError::new(
            code::SUBSCRIPTION_LIFECYCLE_TIMEOUT,
            format!(
                "the lifecycle sweep did not finish within {} s",
                RUN_TIMEOUT.as_secs()
            ),
        )),
    };
    match res {
        Ok(_) => true,
        Err(e) => {
            report_failure(Task::SubscriptionLifecycle, &e, target).await;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_proto::{Band, BillingCycle, Tier};

    fn sub(state: SubscriptionState) -> Subscription {
        Subscription {
            id: "S1".to_owned(),
            tenant_id: "t1".to_owned(),
            branch_id: "b1".to_owned(),
            state,
            tier: Tier::Basic,
            cycle: BillingCycle::Monthly,
            band: Band::In,
            currency: "INR".to_owned(),
            price_book_id: "pb".to_owned(),
            addon_devices_20: 0,
            addon_tunnel: 0,
            addon_owner_login: 0,
            mandate_method: None,
            mandate_detail: None,
            razorpay_subscription_id: Some("sub_1".to_owned()),
            rev: 1,
            current_period_end: Some(10 * DAY_MS),
            grace_until: None,
            first_paid_at: Some(1),
            cancel_at_period_end: false,
            created_at: 0,
            updated_at: 0,
            payment_country: None,
        }
    }

    #[test]
    fn grace_runs_seven_days_from_the_period_end() {
        assert_eq!(grace_until(10 * DAY_MS), 17 * DAY_MS);
        let until = 17 * DAY_MS;
        assert_eq!(grace_day(until, 10 * DAY_MS), 1);
        assert_eq!(grace_day(until, 10 * DAY_MS + DAY_MS - 1), 1);
        assert_eq!(grace_day(until, 11 * DAY_MS), 2);
        assert_eq!(grace_day(until, 14 * DAY_MS), 5);
        assert_eq!(grace_day(until, 17 * DAY_MS - 1), 7);
        assert_eq!(grace_day(until, 30 * DAY_MS), 7);
        assert_eq!(grace_day(until, 0), 1);
    }

    #[test]
    fn the_owner_is_warned_on_days_one_five_and_seven() {
        let days: Vec<_> = (1..=7).map(warning_day).collect();
        assert_eq!(
            days,
            [
                Some(1),
                Some(1),
                Some(1),
                Some(1),
                Some(5),
                Some(5),
                Some(7)
            ]
        );
        assert_eq!(warning_day(0), None);
    }

    #[test]
    fn a_renewal_that_is_not_paid_goes_to_grace_only_when_the_period_ends() {
        let mut waiting = sub(SubscriptionState::RenewalNeedsApproval);
        assert_eq!(next_step(&waiting, false, 10 * DAY_MS - 1), None);
        assert_eq!(
            next_step(&waiting, false, 10 * DAY_MS),
            Some(Step::Grace(17 * DAY_MS))
        );
        // A failed charge is marked on an active subscription; without the mark nothing moves.
        let active = sub(SubscriptionState::Active);
        assert_eq!(next_step(&active, false, 20 * DAY_MS), None);
        assert_eq!(next_step(&active, true, 10 * DAY_MS - 1), None);
        assert_eq!(
            next_step(&active, true, 11 * DAY_MS),
            Some(Step::Grace(17 * DAY_MS))
        );
        waiting.current_period_end = None;
        assert_eq!(next_step(&waiting, false, 99 * DAY_MS), None);
    }

    #[test]
    fn grace_ends_in_a_lapse_on_cloud_time() {
        let mut grace = sub(SubscriptionState::Grace);
        grace.grace_until = Some(17 * DAY_MS);
        assert_eq!(next_step(&grace, false, 17 * DAY_MS - 1), None);
        assert_eq!(next_step(&grace, false, 17 * DAY_MS), Some(Step::Lapse));
        for over in [
            SubscriptionState::Lapsed,
            SubscriptionState::Pending,
            SubscriptionState::PendingReview,
        ] {
            assert_eq!(next_step(&sub(over), true, 99 * DAY_MS), None);
        }
    }

    #[test]
    fn a_cancelled_plan_ends_at_its_period_end_with_no_grace() {
        let mut cancelled = sub(SubscriptionState::Active);
        cancelled.cancel_at_period_end = true;
        assert_eq!(next_step(&cancelled, false, 10 * DAY_MS - 1), None);
        assert_eq!(next_step(&cancelled, false, 10 * DAY_MS), Some(Step::End));
        // A mark of a failed charge does not turn the end into a grace.
        assert_eq!(next_step(&cancelled, true, 10 * DAY_MS), Some(Step::End));
    }

    #[test]
    fn the_alert_for_a_waiting_renewal_and_for_each_grace_day_has_its_own_id() {
        let waiting = sub(SubscriptionState::RenewalNeedsApproval);
        let (id, kind, params) = notice_for(&waiting, "Koramangala", "Asia/Kolkata", 0).unwrap();
        assert_eq!(kind, OwnerAlertKind::RenewalNeedsApproval);
        assert_eq!(id, format!("ra-S1-{}", 10 * DAY_MS));
        assert_eq!(kind.check_params(&params), Ok(()));
        assert!(id.len() <= 60);

        let mut grace = sub(SubscriptionState::Grace);
        grace.grace_until = Some(17 * DAY_MS);
        let at = |day: i64| 10 * DAY_MS + (day - 1) * DAY_MS + 1;
        let (d1, ..) = notice_for(&grace, "K", "Asia/Kolkata", at(1)).unwrap();
        let (d3, ..) = notice_for(&grace, "K", "Asia/Kolkata", at(3)).unwrap();
        let (d5, kind, params) = notice_for(&grace, "K", "Asia/Kolkata", at(5)).unwrap();
        let (d7, ..) = notice_for(&grace, "K", "Asia/Kolkata", at(7)).unwrap();
        assert_eq!(d1, d3, "days 1 to 4 are one warning");
        assert!(d1 != d5 && d5 != d7);
        assert_eq!(kind, OwnerAlertKind::RenewalGrace);
        assert_eq!(kind.check_params(&params), Ok(()));
        assert_eq!(params["day"], "5");
        assert!(d7.len() <= 60);
        // The other states have nothing to say.
        assert!(notice_for(&sub(SubscriptionState::Active), "K", "UTC", 0).is_none());
        assert!(notice_for(&sub(SubscriptionState::Lapsed), "K", "UTC", 0).is_none());
    }

    #[test]
    fn a_long_branch_name_still_fits_an_alert_parameter() {
        let waiting = sub(SubscriptionState::RenewalNeedsApproval);
        let long = "x".repeat(100);
        let (_, kind, params) = notice_for(&waiting, &long, "UTC", 0).unwrap();
        assert_eq!(kind.check_params(&params), Ok(()));
        let (_, _, params) = notice_for(&waiting, "  ", "UTC", 0).unwrap();
        assert_eq!(params["branch"], "branch");
    }

    #[test]
    fn a_date_reads_in_the_branch_timezone() {
        // 2026-11-12 20:00 UTC is already the 13th in Kolkata.
        let ms = chrono::NaiveDate::from_ymd_opt(2026, 11, 12)
            .unwrap()
            .and_hms_opt(20, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis();
        assert_eq!(date_text(ms, "Asia/Kolkata"), "13 Nov 2026");
        assert_eq!(date_text(ms, "UTC"), "12 Nov 2026");
        assert_eq!(date_text(ms, "Not/AZone"), "12 Nov 2026");
    }
}
