//! `POST /v1/branches/{id}/subscription` (Story 10.2): starts a paid subscription for a branch the
//! owner holds. The band comes from the billing country, the live price book is locked onto the
//! subscription, and the answer carries what Razorpay Checkout needs. Payment and activation arrive
//! by webhook (Story 10.3); `GET` on the same path says where the subscription stands. `POST` is
//! gated by `billing_enabled`, like the wizard's paid tiers.

use std::sync::Arc;

use arena_proto::{
    AppError, ChangeConfirmed, ChangePlan, CreateSubscription, ErrorCode, HubPlatform,
    PlanChangePreview, QuoteSubscription, SubscriptionCheckout, SubscriptionQuote,
    SubscriptionState, SubscriptionStatus,
};
use arena_store::control::Subscription;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};

use super::error::ApiError;
use super::json::ApiJson;
use super::owned::OwnedBranch;
use crate::auth::AuthState;
use crate::plan_change::{self, ChangeError};
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

/// `POST /v1/branches/{id}/subscription/quote` (Story 10.4): the checkout summary for a choice,
/// priced by the cloud for the branch's own band. Nothing is written and no provider is called.
async fn quote(
    State(st): State<Arc<AuthState>>,
    owned: OwnedBranch,
    ApiJson(body): ApiJson<QuoteSubscription>,
) -> Result<Json<SubscriptionQuote>, ApiError> {
    if !st.billing_enabled {
        return Err(billing_disabled());
    }
    let quote = subscription::make_quote(&st.db, &owned.branch, &body, st.now())
        .await
        .map_err(into_api)?;
    Ok(Json(quote))
}

/// `GET /v1/branches/{id}/subscription`: where the branch's subscription stands. 404 when the
/// branch has none that is not over. A first payment held for review says so in plain words.
async fn status(
    State(st): State<Arc<AuthState>>,
    owned: OwnedBranch,
) -> Result<Json<SubscriptionStatus>, ApiError> {
    let sub = st
        .db
        .subscription_of_branch(&owned.branch.id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(SubscriptionStatus {
        message: (sub.state == SubscriptionState::PendingReview)
            .then(|| crate::webhook::REVIEW_MESSAGE.to_owned()),
        subscription_id: sub.id,
        state: sub.state,
        tier: sub.tier,
        cycle: sub.cycle,
        band: sub.band,
        current_period_end: sub.current_period_end,
    }))
}

fn change_error(e: ChangeError) -> ApiError {
    match e {
        ChangeError::Invalid(i) => ApiError::validation(i.field, i.message),
        ChangeError::Blocked(reason) => ApiError::validation("tier", reason),
        ChangeError::Stale => AppError::new(
            ErrorCode::Stale,
            "This plan changed while you were looking. Review it and try again.",
        )
        .into(),
        ChangeError::Provider(d) => ApiError::unavailable(format!("billing provider: {d}")),
        ChangeError::Pricing(d) => ApiError::unavailable(format!("pricing: {d}")),
        ChangeError::Db(d) => ApiError::internal(d),
    }
}

/// The branch's subscription and the platform of its main Hub, for a change of plan.
async fn changing(
    st: &AuthState,
    owned: &OwnedBranch,
) -> Result<(Subscription, Option<HubPlatform>), ApiError> {
    if !st.billing_enabled {
        return Err(billing_disabled());
    }
    let sub = st
        .db
        .subscription_of_branch(&owned.branch.id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    let platform = st
        .db
        .active_hub(&owned.branch.id)
        .await
        .map_err(ApiError::internal)?
        .map(|h| h.platform);
    Ok((sub, platform))
}

/// `POST /v1/branches/{id}/subscription/change/preview` (Story 10.9): the prorated charge, the new
/// renewal amount and when the change starts. Nothing is written and no provider is called.
async fn preview_change(
    State(st): State<Arc<AuthState>>,
    owned: OwnedBranch,
    ApiJson(body): ApiJson<ChangePlan>,
) -> Result<Json<PlanChangePreview>, ApiError> {
    let (sub, platform) = changing(&st, &owned).await?;
    let plan = plan_change::plan(&st.db, &sub, platform, &body, st.now())
        .await
        .map_err(change_error)?;
    Ok(Json(plan_change::preview_of(&sub, &plan)))
}

/// `POST /v1/branches/{id}/subscription/change`: confirms a change against the `rev` the owner saw.
async fn confirm_change(
    State(st): State<Arc<AuthState>>,
    owned: OwnedBranch,
    ApiJson(body): ApiJson<ChangePlan>,
) -> Result<Json<ChangeConfirmed>, ApiError> {
    let (sub, platform) = changing(&st, &owned).await?;
    let provider = st
        .billing
        .get()
        .ok_or_else(|| ApiError::unavailable("no billing provider"))?;
    let done = plan_change::confirm(&st.db, provider, &sub, platform, &body, st.now())
        .await
        .map_err(change_error)?;
    Ok(Json(done))
}

#[derive(Debug, serde::Deserialize)]
struct KeepPlan {
    rev: i64,
}

/// `POST /v1/branches/{id}/subscription/change/withdraw`: "Keep current plan".
async fn withdraw_change(
    State(st): State<Arc<AuthState>>,
    owned: OwnedBranch,
    ApiJson(body): ApiJson<KeepPlan>,
) -> Result<StatusCode, ApiError> {
    let (sub, _) = changing(&st, &owned).await?;
    let provider = st
        .billing
        .get()
        .ok_or_else(|| ApiError::unavailable("no billing provider"))?;
    plan_change::withdraw(&st.db, provider, &sub, body.rev, st.now())
        .await
        .map_err(change_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/branches/{id}/subscription", post(create).get(status))
        .route("/branches/{id}/subscription/quote", post(quote))
        .route(
            "/branches/{id}/subscription/change/preview",
            post(preview_change),
        )
        .route("/branches/{id}/subscription/change", post(confirm_change))
        .route(
            "/branches/{id}/subscription/change/withdraw",
            post(withdraw_change),
        )
        .with_state(st)
}
