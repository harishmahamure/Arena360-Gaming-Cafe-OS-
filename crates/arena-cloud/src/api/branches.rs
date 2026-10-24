use std::sync::Arc;

use arena_proto::{AppError, BranchCodeSet, ErrorCode, SetBranchCode, valid_branch_code};
use arena_store::control::BranchCodeOutcome;
use axum::extract::State;
use axum::routing::put;
use axum::{Json, Router};

use super::account::already_set;
use super::error::ApiError;
use super::json::ApiJson;
use super::owned::{OwnedBranch, not_your_branch};
use crate::auth::AuthState;

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
        .route("/branches/{id}/code", put(set_code))
        .with_state(st)
}
