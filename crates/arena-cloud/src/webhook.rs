//! Razorpay events (Story 10.3, AD-6, AD-20): a webhook (or the reconcile job reading the same
//! facts from the provider) becomes a subscription transition through one code path. The
//! signature is checked over the raw body before anything is parsed; an event is recorded and its
//! change applied in one transaction, keyed by the unique Razorpay event id, so a repeat applies
//! nothing; and an older event never moves a paid period back. The entitlement is the only thing
//! that changes for the branch: the Hub learns of it from its next heartbeat's licence, never from
//! a command.

use std::ffi::OsString;
use std::sync::atomic::{AtomicU64, Ordering};

use arena_proto::{MandateMethod, SubscriptionState};
use arena_store::control::{
    AppliedPlanChange, ChangeKind, ChangeStatus, ChangeSubject, ControlDb, ControlError,
    EntitlementWrite, EventOutcome, EventRecord, NewOpsAudit, NewOpsTask, Subscription,
    SubscriptionChange,
};
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::billing::{PaymentInfo, PaymentOrigin, RemoteSubscription, payment_info_from};
use crate::config::get_env;
use crate::plan_change;
use crate::subscription::TASK_CANCEL_SUBSCRIPTION;

type HmacSha256 = Hmac<Sha256>;

/// The webhook secret Razorpay signs with (a secret: never logged).
pub const SECRET_ENV: &str = "ARENA_RAZORPAY_WEBHOOK_SECRET";

/// The secret the recorded fake and the tests sign with.
pub const FAKE_WEBHOOK_SECRET: &str = "whsec_fake_arena360";

/// The `ops_tasks.kind` left when a first payment does not match the subscription's band.
pub const TASK_REVIEW: &str = "subscription_review";

/// What the owner is told while a payment is held for review.
pub const REVIEW_MESSAGE: &str =
    "We're checking your payment \u{2014} this usually takes a working day";

/// How many times an event is retried when the subscription moves under it.
const ATTEMPTS: u32 = 3;

/// The webhook secret.
#[derive(Clone, PartialEq, Eq)]
pub struct WebhookSecret(Vec<u8>);

impl std::fmt::Debug for WebhookSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WebhookSecret([redacted])")
    }
}

impl WebhookSecret {
    pub fn new(secret: &str) -> Result<Self, String> {
        let plain =
            |v: &str| !v.is_empty() && !v.chars().any(|c| c.is_whitespace() || c.is_control());
        if plain(secret) {
            Ok(Self(secret.as_bytes().to_vec()))
        } else {
            Err("the webhook secret is empty or has whitespace".to_owned())
        }
    }

    /// The secret from `ARENA_RAZORPAY_WEBHOOK_SECRET`; `None` when it is not set. Errors name the
    /// variable, never its value.
    pub fn from_env(env: &impl Fn(&str) -> Option<OsString>) -> Result<Option<Self>, String> {
        match get_env(env, SECRET_ENV).filter(|v| !v.is_empty()) {
            None => Ok(None),
            Some(v) => Self::new(&v)
                .map(Some)
                .map_err(|e| format!("{SECRET_ENV}: {e}")),
        }
    }

    fn mac(&self) -> HmacSha256 {
        HmacSha256::new_from_slice(&self.0).expect("HMAC takes any key length")
    }

    /// The `X-Razorpay-Signature` of `body`: HMAC-SHA256, lowercase hex.
    pub fn sign(&self, body: &[u8]) -> String {
        let mut mac = self.mac();
        mac.update(body);
        crate::auth::hex(&mac.finalize().into_bytes())
    }

    /// Whether `signature` is the signature of `body`, compared in constant time.
    pub fn verify(&self, body: &[u8], signature: &str) -> bool {
        let Some(given) = crate::auth::unhex(signature.trim()) else {
            return false;
        };
        let mut mac = self.mac();
        mac.update(body);
        mac.verify_slice(&given).is_ok()
    }
}

/// Webhook handling in numbers, kept in process and logged like the other cloud metrics (the
/// OpenTelemetry export is deferred with the Story 2.3 exporter).
#[derive(Debug, Default)]
pub struct WebhookMetrics {
    failures: AtomicU64,
    handled: AtomicU64,
    duplicates: AtomicU64,
    total_ms: AtomicU64,
    max_ms: AtomicU64,
}

/// A reading of [`WebhookMetrics`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WebhookCounts {
    /// Requests refused (bad signature, bad body) or that failed to apply: `webhook_failures`.
    pub failures: u64,
    /// Events recorded, including those with nothing to apply.
    pub handled: u64,
    /// Events already recorded.
    pub duplicates: u64,
    /// Time spent handling the recorded ones, and the longest one: `webhook_handling_ms`.
    pub total_ms: u64,
    pub max_ms: u64,
}

impl WebhookMetrics {
    pub fn failure(&self, reason: &str) {
        let n = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
        tracing::warn!(
            reason,
            webhook_failures = n,
            "a Razorpay webhook was refused"
        );
    }

    pub fn handled(&self, kind: &str, took_ms: u64) {
        self.handled.fetch_add(1, Ordering::Relaxed);
        self.total_ms.fetch_add(took_ms, Ordering::Relaxed);
        self.max_ms.fetch_max(took_ms, Ordering::Relaxed);
        tracing::info!(
            kind,
            webhook_handling_ms = took_ms,
            "a Razorpay event was handled"
        );
    }

    pub fn duplicate(&self) {
        self.duplicates.fetch_add(1, Ordering::Relaxed);
    }

    pub fn counts(&self) -> WebhookCounts {
        WebhookCounts {
            failures: self.failures.load(Ordering::Relaxed),
            handled: self.handled.load(Ordering::Relaxed),
            duplicates: self.duplicates.load(Ordering::Relaxed),
            total_ms: self.total_ms.load(Ordering::Relaxed),
            max_ms: self.max_ms.load(Ordering::Relaxed),
        }
    }
}

/// The facts of one event that matter to a subscription.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedEvent {
    /// `subscription.charged`, `subscription.activated`, ...
    pub kind: String,
    pub razorpay_subscription_id: Option<String>,
    /// The end of the period the event reports, in epoch milliseconds.
    pub current_end_ms: Option<i64>,
    /// When Razorpay says the event happened.
    pub created_at_ms: Option<i64>,
    pub payment: Option<PaymentInfo>,
    /// What the payment charged, in minor units (GST included), when the event carries one.
    pub amount_minor: Option<i64>,
}

/// Why a verified body is not an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadEvent(pub &'static str);

/// Reads an event body. The subscription and payment sit under `payload.<kind>.entity`; Razorpay
/// counts time in seconds.
pub fn parse(body: &[u8]) -> Result<ParsedEvent, BadEvent> {
    let v: Value = serde_json::from_slice(body).map_err(|_| BadEvent("the body is not JSON"))?;
    let kind = v
        .get("event")
        .and_then(Value::as_str)
        .filter(|k| !k.is_empty() && k.len() <= 64)
        .ok_or(BadEvent("the event has no name"))?
        .to_owned();
    let entity = |name: &str| v.pointer(&format!("/payload/{name}/entity"));
    let subscription = entity("subscription");
    let seconds_ms =
        |x: Option<&Value>| x.and_then(Value::as_i64).and_then(|s| s.checked_mul(1000));
    Ok(ParsedEvent {
        kind,
        razorpay_subscription_id: subscription
            .and_then(|s| s.get("id"))
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= 64)
            .map(str::to_owned),
        current_end_ms: seconds_ms(subscription.and_then(|s| s.get("current_end"))),
        created_at_ms: seconds_ms(v.get("created_at")),
        payment: entity("payment").map(payment_info_from),
        amount_minor: entity("payment")
            .and_then(|p| p.get("amount"))
            .and_then(Value::as_i64)
            .filter(|a| *a >= 0),
    })
}

/// The event the reconcile job derives from what Razorpay holds for a subscription, with an id
/// that is the same for the same state so a reconcile that sees nothing new records nothing.
pub fn from_remote(
    razorpay_subscription_id: &str,
    remote: &RemoteSubscription,
) -> (String, ParsedEvent) {
    let kind = match remote.status.as_str() {
        "authenticated" => "subscription.authenticated".to_owned(),
        "active" if remote.paid_count > 0 => "subscription.charged".to_owned(),
        "active" => "subscription.activated".to_owned(),
        other => format!("subscription.{other}"),
    };
    let id = format!(
        "reconcile:{razorpay_subscription_id}:{}:{}:{}",
        remote.status,
        remote.paid_count,
        remote.current_end_ms.unwrap_or(0)
    );
    (
        id,
        ParsedEvent {
            kind,
            razorpay_subscription_id: Some(razorpay_subscription_id.to_owned()),
            current_end_ms: remote.current_end_ms,
            created_at_ms: None,
            payment: remote.payment.clone(),
            amount_minor: None,
        },
    )
}

/// What an event does to a subscription.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub new_state: SubscriptionState,
    pub period_end: Option<i64>,
    pub first_paid_at: Option<i64>,
    pub payment_country: Option<String>,
    pub mandate: Option<MandateMethod>,
    /// Names the method: the card's last four or the mandate's bank.
    pub mandate_detail: Option<String>,
    /// The branch becomes entitled to the subscription's tier and add-ons.
    pub issue_entitlement: bool,
    /// The first payment does not match the band: hold it for an operator.
    pub review: bool,
    /// The event is a charge: it is invoiced (Story 10.7).
    pub charged: bool,
}

/// The transition `ev` causes on `sub`, or `None` when it changes nothing. `authenticated` alone
/// activates nothing; `activated` or `charged` on a `pending` subscription activates it (or holds
/// it for review when the first payment's country does not match its band); a later `charged`
/// moves the period forward and returns the subscription to `active`, but an event for a period
/// no later than the one held changes nothing.
pub fn decide(sub: &Subscription, ev: &ParsedEvent, now: i64) -> Option<Decision> {
    let charged = ev.kind == "subscription.charged";
    if !charged && ev.kind != "subscription.activated" {
        return None;
    }
    let paid_at = charged.then(|| ev.created_at_ms.unwrap_or(now));
    let mandate = ev.payment.as_ref().and_then(|p| p.method);
    let mandate_detail = ev.payment.as_ref().and_then(|p| p.detail.clone());
    match sub.state {
        SubscriptionState::Pending => {
            let origin = ev
                .payment
                .as_ref()
                .map(|p| p.origin.clone())
                .filter(|o| *o != PaymentOrigin::Unknown)
                .or_else(|| sub.payment_country.as_deref().map(PaymentOrigin::parse))
                .unwrap_or_default();
            let review = !origin.fits(sub.band);
            Some(Decision {
                new_state: if review {
                    SubscriptionState::PendingReview
                } else {
                    SubscriptionState::Active
                },
                period_end: ev.current_end_ms,
                first_paid_at: paid_at,
                payment_country: origin.as_text(),
                mandate,
                mandate_detail,
                issue_entitlement: !review,
                review,
                charged,
            })
        }
        SubscriptionState::Active
        | SubscriptionState::RenewalNeedsApproval
        | SubscriptionState::Grace
            if charged =>
        {
            let newer = match (ev.current_end_ms, sub.current_period_end) {
                (Some(new), Some(held)) => new > held,
                (Some(_), None) => true,
                (None, _) => false,
            };
            if newer {
                Some(Decision {
                    new_state: SubscriptionState::Active,
                    period_end: ev.current_end_ms,
                    first_paid_at: paid_at,
                    payment_country: None,
                    mandate,
                    mandate_detail,
                    issue_entitlement: false,
                    review: false,
                    charged: true,
                })
            } else if sub.first_paid_at.is_none() {
                // `activated` came first: the first charge still has to be dated.
                Some(Decision {
                    new_state: sub.state,
                    period_end: None,
                    first_paid_at: paid_at,
                    payment_country: None,
                    mandate,
                    mandate_detail,
                    issue_entitlement: false,
                    review: false,
                    charged: true,
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

/// The add-ons a subscription entitles, as the entitlement stores them.
pub fn addons_of(sub: &Subscription) -> Vec<String> {
    let mut out = Vec::new();
    if sub.addon_devices_20 > 0 {
        out.push(format!("devices_20:{}", sub.addon_devices_20));
    }
    if sub.addon_tunnel > 0 {
        out.push("tunnel".to_owned());
    }
    if sub.addon_owner_login > 0 {
        out.push(format!("owner_login:{}", sub.addon_owner_login));
    }
    out
}

fn new_id() -> String {
    ulid::Ulid::generate().to_string()
}

/// Where an event came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Webhook,
    Reconcile,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Webhook => "webhook",
            Self::Reconcile => "reconcile",
        }
    }
}

/// What handling an event did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handled {
    /// The subscription moved to `state` (or, for a renewal, to a later period).
    Applied {
        subscription_id: String,
        /// The branch the subscription belongs to (the wizard stream wakes its owner).
        branch_id: String,
        state: SubscriptionState,
        /// A first payment was held for review.
        review: bool,
        /// The Razorpay subscription a completed plan change replaced: to be cancelled so it never
        /// bills beside the new one (Story 10.9).
        replaced_razorpay_id: Option<String>,
        /// The replaced subscription is told to end with its paid cycle instead of at once: the
        /// owner authorised the new monthly subscription of a switch that starts at the period end
        /// (Story 10.10).
        retire_at_cycle_end: bool,
    },
    /// Recorded, nothing to change (an older period, `authenticated`, a kind we do not act on).
    Recorded,
    /// No subscription of ours matches; recorded so a repeat is a no-op (the reconcile job covers
    /// a payment whose subscription we did not know yet).
    Unmatched,
    /// The Razorpay event id was already recorded.
    Duplicate,
}

/// Records one event and applies what it causes, in one transaction, issuing the tax invoice when
/// the event is a charge. `event_id` is Razorpay's event id (the reconcile job's own for a fetched
/// state).
pub async fn handle(
    db: &ControlDb,
    event_id: &str,
    ev: &ParsedEvent,
    payload_hash: &str,
    source: Source,
    now: i64,
) -> Result<Handled, ControlError> {
    for _ in 0..ATTEMPTS {
        let rid = ev.razorpay_subscription_id.as_deref();
        let mut sub = match rid {
            Some(rid) => db.subscription_by_razorpay_id(rid).await?,
            None => None,
        };
        // A charge on the new Razorpay subscription that authorises a held change's mandate.
        let mut held = None;
        if sub.is_none()
            && let Some(rid) = rid
            && let Some(change) = db.held_plan_change_by_razorpay_id(rid).await?
        {
            sub = db.subscription(&change.subscription_id).await?;
            held = Some(change);
        }
        let open_change = match (&sub, &held) {
            (_, Some(change)) => Some(change.clone()),
            (Some(s), None) => db.open_plan_change(&s.id).await?,
            (None, None) => None,
        };
        let mut decision = match (&sub, &held) {
            (Some(s), None) => decide(s, ev, now),
            _ => None,
        };
        // A change of plan, billing cycle or payment method this event completes (Stories 10.9
        // and 10.10).
        let newer = decision
            .as_ref()
            .is_some_and(|d| d.charged && d.period_end.is_some());
        let completes = match (&sub, &open_change) {
            (Some(s), Some(change)) if s.state == SubscriptionState::Active || newer => {
                plan_change::completion(change, &ev.kind, held.is_some(), newer)
            }
            _ => None,
        };
        let subject = open_change
            .as_ref()
            .filter(|_| completes.is_some())
            .map(|c| c.subject);
        if completes.is_some()
            && let Some(s) = &sub
        {
            match &mut decision {
                Some(d) => d.issue_entitlement = true,
                None => {
                    // The new period a switch of cycle starts: a year out to yearly, and the
                    // period the first monthly charge reports to monthly.
                    let period_end = match (&open_change, completes) {
                        (Some(ch), Some(plan_change::Completion::Mandate))
                            if ch.subject == ChangeSubject::Cycle =>
                        {
                            Some(if ch.kind == ChangeKind::Now {
                                ch.effective_at
                            } else {
                                ev.current_end_ms
                                    .filter(|e| s.current_period_end.is_none_or(|p| *e > p))
                                    .unwrap_or_else(|| {
                                        crate::cycle_change::months_after(
                                            s.current_period_end.unwrap_or(now),
                                            1,
                                        )
                                    })
                            })
                        }
                        // A new mandate whose first event is its charge (the authenticated event
                        // was missed): that charge is the renewal it was set to start at.
                        (Some(ch), Some(plan_change::Completion::Mandate))
                            if ch.subject == ChangeSubject::Method
                                && ev.kind == "subscription.charged" =>
                        {
                            ev.current_end_ms
                                .filter(|e| s.current_period_end.is_none_or(|p| *e > p))
                        }
                        _ => None,
                    };
                    // The old mandate keeps showing until the switch to monthly applies.
                    let authorised_only = completes == Some(plan_change::Completion::Authorised);
                    decision = Some(Decision {
                        new_state: s.state,
                        period_end,
                        first_paid_at: None,
                        payment_country: None,
                        mandate: ev
                            .payment
                            .as_ref()
                            .and_then(|p| p.method)
                            .filter(|_| !authorised_only),
                        mandate_detail: ev
                            .payment
                            .as_ref()
                            .and_then(|p| p.detail.clone())
                            .filter(|_| !authorised_only),
                        issue_entitlement: subject == Some(ChangeSubject::Plan),
                        review: false,
                        // Authorising a mandate is not a charge to invoice.
                        charged: ev.kind == "subscription.charged" && !authorised_only,
                    });
                }
            }
        }
        // The subscription as it reads once the change has taken effect: its tier and add-ons
        // grant the entitlement and price the invoice.
        let effective = match (&sub, &open_change, completes) {
            (Some(s), Some(change), Some(_)) => Some(plan_change::with_change(s, change)),
            _ => None,
        };
        // The old Razorpay subscription a completed mandate replaces: it is cancelled so it never
        // bills beside the new one, and left to an operator when that call fails.
        // The old subscription of a switch to monthly was told to end with its cycle when the
        // owner authorised the new one, so its first charge has nothing left to cancel.
        let already_ending = open_change.as_ref().is_some_and(|c| {
            c.subject == ChangeSubject::Cycle && c.status == ChangeStatus::Charging
        });
        let retire_at_cycle_end = completes == Some(plan_change::Completion::Authorised);
        let replaced = match (&sub, completes) {
            (Some(s), Some(plan_change::Completion::Mandate)) if !already_ending => {
                s.razorpay_subscription_id.clone()
            }
            (Some(s), Some(plan_change::Completion::Authorised)) => {
                s.razorpay_subscription_id.clone()
            }
            _ => None,
        };
        let replaced_details = replaced.as_ref().zip(sub.as_ref()).map(|(rid, s)| {
            json!({
                "razorpay_subscription_id": rid,
                "branch_id": s.branch_id,
            })
            .to_string()
        });
        // What Razorpay charged for a prorated change, against what the cloud computed.
        let charge_total = match (&open_change, completes) {
            (
                Some(change),
                Some(plan_change::Completion::Prorated | plan_change::Completion::Mandate),
            ) if change.subject == ChangeSubject::Plan
                || (change.subject == ChangeSubject::Cycle && change.kind == ChangeKind::Now) =>
            {
                Some(ev.amount_minor.unwrap_or(change.charge_minor))
            }
            _ => None,
        };
        let mismatch = match (&open_change, charge_total) {
            (Some(change), Some(charged)) if charged != change.charge_minor => Some(
                json!({
                    "subscription_id": change.subscription_id,
                    "change_id": change.id,
                    "computed_minor": change.charge_minor,
                    "charged_minor": charged,
                })
                .to_string(),
            ),
            _ => None,
        };
        if let Some(m) = &mismatch {
            tracing::error!(detail = %m, "Razorpay charged another amount than the cloud computed for a plan change");
        }
        let record_id = new_id();
        let record = EventRecord {
            id: &record_id,
            razorpay_event_id: event_id,
            kind: &ev.kind,
            subscription_id: sub.as_ref().map(|s| s.id.as_str()),
            payload_hash,
            source: source.as_str(),
            received_at: now,
        };
        let history_id = new_id();
        let task_id = new_id();
        let addons = effective
            .as_ref()
            .or(sub.as_ref())
            .map(|s| json!(addons_of(s)).to_string())
            .unwrap_or_default();
        let task_details = sub.as_ref().map(|s| {
            json!({
                "branch_id": s.branch_id,
                "tenant_id": s.tenant_id,
                "band": s.band.as_str(),
                "payment_country": decision.as_ref().and_then(|d| d.payment_country.clone()),
                "razorpay_subscription_id": s.razorpay_subscription_id,
            })
            .to_string()
        });
        let change = match (&sub, &decision) {
            (Some(s), Some(d)) => Some(SubscriptionChange {
                subscription_id: &s.id,
                expect_rev: s.rev,
                old_state: s.state,
                new_state: d.new_state,
                period_end: d
                    .period_end
                    .map(|p| p.max(s.current_period_end.unwrap_or(p))),
                first_paid_at: d.first_paid_at,
                payment_country: d.payment_country.as_deref(),
                mandate_method: d.mandate,
                mandate_detail: d.mandate_detail.as_deref(),
                entitlement: d.issue_entitlement.then(|| EntitlementWrite {
                    tier: effective.as_ref().unwrap_or(s).tier,
                    addons_json: &addons,
                }),
                history_id: &history_id,
                cause: event_id,
                ops_task: if d.review {
                    Some(NewOpsTask {
                        id: &task_id,
                        kind: TASK_REVIEW,
                        target: &s.id,
                        details_json: task_details.as_deref().unwrap_or("{}"),
                    })
                } else if let Some(details) = &replaced_details {
                    Some(NewOpsTask {
                        id: &task_id,
                        kind: TASK_CANCEL_SUBSCRIPTION,
                        target: &s.id,
                        details_json: details,
                    })
                } else {
                    mismatch.as_deref().map(|details| NewOpsTask {
                        id: &task_id,
                        kind: plan_change::TASK_PRORATION_MISMATCH,
                        target: &s.id,
                        details_json: details,
                    })
                },
                audit: None,
                close_task_kind: None,
                plan_change: open_change
                    .as_ref()
                    .filter(|_| completes.is_some())
                    .map(|ch| AppliedPlanChange {
                        change_id: &ch.id,
                        tier: ch.tier,
                        addon_devices_20: ch.addon_devices_20,
                        addon_tunnel: ch.addon_tunnel,
                        addon_owner_login: ch.addon_owner_login,
                        cycle: ch.cycle,
                        authorise_only: retire_at_cycle_end,
                        razorpay_subscription_id: held
                            .as_ref()
                            .and_then(|h| h.razorpay_subscription_id.as_deref()),
                    }),
            }),
            _ => None,
        };
        // A charge is invoiced in the same transaction. A price the subscription's book lacks, or
        // a seller that is misconfigured, fails the call before anything is recorded, so the
        // webhook is retried. A prorated charge is invoiced for what was charged.
        let invoice = match (&sub, &decision, &change) {
            (Some(s), Some(d), Some(c)) if d.charged => {
                let billed = effective.as_ref().unwrap_or(s);
                let period_end = c.period_end.or(s.current_period_end);
                match (completes, charge_total) {
                    (
                        Some(plan_change::Completion::Prorated | plan_change::Completion::Mandate),
                        Some(total),
                    ) => {
                        if total > 0 {
                            let (amount, tax) = plan_change::split_total(total, s.band);
                            let line = if subject == Some(ChangeSubject::Cycle) {
                                plan_change::cycle_line(billed.tier)
                            } else {
                                plan_change::proration_line(billed.tier)
                            };
                            Some(
                                crate::invoice::for_proration(
                                    db, billed, &line, amount, tax, period_end, now,
                                )
                                .await?,
                            )
                        } else {
                            None
                        }
                    }
                    _ => Some(crate::invoice::for_charge(db, billed, period_end, now).await?),
                }
            }
            _ => None,
        };
        match db
            .apply_billing_event(&record, change.as_ref(), invoice.as_ref(), now)
            .await?
        {
            EventOutcome::Recorded => {
                return Ok(match (&sub, &decision) {
                    (Some(s), Some(d)) => Handled::Applied {
                        subscription_id: s.id.clone(),
                        branch_id: s.branch_id.clone(),
                        state: d.new_state,
                        review: d.review,
                        replaced_razorpay_id: replaced.clone(),
                        retire_at_cycle_end,
                    },
                    (Some(_), None) => Handled::Recorded,
                    (None, _) => Handled::Unmatched,
                });
            }
            EventOutcome::Duplicate => return Ok(Handled::Duplicate),
            EventOutcome::Conflict => {}
        }
    }
    Err(ControlError::Db(
        "the subscription kept changing while the event was applied".to_owned(),
    ))
}

/// SHA-256 of a payload, lowercase hex: what `billing_events.payload_hash` holds.
pub fn payload_hash(body: &[u8]) -> String {
    crate::auth::hex(&Sha256::digest(body))
}

/// Tells the ops channel (when `ARENA_ALERT_WEBHOOK` is set) that a payment is held for review.
/// Best effort: the ops task row is the record.
pub async fn alert_ops(subscription_id: &str) {
    let env = |k: &str| std::env::var_os(k);
    let Some(url) = crate::alert::webhook_from_env(&env) else {
        return;
    };
    let text = format!(
        "[{}] subscription {subscription_id}: the first payment does not match its price band; \
         review it with `arena-cloud ops subscription review`",
        crate::alert::env_name_from_env(&env)
    );
    if let Err(e) = crate::alert::post(&url, &text, crate::alert::ALERT_TIMEOUT).await {
        tracing::error!(detail = %e, "the review alert POST failed");
    }
}

/// Why an operator's review did not apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewError {
    Unknown(String),
    /// The subscription is not waiting for review.
    NotInReview(SubscriptionState),
    /// The subscription kept changing.
    Busy,
    Db(String),
}

/// An operator clears a review: the held subscription becomes `active` and the branch is
/// entitled, through the same write as a webhook activation, with an `ops_audit` row and the
/// review task closed in the same transaction.
pub async fn approve_review(
    db: &ControlDb,
    subscription_id: &str,
    operator: &str,
    reason: &str,
    now: i64,
) -> Result<Subscription, ReviewError> {
    let dberr = |e: ControlError| ReviewError::Db(e.to_string());
    for _ in 0..ATTEMPTS {
        let sub = db
            .subscription(subscription_id)
            .await
            .map_err(dberr)?
            .ok_or_else(|| ReviewError::Unknown(subscription_id.to_owned()))?;
        if sub.state != SubscriptionState::PendingReview {
            return Err(ReviewError::NotInReview(sub.state));
        }
        let addons = json!(addons_of(&sub)).to_string();
        let history_id = new_id();
        let audit_id = new_id();
        let cause = format!("ops:review:{operator}");
        let details = json!({
            "branch_id": sub.branch_id,
            "decision": "approve",
            "payment_country": sub.payment_country,
        })
        .to_string();
        let change = SubscriptionChange {
            subscription_id: &sub.id,
            expect_rev: sub.rev,
            old_state: sub.state,
            new_state: SubscriptionState::Active,
            period_end: None,
            first_paid_at: None,
            payment_country: None,
            mandate_method: None,
            mandate_detail: None,
            entitlement: Some(EntitlementWrite {
                tier: sub.tier,
                addons_json: &addons,
            }),
            history_id: &history_id,
            cause: &cause,
            ops_task: None,
            audit: Some(NewOpsAudit {
                id: &audit_id,
                operator,
                action: "subscription_review",
                reason,
                details_json: &details,
            }),
            close_task_kind: Some(TASK_REVIEW),
            plan_change: None,
        };
        if db
            .apply_subscription_change(&change, now)
            .await
            .map_err(dberr)?
        {
            return db
                .subscription(subscription_id)
                .await
                .map_err(dberr)?
                .ok_or_else(|| ReviewError::Unknown(subscription_id.to_owned()));
        }
    }
    Err(ReviewError::Busy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_proto::{Band, BillingCycle, Tier};

    fn sub(state: SubscriptionState) -> Subscription {
        Subscription {
            id: "s1".to_owned(),
            tenant_id: "t1".to_owned(),
            branch_id: "b1".to_owned(),
            state,
            tier: Tier::Basic,
            cycle: BillingCycle::Monthly,
            band: Band::In,
            currency: "INR".to_owned(),
            price_book_id: "pb_v1".to_owned(),
            addon_devices_20: 0,
            addon_tunnel: 0,
            addon_owner_login: 0,
            mandate_method: None,
            mandate_detail: None,
            razorpay_subscription_id: Some("sub_1".to_owned()),
            rev: 1,
            current_period_end: None,
            grace_until: None,
            first_paid_at: None,
            cancel_at_period_end: false,
            created_at: 0,
            updated_at: 0,
            payment_country: None,
        }
    }

    fn event(kind: &str, end_s: i64, origin: PaymentOrigin) -> ParsedEvent {
        ParsedEvent {
            kind: kind.to_owned(),
            razorpay_subscription_id: Some("sub_1".to_owned()),
            current_end_ms: Some(end_s * 1000),
            created_at_ms: Some(5_000),
            payment: Some(PaymentInfo {
                method: Some(MandateMethod::Card),
                origin,
                detail: None,
            }),
            amount_minor: None,
        }
    }

    fn india() -> PaymentOrigin {
        PaymentOrigin::Country("IN".to_owned())
    }

    #[test]
    fn the_signature_is_the_hmac_hex_of_the_raw_body_and_nothing_else_passes() {
        let s = WebhookSecret::new("whsec_test").unwrap();
        let body = br#"{"event":"subscription.charged"}"#;
        let sig = s.sign(body);
        assert_eq!(sig.len(), 64);
        assert!(s.verify(body, &sig));
        assert!(s.verify(body, &format!(" {} ", sig.to_uppercase())));
        assert!(!s.verify(b"{}", &sig));
        assert!(!s.verify(body, ""));
        assert!(!s.verify(body, "zz"));
        assert!(!s.verify(body, &sig[..62]));
        let other = WebhookSecret::new("whsec_other").unwrap();
        assert!(!other.verify(body, &sig));
        assert!(!format!("{s:?}").contains("whsec_test"));
        assert!(WebhookSecret::new("has space").is_err());
        assert!(WebhookSecret::new("").is_err());
    }

    #[test]
    fn the_secret_comes_from_the_environment_and_errors_never_show_it() {
        let env = |v: &'static str| move |k: &str| (k == SECRET_ENV).then(|| OsString::from(v));
        assert_eq!(WebhookSecret::from_env(&env("")).unwrap(), None);
        assert!(WebhookSecret::from_env(&env("whsec_1")).unwrap().is_some());
        let err = WebhookSecret::from_env(&env("bad secret")).unwrap_err();
        assert!(
            err.contains(SECRET_ENV) && !err.contains("bad secret"),
            "{err}"
        );
    }

    #[test]
    fn an_event_body_yields_its_subscription_period_and_payment() {
        let body = json!({
            "event": "subscription.charged",
            "created_at": 1_800_000_000,
            "payload": {
                "subscription": {"entity": {"id": "sub_9", "current_end": 1_802_592_000}},
                "payment": {"entity": {"method": "card", "card": {"international": false}}},
            }
        })
        .to_string();
        let ev = parse(body.as_bytes()).unwrap();
        assert_eq!(ev.kind, "subscription.charged");
        assert_eq!(ev.razorpay_subscription_id.as_deref(), Some("sub_9"));
        assert_eq!(ev.current_end_ms, Some(1_802_592_000_000));
        assert_eq!(ev.created_at_ms, Some(1_800_000_000_000));
        let p = ev.payment.unwrap();
        assert_eq!(p.method, Some(MandateMethod::Card));
        assert_eq!(p.origin, india());
        assert!(parse(b"nope").is_err());
        assert!(parse(br#"{"payload":{}}"#).is_err());
        let bare = parse(br#"{"event":"payment.failed"}"#).unwrap();
        assert!(bare.razorpay_subscription_id.is_none() && bare.payment.is_none());
    }

    #[test]
    fn a_remote_state_becomes_the_same_event_every_time() {
        let remote = RemoteSubscription {
            status: "active".to_owned(),
            current_end_ms: Some(9_000),
            paid_count: 1,
            payment: None,
        };
        let (id, ev) = from_remote("sub_1", &remote);
        assert_eq!(ev.kind, "subscription.charged");
        assert_eq!(from_remote("sub_1", &remote).0, id);
        let unpaid = RemoteSubscription {
            paid_count: 0,
            ..remote.clone()
        };
        assert_eq!(
            from_remote("sub_1", &unpaid).1.kind,
            "subscription.activated"
        );
        let auth = RemoteSubscription {
            status: "authenticated".to_owned(),
            ..remote
        };
        assert_eq!(
            from_remote("sub_1", &auth).1.kind,
            "subscription.authenticated"
        );
    }

    #[test]
    fn a_first_charge_activates_and_authenticated_alone_does_not() {
        let s = sub(SubscriptionState::Pending);
        assert_eq!(
            decide(&s, &event("subscription.authenticated", 100, india()), 1),
            None
        );
        let d = decide(&s, &event("subscription.charged", 100, india()), 1).unwrap();
        assert_eq!(d.new_state, SubscriptionState::Active);
        assert!(d.issue_entitlement && !d.review);
        assert_eq!(d.first_paid_at, Some(5_000));
        assert_eq!(d.period_end, Some(100_000));
        assert_eq!(d.payment_country.as_deref(), Some("IN"));
        assert_eq!(d.mandate, Some(MandateMethod::Card));
        // `activated` activates too, but dates no payment.
        let d = decide(&s, &event("subscription.activated", 100, india()), 1).unwrap();
        assert_eq!(d.new_state, SubscriptionState::Active);
        assert_eq!(d.first_paid_at, None);
    }

    #[test]
    fn a_payment_from_another_country_is_held_for_review_with_no_entitlement() {
        let s = sub(SubscriptionState::Pending);
        for origin in [
            PaymentOrigin::Country("US".to_owned()),
            PaymentOrigin::International,
        ] {
            let d = decide(&s, &event("subscription.charged", 100, origin), 1).unwrap();
            assert_eq!(d.new_state, SubscriptionState::PendingReview);
            assert!(d.review && !d.issue_entitlement);
            assert_eq!(d.first_paid_at, Some(5_000), "the money was taken");
        }
        // A US band accepts an international card and refuses a domestic Indian one.
        let mut us = sub(SubscriptionState::Pending);
        us.band = Band::Us;
        let intl = decide(
            &us,
            &event("subscription.charged", 100, PaymentOrigin::International),
            1,
        );
        assert_eq!(intl.unwrap().new_state, SubscriptionState::Active);
        let home = decide(&us, &event("subscription.charged", 100, india()), 1);
        assert_eq!(home.unwrap().new_state, SubscriptionState::PendingReview);
        // What Razorpay does not say is not held back.
        let unknown = decide(
            &s,
            &event("subscription.charged", 100, PaymentOrigin::Unknown),
            1,
        );
        assert_eq!(unknown.unwrap().new_state, SubscriptionState::Active);
    }

    #[test]
    fn a_review_is_remembered_when_a_later_event_has_no_payment() {
        let mut s = sub(SubscriptionState::Pending);
        s.payment_country = Some("US".to_owned());
        let mut ev = event("subscription.activated", 100, india());
        ev.payment = None;
        assert_eq!(
            decide(&s, &ev, 1).unwrap().new_state,
            SubscriptionState::PendingReview
        );
    }

    #[test]
    fn a_renewal_moves_the_period_forward_and_never_back() {
        let mut s = sub(SubscriptionState::Grace);
        s.current_period_end = Some(100_000);
        s.first_paid_at = Some(1);
        let renewal = decide(&s, &event("subscription.charged", 200, india()), 1).unwrap();
        assert_eq!(renewal.new_state, SubscriptionState::Active);
        assert_eq!(renewal.period_end, Some(200_000));
        assert!(!renewal.issue_entitlement);
        assert_eq!(
            decide(&s, &event("subscription.charged", 100, india()), 1),
            None
        );
        assert_eq!(
            decide(&s, &event("subscription.charged", 50, india()), 1),
            None
        );
        assert_eq!(
            decide(&s, &event("subscription.activated", 300, india()), 1),
            None
        );
        let mut active = s.clone();
        active.state = SubscriptionState::Active;
        assert_eq!(
            decide(&active, &event("subscription.charged", 100, india()), 1),
            None
        );
        for over in [
            SubscriptionState::PendingReview,
            SubscriptionState::Lapsed,
            SubscriptionState::Abandoned,
            SubscriptionState::Refunded,
        ] {
            let mut o = s.clone();
            o.state = over;
            assert_eq!(
                decide(&o, &event("subscription.charged", 900, india()), 1),
                None
            );
        }
    }

    #[test]
    fn a_charge_after_an_activation_still_dates_the_first_payment() {
        let mut s = sub(SubscriptionState::Active);
        s.current_period_end = Some(100_000);
        let d = decide(&s, &event("subscription.charged", 100, india()), 1).unwrap();
        assert_eq!(d.new_state, SubscriptionState::Active);
        assert_eq!((d.period_end, d.first_paid_at), (None, Some(5_000)));
    }

    #[test]
    fn add_ons_are_listed_by_quantity() {
        let mut s = sub(SubscriptionState::Active);
        assert!(addons_of(&s).is_empty());
        s.addon_devices_20 = 3;
        s.addon_tunnel = 1;
        assert_eq!(addons_of(&s), ["devices_20:3", "tunnel"]);
    }

    #[test]
    fn metrics_count_failures_handling_time_and_duplicates() {
        let m = WebhookMetrics::default();
        m.failure("bad signature");
        m.handled("subscription.charged", 12);
        m.handled("subscription.charged", 30);
        m.duplicate();
        assert_eq!(
            m.counts(),
            WebhookCounts {
                failures: 1,
                handled: 2,
                duplicates: 1,
                total_ms: 42,
                max_ms: 30
            }
        );
    }
}
