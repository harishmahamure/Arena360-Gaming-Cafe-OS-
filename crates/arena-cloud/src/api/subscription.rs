//! `POST /v1/branches/{id}/subscription` (Story 10.2): starts a paid subscription for a branch the
//! owner holds. The band comes from the billing country, the live price book is locked onto the
//! subscription, and the answer carries what Razorpay Checkout needs. Paying and activation are
//! the webhook story's. Gated by `billing_enabled`, like the wizard's paid tiers.

use std::sync::Arc;

use arena_proto::{AppError, CreateSubscription, ErrorCode, SubscriptionCheckout};
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};

use super::error::ApiError;
use super::json::ApiJson;
use super::owned::OwnedBranch;
use crate::auth::AuthState;
use crate::subscription::{self, SubscriptionError};

fn billing_disabled() -> ApiError {
    AppError::with_field(
        ErrorCode::BillingDisabled,
        "tier",
        "Paid plans are coming soon. Choose Free for now.",
    )
    .into()
}

fn into_api(e: SubscriptionError) -> ApiError {
    match e {
        SubscriptionError::Invalid(i) => ApiError::validation(i.field, i.message),
        SubscriptionError::BandNotAvailable => AppError::with_field(
            ErrorCode::BandNotAvailable,
            "billing_country",
            "Paid plans aren't available in your country yet. Free is.",
        )
        .into(),
        SubscriptionError::Exists(_) => AppError::new(
            ErrorCode::SubscriptionExists,
            "This branch already has a subscription.",
        )
        .into(),
        SubscriptionError::Provider(d) => ApiError::unavailable(format!("billing provider: {d}")),
        SubscriptionError::Pricing(d) => ApiError::unavailable(format!("pricing: {d}")),
        SubscriptionError::Db(d) => ApiError::internal(d),
    }
}

async fn create(
    State(st): State<Arc<AuthState>>,
    owned: OwnedBranch,
    ApiJson(body): ApiJson<CreateSubscription>,
) -> Result<(StatusCode, Json<SubscriptionCheckout>), ApiError> {
    if !st.billing_enabled {
        return Err(billing_disabled());
    }
    let provider = st
        .billing
        .get()
        .ok_or_else(|| ApiError::unavailable("no billing provider"))?;
    let checkout = subscription::create(&st.db, provider, &owned.branch, &body, st.now())
        .await
        .map_err(into_api)?;
    Ok((StatusCode::CREATED, Json(checkout)))
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/branches/{id}/subscription", post(create))
        .with_state(st)
}
