use std::sync::Arc;

use arena_proto::{
    CafeDetails, StepState, WizardBranch, WizardState, WizardStep, WizardStepInfo, WizardTenant,
    country, current_step,
};
use arena_store::control::{Branch, BranchProfile, NewTenantBranch, Tenant};
use axum::extract::{Query, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::Deserialize;

use super::error::ApiError;
use super::gate::RequireSetupComplete;
use super::json::ApiJson;
use super::owned::not_your_branch;
use crate::auth::{AuthState, valid_text};
use crate::{branch_code, gstin};

fn branch_dto(b: &Branch) -> WizardBranch {
    WizardBranch {
        id: b.id.clone(),
        name: b.name.clone(),
        city: b.city.clone(),
        country: b.country.clone(),
        currency: b.currency.clone(),
        timezone: b.timezone.clone(),
        gst_registered: b.gst_registered,
        gstin: b.gstin.clone(),
        branch_code: b.branch_code.clone(),
        code_editable: b.code_locked_at.is_none(),
    }
}

async fn first_branch(
    st: &AuthState,
    owner_id: &str,
) -> Result<Option<(Tenant, Option<Branch>)>, ApiError> {
    let Some(tenant) = st
        .db
        .tenant_of_owner(owner_id)
        .await
        .map_err(ApiError::internal)?
    else {
        return Ok(None);
    };
    let branch = st
        .db
        .branches_of_tenant(&tenant.id)
        .await
        .map_err(ApiError::internal)?
        .into_iter()
        .next();
    Ok(Some((tenant, branch)))
}

pub async fn wizard_state(
    st: &AuthState,
    owner_id: &str,
    branch_id: Option<&str>,
) -> Result<WizardState, ApiError> {
    let found = first_branch(st, owner_id).await?;
    let (tenant, branch) = match found {
        Some((t, b)) => (Some(t), b),
        None => (None, None),
    };
    let branch = match branch_id {
        Some(id) => Some(
            st.db
                .owned_branch(owner_id, id)
                .await
                .map_err(ApiError::internal)?
                .ok_or_else(not_your_branch)?,
        ),
        None => branch,
    };
    let states: Vec<(WizardStep, StepState)> = match &branch {
        Some(b) => st
            .db
            .onboarding_progress(&b.id)
            .await
            .map_err(ApiError::internal)?
            .into_iter()
            .map(|p| (p.step, p.state))
            .collect(),
        None => Vec::new(),
    };
    let steps = WizardStep::ALL
        .into_iter()
        .map(|key| WizardStepInfo {
            key,
            state: states
                .iter()
                .find(|(s, _)| *s == key)
                .map_or(StepState::Pending, |(_, state)| *state),
            can_skip: key.can_skip(),
        })
        .collect();
    Ok(WizardState {
        current: current_step(&states),
        steps,
        tenant: tenant.map(|t| WizardTenant {
            id: t.id,
            name: t.name,
        }),
        branch: branch.as_ref().map(branch_dto),
    })
}

#[derive(Debug, Deserialize)]
struct WizardQuery {
    branch_id: Option<String>,
}

async fn get_wizard(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    Query(q): Query<WizardQuery>,
) -> Result<Json<WizardState>, ApiError> {
    Ok(Json(
        wizard_state(&st, &signed.owner.id, q.branch_id.as_deref()).await?,
    ))
}

pub fn validate_cafe(body: &CafeDetails) -> Result<BranchProfile, ApiError> {
    if !valid_text(&body.cafe_name) {
        return Err(ApiError::validation(
            "cafe_name",
            "Enter your café's name (up to 80 characters).",
        ));
    }
    if !valid_text(&body.city) {
        return Err(ApiError::validation(
            "city",
            "Enter your city (up to 80 characters).",
        ));
    }
    let Some(c) = country(body.country.trim()) else {
        return Err(ApiError::validation(
            "country",
            "Choose a country from the list.",
        ));
    };
    let currency = body.currency.trim();
    if !arena_proto::currency::is_known_currency(currency) {
        return Err(ApiError::validation(
            "currency",
            "Choose a currency from the list.",
        ));
    }
    let timezone = body.timezone.trim();
    if !arena_proto::currency::is_canonical_timezone(timezone) {
        return Err(ApiError::validation(
            "timezone",
            "Choose a time zone from the list.",
        ));
    }
    if body.gst_registered && !c.gst {
        return Err(ApiError::validation(
            "gst_registered",
            "GST registration applies only in India.",
        ));
    }
    let gstin = body
        .gstin
        .as_deref()
        .map(gstin::normalize)
        .filter(|g| !g.is_empty());
    let gstin = match (body.gst_registered, gstin) {
        (true, None) => {
            return Err(ApiError::validation("gstin", "Enter your GSTIN."));
        }
        (true, Some(g)) if !gstin::valid_gstin(&g) => {
            return Err(ApiError::validation(
                "gstin",
                "That GSTIN isn't valid. Check it against your GST certificate.",
            ));
        }
        (true, Some(g)) => Some(g),
        (false, Some(_)) => {
            return Err(ApiError::validation(
                "gstin",
                "Leave GSTIN empty when the café isn't GST registered.",
            ));
        }
        (false, None) => None,
    };
    Ok(BranchProfile {
        name: body.cafe_name.trim().to_owned(),
        city: body.city.trim().to_owned(),
        country: c.code.to_owned(),
        currency: currency.to_owned(),
        timezone: timezone.to_owned(),
        gst_registered: body.gst_registered,
        gstin,
    })
}

fn new_code(profile: &BranchProfile, taken: &[String]) -> Result<String, ApiError> {
    branch_code::assign(&profile.name, &profile.city, taken)
        .ok_or_else(|| ApiError::internal("no free branch code"))
}

async fn put_cafe(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    ApiJson(body): ApiJson<CafeDetails>,
) -> Result<Json<WizardState>, ApiError> {
    let profile = validate_cafe(&body)?;
    let owner_id = &signed.owner.id;
    let now = st.now();
    if let Some(id) = body.branch_id.as_deref() {
        let branch = st
            .db
            .owned_branch(owner_id, id)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(not_your_branch)?;
        if !st
            .db
            .update_branch_profile(&branch.tenant_id, &branch.id, &profile, now)
            .await
            .map_err(ApiError::internal)?
        {
            return Err(ApiError::internal("café update changed no row"));
        }
        tracing::info!(owner_id = %owner_id, branch_id = %branch.id, "branch details updated");
        return Ok(Json(wizard_state(&st, owner_id, Some(id)).await?));
    }
    let mut existing = first_branch(&st, owner_id).await?;
    if existing.is_none() {
        let tenant_id = ulid::Ulid::generate().to_string();
        let branch_id = ulid::Ulid::generate().to_string();
        let code = new_code(&profile, &[])?;
        let created = st
            .db
            .create_tenant_with_branch(&NewTenantBranch {
                tenant_id: &tenant_id,
                owner_id,
                tenant_name: &profile.name,
                branch_id: &branch_id,
                branch_code: &code,
                profile: &profile,
                now,
            })
            .await
            .map_err(ApiError::internal)?;
        if created {
            tracing::info!(owner_id = %owner_id, tenant_id = %tenant_id, branch_id = %branch_id, "tenant and first branch created");
            return Ok(Json(wizard_state(&st, owner_id, None).await?));
        }
        existing = first_branch(&st, owner_id).await?;
    }
    if let Some((tenant, None)) = &existing {
        let taken = st
            .db
            .tenant_branch_codes(&tenant.id)
            .await
            .map_err(ApiError::internal)?;
        let code = new_code(&profile, &taken)?;
        let branch_id = ulid::Ulid::generate().to_string();
        let created = st
            .db
            .create_first_branch(&tenant.id, &profile.name, &branch_id, &code, &profile, now)
            .await
            .map_err(ApiError::internal)?;
        if created {
            tracing::info!(owner_id = %owner_id, tenant_id = %tenant.id, branch_id = %branch_id, "first branch created for an existing tenant");
            return Ok(Json(wizard_state(&st, owner_id, None).await?));
        }
        existing = first_branch(&st, owner_id).await?;
    }
    let Some((tenant, Some(branch))) = existing else {
        return Err(ApiError::internal("no tenant or branch after create"));
    };
    if !st
        .db
        .update_cafe(&tenant.id, &profile.name, &branch.id, &profile, now)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::internal("café update changed no row"));
    }
    tracing::info!(owner_id = %owner_id, branch_id = %branch.id, "café details updated");
    Ok(Json(wizard_state(&st, owner_id, None).await?))
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/wizard", get(get_wizard))
        .route("/wizard/cafe", put(put_cafe))
        .with_state(st)
}
