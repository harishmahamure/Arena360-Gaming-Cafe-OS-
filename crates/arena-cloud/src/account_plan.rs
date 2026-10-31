//! What Account -> Subscription shows (Story 10.8, FR178, FR181): each branch's plan, its state in
//! the owner's words, the add-ons, the renewal and the payment method, and the invoice list. Every
//! amount comes from the price book locked onto the branch's own subscription, never the live
//! book, so a later publish changes nothing the owner reads here.

use arena_proto::{
    AddOns, BillingCycle, BranchPlan, HubPlatform, InvoiceKind, InvoiceRow, PaymentMethodView,
    PlanPrice, PlanState, PriceItem, SubscriptionState, Tier,
};
use arena_store::control::{Branch, ControlDb, Invoice, Subscription};

use crate::subscription::{GST_PERCENT, QuoteError, quote};

/// Why a plan could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    Db(String),
}

/// The subscription a branch's card is about: the newest one that still says something to the
/// owner. An abandoned, cancelled, ended or refunded one leaves the branch on Free; a lapsed one
/// is shown as Lapsed.
pub fn current(subs: &[Subscription]) -> Option<&Subscription> {
    subs.iter().find(|s| {
        !matches!(
            s.state,
            SubscriptionState::Abandoned
                | SubscriptionState::Cancelled
                | SubscriptionState::Ended
                | SubscriptionState::Refunded
        )
    })
}

/// How a subscription reads to the owner.
pub fn plan_state(sub: &Subscription) -> PlanState {
    match sub.state {
        SubscriptionState::Pending => PlanState::Pending,
        SubscriptionState::PendingReview => PlanState::PendingReview,
        SubscriptionState::Active if sub.cancel_at_period_end => PlanState::Cancels,
        SubscriptionState::Active => PlanState::Active,
        SubscriptionState::RenewalNeedsApproval => PlanState::NeedsApproval,
        SubscriptionState::Grace => PlanState::Grace,
        SubscriptionState::Lapsed => PlanState::Lapsed,
        SubscriptionState::Cancelled
        | SubscriptionState::Ended
        | SubscriptionState::Refunded
        | SubscriptionState::Abandoned => PlanState::Free,
    }
}

fn price_item(tier: Tier) -> Option<PriceItem> {
    match tier {
        Tier::Basic => Some(PriceItem::Basic),
        Tier::Premium => Some(PriceItem::Premium),
        Tier::Free => None,
    }
}

/// The card of a branch with no paid plan.
fn free(branch: &Branch, tier: Tier, platform: Option<HubPlatform>) -> BranchPlan {
    BranchPlan {
        branch_id: branch.id.clone(),
        branch_name: branch.name.clone(),
        state: PlanState::Free,
        tier,
        cycle: None,
        band: None,
        currency: None,
        add_ons: AddOns::default(),
        current_period_end: None,
        gst_percent: 0,
        prices: Vec::new(),
        payment_method: None,
        message: None,
        rev: 0,
        hub_platform: platform,
        pending_change: None,
        grace_until: None,
    }
}

/// The card of `branch`: `tier` is what it is entitled to now and `subs` its subscriptions,
/// newest first.
pub async fn plan_of(
    db: &ControlDb,
    branch: &Branch,
    tier: Tier,
    platform: Option<HubPlatform>,
    subs: &[Subscription],
) -> Result<BranchPlan, PlanError> {
    let Some(sub) = current(subs) else {
        return Ok(free(branch, tier, platform));
    };
    let pending_change = db
        .open_plan_change(&sub.id)
        .await
        .map_err(|e| PlanError::Db(e.to_string()))?
        .map(|c| crate::plan_change::pending_of(&c));
    let add_ons = AddOns {
        devices_20: sub.addon_devices_20,
        tunnel: sub.addon_tunnel,
        owner_login: sub.addon_owner_login,
    };
    let mut prices = Vec::new();
    if let Some(item) = price_item(sub.tier) {
        for cycle in [BillingCycle::Monthly, BillingCycle::Yearly] {
            match quote(db, &sub.price_book_id, sub.band, item, cycle, &add_ons).await {
                Ok(q) => prices.push(PlanPrice {
                    cycle,
                    amount_minor: q.amount_minor,
                    tax_minor: q.tax_minor,
                    total_minor: q.total_minor,
                }),
                // The locked book has no price for this cycle: the card shows the others.
                Err(QuoteError::NoPrice(_)) => {}
                Err(QuoteError::Db(e)) => return Err(PlanError::Db(e)),
            }
        }
    }
    let state = plan_state(sub);
    Ok(BranchPlan {
        branch_id: branch.id.clone(),
        branch_name: branch.name.clone(),
        state,
        tier: sub.tier,
        cycle: Some(sub.cycle),
        band: Some(sub.band),
        currency: Some(sub.currency.clone()),
        add_ons,
        current_period_end: sub.current_period_end,
        gst_percent: if sub.band.gst_applicable() {
            u32::try_from(GST_PERCENT).unwrap_or(0)
        } else {
            0
        },
        prices,
        payment_method: sub.mandate_method.map(|method| PaymentMethodView {
            method,
            detail: sub.mandate_detail.clone(),
        }),
        message: (state == PlanState::PendingReview)
            .then(|| crate::webhook::REVIEW_MESSAGE.to_owned()),
        rev: sub.rev,
        hub_platform: platform,
        pending_change,
        grace_until: sub.grace_until.filter(|_| state == PlanState::Grace),
    })
}

/// One invoice list line. A credit note reads as a negative amount.
pub fn invoice_row(inv: &Invoice, branch_name: &str) -> InvoiceRow {
    let kind = InvoiceKind::parse(&inv.kind).unwrap_or(InvoiceKind::Invoice);
    InvoiceRow {
        id: inv.id.clone(),
        number: inv.number.clone(),
        kind,
        fy: inv.fy.clone(),
        branch_id: inv.branch_id.clone(),
        branch_name: branch_name.to_owned(),
        issued_at: inv.issued_at,
        currency: inv.currency.clone(),
        total_minor: match kind {
            InvoiceKind::Invoice => inv.total_minor,
            InvoiceKind::CreditNote => -inv.total_minor,
        },
        original_number: inv.original_number.clone(),
        pdf_ready: inv.pdf_key.is_some(),
    }
}

/// Whether `fy` looks like a financial year, `2026-27`: anything else is refused rather than
/// silently matching nothing.
pub fn valid_fy(fy: &str) -> bool {
    let b = fy.as_bytes();
    b.len() == 7
        && b[4] == b'-'
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[5..].iter().all(u8::is_ascii_digit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_proto::Band;

    fn sub(state: SubscriptionState, cancel: bool) -> Subscription {
        Subscription {
            id: "s1".to_owned(),
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
            razorpay_subscription_id: None,
            rev: 1,
            current_period_end: None,
            grace_until: None,
            first_paid_at: None,
            cancel_at_period_end: cancel,
            created_at: 0,
            updated_at: 0,
            payment_country: None,
        }
    }

    #[test]
    fn every_subscription_state_reads_as_one_plan_state() {
        use PlanState as P;
        use SubscriptionState as S;
        let read = |s, cancel| plan_state(&sub(s, cancel));
        assert_eq!(read(S::Pending, false), P::Pending);
        assert_eq!(read(S::PendingReview, false), P::PendingReview);
        assert_eq!(read(S::Active, false), P::Active);
        assert_eq!(read(S::Active, true), P::Cancels);
        assert_eq!(read(S::RenewalNeedsApproval, false), P::NeedsApproval);
        assert_eq!(read(S::Grace, false), P::Grace);
        assert_eq!(read(S::Lapsed, false), P::Lapsed);
        for over in [S::Cancelled, S::Ended, S::Refunded, S::Abandoned] {
            assert_eq!(read(over, false), P::Free);
        }
    }

    #[test]
    fn the_card_skips_what_is_over_and_keeps_a_lapsed_plan() {
        let subs = [
            sub(SubscriptionState::Abandoned, false),
            sub(SubscriptionState::Lapsed, false),
            sub(SubscriptionState::Cancelled, false),
        ];
        assert_eq!(
            current(&subs).map(|s| s.state),
            Some(SubscriptionState::Lapsed)
        );
        assert!(current(&[sub(SubscriptionState::Cancelled, false)]).is_none());
        assert!(current(&[]).is_none());
    }

    #[test]
    fn a_financial_year_filter_is_four_digits_a_dash_and_two() {
        assert!(valid_fy("2026-27"));
        for bad in [
            "",
            "2026",
            "2026-2027",
            "26-27",
            "2026/27",
            "abcd-ef",
            "2026-27 ",
        ] {
            assert!(!valid_fy(bad), "{bad:?}");
        }
    }
}
