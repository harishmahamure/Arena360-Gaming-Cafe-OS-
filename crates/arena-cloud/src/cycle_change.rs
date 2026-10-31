//! Switching a paid subscription's billing cycle and changing how it is paid (Story 10.10, FR178,
//! FR179, AD-20). Razorpay cannot change the period of a subscription, so both are a new Razorpay
//! subscription that the owner authorises through Checkout, on the same `subscriptions` row, band
//! and entitlement; nothing changes, and nothing is charged, until it is authorised, and the old
//! subscription is ended only after that.
//!
//! To yearly (an upgrade): the yearly price (10x monthly, GST on band IN on top) less the unused
//! days of the paid month, charged today as the new subscription's upfront charge, with the next
//! renewal a year out. To monthly: scheduled for the end of the paid year with no refund; the old
//! yearly subscription ends with its paid cycle once the owner has authorised the new one. A new
//! payment method: a new subscription for the same plan that starts at the period end and charges
//! nothing; authorising it replaces the mandate. Every amount comes from the price book locked onto
//! the subscription.

use arena_proto::{
    AddOns, BillingCycle, ChangeConfirmed, CycleSwitchPreview, MandateMethod, PlanChangeKind,
    SubscriptionCheckout, SwitchCycle,
};
use arena_store::control::{
    ChangeKind, ChangeStatus, ChangeSubject, ControlDb, NewPlanChange, ProposeOutcome, Subscription,
};

use crate::billing::{BillingProvider, MandateSpec, PlanChangeSpec, SubscriptionResult};
use crate::plan_change::{
    ChangeError, days_left, db_err, gate_method, gate_state, gst_on, gst_percent, item_of, kind_of,
    new_id, period_of, priced, prorate,
};
use crate::subscription::{Invalid, Quote, allowed_methods, quote, quote_for};

/// Razorpay will not start a subscription any sooner than this after it is created.
const MIN_LEAD_MS: i64 = 15 * 60 * 1000;

/// Everything a cycle switch comes to, before anything is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CyclePlan {
    pub kind: ChangeKind,
    pub cycle: BillingCycle,
    pub next: Quote,
    pub credit_total: i64,
    pub charge_amount: i64,
    pub charge_tax: i64,
    pub days_left: i64,
    pub effective_at: i64,
    pub renews_at: i64,
    pub methods: Vec<MandateMethod>,
    pub notice: Option<&'static str>,
}

impl CyclePlan {
    pub fn charge_total(&self) -> i64 {
        self.charge_amount + self.charge_tax
    }
}

/// `at` moved on by `months` calendar months.
pub(crate) fn months_after(at: i64, months: u32) -> i64 {
    chrono::DateTime::from_timestamp_millis(at)
        .and_then(|t| t.checked_add_months(chrono::Months::new(months)))
        .map_or(at, |t| t.timestamp_millis())
}

/// Works out switching `sub` to `req.cycle`. Only an `active` subscription that is not set to cancel
/// can switch, and only to the cycle it is not on.
pub async fn plan_cycle(
    db: &ControlDb,
    sub: &Subscription,
    req: &SwitchCycle,
    now: i64,
) -> Result<CyclePlan, ChangeError> {
    gate_state(sub)?;
    let item = item_of(sub.tier).ok_or_else(|| ChangeError::Pricing("free".to_owned()))?;
    if req.cycle == sub.cycle {
        return Err(ChangeError::Invalid(Invalid {
            field: "cycle",
            message: "This branch is already billed this way.",
        }));
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
    let Some(period_end) = sub.current_period_end else {
        return Err(ChangeError::Blocked(
            "This plan can't be changed right now.",
        ));
    };
    let add_ons = AddOns {
        devices_20: sub.addon_devices_20,
        tunnel: sub.addon_tunnel,
        owner_login: sub.addon_owner_login,
    };
    let current = quote_for(db, sub).await.map_err(priced)?;
    let next = quote(db, &sub.price_book_id, sub.band, item, req.cycle, &add_ons)
        .await
        .map_err(priced)?;
    let (methods, notice) = allowed_methods(sub.band, next.total_minor);
    let plan = match req.cycle {
        BillingCycle::Yearly => {
            let (_, period_days) = period_of(period_end, sub.cycle);
            let left = days_left(now, period_end, period_days);
            let credit = prorate(0, current.amount_minor, left, period_days);
            let charge_amount = (next.amount_minor - credit).max(0);
            let charge_tax = gst_on(charge_amount, sub.band);
            CyclePlan {
                kind: ChangeKind::Now,
                cycle: req.cycle,
                credit_total: (next.total_minor - (charge_amount + charge_tax)).max(0),
                charge_amount,
                charge_tax,
                days_left: left,
                effective_at: now,
                renews_at: months_after(now, 12),
                next,
                methods,
                notice,
            }
        }
        BillingCycle::Monthly => CyclePlan {
            kind: ChangeKind::PeriodEnd,
            cycle: req.cycle,
            credit_total: 0,
            charge_amount: 0,
            charge_tax: 0,
            days_left: 0,
            effective_at: period_end,
            renews_at: months_after(period_end, 1),
            next,
            methods,
            notice,
        },
    };
    Ok(plan)
}

/// The summary the owner confirms.
pub fn preview_of(sub: &Subscription, p: &CyclePlan) -> CycleSwitchPreview {
    CycleSwitchPreview {
        rev: sub.rev,
        kind: kind_of(p.kind),
        currency: p.next.currency.clone(),
        cycle: p.cycle,
        gst_percent: gst_percent(sub.band),
        cycle_amount_minor: p.next.amount_minor,
        cycle_tax_minor: p.next.tax_minor,
        cycle_total_minor: p.next.total_minor,
        credit_total_minor: p.credit_total,
        days_left: u32::try_from(p.days_left).unwrap_or(0),
        charge_amount_minor: p.charge_amount,
        charge_tax_minor: p.charge_tax,
        charge_total_minor: p.charge_total(),
        effective_at: p.effective_at,
        renews_at: p.renews_at,
        needs_mandate: true,
        methods: p.methods.clone(),
        notice: p.notice.map(str::to_owned),
    }
}

/// The new Razorpay subscription of a change, as the owner's Checkout opens it.
#[allow(clippy::too_many_arguments)]
async fn start_mandate(
    db: &ControlDb,
    provider: &BillingProvider,
    sub: &Subscription,
    new: NewPlanChange,
    rev: i64,
    mandate: MandateSpec,
    quote: &Quote,
    notice: Option<&'static str>,
    now: i64,
) -> Result<SubscriptionCheckout, ChangeError> {
    let key_id = provider
        .key_id()
        .ok_or_else(|| ChangeError::Provider("no checkout key".to_owned()))?
        .to_owned();
    let change_id = new.id.clone();
    match db
        .propose_plan_change(&new, rev, now)
        .await
        .map_err(db_err)?
    {
        ProposeOutcome::Proposed => {}
        ProposeOutcome::Stale => return Err(ChangeError::Stale),
    }
    let sent = match provider.create_mandate(&mandate).await {
        Ok(SubscriptionResult::Created(rid)) => {
            let attached = db.attach_change_razorpay(&change_id, &rid, now).await;
            if !matches!(attached, Ok(true)) {
                // The owner could still authorise this subscription with no change to match it.
                if let Err(e) = provider.cancel_subscription(&rid).await {
                    tracing::error!(subscription_id = %sub.id, error = %e.0, "could not cancel an unattached mandate");
                }
            }
            match attached {
                Ok(true) => Ok(SubscriptionCheckout {
                    subscription_id: sub.id.clone(),
                    state: sub.state,
                    key_id,
                    razorpay_subscription_id: rid,
                    band: sub.band,
                    currency: sub.currency.clone(),
                    tier: sub.tier,
                    cycle: mandate.change.cycle,
                    methods: mandate.methods.clone(),
                    notice: notice.map(str::to_owned),
                    amount_minor: quote.amount_minor,
                    tax_minor: quote.tax_minor,
                    total_minor: quote.total_minor,
                }),
                Ok(false) => Err(ChangeError::Stale),
                Err(e) => Err(db_err(e)),
            }
        }
        Ok(SubscriptionResult::Deferred) => Err(ChangeError::Provider(
            "the billing provider is manual".to_owned(),
        )),
        Err(e) => Err(ChangeError::Provider(e.0)),
    };
    if sent.is_err() {
        // The mandate was not created at the provider: the change must not stay open.
        if let Err(close) = db
            .close_plan_change(&sub.id, &change_id, ChangeStatus::Withdrawn, now)
            .await
        {
            tracing::error!(subscription_id = %sub.id, error = %close, "could not withdraw a failed change");
        }
    }
    sent
}

fn spec_of(
    sub: &Subscription,
    razorpay_id: &str,
    change_id: &str,
    cycle: BillingCycle,
    renewal_minor: i64,
    charge_minor: i64,
    now: bool,
) -> Result<PlanChangeSpec, ChangeError> {
    Ok(PlanChangeSpec {
        subscription_id: sub.id.clone(),
        razorpay_subscription_id: razorpay_id.to_owned(),
        change_id: change_id.to_owned(),
        book_id: sub.price_book_id.clone(),
        band: sub.band,
        item: item_of(sub.tier).ok_or_else(|| ChangeError::Pricing("free".to_owned()))?,
        cycle,
        currency: sub.currency.clone(),
        renewal_minor,
        charge_minor,
        now,
    })
}

fn current_razorpay_id(sub: &Subscription) -> Result<&str, ChangeError> {
    sub.razorpay_subscription_id
        .as_deref()
        .ok_or(ChangeError::Blocked(
            "This plan can't be changed right now.",
        ))
}

/// Confirms a cycle switch against the `rev` the owner looked at: stores it as held, creates the new
/// Razorpay subscription and answers its Checkout. The live subscription is untouched.
pub async fn confirm_cycle(
    db: &ControlDb,
    provider: &BillingProvider,
    sub: &Subscription,
    req: &SwitchCycle,
    now: i64,
) -> Result<ChangeConfirmed, ChangeError> {
    if sub.rev != req.rev {
        return Err(ChangeError::Stale);
    }
    let p = plan_cycle(db, sub, req, now).await?;
    let razorpay_id = current_razorpay_id(sub)?;
    let change_id = new_id();
    let period_end = sub.current_period_end.unwrap_or(now);
    let new = NewPlanChange {
        id: change_id.clone(),
        subscription_id: sub.id.clone(),
        kind: p.kind,
        status: ChangeStatus::Held,
        subject: ChangeSubject::Cycle,
        cycle: Some(p.cycle),
        tier: sub.tier,
        addon_devices_20: sub.addon_devices_20,
        addon_tunnel: sub.addon_tunnel,
        addon_owner_login: sub.addon_owner_login,
        charge_minor: p.charge_total(),
        renewal_minor: p.next.total_minor,
        // To yearly the new period ends a year out; to monthly the switch starts at the period end.
        effective_at: if p.kind == ChangeKind::Now {
            p.renews_at
        } else {
            period_end
        },
        razorpay_subscription_id: None,
    };
    let mandate = MandateSpec {
        change: spec_of(
            sub,
            razorpay_id,
            &change_id,
            p.cycle,
            p.next.total_minor,
            p.charge_total(),
            p.kind == ChangeKind::Now,
        )?,
        start_at_ms: if p.kind == ChangeKind::Now {
            p.renews_at
        } else {
            period_end.max(now + MIN_LEAD_MS)
        },
        methods: p.methods.clone(),
    };
    let checkout = start_mandate(
        db, provider, sub, new, req.rev, mandate, &p.next, p.notice, now,
    )
    .await?;
    tracing::info!(
        subscription_id = %sub.id,
        change_id = %change_id,
        cycle = p.cycle.as_str(),
        charge_minor = p.charge_total(),
        renewal_minor = p.next.total_minor,
        "cycle switch confirmed"
    );
    Ok(ChangeConfirmed {
        kind: kind_of(p.kind),
        effective_at: p.effective_at,
        charge_total_minor: p.charge_total(),
        renewal_total_minor: p.next.total_minor,
        checkout: Some(checkout),
    })
}

/// "Change payment method": a new Razorpay subscription for the same plan, starting at the end of
/// the paid period, whose Checkout offers the methods the current renewal amount allows. Nothing is
/// charged. The mandate is replaced when it is authorised; the old one is cancelled only then.
pub async fn confirm_method(
    db: &ControlDb,
    provider: &BillingProvider,
    sub: &Subscription,
    rev: i64,
    now: i64,
) -> Result<ChangeConfirmed, ChangeError> {
    if sub.rev != rev {
        return Err(ChangeError::Stale);
    }
    gate_method(sub)?;
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
    let Some(period_end) = sub.current_period_end else {
        return Err(ChangeError::Blocked(
            "This plan can't be changed right now.",
        ));
    };
    let razorpay_id = current_razorpay_id(sub)?;
    let current = quote_for(db, sub).await.map_err(priced)?;
    let (methods, notice) = allowed_methods(sub.band, current.total_minor);
    let change_id = new_id();
    let new = NewPlanChange {
        id: change_id.clone(),
        subscription_id: sub.id.clone(),
        kind: ChangeKind::PeriodEnd,
        status: ChangeStatus::Held,
        subject: ChangeSubject::Method,
        cycle: None,
        tier: sub.tier,
        addon_devices_20: sub.addon_devices_20,
        addon_tunnel: sub.addon_tunnel,
        addon_owner_login: sub.addon_owner_login,
        charge_minor: 0,
        renewal_minor: current.total_minor,
        effective_at: period_end,
        razorpay_subscription_id: None,
    };
    let mandate = MandateSpec {
        change: spec_of(
            sub,
            razorpay_id,
            &change_id,
            sub.cycle,
            current.total_minor,
            0,
            false,
        )?,
        start_at_ms: period_end.max(now + MIN_LEAD_MS),
        methods,
    };
    let checkout =
        start_mandate(db, provider, sub, new, rev, mandate, &current, notice, now).await?;
    tracing::info!(
        subscription_id = %sub.id,
        change_id = %change_id,
        "payment method change started"
    );
    Ok(ChangeConfirmed {
        kind: PlanChangeKind::PeriodEnd,
        effective_at: period_end,
        charge_total_minor: 0,
        renewal_total_minor: current.total_minor,
        checkout: Some(checkout),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_moves_on_by_calendar_months() {
        let ms = |y, m, d| {
            chrono::NaiveDate::from_ymd_opt(y, m, d)
                .unwrap()
                .and_hms_opt(8, 0, 0)
                .unwrap()
                .and_utc()
                .timestamp_millis()
        };
        assert_eq!(months_after(ms(2027, 1, 15), 12), ms(2028, 1, 15));
        assert_eq!(months_after(ms(2027, 1, 31), 1), ms(2027, 2, 28));
        assert_eq!(months_after(ms(2027, 12, 15), 1), ms(2028, 1, 15));
    }
}
