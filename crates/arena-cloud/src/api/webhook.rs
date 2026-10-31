//! `POST /v1/webhooks/razorpay` (Story 10.3): Razorpay's events end here and nowhere else. The
//! `X-Razorpay-Signature` HMAC is checked over the raw body before anything is parsed; a bad one
//! answers 400, is counted as a webhook failure and writes nothing. A verified event is recorded
//! and applied in one transaction (see [`crate::webhook`]); a repeat answers 200 and applies
//! nothing. There is no session: the signature is the credential.

use std::sync::Arc;
use std::time::Instant;

use arena_proto::SubscriptionState;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{Value, json};

use super::error::ApiError;
use crate::auth::AuthState;
use crate::lifecycle;
use crate::webhook::{self, Handled, Source};

const SIGNATURE_HEADER: &str = "x-razorpay-signature";
const EVENT_ID_HEADER: &str = "x-razorpay-event-id";

/// The longest event id Razorpay sends is well under this.
const EVENT_ID_MAX: usize = 128;

fn refuse(st: &AuthState, reason: &str, message: &'static str) -> ApiError {
    st.webhook_metrics.failure(reason);
    ApiError::bad_request(message, None)
}

async fn razorpay(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let secret = st
        .webhook_secret
        .get()
        .ok_or_else(|| ApiError::unavailable("no webhook secret"))?;
    let signature = headers
        .get(SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !secret.verify(&body, signature) {
        return Err(refuse(
            &st,
            "bad signature",
            "The signature does not match.",
        ));
    }
    let event_id = headers
        .get(EVENT_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|id| {
            !id.is_empty() && id.len() <= EVENT_ID_MAX && !id.chars().any(char::is_control)
        })
        .ok_or_else(|| refuse(&st, "no event id", "The event has no id."))?;
    let event =
        webhook::parse(&body).map_err(|e| refuse(&st, e.0, "The event could not be read."))?;
    let started = Instant::now();
    let handled = webhook::handle(
        &st.db,
        event_id,
        &event,
        &webhook::payload_hash(&body),
        Source::Webhook,
        st.now(),
    )
    .await
    .map_err(|e| {
        st.webhook_metrics.failure("apply failed");
        ApiError::internal(e)
    })?;
    let status = match &handled {
        Handled::Duplicate => {
            st.webhook_metrics.duplicate();
            "duplicate"
        }
        other => {
            st.webhook_metrics.handled(
                &event.kind,
                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            );
            match other {
                Handled::Applied {
                    subscription_id,
                    branch_id,
                    state,
                    review,
                    replaced_razorpay_id,
                    retire_at_cycle_end,
                    ..
                } => {
                    if let Some(old) = replaced_razorpay_id {
                        retire(&st, subscription_id, old, *retire_at_cycle_end);
                    }
                    // Wakes the owner's wizard stream: the activation marked the plan step done.
                    let _ = st.wizard_tx.send(branch_id.clone());
                    // A renewal that needs approval, or the first day of grace: the owner is told
                    // now (the lifecycle sweep raises the same alert if this one fails).
                    if matches!(
                        state,
                        SubscriptionState::RenewalNeedsApproval | SubscriptionState::Grace
                    ) {
                        let (st, id) = (st.clone(), subscription_id.clone());
                        tokio::spawn(async move { lifecycle::notify_subscription(&st, &id).await });
                    }
                    if *review {
                        let id = subscription_id.clone();
                        tracing::warn!(subscription_id = %id, "a first payment is held for review");
                        tokio::spawn(async move { webhook::alert_ops(&id).await });
                    }
                    "applied"
                }
                Handled::Recorded => "recorded",
                _ => "unmatched",
            }
        }
    };
    Ok(Json(json!({ "status": status })))
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/webhooks/razorpay", post(razorpay))
        .with_state(st)
}

/// Cancels the Razorpay subscription a completed change replaced, so it never bills beside the new
/// one (Stories 10.9, 10.10): at once, or, for a switch to monthly the owner has authorised, with
/// the cycle it has paid for. A failure leaves the operator task the event left open.
fn retire(st: &Arc<AuthState>, subscription_id: &str, razorpay_id: &str, at_cycle_end: bool) {
    let (st, subscription_id, razorpay_id) = (
        st.clone(),
        subscription_id.to_owned(),
        razorpay_id.to_owned(),
    );
    tokio::spawn(async move {
        let Some(provider) = st.billing.get() else {
            return;
        };
        let ended = if at_cycle_end {
            provider.cancel_at_cycle_end(&razorpay_id).await
        } else {
            provider.cancel_subscription(&razorpay_id).await
        };
        match ended {
            Ok(crate::billing::CancelResult::Cancelled) => {
                if let Err(e) = st
                    .db
                    .finish_ops_tasks(
                        crate::subscription::TASK_CANCEL_SUBSCRIPTION,
                        &subscription_id,
                        st.now(),
                    )
                    .await
                {
                    tracing::error!(%subscription_id, error = %e, "could not close the cancel task");
                }
            }
            Ok(crate::billing::CancelResult::Deferred) => {}
            Err(e) => tracing::error!(
                %subscription_id,
                code = %e,
                "could not cancel the replaced Razorpay subscription; an operator must"
            ),
        }
    });
}
