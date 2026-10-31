//! Changing tier and add-ons on a paid subscription (Story 10.9, FR178, FR179, FR182). An upgrade
//! (every part of the plan the same or bigger) starts today and is paid with a prorated charge for
//! the rest of the period: `(new daily rate - old daily rate) x days left`, in minor units, with
//! GST on band IN on top; the entitlement changes when Razorpay confirms that charge. A downgrade
//! (every part the same or smaller) is scheduled for the period end with no refund and changes
//! nothing until then. An upgrade whose new renewal debit is over the UPI Autopay limit on a UPI
//! mandate is held until a card or eNACH mandate exists. Every amount is priced from the price
//! book locked onto the subscription, never the live one.

use arena_proto::{
    AddOns, Band, BillingCycle, ChangeConfirmed, ChangePlan, HubPlatform, MandateMethod,
    PendingPlanChange, PlanChangeKind, PlanChangePreview, PriceItem, SubscriptionCheckout,
    SubscriptionState, Tier,
};
use arena_store::control::{
    ChangeKind, ChangeStatus, ControlDb, NewPlanChange, PlanChange, ProposeOutcome, Subscription,
};

use crate::billing::{
    BillingProvider, ChangeResult, MandateSpec, PlanChangeSpec, SubscriptionResult,
};
use crate::subscription::{
    GST_PERCENT, Invalid, Quote, QuoteError, allowed_methods, quote, quote_for,
    renewal_limit_minor, validate,
};

/// Why "Change plan" is switched off while a renewal is unsettled.
pub const SETTLE_FIRST: &str = "Settle the pending renewal first";

/// The `ops_tasks.kind` left when Razorpay charged another amount than the cloud computed.
pub const TASK_PRORATION_MISMATCH: &str = "proration_mismatch";

const DAY_MS: i64 = 86_400_000;

/// Why a change could not be previewed, confirmed or withdrawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeError {
    Invalid(Invalid),
    /// The subscription is not in a state that can change (`reason` says why, in the owner's words).
    Blocked(&'static str),
    /// The subscription moved since the owner looked.
    Stale,
    Provider(String),
    Pricing(String),
    Db(String),
}

fn db_err(e: arena_store::control::ControlError) -> ChangeError {
    ChangeError::Db(e.to_string())
}

fn priced(e: QuoteError) -> ChangeError {
    match e {
        QuoteError::NoPrice(m) => ChangeError::Pricing(m),
        QuoteError::Db(m) => ChangeError::Db(m),
    }
}

fn item_of(tier: Tier) -> Option<PriceItem> {
    match tier {
        Tier::Basic => Some(PriceItem::Basic),
        Tier::Premium => Some(PriceItem::Premium),
        Tier::Free => None,
    }
}

/// How big a plan is, part by part. Premium includes Tunnel, so it counts as having it.
fn size_of(tier: Tier, add_ons: &AddOns) -> (u8, u32, u32) {
    (
        match tier {
            Tier::Free => 0,
            Tier::Basic => 1,
            Tier::Premium => 2,
        },
        add_ons.devices_20,
        if tier == Tier::Premium {
            1
        } else {
            add_ons.tunnel
        },
    )
}

/// Which way a change goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Same,
    Up,
    Down,
    /// Some parts grow and others shrink.
    Mixed,
}

pub fn direction(current: (u8, u32, u32), next: (u8, u32, u32)) -> Direction {
    let parts = [
        (current.0, next.0),
        (
            current.1.try_into().unwrap_or(u8::MAX),
            next.1.try_into().unwrap_or(u8::MAX),
        ),
        (
            current.2.try_into().unwrap_or(u8::MAX),
            next.2.try_into().unwrap_or(u8::MAX),
        ),
    ];
    let up = parts.iter().any(|(c, n)| n > c);
    let down = parts.iter().any(|(c, n)| n < c);
    match (up, down) {
        (false, false) => Direction::Same,
        (true, false) => Direction::Up,
        (false, true) => Direction::Down,
        (true, true) => Direction::Mixed,
    }
}

/// The start of the period that ends at `period_end` (a cycle earlier, by the calendar) and its
/// length in days.
pub fn period_of(period_end: i64, cycle: BillingCycle) -> (i64, i64) {
    let months = match cycle {
        BillingCycle::Monthly => 1,
        BillingCycle::Yearly => 12,
    };
    let start = chrono::DateTime::from_timestamp_millis(period_end)
        .and_then(|e| e.checked_sub_months(chrono::Months::new(months)))
        .map_or(period_end, |s| s.timestamp_millis());
    let days = ((period_end - start + DAY_MS - 1) / DAY_MS).max(1);
    (start, days)
}

/// The whole days left in the period at `now`, never more than the period has.
pub fn days_left(now: i64, period_end: i64, period_days: i64) -> i64 {
    ((period_end - now + DAY_MS - 1) / DAY_MS).clamp(0, period_days)
}

/// `(new - old)` per day for `days_left` days of a `period_days` period, in minor units, rounded
/// half up once (not per day). Never negative.
pub fn prorate(old_minor: i64, new_minor: i64, days_left: i64, period_days: i64) -> i64 {
    if period_days <= 0 || new_minor <= old_minor {
        return 0;
    }
    let num = i128::from(new_minor - old_minor) * i128::from(days_left);
    let den = i128::from(period_days);
    i64::try_from((num * 2 + den) / (den * 2)).unwrap_or(i64::MAX)
}

fn gst_on(amount: i64, band: Band) -> i64 {
    if band.gst_applicable() {
        (amount * GST_PERCENT + 50) / 100
    } else {
        0
    }
}

/// Everything a change comes to, before anything is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub kind: ChangeKind,
    pub current: Quote,
    pub next: Quote,
    pub charge_amount: i64,
    pub charge_tax: i64,
    pub days_left: i64,
    pub effective_at: i64,
    pub needs_mandate: bool,
    pub methods: Vec<MandateMethod>,
    pub notice: Option<&'static str>,
    pub item: PriceItem,
    pub tier: Tier,
    pub add_ons: AddOns,
}

impl Plan {
    pub fn charge_total(&self) -> i64 {
        self.charge_amount + self.charge_tax
    }
}

/// Works out the change of `sub` to `req`. Only an `active` subscription that is not set to cancel
/// can change; the paid period must be known.
pub async fn plan(
    db: &ControlDb,
    sub: &Subscription,
    platform: Option<HubPlatform>,
    req: &ChangePlan,
    now: i64,
) -> Result<Plan, ChangeError> {
    match sub.state {
        SubscriptionState::Active if !sub.cancel_at_period_end => {}
        SubscriptionState::PendingReview
        | SubscriptionState::RenewalNeedsApproval
        | SubscriptionState::Grace => return Err(ChangeError::Blocked(SETTLE_FIRST)),
        _ => {
            return Err(ChangeError::Blocked(
                "This plan can't be changed right now.",
            ));
        }
    }
    let item = validate(req.tier, &req.add_ons).map_err(ChangeError::Invalid)?;
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
    if req.add_ons.tunnel > sub.addon_tunnel && platform == Some(HubPlatform::Android) {
        return Err(ChangeError::Invalid(Invalid {
            field: "add_ons.tunnel",
            message: "Tunnel needs the Windows Hub.",
        }));
    }
    let Some(period_end) = sub.current_period_end else {
        return Err(ChangeError::Blocked(
            "This plan can't be changed right now.",
        ));
    };
    let current_add_ons = AddOns {
        devices_20: sub.addon_devices_20,
        tunnel: sub.addon_tunnel,
        owner_login: sub.addon_owner_login,
    };
    let kind = match direction(
        size_of(sub.tier, &current_add_ons),
        size_of(req.tier, &req.add_ons),
    ) {
        Direction::Same => {
            return Err(ChangeError::Invalid(Invalid {
                field: "tier",
                message: "Pick a different plan or add-ons.",
            }));
        }
        Direction::Mixed => {
            return Err(ChangeError::Invalid(Invalid {
                field: "tier",
                message: "Raise or lower the plan in one change, not both.",
            }));
        }
        Direction::Up => ChangeKind::Now,
        Direction::Down => ChangeKind::PeriodEnd,
    };
    let current = quote_for(db, sub).await.map_err(priced)?;
    let next = quote(
        db,
        &sub.price_book_id,
        sub.band,
        item,
        sub.cycle,
        &req.add_ons,
    )
    .await
    .map_err(priced)?;
    let (_, period_days) = period_of(period_end, sub.cycle);
    let left = days_left(now, period_end, period_days);
    let (charge_amount, charge_tax, left, effective_at) = match kind {
        ChangeKind::Now => {
            let amount = prorate(current.amount_minor, next.amount_minor, left, period_days);
            (amount, gst_on(amount, sub.band), left, now)
        }
        ChangeKind::PeriodEnd => (0, 0, 0, period_end),
    };
    let needs_mandate = kind == ChangeKind::Now
        && sub.mandate_method == Some(MandateMethod::Upi)
        && next.total_minor > renewal_limit_minor(sub.band);
    let (methods, notice) = allowed_methods(sub.band, next.total_minor);
    Ok(Plan {
        kind,
        current,
        next,
        charge_amount,
        charge_tax,
        days_left: left,
        effective_at,
        needs_mandate,
        methods,
        notice,
        item,
        tier: req.tier,
        add_ons: req.add_ons,
    })
}

fn kind_of(kind: ChangeKind) -> PlanChangeKind {
    match kind {
        ChangeKind::Now => PlanChangeKind::Now,
        ChangeKind::PeriodEnd => PlanChangeKind::PeriodEnd,
    }
}

fn gst_percent(band: Band) -> u32 {
    if band.gst_applicable() {
        u32::try_from(GST_PERCENT).unwrap_or(0)
    } else {
        0
    }
}

/// The summary the owner confirms: the prorated charge, the new renewal amount and when it starts.
pub fn preview_of(sub: &Subscription, p: &Plan) -> PlanChangePreview {
    PlanChangePreview {
        rev: sub.rev,
        kind: kind_of(p.kind),
        currency: p.next.currency.clone(),
        tier: p.tier,
        add_ons: p.add_ons,
        gst_percent: gst_percent(sub.band),
        charge_amount_minor: p.charge_amount,
        charge_tax_minor: p.charge_tax,
        charge_total_minor: p.charge_total(),
        renewal_amount_minor: p.next.amount_minor,
        renewal_tax_minor: p.next.tax_minor,
        renewal_total_minor: p.next.total_minor,
        effective_at: p.effective_at,
        days_left: u32::try_from(p.days_left).unwrap_or(0),
        needs_mandate: p.needs_mandate,
        methods: p.methods.clone(),
        notice: p.notice.map(str::to_owned),
    }
}

/// The change waiting on a card.
pub fn pending_of(change: &PlanChange) -> PendingPlanChange {
    PendingPlanChange {
        kind: kind_of(change.kind),
        tier: change.tier,
        add_ons: AddOns {
            devices_20: change.addon_devices_20,
            tunnel: change.addon_tunnel,
            owner_login: change.addon_owner_login,
        },
        effective_at: change.effective_at,
        renewal_total_minor: change.renewal_minor,
        needs_mandate: change.status == ChangeStatus::Held,
    }
}

fn spec_of(sub: &Subscription, change_id: &str, p: &Plan, razorpay_id: &str) -> PlanChangeSpec {
    PlanChangeSpec {
        subscription_id: sub.id.clone(),
        razorpay_subscription_id: razorpay_id.to_owned(),
        change_id: change_id.to_owned(),
        book_id: sub.price_book_id.clone(),
        band: sub.band,
        item: p.item,
        cycle: sub.cycle,
        currency: sub.currency.clone(),
        renewal_minor: p.next.total_minor,
        charge_minor: p.charge_total(),
        now: p.kind == ChangeKind::Now,
    }
}

fn new_id() -> String {
    format!("pc_{}", ulid::Ulid::generate().to_string().to_lowercase())
}

/// Confirms a change: stores it against the `rev` the owner looked at (a second change from another
/// device fails `Stale` before anything reaches Razorpay), then updates the Razorpay subscription.
/// An upgrade is charged by Razorpay at once and takes effect when the charge is confirmed; a
/// downgrade is scheduled for the period end; a change that needs a new mandate creates the
/// subscription to authorise it and answers its Checkout, charging nothing and leaving the plan as
/// it was until the mandate exists. A provider failure withdraws the stored change.
pub async fn confirm(
    db: &ControlDb,
    provider: &BillingProvider,
    sub: &Subscription,
    platform: Option<HubPlatform>,
    req: &ChangePlan,
    now: i64,
) -> Result<ChangeConfirmed, ChangeError> {
    if sub.rev != req.rev {
        return Err(ChangeError::Stale);
    }
    let p = plan(db, sub, platform, req, now).await?;
    let Some(razorpay_id) = sub.razorpay_subscription_id.as_deref() else {
        return Err(ChangeError::Blocked(
            "This plan can't be changed right now.",
        ));
    };
    let key_id = match p.needs_mandate {
        true => Some(
            provider
                .key_id()
                .ok_or_else(|| ChangeError::Provider("no checkout key".to_owned()))?,
        ),
        false => None,
    };
    let change_id = new_id();
    let new = NewPlanChange {
        id: change_id.clone(),
        subscription_id: sub.id.clone(),
        kind: p.kind,
        status: match (p.kind, p.needs_mandate) {
            (_, true) => ChangeStatus::Held,
            (ChangeKind::Now, false) => ChangeStatus::Charging,
            (ChangeKind::PeriodEnd, false) => ChangeStatus::Scheduled,
        },
        tier: p.tier,
        addon_devices_20: p.add_ons.devices_20,
        addon_tunnel: p.add_ons.tunnel,
        addon_owner_login: p.add_ons.owner_login,
        charge_minor: p.charge_total(),
        renewal_minor: p.next.total_minor,
        effective_at: p.effective_at,
        razorpay_subscription_id: None,
    };
    match db
        .propose_plan_change(&new, req.rev, now)
        .await
        .map_err(db_err)?
    {
        ProposeOutcome::Proposed => {}
        ProposeOutcome::Stale => return Err(ChangeError::Stale),
    }
    let spec = spec_of(sub, &change_id, &p, razorpay_id);
    let mut checkout = None;
    let sent = if p.needs_mandate {
        let mandate = MandateSpec {
            change: spec,
            start_at_ms: sub.current_period_end.unwrap_or(now),
            methods: p.methods.clone(),
        };
        match provider.create_mandate(&mandate).await {
            Ok(SubscriptionResult::Created(rid)) => {
                match db.attach_change_razorpay(&change_id, &rid, now).await {
                    Ok(true) => {
                        checkout = Some(SubscriptionCheckout {
                            subscription_id: sub.id.clone(),
                            state: sub.state,
                            key_id: key_id.unwrap_or_default().to_owned(),
                            razorpay_subscription_id: rid,
                            band: sub.band,
                            currency: sub.currency.clone(),
                            tier: p.tier,
                            cycle: sub.cycle,
                            methods: p.methods.clone(),
                            notice: p.notice.map(str::to_owned),
                            amount_minor: p.next.amount_minor,
                            tax_minor: p.next.tax_minor,
                            total_minor: p.next.total_minor,
                        });
                        Ok(())
                    }
                    Ok(false) => Err(ChangeError::Stale),
                    Err(e) => Err(db_err(e)),
                }
            }
            Ok(SubscriptionResult::Deferred) => Err(ChangeError::Provider(
                "the billing provider is manual".to_owned(),
            )),
            Err(e) => Err(ChangeError::Provider(e.0)),
        }
    } else {
        match provider.change_plan(&spec).await {
            Ok(ChangeResult::Updated) => Ok(()),
            Ok(ChangeResult::Deferred) => Err(ChangeError::Provider(
                "the billing provider is manual".to_owned(),
            )),
            Err(e) => Err(ChangeError::Provider(e.0)),
        }
    };
    if let Err(e) = sent {
        // The change was not made at the provider: it must not stay open.
        if let Err(close) = db
            .close_plan_change(&sub.id, &change_id, ChangeStatus::Withdrawn, now)
            .await
        {
            tracing::error!(subscription_id = %sub.id, error = %close, "could not withdraw a failed plan change");
        }
        return Err(e);
    }
    tracing::info!(
        subscription_id = %sub.id,
        change_id = %change_id,
        kind = new.kind.as_str(),
        status = new.status.as_str(),
        charge_minor = new.charge_minor,
        renewal_minor = new.renewal_minor,
        "plan change confirmed"
    );
    Ok(ChangeConfirmed {
        kind: kind_of(p.kind),
        effective_at: p.effective_at,
        charge_total_minor: p.charge_total(),
        renewal_total_minor: p.next.total_minor,
        checkout,
    })
}

/// "Keep current plan": withdraws a scheduled or held change. A scheduled change is moved back to
/// the current plan at the provider first, so the period end does not switch it; a held change
/// cancels its unused mandate subscription. A change that is charging cannot be withdrawn.
pub async fn withdraw(
    db: &ControlDb,
    provider: &BillingProvider,
    sub: &Subscription,
    rev: i64,
    now: i64,
) -> Result<(), ChangeError> {
    if sub.rev != rev {
        return Err(ChangeError::Stale);
    }
    let Some(change) = db.open_plan_change(&sub.id).await.map_err(db_err)? else {
        return Err(ChangeError::Blocked("There is no change to keep out."));
    };
    match change.status {
        ChangeStatus::Charging => {
            return Err(ChangeError::Blocked("This change is already being paid."));
        }
        ChangeStatus::Held => {
            if let Some(rid) = &change.razorpay_subscription_id
                && let Err(e) = provider.cancel_subscription(rid).await
            {
                return Err(ChangeError::Provider(e.0));
            }
        }
        ChangeStatus::Scheduled => {
            let Some(razorpay_id) = sub.razorpay_subscription_id.as_deref() else {
                return Err(ChangeError::Blocked(
                    "This plan can't be changed right now.",
                ));
            };
            let current = quote_for(db, sub).await.map_err(priced)?;
            let item = item_of(sub.tier).ok_or_else(|| ChangeError::Pricing("free".to_owned()))?;
            let back = PlanChangeSpec {
                subscription_id: sub.id.clone(),
                razorpay_subscription_id: razorpay_id.to_owned(),
                change_id: format!("{}_keep", change.id),
                book_id: sub.price_book_id.clone(),
                band: sub.band,
                item,
                cycle: sub.cycle,
                currency: sub.currency.clone(),
                renewal_minor: current.total_minor,
                charge_minor: 0,
                now: false,
            };
            match provider.change_plan(&back).await {
                Ok(ChangeResult::Updated) => {}
                Ok(ChangeResult::Deferred) => {
                    return Err(ChangeError::Provider(
                        "the billing provider is manual".to_owned(),
                    ));
                }
                Err(e) => return Err(ChangeError::Provider(e.0)),
            }
        }
        ChangeStatus::Applied | ChangeStatus::Withdrawn | ChangeStatus::Superseded => {}
    }
    if db
        .close_plan_change(&sub.id, &change.id, ChangeStatus::Withdrawn, now)
        .await
        .map_err(db_err)?
    {
        Ok(())
    } else {
        Err(ChangeError::Stale)
    }
}

/// What a charge event completes for a subscription that has an open change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    /// The prorated charge of an upgrade, inside the paid period.
    Prorated,
    /// A renewal charge at the period end: a scheduled change (or an upgrade whose renewal landed
    /// first) takes effect with it.
    Renewal,
    /// The first charge of the new subscription that authorises a held change's mandate.
    Mandate,
}

/// Whether the charge `ev_kind` (on the subscription's current Razorpay subscription, or on the
/// held change's new one when `on_held`) completes `change`. `newer_period` says the charge moves
/// the paid period forward.
pub fn completion(
    change: &PlanChange,
    charged: bool,
    on_held: bool,
    newer_period: bool,
) -> Option<Completion> {
    if !charged {
        return None;
    }
    match (change.status, on_held) {
        (ChangeStatus::Held, true) => Some(Completion::Mandate),
        (ChangeStatus::Charging, false) if !newer_period => Some(Completion::Prorated),
        (ChangeStatus::Charging | ChangeStatus::Scheduled, false) if newer_period => {
            Some(Completion::Renewal)
        }
        _ => None,
    }
}

/// The subscription as it reads once `change` has taken effect.
pub fn with_change(sub: &Subscription, change: &PlanChange) -> Subscription {
    Subscription {
        tier: change.tier,
        addon_devices_20: change.addon_devices_20,
        addon_tunnel: change.addon_tunnel,
        addon_owner_login: change.addon_owner_login,
        ..sub.clone()
    }
}

/// The invoice line of a prorated charge.
pub fn proration_line(tier: Tier) -> String {
    let name = match tier {
        Tier::Premium => "Premium",
        Tier::Basic | Tier::Free => "Basic",
    };
    format!("Arena 360 plan change to {name} (prorated for the rest of the period)")
}

/// The amount (GST excluded) and GST in a charge of `total_minor` on `band`, backing GST out of a
/// total Razorpay reports.
pub fn split_total(total_minor: i64, band: Band) -> (i64, i64) {
    if band.gst_applicable() {
        let amount = (total_minor * 100 + (100 + GST_PERCENT) / 2) / (100 + GST_PERCENT);
        (amount, total_minor - amount)
    } else {
        (total_minor, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_charge_is_the_daily_rate_difference_for_the_days_left_rounded_once() {
        // 20,000 -> 70,000 over a 30-day month with 15 days left: 50,000 x 15 / 30.
        assert_eq!(prorate(20_000, 70_000, 15, 30), 25_000);
        // 5,000 x 10 / 31 = 1,612.9 -> 1,613
        assert_eq!(prorate(0, 5_000, 10, 31), 1_613);
        // Half a paisa rounds up.
        assert_eq!(prorate(0, 1, 1, 2), 1);
        // Nothing left, or nothing bigger, charges nothing.
        assert_eq!(prorate(20_000, 70_000, 0, 30), 0);
        assert_eq!(prorate(70_000, 20_000, 15, 30), 0);
    }

    #[test]
    fn a_period_has_its_calendar_length_and_days_left_round_up_to_whole_days() {
        let ms = |y, m, d, h| {
            chrono::NaiveDate::from_ymd_opt(y, m, d)
                .unwrap()
                .and_hms_opt(h, 0, 0)
                .unwrap()
                .and_utc()
                .timestamp_millis()
        };
        let end = ms(2027, 3, 1, 8);
        assert_eq!(period_of(end, BillingCycle::Monthly).1, 28);
        assert_eq!(period_of(ms(2028, 1, 1, 8), BillingCycle::Yearly).1, 365);
        assert_eq!(days_left(ms(2027, 2, 15, 8), end, 28), 14);
        assert_eq!(days_left(ms(2027, 2, 14, 20), end, 28), 15);
        assert_eq!(days_left(end + 1, end, 28), 0);
        assert_eq!(days_left(ms(2020, 1, 1, 0), end, 28), 28);
    }

    #[test]
    fn a_change_goes_one_way_and_premium_counts_as_having_tunnel() {
        let none = AddOns::default();
        let packs = |n| AddOns {
            devices_20: n,
            ..AddOns::default()
        };
        let tunnel = AddOns {
            tunnel: 1,
            ..AddOns::default()
        };
        let d = |a: (Tier, AddOns), b: (Tier, AddOns)| {
            direction(size_of(a.0, &a.1), size_of(b.0, &b.1))
        };
        assert_eq!(d((Tier::Basic, none), (Tier::Premium, none)), Direction::Up);
        assert_eq!(
            d((Tier::Premium, none), (Tier::Basic, none)),
            Direction::Down
        );
        assert_eq!(
            d((Tier::Basic, packs(1)), (Tier::Basic, packs(3))),
            Direction::Up
        );
        assert_eq!(
            d((Tier::Basic, packs(3)), (Tier::Basic, packs(1))),
            Direction::Down
        );
        assert_eq!(d((Tier::Basic, none), (Tier::Basic, none)), Direction::Same);
        // Basic with Tunnel to Premium: Tunnel is included, so only the tier grows.
        assert_eq!(
            d((Tier::Basic, tunnel), (Tier::Premium, none)),
            Direction::Up
        );
        // Premium to Basic with Tunnel keeps Tunnel and lowers the tier.
        assert_eq!(
            d((Tier::Premium, none), (Tier::Basic, tunnel)),
            Direction::Down
        );
        assert_eq!(
            d((Tier::Basic, packs(3)), (Tier::Premium, packs(1))),
            Direction::Mixed
        );
    }

    #[test]
    fn gst_is_backed_out_of_a_reported_total_on_band_in_only() {
        assert_eq!(split_total(11_800, Band::In), (10_000, 1_800));
        assert_eq!(split_total(11_801, Band::In), (10_001, 1_800));
        assert_eq!(split_total(500, Band::Us), (500, 0));
    }

    fn change(status: ChangeStatus) -> PlanChange {
        PlanChange {
            id: "pc".to_owned(),
            subscription_id: "sb".to_owned(),
            kind: ChangeKind::Now,
            status,
            tier: Tier::Premium,
            addon_devices_20: 0,
            addon_tunnel: 0,
            addon_owner_login: 0,
            charge_minor: 0,
            renewal_minor: 0,
            effective_at: 0,
            razorpay_subscription_id: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn only_the_right_charge_completes_each_kind_of_change() {
        use Completion::{Mandate, Prorated, Renewal};
        let c = |s, charged, held, newer| completion(&change(s), charged, held, newer);
        assert_eq!(
            c(ChangeStatus::Charging, true, false, false),
            Some(Prorated)
        );
        assert_eq!(c(ChangeStatus::Charging, true, false, true), Some(Renewal));
        assert_eq!(c(ChangeStatus::Scheduled, true, false, true), Some(Renewal));
        assert_eq!(c(ChangeStatus::Scheduled, true, false, false), None);
        assert_eq!(c(ChangeStatus::Held, true, true, false), Some(Mandate));
        assert_eq!(c(ChangeStatus::Held, true, false, true), None);
        assert_eq!(c(ChangeStatus::Charging, false, false, false), None);
    }
}
