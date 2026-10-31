//! Full refund within 7 days of the first payment (Story 10.15, FR180, AD-19, AD-20, AD-21).
//!
//! An owner who cancels an `active` plan inside 7 days (cloud time) of `first_paid_at` is offered
//! "Cancel and get a full refund": after step-up MFA a `refund_requests` row is written and
//! nothing else moves. An Arena 360 operator then runs the audited `arena-cloud ops refund approve`
//! (or `reject`): approving refunds every payment of the subscription in full at Razorpay and
//! cancels it at once, and the request becomes `issued`. Razorpay's `refund.processed` marks each
//! payment processed and issues a credit note `A360/<FY>/CN<seq:06>` reversing the invoice that
//! charged it; when the last one lands the subscription is `refunded`, the entitlement is Free (the
//! next heartbeat delivers a Free licence) and the owner is told.
//!
//! Only the whole amount is ever refunded and a subscription has one request, ever: anything else
//! is `REFUND_NOT_ALLOWED`. The refund stays on the subscription, so partner commission (Epic 11)
//! is computed net of it.

use std::collections::BTreeMap;

use arena_proto::{OwnerAlertKind, RefundOffer, RefundStatus, RefundView, SubscriptionState};
use arena_store::control::{
    ControlDb, ControlError, EventOutcome, EventRecord, Invoice, Lifecycle, NewInvoice,
    NewRefundRequest, ProcessedRefund, REFUND_ISSUED, REFUND_PROCESSED, REFUND_REJECTED,
    REFUND_REQUESTED, RefundDecision, RefundRequest, RequestOutcome, Subscription,
    SubscriptionChange,
};

use crate::auth::AuthState;
use crate::billing::{BillingProvider, RefundResult};
use crate::invoice::{fy_of, money_with_currency};
use crate::owner_alerts::{self, Raise};
use crate::webhook::{Handled, ParsedEvent, Source};

/// The Razorpay event that confirms a refund.
pub const PROCESSED_EVENT: &str = "refund.processed";

/// The window for a full refund, from the first payment, on cloud time.
pub const WINDOW_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// How many times an event is retried when another one moves the request under it.
const ATTEMPTS: u32 = 3;

const NOT_ALLOWED: &str = "A full refund isn't available for this plan.";

/// Why a refund request was not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefundError {
    /// `REFUND_NOT_ALLOWED`, with the reason in the owner's words.
    NotAllowed(&'static str),
    /// The subscription moved since the owner looked.
    Stale,
    Db(String),
}

impl From<ControlError> for RefundError {
    fn from(e: ControlError) -> Self {
        Self::Db(e.to_string())
    }
}

/// What a refund covers: each payment of the subscription, as `(payment id, amount)`, and the
/// total.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payments {
    pub payments: Vec<(String, i64)>,
    pub total_minor: i64,
}

/// The payments of `sub` that its invoices charged. `None` when it has none, or when an invoice
/// does not name its payment (then an operator refunds by hand).
pub async fn payments_of(
    db: &ControlDb,
    sub: &Subscription,
) -> Result<Option<Payments>, ControlError> {
    let invoices = db.invoices_of_subscription(&sub.id).await?;
    let mut by_payment: BTreeMap<String, i64> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for inv in &invoices {
        let Some(payment) = inv.payment_id.clone() else {
            return Ok(None);
        };
        if !by_payment.contains_key(&payment) {
            order.push(payment.clone());
        }
        *by_payment.entry(payment).or_insert(0) += inv.total_minor;
    }
    let payments: Vec<(String, i64)> = order
        .into_iter()
        .filter_map(|p| by_payment.get(&p).map(|a| (p, *a)))
        .filter(|(_, a)| *a > 0)
        .collect();
    let total_minor = payments.iter().map(|(_, a)| a).sum();
    Ok((total_minor > 0).then_some(Payments {
        payments,
        total_minor,
    }))
}

/// Whether `now` is still inside the 7 days that began at `first_paid_at`.
pub fn in_window(first_paid_at: i64, now: i64) -> bool {
    now < first_paid_at + WINDOW_MS
}

/// The full refund `sub` offers at `now`, or `None`: it must be `active`, inside the window, have
/// no request yet and no change of plan waiting.
pub async fn offer(
    db: &ControlDb,
    sub: &Subscription,
    now: i64,
) -> Result<Option<RefundOffer>, ControlError> {
    let Some(first_paid_at) = sub.first_paid_at else {
        return Ok(None);
    };
    if sub.state != SubscriptionState::Active || !in_window(first_paid_at, now) {
        return Ok(None);
    }
    if db.refund_request_of(&sub.id).await?.is_some()
        || db.open_plan_change(&sub.id).await?.is_some()
    {
        return Ok(None);
    }
    Ok(payments_of(db, sub).await?.map(|p| RefundOffer {
        amount_minor: p.total_minor,
        currency: sub.currency.clone(),
        until: first_paid_at + WINDOW_MS,
    }))
}

/// Takes the owner's request for a full refund, against the `rev` they saw. `amount_minor`, when
/// sent, must be the whole amount offered.
pub async fn request(
    db: &ControlDb,
    sub: &Subscription,
    rev: i64,
    amount_minor: Option<i64>,
    now: i64,
) -> Result<RefundRequest, RefundError> {
    if sub.rev != rev {
        return Err(RefundError::Stale);
    }
    if sub.state != SubscriptionState::Active {
        return Err(RefundError::NotAllowed(NOT_ALLOWED));
    }
    let first_paid_at = sub
        .first_paid_at
        .ok_or(RefundError::NotAllowed(NOT_ALLOWED))?;
    if !in_window(first_paid_at, now) {
        return Err(RefundError::NotAllowed(
            "The 7 days for a full refund have passed. You can still cancel; the plan stays until the period ends.",
        ));
    }
    if db.refund_request_of(&sub.id).await?.is_some() {
        return Err(RefundError::NotAllowed(
            "A refund was already requested for this plan.",
        ));
    }
    if db.open_plan_change(&sub.id).await?.is_some() {
        return Err(RefundError::NotAllowed(
            "Keep your current plan or wait for the pending change first.",
        ));
    }
    let Some(payments) = payments_of(db, sub).await? else {
        return Err(RefundError::NotAllowed(NOT_ALLOWED));
    };
    if amount_minor.is_some_and(|a| a != payments.total_minor) {
        return Err(RefundError::NotAllowed(
            "Only a full refund of everything paid is possible.",
        ));
    }
    let id = crate::plan_change::new_id();
    let first_payment = &payments.payments[0].0;
    match db
        .create_refund_request(
            &NewRefundRequest {
                id: &id,
                subscription_id: &sub.id,
                expect_rev: sub.rev,
                tenant_id: &sub.tenant_id,
                branch_id: &sub.branch_id,
                payment_id: first_payment,
                amount_minor: payments.total_minor,
                currency: &sub.currency,
            },
            now,
        )
        .await?
    {
        RequestOutcome::Created => {}
        RequestOutcome::Stale => return Err(RefundError::Stale),
        RequestOutcome::Exists => {
            return Err(RefundError::NotAllowed(
                "A refund was already requested for this plan.",
            ));
        }
    }
    db.refund_request(&id)
        .await?
        .ok_or_else(|| RefundError::Db("the refund request vanished".to_owned()))
}

/// How a request reads to the owner.
pub fn view_of(req: &RefundRequest) -> RefundView {
    RefundView {
        status: match req.status.as_str() {
            REFUND_ISSUED => RefundStatus::Issued,
            REFUND_PROCESSED => RefundStatus::Processed,
            REFUND_REJECTED => RefundStatus::Rejected,
            _ => RefundStatus::Requested,
        },
        amount_minor: req.amount_minor,
        currency: req.currency.clone(),
        requested_at: req.requested_at,
        reason: req
            .decided_reason
            .clone()
            .filter(|_| req.status == REFUND_REJECTED),
    }
}

/// Why an operator's decision did not go through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecideError {
    Unknown(String),
    /// The request is not waiting for a decision.
    NotRequested {
        request_id: String,
        status: String,
    },
    /// An invoice of the subscription does not name its payment, so it cannot be refunded here.
    NoPayment(String),
    /// The provider refused or failed; the request stays `requested` with the error.
    Provider(String),
    /// The provider is `manual`: refund in Razorpay's dashboard.
    Manual,
    Db(String),
}

impl std::fmt::Display for DecideError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(id) => write!(f, "UNKNOWN_REFUND: no refund request {id:?}"),
            Self::NotRequested { request_id, status } => write!(
                f,
                "NOT_REQUESTED: refund request {request_id:?} is {status}, not requested"
            ),
            Self::NoPayment(n) => write!(
                f,
                "NO_PAYMENT: invoice {n} does not name its Razorpay payment; refund it by hand"
            ),
            Self::Provider(e) => write!(f, "REFUND_FAILED: {e}; the request stays requested"),
            Self::Manual => write!(
                f,
                "MANUAL_PROVIDER: billing.provider is manual; refund in the Razorpay dashboard"
            ),
            Self::Db(m) => write!(f, "CONTROL_DB_FAILED: {m}"),
        }
    }
}

impl std::error::Error for DecideError {}

impl From<ControlError> for DecideError {
    fn from(e: ControlError) -> Self {
        Self::Db(e.to_string())
    }
}

/// What an approval did, for the operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approved {
    pub request: RefundRequest,
    /// Each payment refunded, as `(payment id, amount, Razorpay refund id)`.
    pub refunds: Vec<(String, i64, String)>,
}

async fn load_requested(db: &ControlDb, request_id: &str) -> Result<RefundRequest, DecideError> {
    let req = db
        .refund_request(request_id)
        .await?
        .ok_or_else(|| DecideError::Unknown(request_id.to_owned()))?;
    if req.status != REFUND_REQUESTED {
        return Err(DecideError::NotRequested {
            request_id: request_id.to_owned(),
            status: req.status,
        });
    }
    Ok(req)
}

/// Keeps the provider's error on the request, for the operator to read, and returns it.
async fn provider_failed(db: &ControlDb, request_id: &str, error: String) -> DecideError {
    if let Err(e) = db.set_refund_error(request_id, Some(&error)).await {
        tracing::error!(%request_id, error = %e, "could not keep the refund error");
    }
    DecideError::Provider(error)
}

/// Approves a request: refunds every payment of the subscription in full, cancels it at once and
/// moves the request to `issued`. A payment already refunded by an earlier attempt is skipped, so
/// a retry after a provider error never refunds twice; the error is kept on the request.
pub async fn approve(
    db: &ControlDb,
    provider: &BillingProvider,
    request_id: &str,
    operator: &str,
    reason: &str,
    now: i64,
) -> Result<Approved, DecideError> {
    let req = load_requested(db, request_id).await?;
    let sub = db
        .subscription(&req.subscription_id)
        .await?
        .ok_or_else(|| DecideError::Db(format!("subscription {} is gone", req.subscription_id)))?;
    let payments = payments_of(db, &sub)
        .await?
        .ok_or_else(|| DecideError::NoPayment(req.subscription_id.clone()))?;
    let done = db.refund_payments(&req.id).await?;
    let mut refunds = Vec::new();
    for (payment_id, amount) in &payments.payments {
        if let Some(prior) = done.iter().find(|p| &p.payment_id == payment_id) {
            refunds.push((
                payment_id.clone(),
                prior.amount_minor,
                prior.razorpay_refund_id.clone().unwrap_or_default(),
            ));
            continue;
        }
        match provider.refund_payment(payment_id, *amount).await {
            Ok(RefundResult::Issued(refund_id)) => {
                db.record_payment_refunded(&req.id, payment_id, *amount, &refund_id)
                    .await?;
                refunds.push((payment_id.clone(), *amount, refund_id));
            }
            Ok(RefundResult::Deferred) => {
                let _ = db
                    .set_refund_error(&req.id, Some("billing.provider is manual"))
                    .await;
                return Err(DecideError::Manual);
            }
            Err(e) => return Err(provider_failed(db, &req.id, e.to_string()).await),
        }
    }
    if let Some(razorpay_id) = &sub.razorpay_subscription_id
        && let Err(e) = provider.cancel_subscription(razorpay_id).await
    {
        return Err(provider_failed(db, &req.id, e.to_string()).await);
    }
    let details = serde_json::json!({
        "request_id": req.id,
        "branch_id": req.branch_id,
        "currency": req.currency,
        "amount_minor": payments.total_minor,
        "refunds": refunds
            .iter()
            .map(|(p, a, r)| serde_json::json!({"payment_id": p, "amount_minor": a, "refund_id": r}))
            .collect::<Vec<_>>(),
    })
    .to_string();
    let audit_id = crate::plan_change::new_id();
    let decided = db
        .mark_refund_issued(
            &RefundDecision {
                request_id: &req.id,
                subscription_id: &req.subscription_id,
                operator,
                reason,
                audit_id: &audit_id,
                action: "refund.approve",
                details_json: &details,
            },
            now,
        )
        .await?;
    if !decided {
        return Err(DecideError::NotRequested {
            request_id: req.id,
            status: "decided elsewhere".to_owned(),
        });
    }
    let request = db
        .refund_request(&req.id)
        .await?
        .ok_or_else(|| DecideError::Unknown(req.id.clone()))?;
    Ok(Approved { request, refunds })
}

/// Rejects a request with a reason the owner reads; the subscription stays `active`.
pub async fn reject(
    db: &ControlDb,
    request_id: &str,
    operator: &str,
    reason: &str,
    now: i64,
) -> Result<RefundRequest, DecideError> {
    let req = load_requested(db, request_id).await?;
    let details = serde_json::json!({
        "request_id": req.id,
        "branch_id": req.branch_id,
        "currency": req.currency,
        "amount_minor": req.amount_minor,
    })
    .to_string();
    let audit_id = crate::plan_change::new_id();
    let decided = db
        .mark_refund_rejected(
            &RefundDecision {
                request_id: &req.id,
                subscription_id: &req.subscription_id,
                operator,
                reason,
                audit_id: &audit_id,
                action: "refund.reject",
                details_json: &details,
            },
            now,
        )
        .await?;
    if !decided {
        return Err(DecideError::NotRequested {
            request_id: req.id,
            status: "decided elsewhere".to_owned(),
        });
    }
    db.refund_request(&req.id)
        .await?
        .ok_or_else(|| DecideError::Unknown(req.id.clone()))
}

/// The credit note that reverses `inv`: the same lines and amounts under the next `CN` number of
/// the financial year `now` falls in.
pub fn credit_note_of(inv: &Invoice, now: i64) -> NewInvoice {
    NewInvoice {
        id: ulid::Ulid::generate().to_string(),
        fy: fy_of(now),
        tenant_id: inv.tenant_id.clone(),
        branch_id: inv.branch_id.clone(),
        subscription_id: inv.subscription_id.clone(),
        issued_at: now,
        period_start: inv.period_start,
        period_end: inv.period_end,
        band: inv.band.clone(),
        currency: inv.currency.clone(),
        lines_json: inv.lines_json.clone(),
        subtotal_minor: inv.subtotal_minor,
        cgst_minor: inv.cgst_minor,
        sgst_minor: inv.sgst_minor,
        igst_minor: inv.igst_minor,
        total_minor: inv.total_minor,
        sac: inv.sac.clone(),
        buyer_name: inv.buyer_name.clone(),
        buyer_gstin: inv.buyer_gstin.clone(),
        buyer_state_code: inv.buyer_state_code.clone(),
        seller_name: inv.seller_name.clone(),
        seller_address: inv.seller_address.clone(),
        seller_gstin: inv.seller_gstin.clone(),
        note: inv.note.clone(),
        payment_id: None,
        credit_for: Some(inv.number.clone()),
    }
}

/// Applies `refund.processed`: marks the payment processed, issues the credit note of its invoice
/// and, when it was the last payment, makes the subscription `refunded` and the branch Free, in
/// the transaction that records the event. An event for a payment no request refunded is only
/// recorded.
pub async fn handle_processed(
    db: &ControlDb,
    event_id: &str,
    ev: &ParsedEvent,
    payload_hash: &str,
    source: Source,
    now: i64,
) -> Result<Handled, ControlError> {
    for _ in 0..ATTEMPTS {
        let request = match &ev.refund {
            Some(fact) => db.refund_request_of_payment(&fact.payment_id).await?,
            None => None,
        };
        let record_id = crate::plan_change::new_id();
        let record = |subscription_id| EventRecord {
            id: &record_id,
            razorpay_event_id: event_id,
            kind: &ev.kind,
            subscription_id,
            payload_hash,
            source: source.as_str(),
            received_at: now,
        };
        let (Some(fact), Some(request)) = (&ev.refund, &request) else {
            return Ok(
                match db
                    .apply_billing_event(&record(None), None, None, now)
                    .await?
                {
                    EventOutcome::Duplicate => Handled::Duplicate,
                    _ => Handled::Unmatched,
                },
            );
        };
        let payments = db.refund_payments(&request.id).await?;
        let this = payments.iter().find(|p| p.payment_id == fact.payment_id);
        if this.is_none_or(|p| p.processed_at.is_some()) {
            // A second event for a refund already processed changes nothing.
            return Ok(
                match db
                    .apply_billing_event(&record(Some(&request.subscription_id)), None, None, now)
                    .await?
                {
                    EventOutcome::Duplicate => Handled::Duplicate,
                    _ => Handled::Recorded,
                },
            );
        }
        let Some(sub) = db.subscription(&request.subscription_id).await? else {
            return Err(ControlError::Db(format!(
                "subscription {} is gone",
                request.subscription_id
            )));
        };
        let remaining = i64::try_from(
            payments
                .iter()
                .filter(|p| p.processed_at.is_none() && p.payment_id != fact.payment_id)
                .count(),
        )
        .unwrap_or(i64::MAX);
        let note = db
            .invoice_of_payment(&fact.payment_id)
            .await?
            .map(|inv| credit_note_of(&inv, now));
        let history_id = crate::plan_change::new_id();
        let complete = (remaining == 0).then(|| SubscriptionChange {
            subscription_id: &sub.id,
            expect_rev: sub.rev,
            old_state: sub.state,
            new_state: SubscriptionState::Refunded,
            period_end: None,
            first_paid_at: None,
            payment_country: None,
            mandate_method: None,
            mandate_detail: None,
            entitlement: None,
            history_id: &history_id,
            cause: event_id,
            ops_task: None,
            audit: None,
            close_task_kind: None,
            plan_change: None,
            lifecycle: Lifecycle {
                lapse: true,
                ..Lifecycle::default()
            },
        });
        // A subscription that is already over keeps its state; only the request completes.
        let complete = complete.filter(|_| sub.state != SubscriptionState::Refunded);
        match db
            .apply_refund_processed(
                &record(Some(&sub.id)),
                &ProcessedRefund {
                    request_id: &request.id,
                    payment_id: &fact.payment_id,
                    remaining,
                },
                note.as_ref(),
                complete.as_ref(),
                now,
            )
            .await?
        {
            EventOutcome::Recorded => {
                return Ok(Handled::Refunded {
                    request_id: request.id.clone(),
                    subscription_id: sub.id.clone(),
                    branch_id: sub.branch_id.clone(),
                    complete: remaining == 0,
                });
            }
            EventOutcome::Duplicate => return Ok(Handled::Duplicate),
            EventOutcome::Conflict => {}
        }
    }
    Err(ControlError::Db(
        "the refund request kept changing while the event was applied".to_owned(),
    ))
}

/// Tells the owner "Refund of <amount> processed". Best effort, once per request (the alert id is
/// the request's).
pub async fn notify_processed(st: &AuthState, request_id: &str) {
    let req = match st.db.refund_request(request_id).await {
        Ok(Some(r)) => r,
        Ok(None) => return,
        Err(e) => {
            tracing::error!(%request_id, error = %e, "refund alert: request unreadable");
            return;
        }
    };
    let branch = match st.db.branch(&req.branch_id).await {
        Ok(Some(b)) => b,
        Ok(None) => return,
        Err(e) => {
            tracing::error!(%request_id, error = %e, "refund alert: branch unreadable");
            return;
        }
    };
    let params = BTreeMap::from([(
        "amount".to_owned(),
        money_with_currency(req.amount_minor, &req.currency),
    )]);
    let id = format!("rf-{}", req.id);
    if let Err(e) = owner_alerts::raise(
        st,
        Raise {
            id: Some(&id),
            branch: &branch,
            kind: OwnerAlertKind::RefundProcessed,
            params: &params,
            whatsapp: true,
        },
    )
    .await
    {
        tracing::warn!(%request_id, code = ?e.body().code, "the owner was not told the refund was processed");
    }
}
