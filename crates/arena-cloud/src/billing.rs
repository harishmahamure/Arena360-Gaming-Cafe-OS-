//! The billing provider (Story 10.1, AD-34): the one place the cloud talks to Razorpay. A
//! [`BillingProvider`] is the recorded [`FakeRazorpay`] (dev and tests), the real
//! [`RazorpayClient`], or `Manual`, where every call is a no-op that leaves an operator task so
//! a Razorpay outage never blocks the rest of the cloud. It creates plans (Story 10.1) and
//! creates and cancels subscriptions (Story 10.2); the later billing stories add payments,
//! refunds and webhooks to the same adapter.
//! Errors carry a stable code, never a key, a secret or a response body.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::sync::Mutex;
use std::time::Duration;

use arena_proto::{Band, BillingCycle, MandateMethod, PriceItem};
use serde_json::{Value, json};

use crate::config::get_env;

/// `razorpay` or `manual`. Unset: `razorpay` when the keys are set, else `manual`.
pub const PROVIDER_ENV: &str = "ARENA_BILLING_PROVIDER";
/// The Razorpay API key id.
pub const KEY_ID_ENV: &str = "ARENA_RAZORPAY_KEY_ID";
/// The Razorpay API key secret (a secret: never logged).
pub const KEY_SECRET_ENV: &str = "ARENA_RAZORPAY_KEY_SECRET";
/// Optional: the API base, for a mock server (default the production API).
pub const API_BASE_ENV: &str = "ARENA_RAZORPAY_API_BASE";

const DEFAULT_API_BASE: &str = "https://api.razorpay.com/v1";
const CALL_TIMEOUT: Duration = Duration::from_secs(20);

/// One Razorpay plan to create from a price book row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanSpec {
    pub book_id: String,
    pub band: Band,
    pub item: PriceItem,
    pub cycle: BillingCycle,
    pub currency: String,
    pub amount_minor: i64,
}

impl PlanSpec {
    /// The name a plan carries in the Razorpay dashboard.
    pub fn name(&self) -> String {
        format!(
            "Arena 360 {} {} ({})",
            self.item.as_str(),
            self.cycle.as_str(),
            self.band.as_str()
        )
    }

    fn key(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.book_id,
            self.band.as_str(),
            self.item.as_str(),
            self.cycle.as_str()
        )
    }
}

/// What a plan call did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanResult {
    Created(String),
    /// The provider is `manual`: nothing was called, an operator creates the plan.
    Deferred,
}

/// One Razorpay subscription to create for a branch. `total_minor` is what each renewal debits,
/// GST included on band IN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionSpec {
    /// Our subscription id; the fake answers a repeat of it with the same Razorpay id.
    pub subscription_id: String,
    pub branch_id: String,
    pub book_id: String,
    pub band: Band,
    /// `basic` or `premium`.
    pub item: PriceItem,
    pub cycle: BillingCycle,
    pub currency: String,
    pub total_minor: i64,
    /// The payment methods the renewal may use.
    pub methods: Vec<MandateMethod>,
}

impl SubscriptionSpec {
    /// The name the per-subscription plan carries in the Razorpay dashboard.
    pub fn plan_name(&self) -> String {
        format!(
            "Arena 360 {} {} ({}) {}",
            self.item.as_str(),
            self.cycle.as_str(),
            self.band.as_str(),
            self.subscription_id
        )
    }
}

/// What a subscription call did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubscriptionResult {
    /// The Razorpay subscription id.
    Created(String),
    /// The provider is `manual`: nothing was called, so there is nothing to pay through.
    Deferred,
}

/// What a cancel call did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelResult {
    Cancelled,
    /// The provider is `manual`: nothing was called, an operator cancels it.
    Deferred,
}

/// What a refund call did (Story 10.15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefundResult {
    /// Razorpay's refund id.
    Issued(String),
    /// The provider is `manual`: nothing was called, an operator refunds it in Razorpay's dashboard.
    Deferred,
}

/// A change of tier or add-ons on a live Razorpay subscription (Story 10.9): the subscription is
/// moved to a plan priced at the new renewal total, at once (an upgrade, which Razorpay charges
/// the prorated difference for) or at the end of the cycle (a downgrade).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanChangeSpec {
    pub subscription_id: String,
    pub razorpay_subscription_id: String,
    pub change_id: String,
    pub book_id: String,
    pub band: Band,
    /// `basic` or `premium`.
    pub item: PriceItem,
    pub cycle: BillingCycle,
    pub currency: String,
    /// What each renewal debits after the change, GST included.
    pub renewal_minor: i64,
    /// The prorated charge the cloud computed, GST included; 0 at the cycle end.
    pub charge_minor: i64,
    /// `true`: switch now. `false`: at the end of the cycle.
    pub now: bool,
}

impl PlanChangeSpec {
    fn plan_name(&self) -> String {
        format!(
            "Arena 360 {} {} ({}) {} {}",
            self.item.as_str(),
            self.cycle.as_str(),
            self.band.as_str(),
            self.subscription_id,
            self.change_id
        )
    }
}

/// A new Razorpay subscription that only authorises a card or eNACH mandate for a change whose
/// renewal debit is too big for the current UPI mandate (Story 10.9). It starts at the end of the
/// current period, so the renewal moves to it, and carries the prorated charge as an upfront add-on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MandateSpec {
    pub change: PlanChangeSpec,
    /// When the new subscription's first renewal debits (epoch ms): the end of the paid period.
    pub start_at_ms: i64,
    pub methods: Vec<MandateMethod>,
}

/// What a plan change call did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeResult {
    Updated,
    /// The provider is `manual`: nothing was called, so nothing was charged.
    Deferred,
}

/// The key id the fake hands to Checkout.
pub const FAKE_KEY_ID: &str = "rzp_test_fake";

/// Where a payment came from, as far as Razorpay says.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PaymentOrigin {
    /// The card issuer's or the bank's country (ISO 3166-1 alpha-2).
    Country(String),
    /// A card issued outside India; Razorpay does not say where.
    International,
    #[default]
    Unknown,
}

impl PaymentOrigin {
    /// How the origin is stored on the subscription.
    pub fn as_text(&self) -> Option<String> {
        match self {
            Self::Country(c) => Some(c.clone()),
            Self::International => Some("INTL".to_owned()),
            Self::Unknown => None,
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "INTL" => Self::International,
            c if c.len() == 2 && c.bytes().all(|b| b.is_ascii_uppercase()) => {
                Self::Country(c.to_owned())
            }
            _ => Self::Unknown,
        }
    }

    /// Whether a payment from here is what a subscription at `band` expects. What Razorpay does
    /// not say counts as a match: only a payment known to come from elsewhere is held for review.
    pub fn fits(&self, band: Band) -> bool {
        match self {
            Self::Unknown => true,
            Self::Country(c) => c == band.as_str(),
            Self::International => band != Band::In,
        }
    }
}

/// How a payment was made.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PaymentInfo {
    /// Razorpay's id of the payment (`pay_...`), once the event or the fetch carries one.
    pub id: Option<String>,
    pub method: Option<MandateMethod>,
    /// What names the method to its owner: the last four digits of a card or the bank of a
    /// mandate. A UPI handle is personal data and is never read.
    pub detail: Option<String>,
    pub origin: PaymentOrigin,
}

/// A subscription as the provider holds it now (what the reconcile job reads).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSubscription {
    /// Razorpay's status: `created`, `authenticated`, `active`, `pending`, `halted`, ...
    pub status: String,
    /// The end of the current period in epoch milliseconds.
    pub current_end_ms: Option<i64>,
    /// How many charges have succeeded.
    pub paid_count: u32,
    /// How the latest charge was paid, once there is one.
    pub payment: Option<PaymentInfo>,
}

/// A Razorpay id is letters, digits and underscores: the only shape put into a request path.
fn plain_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// How a Razorpay payment entity was paid. UPI, bank mandates and net banking exist only in
/// India; a card carries its issuer's country when Razorpay sends one, else whether it is
/// international.
pub fn payment_info_from(payment: &Value) -> PaymentInfo {
    let method = payment.get("method").and_then(Value::as_str);
    let card = payment.get("card").filter(|c| c.is_object());
    let origin = match method {
        Some("upi" | "emandate" | "netbanking") => PaymentOrigin::Country("IN".to_owned()),
        _ => match card {
            Some(card) => {
                let country = card
                    .get("country")
                    .and_then(Value::as_str)
                    .map(str::to_ascii_uppercase)
                    .filter(|c| c.len() == 2 && c.bytes().all(|b| b.is_ascii_uppercase()));
                match (country, card.get("international").and_then(Value::as_bool)) {
                    (Some(c), _) => PaymentOrigin::Country(c),
                    (None, Some(false)) => PaymentOrigin::Country("IN".to_owned()),
                    (None, Some(true)) => PaymentOrigin::International,
                    (None, None) => PaymentOrigin::Unknown,
                }
            }
            None => PaymentOrigin::Unknown,
        },
    };
    let detail = match method {
        Some("card") => card
            .and_then(|c| c.get("last4"))
            .and_then(Value::as_str)
            .filter(|l| l.len() == 4 && l.bytes().all(|b| b.is_ascii_digit()))
            .map(str::to_owned),
        Some("emandate" | "netbanking") => payment
            .get("bank")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|b| !b.is_empty() && b.len() <= 40)
            .map(str::to_owned),
        _ => None,
    };
    PaymentInfo {
        id: payment
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| plain_id(id))
            .map(str::to_owned),
        method: match method {
            Some("upi") => Some(MandateMethod::Upi),
            Some("card") => Some(MandateMethod::Card),
            Some("emandate") => Some(MandateMethod::Emandate),
            _ => None,
        },
        origin,
        detail,
    }
}

/// Why a provider call failed: a stable code and no secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BillingError(pub String);

impl std::fmt::Display for BillingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BillingError {}

#[derive(Default)]
struct FakeState {
    /// Plan key -> the plan id handed out for it.
    plans: BTreeMap<String, String>,
    created: Vec<PlanSpec>,
    fail_after: Option<u32>,
    /// Our subscription id -> the Razorpay subscription id handed out for it.
    subscriptions: BTreeMap<String, String>,
    subscription_specs: Vec<SubscriptionSpec>,
    cancelled: Vec<String>,
    /// Subscriptions told to end with their paid cycle rather than at once (Story 10.10).
    cancelled_at_cycle_end: Vec<String>,
    /// Subscriptions whose cancel at cycle end was taken back (Story 10.14).
    kept: Vec<String>,
    /// Payments refunded in full, as `(payment id, amount in minor units)` (Story 10.15).
    refunds: Vec<(String, i64)>,
    fail_refunds: bool,
    fail_subscriptions: bool,
    /// Razorpay subscription id -> what a test says Razorpay holds for it.
    remote: BTreeMap<String, RemoteSubscription>,
    plan_changes: Vec<PlanChangeSpec>,
    mandates: Vec<MandateSpec>,
    fail_changes: bool,
}

/// The recorded fake: keeps the plans it "created" in memory, answers a repeat of the same row
/// with the same id and fails every call after a set number of creations when a test asks.
/// Nothing leaves the process.
#[derive(Default)]
pub struct FakeRazorpay {
    state: Mutex<FakeState>,
}

impl FakeRazorpay {
    pub fn new() -> Self {
        Self::default()
    }

    fn with<R>(&self, f: impl FnOnce(&mut FakeState) -> R) -> R {
        f(&mut self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner))
    }

    /// Lets `n` more plans be created, then fails every call with `FAKE_PLAN_FAILED`.
    pub fn fail_after(&self, n: u32) {
        self.with(|s| s.fail_after = Some(n));
    }

    /// Lets every call succeed again.
    pub fn heal(&self) {
        self.with(|s| s.fail_after = None);
    }

    /// The plans created so far, in order (a repeat of a row creates nothing).
    pub fn created(&self) -> Vec<PlanSpec> {
        self.with(|s| s.created.clone())
    }

    /// Makes every subscription call (create and cancel) fail with `FAKE_SUBSCRIPTION_FAILED`, or
    /// succeed again.
    pub fn fail_subscriptions(&self, fail: bool) {
        self.with(|s| s.fail_subscriptions = fail);
    }

    /// The subscriptions created so far, in order (a repeat of one creates nothing).
    pub fn subscriptions(&self) -> Vec<SubscriptionSpec> {
        self.with(|s| s.subscription_specs.clone())
    }

    /// The Razorpay subscription ids cancelled so far, in order.
    pub fn cancelled(&self) -> Vec<String> {
        self.with(|s| s.cancelled.clone())
    }

    /// The Razorpay subscription ids told to end with their paid cycle so far, in order.
    pub fn cancelled_at_cycle_end(&self) -> Vec<String> {
        self.with(|s| s.cancelled_at_cycle_end.clone())
    }

    /// The subscriptions whose scheduled cancellation was withdrawn, in order.
    pub fn kept(&self) -> Vec<String> {
        self.with(|s| s.kept.clone())
    }

    /// The payments refunded so far, as `(payment id, amount in minor units)`, in order.
    pub fn refunded(&self) -> Vec<(String, i64)> {
        self.with(|s| s.refunds.clone())
    }

    /// Makes every refund call fail with `FAKE_REFUND_FAILED`, or succeed again.
    pub fn fail_refunds(&self, fail: bool) {
        self.with(|s| s.fail_refunds = fail);
    }

    fn refund_payment(&self, payment_id: &str, amount_minor: i64) -> Result<String, BillingError> {
        self.with(|s| {
            if s.fail_refunds {
                return Err(BillingError("FAKE_REFUND_FAILED".to_owned()));
            }
            s.refunds.push((payment_id.to_owned(), amount_minor));
            Ok(format!("rfnd_fake_{}", s.refunds.len()))
        })
    }

    /// Makes every plan change call fail with `FAKE_CHANGE_FAILED`, or succeed again.
    pub fn fail_changes(&self, fail: bool) {
        self.with(|s| s.fail_changes = fail);
    }

    /// The plan changes sent to the provider so far, in order.
    pub fn plan_changes(&self) -> Vec<PlanChangeSpec> {
        self.with(|s| s.plan_changes.clone())
    }

    /// The mandate subscriptions created for held changes so far, in order.
    pub fn mandates(&self) -> Vec<MandateSpec> {
        self.with(|s| s.mandates.clone())
    }

    fn change_plan(&self, spec: &PlanChangeSpec) -> Result<(), BillingError> {
        self.with(|s| {
            if s.fail_changes {
                return Err(BillingError("FAKE_CHANGE_FAILED".to_owned()));
            }
            s.plan_changes.push(spec.clone());
            Ok(())
        })
    }

    fn create_mandate(&self, spec: &MandateSpec) -> Result<String, BillingError> {
        self.with(|s| {
            if s.fail_changes {
                return Err(BillingError("FAKE_CHANGE_FAILED".to_owned()));
            }
            let id = format!("sub_fake_m{:03}", s.mandates.len() + 1);
            s.mandates.push(spec.clone());
            Ok(id)
        })
    }

    fn create_subscription(&self, spec: &SubscriptionSpec) -> Result<String, BillingError> {
        self.with(|s| {
            if s.fail_subscriptions {
                return Err(BillingError("FAKE_SUBSCRIPTION_FAILED".to_owned()));
            }
            if let Some(id) = s.subscriptions.get(&spec.subscription_id) {
                return Ok(id.clone());
            }
            let id = format!("sub_fake_{:03}", s.subscription_specs.len() + 1);
            s.subscriptions
                .insert(spec.subscription_id.clone(), id.clone());
            s.subscription_specs.push(spec.clone());
            Ok(id)
        })
    }

    /// Says what Razorpay holds for `razorpay_subscription_id` from now on (what the reconcile
    /// job reads).
    pub fn set_remote(&self, razorpay_subscription_id: &str, remote: RemoteSubscription) {
        self.with(|s| {
            s.remote.insert(razorpay_subscription_id.to_owned(), remote);
        });
    }

    fn fetch_subscription(
        &self,
        razorpay_subscription_id: &str,
    ) -> Result<Option<RemoteSubscription>, BillingError> {
        self.with(|s| {
            if s.fail_subscriptions {
                return Err(BillingError("FAKE_SUBSCRIPTION_FAILED".to_owned()));
            }
            Ok(s.remote.get(razorpay_subscription_id).cloned())
        })
    }

    fn cancel_subscription(&self, razorpay_subscription_id: &str) -> Result<(), BillingError> {
        self.with(|s| {
            if s.fail_subscriptions {
                return Err(BillingError("FAKE_SUBSCRIPTION_FAILED".to_owned()));
            }
            if !s.cancelled.iter().any(|c| c == razorpay_subscription_id) {
                s.cancelled.push(razorpay_subscription_id.to_owned());
            }
            Ok(())
        })
    }

    fn cancel_at_cycle_end(&self, razorpay_subscription_id: &str) -> Result<(), BillingError> {
        self.with(|s| {
            if s.fail_subscriptions {
                return Err(BillingError("FAKE_SUBSCRIPTION_FAILED".to_owned()));
            }
            if !s
                .cancelled_at_cycle_end
                .iter()
                .any(|c| c == razorpay_subscription_id)
            {
                s.cancelled_at_cycle_end
                    .push(razorpay_subscription_id.to_owned());
            }
            Ok(())
        })
    }

    fn keep_after_cancel(&self, razorpay_subscription_id: &str) -> Result<(), BillingError> {
        self.with(|s| {
            if s.fail_subscriptions {
                return Err(BillingError("FAKE_SUBSCRIPTION_FAILED".to_owned()));
            }
            s.cancelled_at_cycle_end
                .retain(|c| c != razorpay_subscription_id);
            s.kept.push(razorpay_subscription_id.to_owned());
            Ok(())
        })
    }

    fn create_plan(&self, spec: &PlanSpec) -> Result<String, BillingError> {
        self.with(|s| {
            if let Some(id) = s.plans.get(&spec.key()) {
                return Ok(id.clone());
            }
            match s.fail_after.as_mut() {
                Some(0) => return Err(BillingError("FAKE_PLAN_FAILED".to_owned())),
                Some(n) => *n -= 1,
                None => {}
            }
            let id = format!("plan_fake_{:03}", s.created.len() + 1);
            s.plans.insert(spec.key(), id.clone());
            s.created.push(spec.clone());
            Ok(id)
        })
    }
}

/// The Razorpay API client.
pub struct RazorpayClient {
    client: reqwest::Client,
    base: String,
    key_id: String,
    key_secret: String,
}

impl std::fmt::Debug for RazorpayClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RazorpayClient")
            .field("key_id", &self.key_id)
            .field("key_secret", &"[redacted]")
            .finish_non_exhaustive()
    }
}

impl RazorpayClient {
    pub fn new(key_id: &str, key_secret: &str, base: Option<&str>) -> Result<Self, String> {
        let plain =
            |v: &str| !v.is_empty() && !v.chars().any(|c| c.is_whitespace() || c.is_control());
        if !plain(key_id) {
            return Err("the key id is empty or has whitespace".to_owned());
        }
        if !plain(key_secret) {
            return Err("the key secret is empty or has whitespace".to_owned());
        }
        Ok(Self {
            client: crate::alert::client_builder(CALL_TIMEOUT)?
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|e| e.without_url().to_string())?,
            base: base
                .unwrap_or(DEFAULT_API_BASE)
                .trim_end_matches('/')
                .to_owned(),
            key_id: key_id.to_owned(),
            key_secret: key_secret.to_owned(),
        })
    }

    async fn create_plan(&self, spec: &PlanSpec) -> Result<String, BillingError> {
        let body = json!({
            "period": spec.cycle.as_str(),
            "interval": 1,
            "item": {
                "name": spec.name(),
                "amount": spec.amount_minor,
                "currency": spec.currency,
            },
            "notes": {
                "price_book": spec.book_id,
                "band": spec.band.as_str(),
                "item": spec.item.as_str(),
                "cycle": spec.cycle.as_str(),
            },
        });
        Self::id_of(&self.post("/plans", &body).await?)
    }

    /// Razorpay bills one plan per subscription and charges add-ons once, so the subscription
    /// gets a plan priced at the locked total (tier and add-ons, GST included), then subscribes
    /// to it.
    async fn create_subscription(&self, spec: &SubscriptionSpec) -> Result<String, BillingError> {
        let notes = json!({
            "subscription_id": spec.subscription_id,
            "branch_id": spec.branch_id,
            "price_book": spec.book_id,
            "band": spec.band.as_str(),
            "tier": spec.item.as_str(),
            "cycle": spec.cycle.as_str(),
        });
        let plan = json!({
            "period": spec.cycle.as_str(),
            "interval": 1,
            "item": {
                "name": spec.plan_name(),
                "amount": spec.total_minor,
                "currency": spec.currency,
            },
            "notes": notes,
        });
        let plan_id = Self::id_of(&self.post("/plans", &plan).await?)?;
        let total_count = match spec.cycle {
            BillingCycle::Monthly => 120,
            BillingCycle::Yearly => 10,
        };
        let sub = json!({
            "plan_id": plan_id,
            "total_count": total_count,
            "customer_notify": 1,
            "notes": notes,
        });
        Self::id_of(&self.post("/subscriptions", &sub).await?)
    }

    /// Creates the plan priced at the new renewal total.
    async fn plan_for_change(&self, spec: &PlanChangeSpec) -> Result<String, BillingError> {
        let plan = json!({
            "period": spec.cycle.as_str(),
            "interval": 1,
            "item": {
                "name": spec.plan_name(),
                "amount": spec.renewal_minor,
                "currency": spec.currency,
            },
            "notes": {
                "subscription_id": spec.subscription_id,
                "change_id": spec.change_id,
                "price_book": spec.book_id,
                "band": spec.band.as_str(),
                "tier": spec.item.as_str(),
                "cycle": spec.cycle.as_str(),
            },
        });
        Self::id_of(&self.post("/plans", &plan).await?)
    }

    /// Moves the live subscription to the new plan: Razorpay charges the prorated difference at
    /// once for `now`, and switches at the end of the cycle otherwise.
    async fn change_plan(&self, spec: &PlanChangeSpec) -> Result<(), BillingError> {
        if !plain_id(&spec.razorpay_subscription_id) {
            return Err(BillingError("RAZORPAY_BAD_ID".to_owned()));
        }
        let plan_id = self.plan_for_change(spec).await?;
        let body = json!({
            "plan_id": plan_id,
            "schedule_change_at": if spec.now { "now" } else { "cycle_end" },
            "customer_notify": 1,
        });
        self.patch(
            &format!("/subscriptions/{}", spec.razorpay_subscription_id),
            &body,
        )
        .await
        .map(|_| ())
    }

    /// A subscription that starts at the end of the paid period and charges the prorated amount
    /// when the owner authorises its mandate.
    async fn create_mandate(&self, spec: &MandateSpec) -> Result<String, BillingError> {
        let plan_id = self.plan_for_change(&spec.change).await?;
        let total_count = match spec.change.cycle {
            BillingCycle::Monthly => 120,
            BillingCycle::Yearly => 10,
        };
        let mut sub = json!({
            "plan_id": plan_id,
            "total_count": total_count,
            "customer_notify": 1,
            "start_at": spec.start_at_ms / 1000,
            "notes": {
                "subscription_id": spec.change.subscription_id,
                "change_id": spec.change.change_id,
            },
        });
        if spec.change.charge_minor > 0 {
            sub["addons"] = json!([{
                "item": {
                    "name": "Arena 360 plan change (prorated)",
                    "amount": spec.change.charge_minor,
                    "currency": spec.change.currency,
                },
            }]);
        }
        Self::id_of(&self.post("/subscriptions", &sub).await?)
    }

    /// Cancels at once, not at the end of the cycle.
    async fn cancel_subscription(
        &self,
        razorpay_subscription_id: &str,
    ) -> Result<(), BillingError> {
        if !plain_id(razorpay_subscription_id) {
            return Err(BillingError("RAZORPAY_BAD_ID".to_owned()));
        }
        self.post(
            &format!("/subscriptions/{razorpay_subscription_id}/cancel"),
            &json!({ "cancel_at_cycle_end": 0 }),
        )
        .await
        .map(|_| ())
    }

    /// Ends the subscription with the cycle it has paid for: it bills no more, and nothing already
    /// paid is touched.
    async fn cancel_at_cycle_end(
        &self,
        razorpay_subscription_id: &str,
    ) -> Result<(), BillingError> {
        if !plain_id(razorpay_subscription_id) {
            return Err(BillingError("RAZORPAY_BAD_ID".to_owned()));
        }
        self.post(
            &format!("/subscriptions/{razorpay_subscription_id}/cancel"),
            &json!({ "cancel_at_cycle_end": 1 }),
        )
        .await
        .map(|_| ())
    }

    /// Refunds a payment in full (`amount_minor` is the whole payment); Razorpay answers the refund
    /// id and confirms with `refund.processed`.
    async fn refund_payment(
        &self,
        payment_id: &str,
        amount_minor: i64,
    ) -> Result<String, BillingError> {
        if !plain_id(payment_id) {
            return Err(BillingError("RAZORPAY_BAD_ID".to_owned()));
        }
        let reply = self
            .post(
                &format!("/payments/{payment_id}/refund"),
                &json!({ "amount": amount_minor }),
            )
            .await?;
        Self::id_of(&reply)
    }

    /// Takes back a cancellation scheduled for the end of the cycle: the subscription goes on
    /// billing as before.
    async fn keep_after_cancel(&self, razorpay_subscription_id: &str) -> Result<(), BillingError> {
        if !plain_id(razorpay_subscription_id) {
            return Err(BillingError("RAZORPAY_BAD_ID".to_owned()));
        }
        self.post(
            &format!("/subscriptions/{razorpay_subscription_id}/cancel_scheduled_changes"),
            &json!({}),
        )
        .await
        .map(|_| ())
    }

    /// The subscription as Razorpay holds it now, with how its latest payment was made when it
    /// has been paid. A failed payment lookup leaves the payment unknown rather than failing.
    async fn fetch_subscription(
        &self,
        razorpay_subscription_id: &str,
    ) -> Result<RemoteSubscription, BillingError> {
        if !plain_id(razorpay_subscription_id) {
            return Err(BillingError("RAZORPAY_BAD_ID".to_owned()));
        }
        let sub = self
            .get(&format!("/subscriptions/{razorpay_subscription_id}"))
            .await?;
        let status = sub
            .get("status")
            .and_then(Value::as_str)
            .ok_or_else(|| BillingError("RAZORPAY_BAD_ANSWER".to_owned()))?
            .to_owned();
        let paid_count = sub
            .get("paid_count")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0);
        let current_end_ms = sub
            .get("current_end")
            .and_then(Value::as_i64)
            .and_then(|s| s.checked_mul(1000));
        let payment = if paid_count > 0 {
            self.latest_payment(razorpay_subscription_id).await
        } else {
            None
        };
        Ok(RemoteSubscription {
            status,
            current_end_ms,
            paid_count,
            payment,
        })
    }

    async fn latest_payment(&self, razorpay_subscription_id: &str) -> Option<PaymentInfo> {
        let invoices = self
            .get(&format!(
                "/invoices?subscription_id={razorpay_subscription_id}&count=1"
            ))
            .await
            .ok()?;
        let payment_id = invoices
            .get("items")?
            .as_array()?
            .first()?
            .get("payment_id")?
            .as_str()
            .filter(|id| plain_id(id))?
            .to_owned();
        let payment = self.get(&format!("/payments/{payment_id}")).await.ok()?;
        Some(payment_info_from(&payment))
    }

    fn id_of(json: &Value) -> Result<String, BillingError> {
        json.get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= 64)
            .map(str::to_owned)
            .ok_or_else(|| BillingError("RAZORPAY_BAD_ANSWER".to_owned()))
    }

    /// POSTs `body` to `path` and answers the JSON reply. The error is a stable code: never the
    /// key, a secret or the response body.
    async fn post(&self, path: &str, body: &Value) -> Result<Value, BillingError> {
        self.send(
            self.client
                .post(format!("{}{path}", self.base))
                .header("content-type", "application/json")
                .body(body.to_string()),
        )
        .await
    }

    /// PATCHes `body` to `path`, with the same stable error codes as [`Self::post`].
    async fn patch(&self, path: &str, body: &Value) -> Result<Value, BillingError> {
        self.send(
            self.client
                .patch(format!("{}{path}", self.base))
                .header("content-type", "application/json")
                .body(body.to_string()),
        )
        .await
    }

    /// GETs `path` and answers the JSON reply, with the same stable error codes as [`Self::post`].
    async fn get(&self, path: &str) -> Result<Value, BillingError> {
        self.send(self.client.get(format!("{}{path}", self.base)))
            .await
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<Value, BillingError> {
        let res = req
            .basic_auth(&self.key_id, Some(&self.key_secret))
            .send()
            .await
            .map_err(|e| {
                BillingError(if e.is_timeout() {
                    "RAZORPAY_TIMEOUT".to_owned()
                } else {
                    "RAZORPAY_UNREACHABLE".to_owned()
                })
            })?;
        let status = res.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(BillingError(format!("RAZORPAY_HTTP_{status}")));
        }
        Ok(match res.bytes().await {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            Err(_) => Value::Null,
        })
    }
}

impl std::fmt::Debug for FakeRazorpay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeRazorpay").finish_non_exhaustive()
    }
}

/// The billing provider in use.
#[derive(Debug)]
// One provider lives for the whole process, so the recorded fake being the largest variant costs nothing.
#[allow(clippy::large_enum_variant)]
pub enum BillingProvider {
    Fake(FakeRazorpay),
    Razorpay(RazorpayClient),
    Manual,
}

impl BillingProvider {
    /// Its name as `billing.provider` spells it.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Fake(_) => "fake",
            Self::Razorpay(_) => "razorpay",
            Self::Manual => "manual",
        }
    }

    /// The public key id Razorpay Checkout opens with; `None` while the provider is `manual`.
    pub fn key_id(&self) -> Option<&str> {
        match self {
            Self::Fake(_) => Some(FAKE_KEY_ID),
            Self::Razorpay(r) => Some(&r.key_id),
            Self::Manual => None,
        }
    }

    /// Creates the Razorpay subscription for one branch.
    pub async fn create_subscription(
        &self,
        spec: &SubscriptionSpec,
    ) -> Result<SubscriptionResult, BillingError> {
        match self {
            Self::Fake(f) => f.create_subscription(spec).map(SubscriptionResult::Created),
            Self::Razorpay(r) => r
                .create_subscription(spec)
                .await
                .map(SubscriptionResult::Created),
            Self::Manual => Ok(SubscriptionResult::Deferred),
        }
    }

    /// Reads a Razorpay subscription as the provider holds it now; `None` when the provider is
    /// `manual` (or, for the fake, when no test said what it holds).
    pub async fn fetch_subscription(
        &self,
        razorpay_subscription_id: &str,
    ) -> Result<Option<RemoteSubscription>, BillingError> {
        match self {
            Self::Fake(f) => f.fetch_subscription(razorpay_subscription_id),
            Self::Razorpay(r) => r
                .fetch_subscription(razorpay_subscription_id)
                .await
                .map(Some),
            Self::Manual => Ok(None),
        }
    }

    /// Cancels a Razorpay subscription at once.
    pub async fn cancel_subscription(
        &self,
        razorpay_subscription_id: &str,
    ) -> Result<CancelResult, BillingError> {
        match self {
            Self::Fake(f) => f
                .cancel_subscription(razorpay_subscription_id)
                .map(|()| CancelResult::Cancelled),
            Self::Razorpay(r) => r
                .cancel_subscription(razorpay_subscription_id)
                .await
                .map(|()| CancelResult::Cancelled),
            Self::Manual => Ok(CancelResult::Deferred),
        }
    }

    /// Ends a subscription with the cycle it has paid for, so it never bills beside its
    /// replacement (Story 10.10).
    pub async fn cancel_at_cycle_end(
        &self,
        razorpay_subscription_id: &str,
    ) -> Result<CancelResult, BillingError> {
        match self {
            Self::Fake(f) => f
                .cancel_at_cycle_end(razorpay_subscription_id)
                .map(|()| CancelResult::Cancelled),
            Self::Razorpay(r) => r
                .cancel_at_cycle_end(razorpay_subscription_id)
                .await
                .map(|()| CancelResult::Cancelled),
            Self::Manual => Ok(CancelResult::Deferred),
        }
    }

    /// Takes back a cancellation scheduled for the end of the cycle (Story 10.14).
    pub async fn keep_after_cancel(
        &self,
        razorpay_subscription_id: &str,
    ) -> Result<CancelResult, BillingError> {
        match self {
            Self::Fake(f) => f
                .keep_after_cancel(razorpay_subscription_id)
                .map(|()| CancelResult::Cancelled),
            Self::Razorpay(r) => r
                .keep_after_cancel(razorpay_subscription_id)
                .await
                .map(|()| CancelResult::Cancelled),
            Self::Manual => Ok(CancelResult::Deferred),
        }
    }

    /// Refunds the whole of one payment (Story 10.15).
    pub async fn refund_payment(
        &self,
        payment_id: &str,
        amount_minor: i64,
    ) -> Result<RefundResult, BillingError> {
        match self {
            Self::Fake(f) => f
                .refund_payment(payment_id, amount_minor)
                .map(RefundResult::Issued),
            Self::Razorpay(r) => r
                .refund_payment(payment_id, amount_minor)
                .await
                .map(RefundResult::Issued),
            Self::Manual => Ok(RefundResult::Deferred),
        }
    }

    /// Moves a live subscription to the plan of a change (Story 10.9).
    pub async fn change_plan(&self, spec: &PlanChangeSpec) -> Result<ChangeResult, BillingError> {
        match self {
            Self::Fake(f) => f.change_plan(spec).map(|()| ChangeResult::Updated),
            Self::Razorpay(r) => r.change_plan(spec).await.map(|()| ChangeResult::Updated),
            Self::Manual => Ok(ChangeResult::Deferred),
        }
    }

    /// Creates the subscription that authorises the new mandate of a held change (Story 10.9).
    pub async fn create_mandate(
        &self,
        spec: &MandateSpec,
    ) -> Result<SubscriptionResult, BillingError> {
        match self {
            Self::Fake(f) => f.create_mandate(spec).map(SubscriptionResult::Created),
            Self::Razorpay(r) => r
                .create_mandate(spec)
                .await
                .map(SubscriptionResult::Created),
            Self::Manual => Ok(SubscriptionResult::Deferred),
        }
    }

    /// Creates the plan for one price book row.
    pub async fn create_plan(&self, spec: &PlanSpec) -> Result<PlanResult, BillingError> {
        match self {
            Self::Fake(f) => f.create_plan(spec).map(PlanResult::Created),
            Self::Razorpay(r) => r.create_plan(spec).await.map(PlanResult::Created),
            Self::Manual => Ok(PlanResult::Deferred),
        }
    }
}

/// The provider the environment asks for. `--dev` uses the recorded fake unless `manual` is
/// set; otherwise `ARENA_BILLING_PROVIDER` decides (`razorpay` needs both keys), and when it is
/// unset the keys decide. Errors name the variable, never its value.
pub fn provider_from_env(
    dev: bool,
    env: &impl Fn(&str) -> Option<OsString>,
) -> Result<BillingProvider, String> {
    let named = get_env(env, PROVIDER_ENV).filter(|v| !v.is_empty());
    match named.as_deref() {
        Some("manual") => return Ok(BillingProvider::Manual),
        Some("razorpay") | None => {}
        Some(_) => return Err(format!("{PROVIDER_ENV} must be razorpay or manual")),
    }
    if dev && named.is_none() {
        return Ok(BillingProvider::Fake(FakeRazorpay::new()));
    }
    let key_id = get_env(env, KEY_ID_ENV).filter(|v| !v.is_empty());
    let key_secret = get_env(env, KEY_SECRET_ENV).filter(|v| !v.is_empty());
    match (key_id, key_secret) {
        (Some(id), Some(secret)) => {
            let base = get_env(env, API_BASE_ENV).filter(|v| !v.is_empty());
            RazorpayClient::new(&id, &secret, base.as_deref())
                .map(BillingProvider::Razorpay)
                .map_err(|e| format!("{KEY_ID_ENV}/{KEY_SECRET_ENV}: {e}"))
        }
        (None, None) if named.is_none() => Ok(BillingProvider::Manual),
        _ => Err(format!(
            "{PROVIDER_ENV}=razorpay needs both {KEY_ID_ENV} and {KEY_SECRET_ENV}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |k| {
            pairs
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| OsString::from(v))
        }
    }

    fn spec(item: PriceItem) -> PlanSpec {
        PlanSpec {
            book_id: "pb_v1".to_owned(),
            band: Band::In,
            item,
            cycle: BillingCycle::Monthly,
            currency: "INR".to_owned(),
            amount_minor: 20_000,
        }
    }

    #[test]
    fn the_provider_follows_the_environment() {
        let none = env_of(&[]);
        assert_eq!(provider_from_env(true, &none).unwrap().name(), "fake");
        assert_eq!(provider_from_env(false, &none).unwrap().name(), "manual");
        let keys = env_of(&[(KEY_ID_ENV, "rzp_test_1"), (KEY_SECRET_ENV, "s3cret")]);
        assert_eq!(provider_from_env(false, &keys).unwrap().name(), "razorpay");
        let manual = env_of(&[
            (PROVIDER_ENV, "manual"),
            (KEY_ID_ENV, "rzp_test_1"),
            (KEY_SECRET_ENV, "s3cret"),
        ]);
        assert_eq!(provider_from_env(false, &manual).unwrap().name(), "manual");
        assert_eq!(provider_from_env(true, &manual).unwrap().name(), "manual");
        let half = env_of(&[(PROVIDER_ENV, "razorpay"), (KEY_ID_ENV, "rzp_test_1")]);
        assert!(
            provider_from_env(false, &half)
                .unwrap_err()
                .contains(KEY_SECRET_ENV)
        );
        let bad = env_of(&[(PROVIDER_ENV, "stripe")]);
        assert!(
            provider_from_env(false, &bad)
                .unwrap_err()
                .contains(PROVIDER_ENV)
        );
        let spaced = env_of(&[(KEY_ID_ENV, "rzp test"), (KEY_SECRET_ENV, "s3cret")]);
        let err = provider_from_env(false, &spaced).unwrap_err();
        assert!(!err.contains("s3cret"), "{err}");
    }

    #[test]
    fn debug_never_shows_the_secret() {
        let c = RazorpayClient::new("rzp_test_1", "s3cret-value", None).unwrap();
        let shown = format!("{c:?}");
        assert!(
            shown.contains("rzp_test_1") && !shown.contains("s3cret-value"),
            "{shown}"
        );
    }

    #[tokio::test]
    async fn the_fake_records_plans_and_answers_a_repeat_with_the_same_id() {
        let p = BillingProvider::Fake(FakeRazorpay::new());
        let first = p.create_plan(&spec(PriceItem::Basic)).await.unwrap();
        let again = p.create_plan(&spec(PriceItem::Basic)).await.unwrap();
        let other = p.create_plan(&spec(PriceItem::Premium)).await.unwrap();
        assert_eq!(first, PlanResult::Created("plan_fake_001".to_owned()));
        assert_eq!(first, again);
        assert_eq!(other, PlanResult::Created("plan_fake_002".to_owned()));
        let BillingProvider::Fake(f) = &p else {
            unreachable!()
        };
        assert_eq!(f.created().len(), 2);
    }

    fn sub_spec(id: &str) -> SubscriptionSpec {
        SubscriptionSpec {
            subscription_id: id.to_owned(),
            branch_id: "b1".to_owned(),
            book_id: "pb_v1".to_owned(),
            band: Band::In,
            item: PriceItem::Basic,
            cycle: BillingCycle::Yearly,
            currency: "INR".to_owned(),
            total_minor: 236_000,
            methods: vec![MandateMethod::Card],
        }
    }

    #[tokio::test]
    async fn the_fake_creates_one_subscription_per_ours_and_records_cancels() {
        let p = BillingProvider::Fake(FakeRazorpay::new());
        assert_eq!(p.key_id(), Some("rzp_test_fake"));
        let a = p.create_subscription(&sub_spec("s1")).await.unwrap();
        let again = p.create_subscription(&sub_spec("s1")).await.unwrap();
        let b = p.create_subscription(&sub_spec("s2")).await.unwrap();
        assert_eq!(a, SubscriptionResult::Created("sub_fake_001".to_owned()));
        assert_eq!(a, again);
        assert_eq!(b, SubscriptionResult::Created("sub_fake_002".to_owned()));
        assert_eq!(
            p.cancel_subscription("sub_fake_001").await,
            Ok(CancelResult::Cancelled)
        );
        let BillingProvider::Fake(f) = &p else {
            unreachable!()
        };
        assert_eq!(f.subscriptions().len(), 2);
        assert_eq!(f.cancelled(), ["sub_fake_001"]);
        f.fail_subscriptions(true);
        let err = p.create_subscription(&sub_spec("s3")).await.unwrap_err();
        assert_eq!(err.0, "FAKE_SUBSCRIPTION_FAILED");
        assert!(p.cancel_subscription("sub_fake_002").await.is_err());
        assert_eq!(f.cancelled(), ["sub_fake_001"]);
        f.fail_subscriptions(false);
        assert!(p.create_subscription(&sub_spec("s3")).await.is_ok());
    }

    #[tokio::test]
    async fn manual_defers_subscription_calls_and_has_no_key() {
        let p = BillingProvider::Manual;
        assert_eq!(p.key_id(), None);
        assert_eq!(
            p.create_subscription(&sub_spec("s1")).await,
            Ok(SubscriptionResult::Deferred)
        );
        assert_eq!(
            p.cancel_subscription("sub_1").await,
            Ok(CancelResult::Deferred)
        );
    }

    #[tokio::test]
    async fn the_razorpay_client_refuses_an_unsafe_subscription_id_before_any_call() {
        let c = RazorpayClient::new("rzp_test_1", "s3cret", Some("http://127.0.0.1:1")).unwrap();
        let err = c.cancel_subscription("sub_1/../x").await.unwrap_err();
        assert_eq!(err.0, "RAZORPAY_BAD_ID");
        assert_eq!(BillingProvider::Razorpay(c).key_id(), Some("rzp_test_1"));
    }

    #[test]
    fn a_payment_says_where_it_came_from_as_far_as_razorpay_does() {
        let of = |v: Value| payment_info_from(&v);
        let upi = of(json!({"method": "upi"}));
        assert_eq!(upi.method, Some(MandateMethod::Upi));
        assert_eq!(upi.origin, PaymentOrigin::Country("IN".to_owned()));
        assert_eq!(
            of(json!({"method": "emandate"})).method,
            Some(MandateMethod::Emandate)
        );
        assert_eq!(
            of(json!({"method": "netbanking"})).origin,
            PaymentOrigin::Country("IN".to_owned())
        );
        let card = |c: Value| of(json!({"method": "card", "card": c})).origin;
        assert_eq!(
            card(json!({"international": false})),
            PaymentOrigin::Country("IN".to_owned())
        );
        assert_eq!(
            card(json!({"international": true})),
            PaymentOrigin::International
        );
        assert_eq!(
            card(json!({"international": true, "country": "us"})),
            PaymentOrigin::Country("US".to_owned())
        );
        assert_eq!(card(json!({"country": "usa"})), PaymentOrigin::Unknown);
        assert_eq!(card(json!({})), PaymentOrigin::Unknown);
        assert_eq!(
            of(json!({"method": "wallet"})).origin,
            PaymentOrigin::Unknown
        );
        assert_eq!(of(json!({})).method, None);
    }

    #[test]
    fn a_payment_names_its_method_by_card_digits_or_bank_and_never_a_upi_handle() {
        let detail = |v: Value| payment_info_from(&v).detail;
        assert_eq!(
            detail(json!({"method": "card", "card": {"last4": "4242"}})).as_deref(),
            Some("4242")
        );
        for bad in ["424", "42424", "42a2", ""] {
            assert_eq!(
                detail(json!({"method": "card", "card": {"last4": bad}})),
                None,
                "{bad:?}"
            );
        }
        assert_eq!(
            detail(json!({"method": "emandate", "bank": " HDFC "})).as_deref(),
            Some("HDFC")
        );
        assert_eq!(detail(json!({"method": "emandate", "bank": ""})), None);
        assert_eq!(
            detail(json!({"method": "upi", "vpa": "someone@okbank", "bank": "HDFC"})),
            None
        );
        assert_eq!(detail(json!({"method": "card"})), None);
    }

    #[test]
    fn an_origin_fits_its_band_unless_it_is_known_to_be_elsewhere() {
        let india = PaymentOrigin::Country("IN".to_owned());
        let us = PaymentOrigin::Country("US".to_owned());
        assert!(india.fits(Band::In) && !india.fits(Band::Us));
        assert!(us.fits(Band::Us) && !us.fits(Band::In));
        assert!(PaymentOrigin::International.fits(Band::Sg));
        assert!(!PaymentOrigin::International.fits(Band::In));
        assert!(PaymentOrigin::Unknown.fits(Band::In) && PaymentOrigin::Unknown.fits(Band::My));
        for o in [india, us, PaymentOrigin::International] {
            assert_eq!(PaymentOrigin::parse(&o.as_text().unwrap()), o);
        }
        assert_eq!(PaymentOrigin::Unknown.as_text(), None);
        assert_eq!(PaymentOrigin::parse("india"), PaymentOrigin::Unknown);
    }

    #[tokio::test]
    async fn the_fake_reads_back_what_a_test_says_razorpay_holds() {
        let p = BillingProvider::Fake(FakeRazorpay::new());
        assert_eq!(p.fetch_subscription("sub_1").await, Ok(None));
        let BillingProvider::Fake(f) = &p else {
            unreachable!()
        };
        let remote = RemoteSubscription {
            status: "active".to_owned(),
            current_end_ms: Some(9_000),
            paid_count: 1,
            payment: None,
        };
        f.set_remote("sub_1", remote.clone());
        assert_eq!(p.fetch_subscription("sub_1").await, Ok(Some(remote)));
        f.fail_subscriptions(true);
        assert!(p.fetch_subscription("sub_1").await.is_err());
        assert_eq!(
            BillingProvider::Manual.fetch_subscription("sub_1").await,
            Ok(None)
        );
        let c = RazorpayClient::new("rzp_test_1", "s3cret", Some("http://127.0.0.1:1")).unwrap();
        let err = c.fetch_subscription("sub_1/../x").await.unwrap_err();
        assert_eq!(err.0, "RAZORPAY_BAD_ID");
    }

    #[tokio::test]
    async fn the_fake_fails_on_request_and_manual_defers() {
        let f = FakeRazorpay::new();
        f.fail_after(1);
        let p = BillingProvider::Fake(f);
        p.create_plan(&spec(PriceItem::Basic)).await.unwrap();
        let err = p.create_plan(&spec(PriceItem::Premium)).await.unwrap_err();
        assert_eq!(err.0, "FAKE_PLAN_FAILED");
        assert_eq!(
            BillingProvider::Manual
                .create_plan(&spec(PriceItem::Basic))
                .await,
            Ok(PlanResult::Deferred)
        );
    }
}
