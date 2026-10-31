//! Changes of a paid subscription: tier and add-ons (Story 10.9), billing cycle and payment method
//! (Story 10.10). A change is proposed against
//! the subscription's `rev` (a second change from another device fails as stale), stays open until
//! Razorpay confirms its charge, the period ends or the owner withdraws it, and is applied by the
//! same transaction that records the event which completes it.

use arena_proto::{BillingCycle, Tier};

use super::{ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_text, col_text, text};

/// When a change takes effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// An upgrade: today, paid by a prorated charge.
    Now,
    /// A downgrade or a removal: at the end of the paid period, with no refund.
    PeriodEnd,
}

impl ChangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Now => "now",
            Self::PeriodEnd => "period_end",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "now" => Some(Self::Now),
            "period_end" => Some(Self::PeriodEnd),
            _ => None,
        }
    }
}

/// What a change is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeSubject {
    /// Tier and add-ons (Story 10.9).
    Plan,
    /// A switch between monthly and yearly billing, on a new Razorpay subscription (Story 10.10).
    Cycle,
    /// A new mandate for the same plan (Story 10.10).
    Method,
}

impl ChangeSubject {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Cycle => "cycle",
            Self::Method => "method",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Plan, Self::Cycle, Self::Method]
            .into_iter()
            .find(|v| v.as_str() == s)
    }
}

/// Where a change stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeStatus {
    /// Waiting for the owner to authorise a card or eNACH mandate; nothing is charged.
    Held,
    /// The prorated charge was asked for and Razorpay has not confirmed it.
    Charging,
    /// Waits for the end of the paid period.
    Scheduled,
    Applied,
    Withdrawn,
    Superseded,
}

impl ChangeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Held => "held",
            Self::Charging => "charging",
            Self::Scheduled => "scheduled",
            Self::Applied => "applied",
            Self::Withdrawn => "withdrawn",
            Self::Superseded => "superseded",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            Self::Held,
            Self::Charging,
            Self::Scheduled,
            Self::Applied,
            Self::Withdrawn,
            Self::Superseded,
        ]
        .into_iter()
        .find(|v| v.as_str() == s)
    }
}

const OPEN: &str = "status IN ('held', 'charging', 'scheduled')";

/// One stored change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanChange {
    pub id: String,
    pub subscription_id: String,
    pub kind: ChangeKind,
    pub status: ChangeStatus,
    pub subject: ChangeSubject,
    /// The cycle a `cycle` change switches to.
    pub cycle: Option<BillingCycle>,
    pub tier: Tier,
    pub addon_devices_20: u32,
    pub addon_tunnel: u32,
    pub addon_owner_login: u32,
    /// The prorated charge, GST included.
    pub charge_minor: i64,
    /// What each renewal debits after the change, GST included.
    pub renewal_minor: i64,
    pub effective_at: i64,
    /// The new Razorpay subscription a held change waits on.
    pub razorpay_subscription_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A change to propose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPlanChange {
    pub id: String,
    pub subscription_id: String,
    pub kind: ChangeKind,
    pub status: ChangeStatus,
    pub subject: ChangeSubject,
    pub cycle: Option<BillingCycle>,
    pub tier: Tier,
    pub addon_devices_20: u32,
    pub addon_tunnel: u32,
    pub addon_owner_login: u32,
    pub charge_minor: i64,
    pub renewal_minor: i64,
    pub effective_at: i64,
    pub razorpay_subscription_id: Option<String>,
}

/// What proposing a change did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposeOutcome {
    Proposed,
    /// The subscription moved since the owner looked (`rev`) or is no longer `active`.
    Stale,
}

/// The change an event completes, applied with the subscription's own update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedPlanChange<'a> {
    pub change_id: &'a str,
    pub tier: Tier,
    pub addon_devices_20: u32,
    pub addon_tunnel: u32,
    pub addon_owner_login: u32,
    /// The cycle the subscription switches to (a cycle change).
    pub cycle: Option<BillingCycle>,
    /// Only record that the new mandate is authorised; the change applies with its first charge.
    pub authorise_only: bool,
    /// The Razorpay subscription that replaces the current one (a held change).
    pub razorpay_subscription_id: Option<&'a str>,
}

const COLUMNS: &str = "id, subscription_id, kind, status, tier, addon_devices_20, addon_tunnel, \
     addon_owner_login, charge_minor, renewal_minor, effective_at, razorpay_subscription_id, \
     created_at, updated_at, subject, cycle";

fn count(r: &[SqlValue], i: usize) -> Result<u32, ControlError> {
    u32::try_from(col_int(r, i)?)
        .map_err(|_| ControlError::Db(format!("column {i}: quantity out of range")))
}

fn change_from(r: &[SqlValue]) -> Result<PlanChange, ControlError> {
    let bad = |what: &str, v: &str| ControlError::Db(format!("unknown change {what} {v:?}"));
    let kind = col_text(r, 2)?;
    let status = col_text(r, 3)?;
    let tier = col_text(r, 4)?;
    let subject = col_text(r, 14)?;
    Ok(PlanChange {
        id: col_text(r, 0)?,
        subscription_id: col_text(r, 1)?,
        kind: ChangeKind::parse(&kind).ok_or_else(|| bad("kind", &kind))?,
        status: ChangeStatus::parse(&status).ok_or_else(|| bad("status", &status))?,
        subject: ChangeSubject::parse(&subject).ok_or_else(|| bad("subject", &subject))?,
        cycle: col_opt_text(r, 15)?
            .map(|c| BillingCycle::parse(&c).ok_or_else(|| bad("cycle", &c)))
            .transpose()?,
        tier: Tier::parse(&tier).ok_or_else(|| bad("tier", &tier))?,
        addon_devices_20: count(r, 5)?,
        addon_tunnel: count(r, 6)?,
        addon_owner_login: count(r, 7)?,
        charge_minor: col_int(r, 8)?,
        renewal_minor: col_int(r, 9)?,
        effective_at: col_int(r, 10)?,
        razorpay_subscription_id: col_opt_text(r, 11)?,
        created_at: col_int(r, 12)?,
        updated_at: col_int(r, 13)?,
    })
}

/// The statements that apply a change: the subscription takes the new tier and add-ons (and the
/// new Razorpay subscription) and the change is marked applied.
pub(super) fn apply_stmts(p: &AppliedPlanChange<'_>, subscription_id: &str, now: i64) -> Vec<Stmt> {
    let int = |n: u32| SqlValue::Integer(i64::from(n));
    if p.authorise_only {
        // The owner authorised the new mandate of a switch that starts later: it can no longer be
        // kept out, and its first charge applies it.
        return vec![
            Stmt::new(
                "UPDATE subscription_changes SET status = 'charging', updated_at = ? \
                 WHERE id = ? AND status = 'held'",
                vec![SqlValue::Integer(now), text(p.change_id)],
            )
            .expect(1),
        ];
    }
    vec![
        Stmt::new(
            "UPDATE subscriptions SET tier = ?, addon_devices_20 = ?, addon_tunnel = ?, \
             addon_owner_login = ?, cycle = COALESCE(?, cycle), \
             razorpay_subscription_id = COALESCE(?, razorpay_subscription_id) WHERE id = ?",
            vec![
                text(p.tier.as_str()),
                int(p.addon_devices_20),
                int(p.addon_tunnel),
                int(p.addon_owner_login),
                p.cycle.map_or(SqlValue::Null, |c| text(c.as_str())),
                p.razorpay_subscription_id.map_or(SqlValue::Null, text),
                text(subscription_id),
            ],
        )
        .expect(1),
        Stmt::new(
            "UPDATE subscription_changes SET status = 'applied', updated_at = ? \
             WHERE id = ? AND status IN ('held', 'charging', 'scheduled')",
            vec![SqlValue::Integer(now), text(p.change_id)],
        )
        .expect(1),
    ]
}

impl ControlDb {
    /// The change of `subscription_id` that is still open, if it has one.
    pub async fn open_plan_change(
        &self,
        subscription_id: &str,
    ) -> Result<Option<PlanChange>, ControlError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM subscription_changes WHERE subscription_id = ? AND {OPEN}"
        );
        let rows = self.query(&sql, vec![text(subscription_id)]).await?;
        rows.first().map(|r| change_from(r)).transpose()
    }

    /// The open change waiting on the Razorpay subscription `razorpay_subscription_id`: held for
    /// its mandate, or (a switch to monthly) authorised and waiting for its first charge.
    pub async fn held_plan_change_by_razorpay_id(
        &self,
        razorpay_subscription_id: &str,
    ) -> Result<Option<PlanChange>, ControlError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM subscription_changes WHERE razorpay_subscription_id = ? \
             AND status IN ('held', 'charging')"
        );
        let rows = self
            .query(&sql, vec![text(razorpay_subscription_id)])
            .await?;
        rows.first().map(|r| change_from(r)).transpose()
    }

    /// Every change the subscription ever had, oldest first.
    pub async fn plan_changes_of(
        &self,
        subscription_id: &str,
    ) -> Result<Vec<PlanChange>, ControlError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM subscription_changes WHERE subscription_id = ? \
             ORDER BY created_at, id"
        );
        let rows = self.query(&sql, vec![text(subscription_id)]).await?;
        rows.iter().map(|r| change_from(r)).collect()
    }

    /// Proposes `new` for an `active` subscription that is still at `expect_rev`: the subscription's
    /// `rev` moves on (so a change from another device that read the same `rev` is stale), an open
    /// change is superseded and `new` is stored.
    pub async fn propose_plan_change(
        &self,
        new: &NewPlanChange,
        expect_rev: i64,
        now: i64,
    ) -> Result<ProposeOutcome, ControlError> {
        let int = |n: u32| SqlValue::Integer(i64::from(n));
        let done = self
            .transact(vec![
                Stmt::new(
                    "UPDATE subscriptions SET rev = rev + 1, updated_at = ? \
                     WHERE id = ? AND rev = ? AND state = 'active'",
                    vec![
                        SqlValue::Integer(now),
                        text(&new.subscription_id),
                        SqlValue::Integer(expect_rev),
                    ],
                )
                .expect(1),
                Stmt::new(
                    "UPDATE subscription_changes SET status = 'superseded', updated_at = ? \
                     WHERE subscription_id = ? AND status IN ('held', 'charging', 'scheduled')",
                    vec![SqlValue::Integer(now), text(&new.subscription_id)],
                ),
                Stmt::new(
                    "INSERT INTO subscription_changes (id, subscription_id, kind, status, tier, \
                     addon_devices_20, addon_tunnel, addon_owner_login, charge_minor, \
                     renewal_minor, effective_at, razorpay_subscription_id, created_at, \
                     updated_at, subject, cycle) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    vec![
                        text(&new.id),
                        text(&new.subscription_id),
                        text(new.kind.as_str()),
                        text(new.status.as_str()),
                        text(new.tier.as_str()),
                        int(new.addon_devices_20),
                        int(new.addon_tunnel),
                        int(new.addon_owner_login),
                        SqlValue::Integer(new.charge_minor),
                        SqlValue::Integer(new.renewal_minor),
                        SqlValue::Integer(new.effective_at),
                        new.razorpay_subscription_id
                            .as_deref()
                            .map_or(SqlValue::Null, text),
                        SqlValue::Integer(now),
                        SqlValue::Integer(now),
                        text(new.subject.as_str()),
                        new.cycle.map_or(SqlValue::Null, |c| text(c.as_str())),
                    ],
                )
                .expect(1),
            ])
            .await?;
        Ok(if done {
            ProposeOutcome::Proposed
        } else {
            ProposeOutcome::Stale
        })
    }

    /// Ends the open change `change_id` as `status` (`withdrawn` or `superseded`) and moves the
    /// subscription's `rev` on. `false`: it was no longer open.
    pub async fn close_plan_change(
        &self,
        subscription_id: &str,
        change_id: &str,
        status: ChangeStatus,
        now: i64,
    ) -> Result<bool, ControlError> {
        self.transact(vec![
            Stmt::new(
                "UPDATE subscription_changes SET status = ?, updated_at = ? \
                 WHERE id = ? AND subscription_id = ? \
                 AND status IN ('held', 'charging', 'scheduled')",
                vec![
                    text(status.as_str()),
                    SqlValue::Integer(now),
                    text(change_id),
                    text(subscription_id),
                ],
            )
            .expect(1),
            Stmt::new(
                "UPDATE subscriptions SET rev = rev + 1, updated_at = ? WHERE id = ?",
                vec![SqlValue::Integer(now), text(subscription_id)],
            )
            .expect(1),
        ])
        .await
    }
}

impl ControlDb {
    /// Records the Razorpay subscription that authorises the mandate of a held change. `false`: the
    /// change is no longer held or already has one.
    pub async fn attach_change_razorpay(
        &self,
        change_id: &str,
        razorpay_subscription_id: &str,
        now: i64,
    ) -> Result<bool, ControlError> {
        self.transact(vec![
            Stmt::new(
                "UPDATE subscription_changes SET razorpay_subscription_id = ?, updated_at = ? \
                 WHERE id = ? AND status = 'held' AND razorpay_subscription_id IS NULL",
                vec![
                    text(razorpay_subscription_id),
                    SqlValue::Integer(now),
                    text(change_id),
                ],
            )
            .expect(1),
        ])
        .await
    }

    /// Marks the open operator tasks of `kind` for `target` done (an automatic step finished what
    /// the task asked an operator to do).
    pub async fn finish_ops_tasks(
        &self,
        kind: &str,
        target: &str,
        now: i64,
    ) -> Result<(), ControlError> {
        self.execute(
            "UPDATE ops_tasks SET done_at = ? WHERE kind = ? AND target = ? AND done_at IS NULL",
            vec![SqlValue::Integer(now), text(kind), text(target)],
        )
        .await
        .map(|_| ())
    }
}
