//! `POST /v1/branches/{id}/subscription` (Story 10.2): starts a paid subscription for a branch the
//! owner holds. The band comes from the billing country, the live price book is locked onto the
//! subscription, and the answer carries what Razorpay Checkout needs. Payment and activation arrive
//! by webhook (Story 10.3); `GET` on the same path says where the subscription stands. `POST` is
//! gated by `billing_enabled`, like the wizard's paid tiers.

use std::sync::Arc;

use arena_proto::{
    AppError, CancelPlan, ChangeConfirmed, ChangeMethod, ChangePlan, CreateSubscription,
    CycleSwitchPreview, ErrorCode, HubPlatform, PlanChangePreview, QuoteSubscription,
    RequestRefund, SubscriptionCheckout, SubscriptionQuote, SubscriptionState, SubscriptionStatus,
    SwitchCycle,
};
use arena_store::control::{REFUND_ISSUED, REFUND_REQUESTED, Subscription};
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};

use super::error::ApiError;
use super::json::ApiJson;
use super::owned::OwnedBranch;
use crate::auth::AuthState;
use crate::cancel;
use crate::cycle_change;
use crate::plan_change::{self, ChangeError};
use crate::refund::{self, RefundError};
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

/// The branch's subscription and the platform of its main Hub, for a change of plan. Nothing
/// changes while a refund is on its way: the plan is about to end.
async fn changing(
    st: &AuthState,
    owned: &OwnedBranch,
) -> Result<(Subscription, Option<HubPlatform>), ApiError> {
    let (sub, platform) = loaded(st, owned).await?;
    let refunding = st
        .db
        .refund_request_of(&sub.id)
        .await
        .map_err(ApiError::internal)?
        .is_some_and(|r| matches!(r.status.as_str(), REFUND_REQUESTED | REFUND_ISSUED));
    if refunding {
        return Err(ApiError::validation(
            "tier",
            "A refund is being processed for this plan.",
        ));
    }
    Ok((sub, platform))
}

/// The branch's subscription and the platform of its main Hub.
async fn loaded(
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

/// `POST /v1/branches/{id}/subscription/cycle/preview` (Story 10.10): the yearly price less the
/// unused days, or the day a switch to monthly starts. Nothing is written and no provider is called.
async fn preview_cycle(
    State(st): State<Arc<AuthState>>,
    owned: OwnedBranch,
    ApiJson(body): ApiJson<SwitchCycle>,
) -> Result<Json<CycleSwitchPreview>, ApiError> {
    let (sub, _) = changing(&st, &owned).await?;
    let plan = cycle_change::plan_cycle(&st.db, &sub, &body, st.now())
        .await
        .map_err(change_error)?;
    Ok(Json(cycle_change::preview_of(&sub, &plan)))
}

/// `POST /v1/branches/{id}/subscription/cycle`: confirms a switch of billing cycle against the `rev`
/// the owner saw. The answer carries the Checkout that authorises the new subscription.
async fn confirm_cycle(
    State(st): State<Arc<AuthState>>,
    owned: OwnedBranch,
    ApiJson(body): ApiJson<SwitchCycle>,
) -> Result<Json<ChangeConfirmed>, ApiError> {
    let (sub, _) = changing(&st, &owned).await?;
    let provider = st
        .billing
        .get()
        .ok_or_else(|| ApiError::unavailable("no billing provider"))?;
    let done = cycle_change::confirm_cycle(&st.db, provider, &sub, &body, st.now())
        .await
        .map_err(change_error)?;
    Ok(Json(done))
}

/// `POST /v1/branches/{id}/subscription/payment-method` (Story 10.10): starts replacing the mandate;
/// the answer carries the Checkout with the methods the current renewal amount allows.
async fn change_method(
    State(st): State<Arc<AuthState>>,
    owned: OwnedBranch,
    ApiJson(body): ApiJson<ChangeMethod>,
) -> Result<Json<ChangeConfirmed>, ApiError> {
    let (sub, _) = changing(&st, &owned).await?;
    let provider = st
        .billing
        .get()
        .ok_or_else(|| ApiError::unavailable("no billing provider"))?;
    let done = cycle_change::confirm_method(&st.db, provider, &sub, body.rev, st.now())
        .await
        .map_err(change_error)?;
    Ok(Json(done))
}

/// Cancelling asks for a fresh second factor first (Story 10.14, UX-DR112); the same proof as a
/// sensitive owner command, so the owner app reuses its step-up dialog.
fn require_stepup(owned: &OwnedBranch, now: i64) -> Result<(), ApiError> {
    if super::commands::stepup_fresh(owned.signed.session.stepup_at, now) {
        return Ok(());
    }
    Err(AppError::new(
        ErrorCode::MfaRequired,
        "Confirm it's you with a code to cancel this plan.",
    )
    .into())
}

/// `POST /v1/branches/{id}/subscription/cancel` (Story 10.14): the plan stays until the period
/// end, then the branch moves to Free. Needs a fresh step-up; answers 204.
async fn cancel_plan(
    State(st): State<Arc<AuthState>>,
    owned: OwnedBranch,
    ApiJson(body): ApiJson<CancelPlan>,
) -> Result<StatusCode, ApiError> {
    let (sub, _) = changing(&st, &owned).await?;
    let now = st.now();
    require_stepup(&owned, now)?;
    let provider = st
        .billing
        .get()
        .ok_or_else(|| ApiError::unavailable("no billing provider"))?;
    cancel::cancel(&st.db, provider, &sub, body.rev, now)
        .await
        .map_err(change_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /v1/branches/{id}/subscription/cancel/withdraw` (Story 10.14): "Keep my plan".
async fn keep_plan(
    State(st): State<Arc<AuthState>>,
    owned: OwnedBranch,
    ApiJson(body): ApiJson<CancelPlan>,
) -> Result<StatusCode, ApiError> {
    let (sub, _) = changing(&st, &owned).await?;
    let provider = st
        .billing
        .get()
        .ok_or_else(|| ApiError::unavailable("no billing provider"))?;
    cancel::keep(&st.db, provider, &sub, body.rev, st.now())
        .await
        .map_err(change_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /v1/branches/{id}/subscription/refund` (Story 10.15): "Cancel and get a full refund"
/// inside 7 days of the first payment. Needs a fresh step-up; writes the request an operator
/// approves and answers 204. After 7 days, a second request or a part of the amount it is
/// `REFUND_NOT_ALLOWED`.
async fn refund_plan(
    State(st): State<Arc<AuthState>>,
    owned: OwnedBranch,
    ApiJson(body): ApiJson<RequestRefund>,
) -> Result<StatusCode, ApiError> {
    let (sub, _) = loaded(&st, &owned).await?;
    let now = st.now();
    require_stepup(&owned, now)?;
    refund::request(&st.db, &sub, body.rev, body.amount_minor, now)
        .await
        .map_err(|e| match e {
            RefundError::NotAllowed(reason) => {
                AppError::new(ErrorCode::RefundNotAllowed, reason).into()
            }
            RefundError::Stale => change_error(ChangeError::Stale),
            RefundError::Db(d) => ApiError::internal(d),
        })?;
    Ok(StatusCode::NO_CONTENT)
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
        .route(
            "/branches/{id}/subscription/cycle/preview",
            post(preview_cycle),
        )
        .route("/branches/{id}/subscription/cycle", post(confirm_cycle))
        .route(
            "/branches/{id}/subscription/payment-method",
            post(change_method),
        )
        .route("/branches/{id}/subscription/cancel", post(cancel_plan))
        .route(
            "/branches/{id}/subscription/cancel/withdraw",
            post(keep_plan),
        )
        .route("/branches/{id}/subscription/refund", post(refund_plan))
        .with_state(st)
}
