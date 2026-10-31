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
}
