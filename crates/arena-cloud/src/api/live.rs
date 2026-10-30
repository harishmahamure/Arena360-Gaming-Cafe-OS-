//! Live view (Story 8.40): `GET /v1/live/{branch}` hands the owner app a branch-scoped token
//! (at most 5 minutes) and the Hub's WebSocket address through the tunnel.
//!
//! Entitlement is the signed licence's: a paid tier with the `tunnel` flag. A Free branch or
//! one without the flag is `FEATURE_NOT_ENTITLED`; a branch whose Hub is Android is
//! `PLATFORM_UNSUPPORTED`; a branch with no Hub or no ready tunnel yet is `NO_HUB` (503). The
//! token is signed with the licence key and has its own claims, so the Hub can tell it from a
//! licence; the Hub checks the branch, the expiry and its own `tunnel` flag again.

use std::sync::Arc;

use arena_proto::{
    AppError, ErrorCode, HubPlatform, LIVE_PATH, LIVE_TOKEN_TTL_S, LiveClaims, LiveGrant,
    OWNER_ALL_BRANCHES, TUNNEL_FLAG, Tier, tunnel_hostname,
};
use arena_store::control::TunnelRowState;
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};

use super::error::ApiError;
use super::gate::RequireSetupComplete;
use super::owner_read::branches_for;
use crate::auth::AuthState;
use crate::licence::claims_for;

/// `GET /v1/live/{branch}`.
async fn grant(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    Path(id): Path<String>,
) -> Result<Json<LiveGrant>, ApiError> {
    let now = st.now();
    if id == OWNER_ALL_BRANCHES {
        return Err(ApiError::validation("id", "Pick one branch."));
    }
    // Tenant, rate limit, branch ownership (NOT_FOUND) and Free (FEATURE_NOT_ENTITLED).
    let branches = branches_for(&st, &signed.owner.id, &id, now).await?;
    let branch = &branches[0];
    let entitlement = st
        .db
        .entitlement(&branch.id)
        .await
        .map_err(ApiError::internal)?;
    let claims = claims_for(&branch.tenant_id, &branch.id, entitlement.as_ref(), now);
    if claims.tier == Tier::Free || !claims.flags.iter().any(|f| f == TUNNEL_FLAG) {
        return Err(AppError::new(
            ErrorCode::FeatureNotEntitled,
            "Live view is available on Premium.",
        )
        .into());
    }
    let hub = st
        .db
        .active_hub(&branch.id)
        .await
        .map_err(ApiError::internal)?;
    let Some(hub) = hub else {
        return Err(not_ready());
    };
    if hub.platform != HubPlatform::Windows {
        return Err(AppError::new(
            ErrorCode::PlatformUnsupported,
            "Live view needs the Windows Hub.",
        )
        .into());
    }
    let tunnel = st
        .db
        .branch_tunnel(&branch.id)
        .await
        .map_err(ApiError::internal)?;
    let Some(tunnel) = tunnel.filter(|t| t.state == TunnelRowState::Ready) else {
        return Err(not_ready());
    };
    let live = LiveClaims::new(&branch.tenant_id, &branch.id, now.div_euclid(1000));
    debug_assert_eq!(live.exp - live.iat, LIVE_TOKEN_TTL_S);
    let token = st.licence.sign_live(&live).map_err(ApiError::internal)?;
    debug_assert_eq!(tunnel.hostname, tunnel_hostname(&branch.id));
    Ok(Json(LiveGrant {
        url: format!("wss://{}{LIVE_PATH}", tunnel.hostname),
        token,
        expires_at: live.exp.saturating_mul(1000),
    }))
}

fn not_ready() -> ApiError {
    AppError::new(ErrorCode::NoHub, "The branch's tunnel is not ready yet.").into()
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/live/{branch}", get(grant))
        .with_state(st)
}
