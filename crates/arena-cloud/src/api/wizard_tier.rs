use std::sync::Arc;

use arena_proto::{
    AppError, ChooseTier, ErrorCode, Tier, TierChoices, TierOption, TierUnavailable, WizardState,
};
use arena_store::control::{Branch, FreeTierOutcome};
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use super::account::already_set;
use super::error::ApiError;
use super::gate::RequireSetupComplete;
use super::json::ApiJson;
use super::owned::not_your_branch;
use super::wizard::wizard_state;
use crate::auth::AuthState;

#[derive(Debug, Deserialize)]
struct TierQuery {
    branch_id: Option<String>,
}

fn cafe_first() -> ApiError {
    ApiError::forbidden("Save your café details first.")
}

async fn target_branch(
    st: &AuthState,
    owner_id: &str,
    branch_id: Option<&str>,
) -> Result<Branch, ApiError> {
    if let Some(id) = branch_id {
        return st
            .db
            .owned_branch(owner_id, id)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(|| {
                tracing::warn!(owner_id = %owner_id, "branch outside the owner's tenant refused");
                not_your_branch()
            });
    }
    let tenant = st
        .db
        .tenant_of_owner(owner_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(cafe_first)?;
    st.db
        .branches_of_tenant(&tenant.id)
        .await
        .map_err(ApiError::internal)?
        .into_iter()
        .next()
        .ok_or_else(cafe_first)
}

pub async fn tier_choices(st: &AuthState, branch: &Branch) -> Result<TierChoices, ApiError> {
    let other_free = st
        .db
        .other_free_branch(&branch.tenant_id, &branch.id, st.now())
        .await
        .map_err(ApiError::internal)?;
    let current = st
        .db
        .entitlement(&branch.id)
        .await
        .map_err(ApiError::internal)?
        .map(|e| e.tier_at(st.now()));
    let tiers = Tier::ALL
        .into_iter()
        .map(|key| {
            let reason = match key {
                Tier::Free if other_free => Some(TierUnavailable::OneFreeBranch),
                Tier::Free => None,
                Tier::Basic | Tier::Premium if !st.billing_enabled => {
                    Some(TierUnavailable::BillingDisabled)
                }
                Tier::Basic | Tier::Premium => None,
            };
            TierOption {
                key,
                available: reason.is_none(),
                reason,
                limits: key.defaults().limits,
            }
        })
        .collect();
    Ok(TierChoices {
        tiers,
        billing_enabled: st.billing_enabled,
        current,
    })
}

async fn get_tier(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    Query(q): Query<TierQuery>,
) -> Result<Json<TierChoices>, ApiError> {
    let branch = target_branch(&st, &signed.owner.id, q.branch_id.as_deref()).await?;
    Ok(Json(tier_choices(&st, &branch).await?))
}

fn billing_disabled() -> ApiError {
    AppError::with_field(
        ErrorCode::BillingDisabled,
        "tier",
        "Paid plans are coming soon. Choose Free for now.",
    )
    .into()
}

fn one_free_branch() -> ApiError {
    AppError::with_field(
        ErrorCode::OneFreeBranch,
        "tier",
        "Your account already has a Free branch.",
    )
    .into()
}

async fn put_tier(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    ApiJson(body): ApiJson<ChooseTier>,
) -> Result<Json<WizardState>, ApiError> {
    let owner_id = &signed.owner.id;
    let branch = target_branch(&st, owner_id, body.branch_id.as_deref()).await?;
    match body.tier {
        Tier::Basic | Tier::Premium => {
            tracing::info!(branch_id = %branch.id, tier = body.tier.as_str(), "paid tier refused: billing is not available");
            return Err(billing_disabled());
        }
        Tier::Free => {}
    }
    match st
        .db
        .choose_free_tier(&branch.tenant_id, &branch.id, st.now())
        .await
        .map_err(ApiError::internal)?
    {
        FreeTierOutcome::Written => {
            tracing::info!(owner_id = %owner_id, branch_id = %branch.id, "free tier chosen");
        }
        FreeTierOutcome::AlreadyFree => {}
        FreeTierOutcome::OtherFree => return Err(one_free_branch()),
        FreeTierOutcome::OtherPlan(e) => {
            tracing::info!(branch_id = %branch.id, tier = e.tier.as_str(), source = e.source.as_str(), "free tier refused: the branch has another plan");
            return Err(already_set("This branch already has a plan."));
        }
        FreeTierOutcome::Missing => return Err(not_your_branch()),
    }
    Ok(Json(wizard_state(&st, owner_id).await?))
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/wizard/tier", get(get_tier).put(put_tier))
        .with_state(st)
}
