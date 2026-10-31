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

/// The key id the fake hands to Checkout.
pub const FAKE_KEY_ID: &str = "rzp_test_fake";

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
    fail_subscriptions: bool,
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

    /// Cancels at once, not at the end of the cycle.
    async fn cancel_subscription(
        &self,
        razorpay_subscription_id: &str,
    ) -> Result<(), BillingError> {
        let plain = !razorpay_subscription_id.is_empty()
            && razorpay_subscription_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !plain {
            return Err(BillingError("RAZORPAY_BAD_ID".to_owned()));
        }
        self.post(
            &format!("/subscriptions/{razorpay_subscription_id}/cancel"),
            &json!({ "cancel_at_cycle_end": 0 }),
        )
        .await
        .map(|_| ())
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
        let res = self
            .client
            .post(format!("{}{path}", self.base))
            .basic_auth(&self.key_id, Some(&self.key_secret))
            .header("content-type", "application/json")
            .body(body.to_string())
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
