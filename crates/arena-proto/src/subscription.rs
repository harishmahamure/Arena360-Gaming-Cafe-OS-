//! A branch subscription's vocabulary (Story 10.2): the add-ons an owner picks, the request that
//! starts a subscription, the payment methods a renewal may use and what the owner's browser needs
//! to open Razorpay Checkout.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{Band, BillingCycle, Tier};

/// Where a subscription is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum SubscriptionState {
    Pending,
    PendingReview,
    Active,
    RenewalNeedsApproval,
    Grace,
    Lapsed,
    Cancelled,
    Ended,
    Refunded,
    Abandoned,
}

impl SubscriptionState {
    pub const ALL: [SubscriptionState; 10] = [
        Self::Pending,
        Self::PendingReview,
        Self::Active,
        Self::RenewalNeedsApproval,
        Self::Grace,
        Self::Lapsed,
        Self::Cancelled,
        Self::Ended,
        Self::Refunded,
        Self::Abandoned,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::PendingReview => "pending_review",
            Self::Active => "active",
            Self::RenewalNeedsApproval => "renewal_needs_approval",
            Self::Grace => "grace",
            Self::Lapsed => "lapsed",
            Self::Cancelled => "cancelled",
            Self::Ended => "ended",
            Self::Refunded => "refunded",
            Self::Abandoned => "abandoned",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == s)
    }

    /// A state in which the branch may subscribe again: it holds no open subscription.
    pub fn is_over(self) -> bool {
        matches!(self, Self::Abandoned | Self::Cancelled | Self::Lapsed)
    }
}

/// How a renewal is paid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum MandateMethod {
    /// UPI Autopay.
    Upi,
    Card,
    /// A bank (eNACH) mandate.
    Emandate,
}

impl MandateMethod {
    pub const ALL: [MandateMethod; 3] = [Self::Upi, Self::Card, Self::Emandate];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Upi => "upi",
            Self::Card => "card",
            Self::Emandate => "emandate",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.as_str() == s)
    }
}

/// The add-ons of a subscription, as quantities. `owner_login` is price-book data only and must
/// be 0 until the Owner-login sale ships; `tunnel` is 0 or 1 and Basic only (Premium includes it).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AddOns {
    /// Packs of 20 extra devices, 0 to 10.
    #[serde(default)]
    pub devices_20: u32,
    #[serde(default)]
    pub tunnel: u32,
    #[serde(default)]
    pub owner_login: u32,
}

/// `POST /v1/branches/:id/subscription`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CreateSubscription {
    pub tier: Tier,
    pub cycle: BillingCycle,
    #[serde(default)]
    pub add_ons: AddOns,
    /// The billing country (ISO 3166-1 alpha-2); it alone picks the PPP band.
    pub billing_country: String,
}

/// What the owner's browser needs to open Razorpay Checkout for the new subscription. Amounts are
/// minor units of `currency`; `total_minor` is what each renewal debits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SubscriptionCheckout {
    pub subscription_id: String,
    pub state: SubscriptionState,
    pub key_id: String,
    pub razorpay_subscription_id: String,
    pub band: Band,
    pub currency: String,
    pub tier: Tier,
    pub cycle: BillingCycle,
    pub methods: Vec<MandateMethod>,
    /// Why some methods are missing, when they are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub notice: Option<String>,
    #[ts(type = "number")]
    pub amount_minor: i64,
    #[ts(type = "number")]
    pub tax_minor: i64,
    #[ts(type = "number")]
    pub total_minor: i64,
}

/// `POST /v1/branches/:id/subscription/quote` (Story 10.4): what the owner has picked. The band is
/// the branch country's, never the browser's, and the amounts are the cloud's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct QuoteSubscription {
    pub tier: Tier,
    pub cycle: BillingCycle,
    #[serde(default)]
    pub add_ons: AddOns,
}

/// What a subscription will cost, made by the cloud from the branch's live price book (or, when an
/// unpaid subscription for the same choice is waiting, from that subscription's own book), for the
/// checkout summary. Amounts are minor units of `currency`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SubscriptionQuote {
    pub band: Band,
    pub currency: String,
    pub tier: Tier,
    pub cycle: BillingCycle,
    pub add_ons: AddOns,
    /// Tier plus add-ons, GST excluded.
    #[ts(type = "number")]
    pub amount_minor: i64,
    /// GST, on band IN only; 0 elsewhere.
    #[ts(type = "number")]
    pub tax_minor: i64,
    #[ts(type = "number")]
    pub total_minor: i64,
    /// The GST rate in percent; 0 when the band has none.
    pub gst_percent: u32,
    /// When the first renewal debits, in epoch milliseconds: one cycle after the quote.
    #[ts(type = "number")]
    pub renews_at: i64,
    pub methods: Vec<MandateMethod>,
    /// Why some methods are missing, when they are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub notice: Option<String>,
}

/// What `GET /v1/branches/:id/subscription` tells the owner about the branch's subscription: its
/// state, the end of the paid period and, while the state needs explaining, a sentence for them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SubscriptionStatus {
    pub subscription_id: String,
    pub state: SubscriptionState,
    pub tier: Tier,
    pub cycle: BillingCycle,
    pub band: Band,
    /// The end of the paid period, in epoch milliseconds, once Razorpay has reported one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub current_period_end: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub message: Option<String>,
}

/// How a branch's plan reads on Account -> Subscription (Story 10.8). A branch that never
/// subscribed, or whose subscription ended, is `Free`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum PlanState {
    Free,
    /// Checkout was opened and no payment has arrived.
    Pending,
    /// The first payment is held for an operator to look at.
    PendingReview,
    Active,
    /// A renewal waits for the owner's bank to approve it.
    NeedsApproval,
    Grace,
    /// Active until the end of the paid period, then Free.
    Cancels,
    Lapsed,
}

/// What one billing cycle of the plan costs, from the branch's own locked price book. Amounts
/// are minor units of the plan's currency; `tax_minor` is GST on band IN only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlanPrice {
    pub cycle: BillingCycle,
    #[ts(type = "number")]
    pub amount_minor: i64,
    #[ts(type = "number")]
    pub tax_minor: i64,
    #[ts(type = "number")]
    pub total_minor: i64,
}

/// How a subscription is paid, as the owner recognises it: the method and the last four digits
/// of the card or the bank of the mandate (UPI has no detail).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PaymentMethodView {
    pub method: MandateMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub detail: Option<String>,
}

/// One branch's card on Account -> Subscription.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BranchPlan {
    pub branch_id: String,
    pub branch_name: String,
    pub state: PlanState,
    pub tier: Tier,
    /// The subscription's billing cycle; absent on a Free branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cycle: Option<BillingCycle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub band: Option<Band>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub currency: Option<String>,
    pub add_ons: AddOns,
    /// The end of the paid period: the next renewal, or the day a cancelled plan ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub current_period_end: Option<i64>,
    /// The GST rate in percent; 0 when the band has none.
    pub gst_percent: u32,
    /// Both cycles of the plan, so the Monthly | Yearly switch needs no second call; empty on a
    /// Free branch.
    pub prices: Vec<PlanPrice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub payment_method: Option<PaymentMethodView>,
    /// A sentence for states that need explaining.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub message: Option<String>,
}

/// `GET /v1/account/subscription`: every branch of the owner's café with its plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AccountSubscription {
    pub branches: Vec<BranchPlan>,
}

/// Whether a billing document charges or reverses a charge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum InvoiceKind {
    Invoice,
    CreditNote,
}

impl InvoiceKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "invoice" => Some(Self::Invoice),
            "credit_note" => Some(Self::CreditNote),
            _ => None,
        }
    }
}

/// One line of the invoice list. A credit note's `total_minor` is negative.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct InvoiceRow {
    pub id: String,
    pub number: String,
    pub kind: InvoiceKind,
    /// The financial year, `2026-27`.
    pub fy: String,
    pub branch_id: String,
    pub branch_name: String,
    #[ts(type = "number")]
    pub issued_at: i64,
    pub currency: String,
    #[ts(type = "number")]
    pub total_minor: i64,
    /// The invoice a credit note reverses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub original_number: Option<String>,
    /// Whether the PDF has been rendered and can be downloaded.
    pub pdf_ready: bool,
}

/// `GET /v1/account/invoices`: invoices and credit notes across the café's branches, newest
/// first, with the financial years the café has any in (for the filter).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct InvoiceList {
    pub invoices: Vec<InvoiceRow>,
    pub years: Vec<String>,
}

/// `GET /v1/account/invoices/:id/pdf`: a link that works for `expires_in_s` seconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct InvoiceDownload {
    pub url: String,
    pub expires_in_s: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_only_three_states_are_over() {
        for s in SubscriptionState::ALL {
            assert_eq!(SubscriptionState::parse(s.as_str()), Some(s));
            assert_eq!(
                serde_json::to_string(&s).unwrap(),
                format!("\"{}\"", s.as_str())
            );
        }
        for m in MandateMethod::ALL {
            assert_eq!(MandateMethod::parse(m.as_str()), Some(m));
            assert_eq!(
                serde_json::to_string(&m).unwrap(),
                format!("\"{}\"", m.as_str())
            );
        }
        let over: Vec<_> = SubscriptionState::ALL
            .into_iter()
            .filter(|s| s.is_over())
            .collect();
        assert_eq!(
            over,
            [
                SubscriptionState::Lapsed,
                SubscriptionState::Cancelled,
                SubscriptionState::Abandoned
            ]
        );
    }

    #[test]
    fn a_request_defaults_its_add_ons_and_refuses_negative_quantities() {
        let r: CreateSubscription =
            serde_json::from_str(r#"{"tier":"basic","cycle":"yearly","billing_country":"IN"}"#)
                .unwrap();
        assert_eq!(r.add_ons, AddOns::default());
        let r: CreateSubscription = serde_json::from_str(
            r#"{"tier":"premium","cycle":"monthly","billing_country":"IN","add_ons":{"devices_20":2}}"#,
        )
        .unwrap();
        assert_eq!(r.add_ons.devices_20, 2);
        assert!(
            serde_json::from_str::<CreateSubscription>(
                r#"{"tier":"basic","cycle":"yearly","billing_country":"IN","add_ons":{"devices_20":-1}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn a_quote_request_defaults_its_add_ons_and_carries_no_country() {
        let q: QuoteSubscription =
            serde_json::from_str(r#"{"tier":"premium","cycle":"yearly"}"#).unwrap();
        assert_eq!(q.add_ons, AddOns::default());
        let wire = serde_json::to_value(SubscriptionQuote {
            band: Band::In,
            currency: "INR".to_owned(),
            tier: Tier::Basic,
            cycle: BillingCycle::Monthly,
            add_ons: AddOns::default(),
            amount_minor: 20_000,
            tax_minor: 3_600,
            total_minor: 23_600,
            gst_percent: 18,
            renews_at: 1,
            methods: vec![MandateMethod::Upi],
            notice: None,
        })
        .unwrap();
        assert_eq!(wire["band"], "IN");
        assert_eq!(wire["methods"], serde_json::json!(["upi"]));
        assert!(wire.get("notice").is_none());
    }
}
