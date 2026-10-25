use std::sync::Arc;

use arena_proto::{
    ActivePairingCode, AppError, ErrorCode, InstallHubState, IssuePairingCode, PAIRING_CODE_TTL_MS,
    PairingCodeIssued,
};
use arena_store::control::{Branch, IssueOutcome, NewPairingCode, RateRule};
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use super::error::ApiError;
use super::gate::RequireSetupComplete;
use super::json::ApiJson;
use super::wizard_tier::target_branch;
use crate::auth::AuthState;
use crate::pairing_code::new_pairing_code;

const ISSUE_ATTEMPTS: usize = 5;
const ISSUE_PREFIX: &str = "pairissue:";
const ISSUES_PER_HOUR: u64 = 30;
const HOUR_MS: i64 = 60 * 60 * 1000;

#[derive(Debug, Deserialize)]
struct InstallQuery {
    branch_id: Option<String>,
}

fn step_not_ready() -> ApiError {
    AppError::new(
        ErrorCode::StepNotReady,
        "Finish your café details and choose a plan first.",
    )
    .into()
}

async fn ready_branch(
    st: &AuthState,
    owner_id: &str,
    branch_id: Option<&str>,
) -> Result<Branch, ApiError> {
    let branch = target_branch(st, owner_id, branch_id).await?;
    if !st
        .db
        .pairing_steps_ready(&branch.id)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(step_not_ready());
    }
    Ok(branch)
}

async fn get_install_hub(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    Query(q): Query<InstallQuery>,
) -> Result<Json<InstallHubState>, ApiError> {
    let branch = ready_branch(&st, &signed.owner.id, q.branch_id.as_deref()).await?;
    let active = st
        .db
        .active_pairing_code(&branch.id, st.now())
        .await
        .map_err(ApiError::internal)?
        .map(|c| ActivePairingCode {
            expires_at: c.expires_at,
        });
    let hub_paired = st
        .db
        .active_hub(&branch.id)
        .await
        .map_err(ApiError::internal)?
        .is_some();
    Ok(Json(InstallHubState {
        active,
        hub_paired,
        server_now: st.now(),
    }))
}

async fn post_code(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    ApiJson(body): ApiJson<IssuePairingCode>,
) -> Result<impl IntoResponse, ApiError> {
    let branch = ready_branch(&st, &signed.owner.id, body.branch_id.as_deref()).await?;
    let now = st.now();
    let key = format!("{ISSUE_PREFIX}{}", branch.id);
    let rule = RateRule {
        key: &key,
        prune_prefix: ISSUE_PREFIX,
        bucket_ms: 60_000,
        window_ms: HOUR_MS,
        limit: ISSUES_PER_HOUR,
    };
    if let Err(wait_ms) = st
        .db
        .rate_reserve(&[rule], now)
        .await
        .map_err(ApiError::internal)?
    {
        tracing::warn!(branch_id = %branch.id, "pairing code issue limit reached");
        let retry = u64::try_from((wait_ms + 999) / 1000).unwrap_or(1);
        return Err(ApiError::rate_limited(retry));
    }
    let expires_at = now + PAIRING_CODE_TTL_MS;
    for _ in 0..ISSUE_ATTEMPTS {
        let code = new_pairing_code().map_err(ApiError::internal)?;
        let hash = st.pairing_code_hash(&code);
        let id = ulid::Ulid::generate().to_string();
        let outcome = st
            .db
            .issue_pairing_code(&NewPairingCode {
                id: &id,
                branch_id: &branch.id,
                code_hash: &hash,
                now,
                expires_at,
            })
            .await
            .map_err(ApiError::internal)?;
        match outcome {
            IssueOutcome::Issued => {
                tracing::info!(owner_id = %signed.owner.id, branch_id = %branch.id, pairing_code_id = %id, "hub pairing code issued");
                return Ok((
                    [(header::CACHE_CONTROL, "no-store")],
                    Json(PairingCodeIssued {
                        code,
                        expires_at,
                        server_now: now,
                    }),
                ));
            }
            IssueOutcome::NotReady => return Err(step_not_ready()),
            IssueOutcome::HashTaken => {
                tracing::warn!(branch_id = %branch.id, "pairing code collided; drawing another");
            }
        }
    }
    Err(ApiError::internal("no free pairing code"))
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/wizard/install-hub", get(get_install_hub))
        .route("/wizard/install-hub/code", post(post_code))
        .with_state(st)
}
