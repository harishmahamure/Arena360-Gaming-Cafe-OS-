use std::sync::Arc;

use arena_proto::{
    ActivePairingCode, AppError, ErrorCode, HubPlatform, HubRole, InstallHubState,
    IssuePairingCode, PAIRING_CODE_TTL_MS, PairingCodeIssued, SetupCodeReveal, Tier,
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
    let tier = st
        .db
        .entitlement(&branch.id)
        .await
        .map_err(ApiError::internal)?
        .map_or(Tier::Free, |e| e.tier_at(st.now()));
    let hub_paired = st
        .db
        .active_hub(&branch.id)
        .await
        .map_err(ApiError::internal)?
        .is_some_and(|h| h.last_seen_at.is_some());
    Ok(Json(InstallHubState {
        active,
        hub_paired,
        tier,
        server_now: st.now(),
    }))
}

async fn post_code(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    ApiJson(body): ApiJson<IssuePairingCode>,
) -> Result<impl IntoResponse, ApiError> {
    let branch = ready_branch(&st, &signed.owner.id, body.branch_id.as_deref()).await?;
    issue(&st, &signed.owner.id, &branch, HubRole::Main).await
}

/// A single-use standby code for the branch's second PC (Story 9.18). It needs a plan with the
/// `standby` flag and a paired Windows main Hub; a branch keeps one standby (`STANDBY_EXISTS`).
async fn post_standby_code(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    ApiJson(body): ApiJson<IssuePairingCode>,
) -> Result<impl IntoResponse, ApiError> {
    let branch = ready_branch(&st, &signed.owner.id, body.branch_id.as_deref()).await?;
    let tier = st
        .db
        .entitlement(&branch.id)
        .await
        .map_err(ApiError::internal)?
        .map_or(Tier::Free, |e| e.tier_at(st.now()));
    if !tier.defaults().flags.contains(&"standby") {
        return Err(AppError::new(
            ErrorCode::TierLocked,
            "A standby PC is available on Premium.",
        )
        .into());
    }
    let hub = st
        .db
        .active_hub(&branch.id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| {
            ApiError::from(AppError::new(
                ErrorCode::StepNotReady,
                "Pair the main Hub first.",
            ))
        })?;
    if hub.platform != HubPlatform::Windows {
        return Err(AppError::new(
            ErrorCode::PlatformUnsupported,
            "A standby needs the Windows Hub.",
        )
        .into());
    }
    issue(&st, &signed.owner.id, &branch, HubRole::Standby).await
}

async fn issue(
    st: &AuthState,
    owner_id: &str,
    branch: &Branch,
    role: HubRole,
) -> Result<impl IntoResponse + use<>, ApiError> {
    let now = st.now();
    let key = format!("{ISSUE_PREFIX}{}:{}", branch.id, role.as_str());
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
                role,
                now,
                expires_at,
            })
            .await
            .map_err(ApiError::internal)?;
        match outcome {
            IssueOutcome::Issued => {
                tracing::info!(owner_id = %owner_id, branch_id = %branch.id, role = role.as_str(), pairing_code_id = %id, "hub pairing code issued");
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
            IssueOutcome::StandbyExists => {
                return Err(AppError::new(
                    ErrorCode::StandbyExists,
                    "This branch already has a standby PC.",
                )
                .into());
            }
            IssueOutcome::HashTaken => {
                tracing::warn!(branch_id = %branch.id, "pairing code collided; drawing another");
            }
        }
    }
    Err(ApiError::internal("no free pairing code"))
}

async fn post_reveal_setup_code(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    ApiJson(body): ApiJson<IssuePairingCode>,
) -> Result<impl IntoResponse, ApiError> {
    let branch = target_branch(&st, &signed.owner.id, body.branch_id.as_deref()).await?;
    let seen = st
        .db
        .active_hub(&branch.id)
        .await
        .map_err(ApiError::internal)?
        .is_some_and(|h| h.last_seen_at.is_some());
    if !seen {
        return Err(step_not_ready());
    }
    let gone = || {
        ApiError::from(AppError::new(
            ErrorCode::NotFound,
            "Your Hub sign-in code was already shown. Ask for a new one when you add staff.",
        ))
    };
    let now = st.now();
    let Some(sealed) = st
        .db
        .peek_setup_code(&branch.id, now)
        .await
        .map_err(ApiError::internal)?
    else {
        return Err(gone());
    };
    let code = st
        .mfa_key
        .open(&branch.id, &sealed.ciphertext, &sealed.nonce)
        .map_err(ApiError::internal)?;
    let code = String::from_utf8(code).map_err(ApiError::internal)?;
    if !st
        .db
        .consume_setup_code(&branch.id, now)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(gone());
    }
    tracing::info!(owner_id = %signed.owner.id, branch_id = %branch.id, "owner setup code shown");
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(SetupCodeReveal { code }),
    ))
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/wizard/install-hub", get(get_install_hub))
        .route("/wizard/install-hub/code", post(post_code))
        .route("/wizard/install-hub/standby-code", post(post_standby_code))
        .route("/wizard/setup-code/reveal", post(post_reveal_setup_code))
        .with_state(st)
}
