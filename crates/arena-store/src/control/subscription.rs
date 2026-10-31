//! Branch subscriptions (Story 10.2): the band and price book an owner signed up at, copied onto
//! the row when it is created. A branch holds at most one subscription that is not over. This
//! story only creates a row, attaches the Razorpay subscription to it and abandons an unpaid one;
//! activation and payment belong to the webhook story.

use arena_proto::{Band, BillingCycle, MandateMethod, SubscriptionState, Tier};

use super::price_book::NewOpsTask;
use super::{
    ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_int, col_opt_text, col_text, text,
};

/// One subscription row. Amounts are not stored: they are priced from `price_book_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subscription {
    pub id: String,
    pub tenant_id: String,
    pub branch_id: String,
    pub state: SubscriptionState,
    pub tier: Tier,
    pub cycle: BillingCycle,
    pub band: Band,
    pub currency: String,
    pub price_book_id: String,
    pub addon_devices_20: u32,
    pub addon_tunnel: u32,
    pub addon_owner_login: u32,
    pub mandate_method: Option<MandateMethod>,
    pub razorpay_subscription_id: Option<String>,
    pub rev: i64,
    pub current_period_end: Option<i64>,
    pub grace_until: Option<i64>,
    pub first_paid_at: Option<i64>,
    pub cancel_at_period_end: bool,
    pub created_at: i64,
    pub updated_at: i64,
    /// Where the first payment came from (a country code, or `INTL`), once there is one.
    pub payment_country: Option<String>,
    /// The last four digits of the card, or the bank of the mandate, for the owner to recognise it.
    pub mandate_detail: Option<String>,
}

/// A subscription to create (always `pending`, with no Razorpay id yet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSubscription {
    pub id: String,
    pub tenant_id: String,
    pub branch_id: String,
    pub tier: Tier,
    pub cycle: BillingCycle,
    pub band: Band,
    pub currency: String,
    pub price_book_id: String,
    pub addon_devices_20: u32,
    pub addon_tunnel: u32,
    pub addon_owner_login: u32,
}

/// What reserving a subscription did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReserveOutcome {
    Reserved,
    /// The branch already has a subscription that is not over; nothing was written.
    Exists(SubscriptionState),
}

pub(super) const COLUMNS: &str = "id, tenant_id, branch_id, state, tier, cycle, band, currency, \
     price_book_id, addon_devices_20, addon_tunnel, addon_owner_login, mandate_method, \
     razorpay_subscription_id, rev, current_period_end, grace_until, first_paid_at, \
     cancel_at_period_end, created_at, updated_at, payment_country, mandate_detail";

fn count(r: &[SqlValue], i: usize) -> Result<u32, ControlError> {
    u32::try_from(col_int(r, i)?)
        .map_err(|_| ControlError::Db(format!("column {i}: quantity out of range")))
}

pub(super) fn subscription_from(r: &[SqlValue]) -> Result<Subscription, ControlError> {
    let bad = |what: &str, v: &str| ControlError::Db(format!("unknown subscription {what} {v:?}"));
    let state = col_text(r, 3)?;
    let tier = col_text(r, 4)?;
    let cycle = col_text(r, 5)?;
    let band = col_text(r, 6)?;
    let mandate = col_opt_text(r, 12)?;
    Ok(Subscription {
        id: col_text(r, 0)?,
        tenant_id: col_text(r, 1)?,
        branch_id: col_text(r, 2)?,
        state: SubscriptionState::parse(&state).ok_or_else(|| bad("state", &state))?,
        tier: Tier::parse(&tier).ok_or_else(|| bad("tier", &tier))?,
        cycle: BillingCycle::parse(&cycle).ok_or_else(|| bad("cycle", &cycle))?,
        band: Band::parse(&band).ok_or_else(|| bad("band", &band))?,
        currency: col_text(r, 7)?,
        price_book_id: col_text(r, 8)?,
        addon_devices_20: count(r, 9)?,
        addon_tunnel: count(r, 10)?,
        addon_owner_login: count(r, 11)?,
        mandate_method: mandate
            .map(|m| MandateMethod::parse(&m).ok_or_else(|| bad("mandate", &m)))
            .transpose()?,
        razorpay_subscription_id: col_opt_text(r, 13)?,
        rev: col_int(r, 14)?,
        current_period_end: col_opt_int(r, 15)?,
        grace_until: col_opt_int(r, 16)?,
        first_paid_at: col_opt_int(r, 17)?,
        cancel_at_period_end: col_int(r, 18)? == 1,
        created_at: col_int(r, 19)?,
        updated_at: col_int(r, 20)?,
        payment_country: col_opt_text(r, 21)?,
        mandate_detail: col_opt_text(r, 22)?,
    })
}

const OPEN_STATES: &str = "state NOT IN ('abandoned', 'cancelled', 'lapsed', 'ended', 'refunded')";

impl ControlDb {
    /// The subscription with `id`.
    pub async fn subscription(&self, id: &str) -> Result<Option<Subscription>, ControlError> {
        let sql = format!("SELECT {COLUMNS} FROM subscriptions WHERE id = ?");
        let rows = self.query(&sql, vec![text(id)]).await?;
        rows.first().map(|r| subscription_from(r)).transpose()
    }

    /// The branch's subscription that is not over, if it has one.
    pub async fn subscription_of_branch(
        &self,
        branch_id: &str,
    ) -> Result<Option<Subscription>, ControlError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM subscriptions WHERE branch_id = ? AND {OPEN_STATES} \
             ORDER BY created_at DESC, id DESC"
        );
        let rows = self.query(&sql, vec![text(branch_id)]).await?;
        rows.first().map(|r| subscription_from(r)).transpose()
    }

    /// Every subscription the branch ever had, newest first.
    pub async fn subscriptions_of_branch(
        &self,
        branch_id: &str,
    ) -> Result<Vec<Subscription>, ControlError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM subscriptions WHERE branch_id = ? \
             ORDER BY created_at DESC, id DESC"
        );
        let rows = self.query(&sql, vec![text(branch_id)]).await?;
        rows.iter().map(|r| subscription_from(r)).collect()
    }

    /// Inserts `new` as a `pending` subscription with no Razorpay id, unless the branch already
    /// holds one that is not over. The unique index decides a race between two requests.
    pub async fn reserve_subscription(
        &self,
        new: &NewSubscription,
        now: i64,
    ) -> Result<ReserveOutcome, ControlError> {
        if let Some(open) = self.subscription_of_branch(&new.branch_id).await? {
            return Ok(ReserveOutcome::Exists(open.state));
        }
        let int = |n: u32| SqlValue::Integer(i64::from(n));
        let done = self
            .transact(vec![
                Stmt::new(
                    "INSERT INTO subscriptions (id, tenant_id, branch_id, state, tier, cycle, \
                     band, currency, price_book_id, addon_devices_20, addon_tunnel, \
                     addon_owner_login, created_at, updated_at) \
                     VALUES (?, ?, ?, 'pending', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    vec![
                        text(&new.id),
                        text(&new.tenant_id),
                        text(&new.branch_id),
                        text(new.tier.as_str()),
                        text(new.cycle.as_str()),
                        text(new.band.as_str()),
                        text(&new.currency),
                        text(&new.price_book_id),
                        int(new.addon_devices_20),
                        int(new.addon_tunnel),
                        int(new.addon_owner_login),
                        SqlValue::Integer(now),
                        SqlValue::Integer(now),
                    ],
                )
                .expect(1),
            ])
            .await;
        match done {
            Ok(true) => Ok(ReserveOutcome::Reserved),
            Ok(false) => Err(ControlError::Db(
                "the subscription was not created".to_owned(),
            )),
            Err(ControlError::Db(m)) if m.contains("UNIQUE") => {
                let state = self
                    .subscription_of_branch(&new.branch_id)
                    .await?
                    .map_or(SubscriptionState::Pending, |s| s.state);
                Ok(ReserveOutcome::Exists(state))
            }
            Err(e) => Err(e),
        }
    }

    /// Records the Razorpay subscription on a `pending` row that has none. `false`: the row is
    /// not `pending` or already has one.
    pub async fn set_razorpay_subscription(
        &self,
        id: &str,
        razorpay_subscription_id: &str,
        now: i64,
    ) -> Result<bool, ControlError> {
        self.transact(vec![
            Stmt::new(
                "UPDATE subscriptions SET razorpay_subscription_id = ?, rev = rev + 1, \
                 updated_at = ? WHERE id = ? AND state = 'pending' \
                 AND razorpay_subscription_id IS NULL",
                vec![
                    text(razorpay_subscription_id),
                    SqlValue::Integer(now),
                    text(id),
                ],
            )
            .expect(1),
        ])
        .await
    }

    /// Abandons a `pending` subscription that was never paid (and leaves `task` for an operator
    /// when the provider could not be called). `false`: it is no longer `pending`, or was paid.
    pub async fn abandon_subscription(
        &self,
        id: &str,
        task: Option<&NewOpsTask<'_>>,
        now: i64,
    ) -> Result<bool, ControlError> {
        let mut stmts = vec![
            Stmt::new(
                "UPDATE subscriptions SET state = 'abandoned', rev = rev + 1, updated_at = ? \
                 WHERE id = ? AND state = 'pending' AND first_paid_at IS NULL",
                vec![SqlValue::Integer(now), text(id)],
            )
            .expect(1),
        ];
        if let Some(t) = task {
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
        self.transact(stmts).await
    }

    /// `pending`, unpaid subscriptions created before `created_before`, oldest first.
    pub async fn stale_pending_subscriptions(
        &self,
        created_before: i64,
        limit: u32,
    ) -> Result<Vec<Subscription>, ControlError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM subscriptions WHERE state = 'pending' \
             AND first_paid_at IS NULL AND created_at < ? ORDER BY created_at, id LIMIT ?"
        );
        let rows = self
            .query(
                &sql,
                vec![
                    SqlValue::Integer(created_before),
                    SqlValue::Integer(i64::from(limit)),
                ],
            )
            .await?;
        rows.iter().map(|r| subscription_from(r)).collect()
    }
}
