//! Razorpay events and the subscription transitions they cause (Story 10.3). An event is written
//! to `billing_events` in the same transaction as the change it drives (the subscription row, its
//! history row and, when the branch becomes entitled, its `entitlements` row), so a repeated
//! delivery is a no-op and a crash never leaves an event recorded but not applied.

use arena_proto::{MandateMethod, SubscriptionState, Tier};

use super::invoice::{NewInvoice, issue_stmt};
use super::plan_change::{AppliedPlanChange, apply_stmts};
use super::price_book::NewOpsTask;
use super::subscription::{COLUMNS, Subscription, subscription_from};
use super::{
    ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_int, col_opt_text, col_text, text,
};

/// One Razorpay event to record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRecord<'a> {
    pub id: &'a str,
    pub razorpay_event_id: &'a str,
    pub kind: &'a str,
    /// The subscription it concerns; `None` when no subscription of ours matches.
    pub subscription_id: Option<&'a str>,
    pub payload_hash: &'a str,
    /// `webhook` or `reconcile`.
    pub source: &'a str,
    pub received_at: i64,
}

/// A stored event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BillingEvent {
    pub id: String,
    pub razorpay_event_id: String,
    pub kind: String,
    pub subscription_id: Option<String>,
    pub payload_hash: String,
    pub source: String,
    pub received_at: i64,
    pub applied_at: Option<i64>,
}

/// The paid entitlement a transition issues.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntitlementWrite<'a> {
    pub tier: Tier,
    pub addons_json: &'a str,
}

/// An `ops_audit` row written with the change (its target is the subscription).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOpsAudit<'a> {
    pub id: &'a str,
    pub operator: &'a str,
    pub action: &'a str,
    pub reason: &'a str,
    pub details_json: &'a str,
}

/// What a change does to the renewal life of a subscription (Story 10.11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Lifecycle {
    /// The end of the grace period, on cloud time; read only when the new state is `grace`.
    pub grace_until: Option<i64>,
    /// A renewal charge failed before the paid period ended: the sweep moves the subscription to
    /// `grace` when the period does. Any other change that does not renew the period keeps the
    /// mark; a renewal, a grace or a lapse clears it.
    pub renewal_failed_at: Option<i64>,
    /// The branch's paid entitlement ends: it becomes the Free tier with Free claims. A comp
    /// entitlement is left alone.
    pub lapse: bool,
}

/// What an event does to a subscription. It applies only if the row is still at `expect_rev`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionChange<'a> {
    pub subscription_id: &'a str,
    pub expect_rev: i64,
    pub old_state: SubscriptionState,
    pub new_state: SubscriptionState,
    /// The new end of the paid period; `None` keeps it.
    pub period_end: Option<i64>,
    /// Set when the row has no `first_paid_at` yet.
    pub first_paid_at: Option<i64>,
    /// Set when the row has no payment country yet.
    pub payment_country: Option<&'a str>,
    pub mandate_method: Option<MandateMethod>,
    /// Names the payment method (card last four or bank); set with `mandate_method`.
    pub mandate_detail: Option<&'a str>,
    pub entitlement: Option<EntitlementWrite<'a>>,
    pub history_id: &'a str,
    /// The event id or job that caused it.
    pub cause: &'a str,
    pub ops_task: Option<NewOpsTask<'a>>,
    pub audit: Option<NewOpsAudit<'a>>,
    /// Marks the open operator tasks of this kind for this subscription done.
    pub close_task_kind: Option<&'a str>,
    /// A tier or add-on change this event completes (Story 10.9).
    pub plan_change: Option<AppliedPlanChange<'a>>,
    /// Grace, a failed renewal and a lapse (Story 10.11).
    pub lifecycle: Lifecycle,
}

/// What recording an event did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventOutcome {
    /// The event is recorded (and its change, if any, is applied).
    Recorded,
    /// The Razorpay event id was already recorded; nothing was written.
    Duplicate,
    /// The subscription moved since it was read; nothing was written, read it again.
    Conflict,
}

/// One `subscription_history` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRow {
    pub id: String,
    pub subscription_id: String,
    pub old_state: String,
    pub new_state: String,
    pub cause: String,
    pub cloud_time: i64,
}

const EVENT_COLUMNS: &str = "id, razorpay_event_id, type, subscription_id, payload_hash, source, \
     received_at, applied_at";

fn event_from(r: &[SqlValue]) -> Result<BillingEvent, ControlError> {
    Ok(BillingEvent {
        id: col_text(r, 0)?,
        razorpay_event_id: col_text(r, 1)?,
        kind: col_text(r, 2)?,
        subscription_id: col_opt_text(r, 3)?,
        payload_hash: col_text(r, 4)?,
        source: col_text(r, 5)?,
        received_at: col_int(r, 6)?,
        applied_at: col_opt_int(r, 7)?,
    })
}

fn opt_text(v: Option<&str>) -> SqlValue {
    v.map_or(SqlValue::Null, text)
}

fn opt_int(v: Option<i64>) -> SqlValue {
    v.map_or(SqlValue::Null, SqlValue::Integer)
}

/// The statements that apply `c`: the subscription row, the entitlement it issues, its history row
/// and the operator task and audit row it leaves.
fn change_stmts(c: &SubscriptionChange<'_>, now: i64) -> Vec<Stmt> {
    let mut stmts = Vec::new();
    stmts.push(
        Stmt::new(
            "UPDATE subscriptions SET state = ?, rev = rev + 1, updated_at = ?, \
                 current_period_end = COALESCE(?, current_period_end), \
                 first_paid_at = COALESCE(first_paid_at, ?), \
                 payment_country = COALESCE(payment_country, ?), \
                 mandate_method = COALESCE(?, mandate_method), \
                 mandate_detail = CASE WHEN ? IS NULL THEN mandate_detail ELSE ? END, \
                 grace_until = CASE WHEN ? = 'grace' THEN COALESCE(?, grace_until) ELSE NULL END, \
                 renewal_failed_at = CASE WHEN ? IS NOT NULL THEN COALESCE(renewal_failed_at, ?) \
                 WHEN ? = 'active' AND ? IS NULL THEN renewal_failed_at ELSE NULL END \
                 WHERE id = ? AND rev = ?",
            vec![
                text(c.new_state.as_str()),
                SqlValue::Integer(now),
                opt_int(c.period_end),
                opt_int(c.first_paid_at),
                opt_text(c.payment_country),
                opt_text(c.mandate_method.map(MandateMethod::as_str)),
                opt_text(c.mandate_method.map(MandateMethod::as_str)),
                opt_text(c.mandate_detail),
                text(c.new_state.as_str()),
                opt_int(c.lifecycle.grace_until),
                opt_int(c.lifecycle.renewal_failed_at),
                opt_int(c.lifecycle.renewal_failed_at),
                text(c.new_state.as_str()),
                opt_int(c.period_end),
                text(c.subscription_id),
                SqlValue::Integer(c.expect_rev),
            ],
        )
        .expect(1),
    );
    if let Some(e) = &c.entitlement {
        stmts.push(
            Stmt::new(
                "INSERT INTO entitlements (branch_id, tenant_id, tier, source, \
                     addons_json, issued_at, until, updated_at) \
                     SELECT s.branch_id, s.tenant_id, ?, 'paid', ?, ?, NULL, ? \
                     FROM subscriptions s WHERE s.id = ? \
                     ON CONFLICT (branch_id) DO UPDATE SET tier = excluded.tier, \
                     source = 'paid', addons_json = excluded.addons_json, \
                     issued_at = CASE WHEN entitlements.source = 'paid' \
                     THEN entitlements.issued_at ELSE excluded.issued_at END, \
                     until = NULL, updated_at = excluded.updated_at",
                vec![
                    text(e.tier.as_str()),
                    text(e.addons_json),
                    SqlValue::Integer(now),
                    SqlValue::Integer(now),
                    text(c.subscription_id),
                ],
            )
            .expect(1),
        );
        stmts.push(
            Stmt::new(
                "INSERT INTO onboarding_progress (branch_id, step, state, completed_at) \
                     SELECT s.branch_id, 'tier', 'done', ? FROM subscriptions s \
                     WHERE s.id = ? ON CONFLICT (branch_id, step) DO UPDATE SET \
                     state = 'done', completed_at = COALESCE(\
                     onboarding_progress.completed_at, excluded.completed_at)",
                vec![SqlValue::Integer(now), text(c.subscription_id)],
            )
            .expect(1),
        );
    }
    if let Some(p) = &c.plan_change {
        stmts.extend(apply_stmts(p, c.subscription_id, now));
    }
    if c.lifecycle.lapse {
        // The paid entitlement ends; a comp grant the staff gave the branch is not ours to end.
        stmts.push(Stmt::new(
            "UPDATE entitlements SET tier = 'free', source = 'free', addons_json = '[]',              issued_at = ?, until = NULL, updated_at = ? WHERE source = 'paid'              AND branch_id = (SELECT branch_id FROM subscriptions WHERE id = ?)",
            vec![
                SqlValue::Integer(now),
                SqlValue::Integer(now),
                text(c.subscription_id),
            ],
        ));
    }
    stmts.push(
        Stmt::new(
            "INSERT INTO subscription_history (id, subscription_id, old_state, \
                 new_state, cause, cloud_time) VALUES (?, ?, ?, ?, ?, ?)",
            vec![
                text(c.history_id),
                text(c.subscription_id),
                text(c.old_state.as_str()),
                text(c.new_state.as_str()),
                text(c.cause),
                SqlValue::Integer(now),
            ],
        )
        .expect(1),
    );
    if let Some(t) = &c.ops_task {
        stmts.push(
            Stmt::new(
                "INSERT INTO ops_tasks (id, at, kind, target, details_json, done_at) \
                     VALUES (?, ?, ?, ?, ?, NULL)",
                vec![
                    text(t.id),
                    SqlValue::Integer(now),
                    text(t.kind),
                    text(t.target),
                    text(t.details_json),
                ],
            )
            .expect(1),
        );
    }
    if let Some(a) = &c.audit {
        stmts.push(
            Stmt::new(
                "INSERT INTO ops_audit (id, at, operator, action, target, reason, \
                     details_json) VALUES (?, ?, ?, ?, ?, ?, ?)",
                vec![
                    text(a.id),
                    SqlValue::Integer(now),
                    text(a.operator),
                    text(a.action),
                    text(c.subscription_id),
                    text(a.reason),
                    text(a.details_json),
                ],
            )
            .expect(1),
        );
    }
    if let Some(kind) = c.close_task_kind {
        stmts.push(Stmt::new(
            "UPDATE ops_tasks SET done_at = ? WHERE kind = ? AND target = ? AND done_at IS NULL",
            vec![SqlValue::Integer(now), text(kind), text(c.subscription_id)],
        ));
    }
    stmts
}

impl ControlDb {
    /// The subscription Razorpay knows as `razorpay_subscription_id`.
    pub async fn subscription_by_razorpay_id(
        &self,
        razorpay_subscription_id: &str,
    ) -> Result<Option<Subscription>, ControlError> {
        let sql = format!("SELECT {COLUMNS} FROM subscriptions WHERE razorpay_subscription_id = ?");
        let rows = self
            .query(&sql, vec![text(razorpay_subscription_id)])
            .await?;
        rows.first().map(|r| subscription_from(r)).transpose()
    }

    /// Subscriptions the reconcile job re-reads from Razorpay: `pending`, `renewal_needs_approval`
    /// and `grace` rows that have a Razorpay subscription, least recently touched first.
    pub async fn subscriptions_to_reconcile(
        &self,
        limit: u32,
    ) -> Result<Vec<Subscription>, ControlError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM subscriptions WHERE razorpay_subscription_id IS NOT NULL \
             AND state IN ('pending', 'renewal_needs_approval', 'grace') \
             ORDER BY updated_at, id LIMIT ?"
        );
        let rows = self
            .query(&sql, vec![SqlValue::Integer(i64::from(limit))])
            .await?;
        rows.iter().map(|r| subscription_from(r)).collect()
    }

    /// Subscriptions whose renewal life moves on at `now`: a renewal waiting for approval or a
    /// failed renewal charge whose paid period has ended (to `grace`), and a grace period that has
    /// run out (to `lapsed`). Oldest first.
    pub async fn lifecycle_due(
        &self,
        now: i64,
        limit: u32,
    ) -> Result<Vec<Subscription>, ControlError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM subscriptions WHERE              (state = 'renewal_needs_approval' AND current_period_end <= ?1)              OR (state = 'active' AND renewal_failed_at IS NOT NULL AND current_period_end <= ?1)              OR (state = 'grace' AND grace_until <= ?1)              ORDER BY COALESCE(grace_until, current_period_end), id LIMIT ?2"
        );
        let rows = self
            .query(
                &sql,
                vec![SqlValue::Integer(now), SqlValue::Integer(i64::from(limit))],
            )
            .await?;
        rows.iter().map(|r| subscription_from(r)).collect()
    }

    /// Subscriptions in `renewal_needs_approval` or `grace`: the ones the owner is warned about.
    pub async fn subscriptions_awaiting_payment(&self) -> Result<Vec<Subscription>, ControlError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM subscriptions              WHERE state IN ('renewal_needs_approval', 'grace') ORDER BY updated_at, id"
        );
        let rows = self.query(&sql, Vec::new()).await?;
        rows.iter().map(|r| subscription_from(r)).collect()
    }

    /// How many subscriptions are in each state (the `subscriptions_per_state` metric).
    pub async fn subscription_state_counts(&self) -> Result<Vec<(String, i64)>, ControlError> {
        let rows = self
            .query(
                "SELECT state, COUNT(*) FROM subscriptions GROUP BY state ORDER BY state",
                Vec::new(),
            )
            .await?;
        rows.iter()
            .map(|r| Ok((col_text(r, 0)?, col_int(r, 1)?)))
            .collect()
    }

    /// Whether a renewal charge failed on this subscription before its period ended.
    pub async fn renewal_failed_at(
        &self,
        subscription_id: &str,
    ) -> Result<Option<i64>, ControlError> {
        let rows = self
            .query(
                "SELECT renewal_failed_at FROM subscriptions WHERE id = ?",
                vec![text(subscription_id)],
            )
            .await?;
        rows.first().map_or(Ok(None), |r| col_opt_int(r, 0))
    }

    /// The recorded event with this Razorpay event id.
    pub async fn billing_event(
        &self,
        razorpay_event_id: &str,
    ) -> Result<Option<BillingEvent>, ControlError> {
        let sql = format!("SELECT {EVENT_COLUMNS} FROM billing_events WHERE razorpay_event_id = ?");
        let rows = self.query(&sql, vec![text(razorpay_event_id)]).await?;
        rows.first().map(|r| event_from(r)).transpose()
    }

    /// Every recorded event, oldest first.
    pub async fn billing_events(&self) -> Result<Vec<BillingEvent>, ControlError> {
        let sql = format!("SELECT {EVENT_COLUMNS} FROM billing_events ORDER BY received_at, id");
        let rows = self.query(&sql, Vec::new()).await?;
        rows.iter().map(|r| event_from(r)).collect()
    }

    /// The subscription's transitions, oldest first.
    pub async fn subscription_history(
        &self,
        subscription_id: &str,
    ) -> Result<Vec<HistoryRow>, ControlError> {
        let rows = self
            .query(
                "SELECT id, subscription_id, old_state, new_state, cause, cloud_time \
                 FROM subscription_history WHERE subscription_id = ? ORDER BY cloud_time, id",
                vec![text(subscription_id)],
            )
            .await?;
        rows.iter()
            .map(|r| {
                Ok(HistoryRow {
                    id: col_text(r, 0)?,
                    subscription_id: col_text(r, 1)?,
                    old_state: col_text(r, 2)?,
                    new_state: col_text(r, 3)?,
                    cause: col_text(r, 4)?,
                    cloud_time: col_int(r, 5)?,
                })
            })
            .collect()
    }

    /// Applies `change` with no Razorpay event behind it (an operator's decision). `false`: the
    /// subscription moved since it was read and nothing was written.
    pub async fn apply_subscription_change(
        &self,
        change: &SubscriptionChange<'_>,
        now: i64,
    ) -> Result<bool, ControlError> {
        self.transact(change_stmts(change, now)).await
    }

    /// Records `event` and, in the same transaction, applies `change` and issues `invoice` (the tax
    /// invoice of the charge; only with a change). A Razorpay event id that is already recorded
    /// writes nothing (`Duplicate`); a subscription that is no longer at the revision `change`
    /// read writes nothing either (`Conflict`), and neither does an invoice number that another
    /// transaction took first (`Conflict`: read again and retry).
    pub async fn apply_billing_event(
        &self,
        event: &EventRecord<'_>,
        change: Option<&SubscriptionChange<'_>>,
        invoice: Option<&NewInvoice>,
        now: i64,
    ) -> Result<EventOutcome, ControlError> {
        let mut stmts = vec![
            Stmt::new(
                "INSERT INTO billing_events (id, razorpay_event_id, type, subscription_id, \
                 payload_hash, source, received_at, applied_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                vec![
                    text(event.id),
                    text(event.razorpay_event_id),
                    text(event.kind),
                    opt_text(event.subscription_id),
                    text(event.payload_hash),
                    text(event.source),
                    SqlValue::Integer(event.received_at),
                    opt_int(change.map(|_| now)),
                ],
            )
            .expect(1),
        ];
        if let Some(c) = change {
            stmts.extend(change_stmts(c, now));
            if let Some(inv) = invoice {
                stmts.push(issue_stmt(inv, event.id));
            }
        }
        match self.transact(stmts).await {
            Ok(true) => Ok(EventOutcome::Recorded),
            Ok(false) => Ok(EventOutcome::Conflict),
            Err(ControlError::Db(m)) if m.contains("billing_events") && m.contains("UNIQUE") => {
                Ok(EventOutcome::Duplicate)
            }
            Err(ControlError::Db(m)) if m.contains("billing_invoices") && m.contains("UNIQUE") => {
                Ok(EventOutcome::Conflict)
            }
            Err(e) => Err(e),
        }
    }
}
