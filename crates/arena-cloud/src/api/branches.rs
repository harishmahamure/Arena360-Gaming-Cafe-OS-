use std::sync::Arc;

use arena_proto::{
    AddBranch, AppError, BranchCodeSet, BranchHub, ErrorCode, OwnerBranch, SetBranchCode, Tier,
    UpdateBranch, valid_branch_code,
};
use arena_store::control::{Branch, BranchCodeOutcome, BranchProfile};
use axum::extract::State;
use axum::routing::{get, patch, put};
use axum::{Json, Router};
use serde_json::json;

use super::account::already_set;
use super::commands::submit_command;
use super::error::ApiError;
use super::gate::RequireSetupComplete;
use super::json::ApiJson;
use super::owned::{OwnedBranch, not_your_branch};
use crate::auth::{AuthState, valid_text};
use crate::branch_code;

async fn tier_of(st: &AuthState, branch_id: &str) -> Result<Tier, ApiError> {
    Ok(st
        .db
        .entitlement(branch_id)
        .await
        .map_err(ApiError::internal)?
        .map_or(Tier::Free, |e| e.tier_at(st.now())))
}

async fn row(st: &AuthState, b: &Branch) -> Result<OwnerBranch, ApiError> {
    let tier = tier_of(st, &b.id).await?;
    let hub = st.db.active_hub(&b.id).await.map_err(ApiError::internal)?;
    Ok(OwnerBranch {
        id: b.id.clone(),
        name: b.name.clone(),
        branch_code: b.branch_code.clone(),
        timezone: b.timezone.clone(),
        tier,
        flags: tier
            .defaults()
            .flags
            .iter()
            .map(|f| (*f).to_owned())
            .collect(),
        live: b.live_at.is_some(),
        hub: BranchHub {
            paired: hub.is_some(),
            platform: hub.as_ref().map(|h| h.platform),
            last_seen_at: hub.and_then(|h| h.last_seen_at),
        },
        waiting: st
            .db
            .settings_pending(&b.id)
            .await
            .map_err(ApiError::internal)?,
    })
}

async fn list(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
) -> Result<Json<Vec<OwnerBranch>>, ApiError> {
    let Some(tenant) = st
        .db
        .tenant_of_owner(&signed.owner.id)
        .await
        .map_err(ApiError::internal)?
    else {
        return Ok(Json(Vec::new()));
    };
    let branches = st
        .db
        .branches_of_tenant(&tenant.id)
        .await
        .map_err(ApiError::internal)?;
    let mut out = Vec::with_capacity(branches.len());
    for b in &branches {
        out.push(row(&st, b).await?);
    }
    Ok(Json(out))
}

async fn update(
    State(st): State<Arc<AuthState>>,
    owned: OwnedBranch,
    ApiJson(body): ApiJson<UpdateBranch>,
) -> Result<Json<OwnerBranch>, ApiError> {
    let b = &owned.branch;
    let name = match body.name.as_deref() {
        Some(n) if !valid_text(n) => {
            return Err(ApiError::validation(
                "name",
                "Enter a name of up to 80 characters.",
            ));
        }
        Some(n) => n.trim(),
        None => &b.name,
    };
    let timezone = match body.timezone.as_deref().map(str::trim) {
        Some(z) if !arena_proto::currency::is_canonical_timezone(z) => {
            return Err(ApiError::validation(
                "timezone",
                "Choose a time zone from the list.",
            ));
        }
        Some(z) => z,
        None => &b.timezone,
    };
    let changes = [
        ("branch.name", name, name != b.name),
        ("branch.tz", timezone, timezone != b.timezone),
    ];
    if changes.iter().all(|c| !c.2) {
        return Ok(Json(row(&st, b).await?));
    }
    if st
        .db
        .active_hub(&b.id)
        .await
        .map_err(ApiError::internal)?
        .is_some()
    {
        for (key, value, _) in changes.iter().filter(|c| c.2) {
            submit_command(
                &st,
                &b.id,
                &ulid::Ulid::generate().to_string(),
                "settings.set",
                &json!({"key": key, "value": value}),
                &owned.signed.owner.id,
            )
            .await?;
        }
    }
    if !st
        .db
        .rename_branch(&b.tenant_id, &b.id, name, timezone, st.now())
        .await
        .map_err(ApiError::internal)?
    {
        return Err(not_your_branch());
    }
    tracing::info!(owner_id = %owned.signed.owner.id, branch_id = %b.id, "branch details changed");
    let fresh = st
        .db
        .branch(&b.id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(not_your_branch)?;
    Ok(Json(row(&st, &fresh).await?))
}

async fn add(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    ApiJson(body): ApiJson<AddBranch>,
) -> Result<Json<OwnerBranch>, ApiError> {
    if !valid_text(&body.name) {
        return Err(ApiError::validation(
            "name",
            "Enter a name of up to 80 characters.",
        ));
    }
    let tenant = st
        .db
        .tenant_of_owner(&signed.owner.id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::forbidden("Save your café details first."))?;
    let branches = st
        .db
        .branches_of_tenant(&tenant.id)
        .await
        .map_err(ApiError::internal)?;
    let mut paid = false;
    for b in &branches {
        paid |= tier_of(&st, &b.id).await? != Tier::Free;
    }
    let Some(first) = branches.first().filter(|_| paid) else {
        return Err(AppError::with_field(
            ErrorCode::FeatureNotEntitled,
            "name",
            "Adding a branch is available on Basic.",
        )
        .into());
    };
    let profile = BranchProfile {
        name: body.name.trim().to_owned(),
        city: first.city.clone(),
        country: first.country.clone(),
        currency: first.currency.clone(),
        timezone: first.timezone.clone(),
        gst_registered: first.gst_registered,
        gstin: first.gstin.clone(),
    };
    let taken: Vec<String> = branches.iter().map(|b| b.branch_code.clone()).collect();
    let code = branch_code::assign(&profile.name, &profile.city, &taken)
        .ok_or_else(|| ApiError::internal("no free branch code"))?;
    let id = ulid::Ulid::generate().to_string();
    if !st
        .db
        .add_branch(&tenant.id, &id, &code, &profile, st.now())
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::internal("branch insert changed no row"));
    }
    tracing::info!(owner_id = %signed.owner.id, branch_id = %id, "branch added");
    let fresh = st
        .db
        .branch(&id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::internal("added branch not found"))?;
    Ok(Json(row(&st, &fresh).await?))
}

async fn set_code(
    State(st): State<Arc<AuthState>>,
    owned: OwnedBranch,
    ApiJson(body): ApiJson<SetBranchCode>,
) -> Result<Json<BranchCodeSet>, ApiError> {
    let code = body.code.trim().to_ascii_uppercase();
    if owned.branch.code_locked_at.is_some() {
        return Err(already_set("This branch code is locked."));
    }
    if !valid_branch_code(&code) {
        return Err(ApiError::validation(
            "code",
            "Use 3 to 6 letters or digits.",
        ));
    }
    let b = &owned.branch;
    match st
        .db
        .set_branch_code(&b.tenant_id, &b.id, &code, st.now())
        .await
        .map_err(ApiError::internal)?
    {
        BranchCodeOutcome::Updated => {
            tracing::info!(branch_id = %b.id, "branch code changed");
            Ok(Json(BranchCodeSet { branch_code: code }))
        }
        BranchCodeOutcome::Locked => Err(already_set("This branch code is locked.")),
        BranchCodeOutcome::Taken => Err(AppError::with_field(
            ErrorCode::BranchCodeTaken,
            "code",
            "Another branch already uses this code.",
        )
        .into()),
        BranchCodeOutcome::Missing => Err(not_your_branch()),
    }
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/branches", get(list).post(add))
        .route("/branches/{id}", patch(update))
        .route("/branches/{id}/code", put(set_code))
        .with_state(st)
}
