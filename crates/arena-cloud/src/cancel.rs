//! Cancel a branch's plan at the end of the paid period (Story 10.14, FR178, FR180, AD-20).
//!
//! The owner cancels an `active` subscription: Razorpay is told to stop billing at the cycle end,
//! `cancel_at_period_end` is set and nothing else moves (the entitlement and the licence stay as
//! they are). "Keep my plan" takes the cancellation back at Razorpay and clears the flag; the
//! renewal date is untouched. When the paid period ends on cloud time the jobs role ends the
//! subscription (`ended`, no grace) and the branch drops to the Free tier; the next heartbeat
//! delivers a Free licence for the Hub to handle as in Stories 10.12 and 10.13.
//!
//! No partial refunds. A full refund inside 7 days of the first payment is Story 10.15.

use arena_proto::SubscriptionState;
use arena_store::control::{ControlDb, Lifecycle, Subscription, SubscriptionChange};

use crate::billing::BillingProvider;
use crate::plan_change::{ChangeError, db_err, new_id};

const NOT_NOW: &str = "This plan can't be cancelled right now.";

/// What a request left behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cancelled {
    /// The flag changed.
    Changed,
    /// It was already as asked; nothing was written.
    Already,
}

fn gate(sub: &Subscription) -> Result<(), ChangeError> {
    match sub.state {
        SubscriptionState::Active => Ok(()),
        SubscriptionState::PendingReview
        | SubscriptionState::RenewalNeedsApproval
        | SubscriptionState::Grace => {
            Err(ChangeError::Blocked("Settle the pending renewal first."))
        }
        _ => Err(ChangeError::Blocked(NOT_NOW)),
    }
}

async fn write(
    db: &ControlDb,
    sub: &Subscription,
    cancel: bool,
    now: i64,
) -> Result<bool, ChangeError> {
    let history_id = new_id();
    db.apply_subscription_change(
        &SubscriptionChange {
            subscription_id: &sub.id,
            expect_rev: sub.rev,
            old_state: sub.state,
            new_state: sub.state,
            period_end: None,
            first_paid_at: None,
            payment_country: None,
            mandate_method: None,
            mandate_detail: None,
            entitlement: None,
            history_id: &history_id,
            cause: if cancel {
                "owner:cancel"
            } else {
                "owner:keep-plan"
            },
            ops_task: None,
            audit: None,
            close_task_kind: None,
            plan_change: None,
            lifecycle: Lifecycle {
                cancel_at_period_end: Some(cancel),
                ..Lifecycle::default()
            },
        },
        now,
    )
    .await
    .map_err(db_err)
}

/// Cancels at the end of the paid period, against the `rev` the owner saw.
pub async fn cancel(
    db: &ControlDb,
    provider: &BillingProvider,
    sub: &Subscription,
    rev: i64,
    now: i64,
) -> Result<Cancelled, ChangeError> {
    if sub.rev != rev {
        return Err(ChangeError::Stale);
    }
    gate(sub)?;
    if sub.cancel_at_period_end {
        return Ok(Cancelled::Already);
    }
    if sub.current_period_end.is_none() {
        return Err(ChangeError::Blocked(NOT_NOW));
    }
    if db
        .open_plan_change(&sub.id)
        .await
        .map_err(db_err)?
        .is_some()
    {
        return Err(ChangeError::Blocked(
            "Keep your current plan or wait for the pending change first.",
        ));
    }
    let razorpay_id = sub
        .razorpay_subscription_id
        .as_deref()
        .ok_or(ChangeError::Blocked(NOT_NOW))?;
    provider
        .cancel_at_cycle_end(razorpay_id)
        .await
        .map_err(|e| ChangeError::Provider(e.to_string()))?;
    if write(db, sub, true, now).await? {
        return Ok(Cancelled::Changed);
    }
    // The subscription moved while Razorpay was told: put Razorpay back as it was.
    if let Err(e) = provider.keep_after_cancel(razorpay_id).await {
        tracing::warn!(subscription_id = %sub.id, error = %e, "could not undo a cancel that lost a race");
    }
    Err(ChangeError::Stale)
}

/// "Keep my plan": takes the cancellation back before the period end, against the `rev` seen.
pub async fn keep(
    db: &ControlDb,
    provider: &BillingProvider,
    sub: &Subscription,
    rev: i64,
    now: i64,
) -> Result<Cancelled, ChangeError> {
    if sub.rev != rev {
        return Err(ChangeError::Stale);
    }
    gate(sub)?;
    if !sub.cancel_at_period_end {
        return Ok(Cancelled::Already);
    }
    if sub.current_period_end.is_some_and(|end| end <= now) {
        return Err(ChangeError::Blocked(
            "The period has ended; subscribe again to keep a paid plan.",
        ));
    }
    let razorpay_id = sub
        .razorpay_subscription_id
        .as_deref()
        .ok_or(ChangeError::Blocked(NOT_NOW))?;
    provider
        .keep_after_cancel(razorpay_id)
        .await
        .map_err(|e| ChangeError::Provider(e.to_string()))?;
    if write(db, sub, false, now).await? {
        Ok(Cancelled::Changed)
    } else {
        // Razorpay bills on again; the next request sees the new revision.
        Err(ChangeError::Stale)
    }
}
