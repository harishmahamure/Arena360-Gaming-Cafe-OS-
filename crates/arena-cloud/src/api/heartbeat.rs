use std::sync::Arc;

use arena_proto::{
    AppError, ErrorCode, HEARTBEAT_MIN_GAP_MS, HUB_TOKEN_PREFIX, HeartbeatFence, HeartbeatRequest,
    HeartbeatResponse,
};
use arena_store::control::{Beat, HubRow, RateRule};
use axum::extract::State;
use axum::http::{HeaderMap, header::AUTHORIZATION};
use axum::routing::post;
use axum::{Json, Router};
use sha2::{Digest, Sha256};

use super::account::blocking;
use super::error::ApiError;
use super::json::ApiJson;
use super::pair::check_field;
use super::signup::ceil_s;
use crate::auth::AuthState;
use crate::passwords::{dummy_hash, verify_secret};

const TOKEN_CACHE_MS: i64 = 5 * 60 * 1000;
const RATE_PREFIX: &str = "hb:";

fn refused(code: ErrorCode, message: &str) -> ApiError {
    AppError::new(code, message).into()
}

fn invalid() -> ApiError {
    refused(ErrorCode::TokenInvalid, "This Hub's token isn't valid.")
}

pub(super) async fn authenticate(
    st: &AuthState,
    headers: &HeaderMap,
    now: i64,
) -> Result<HubRow, ApiError> {
    let token = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(invalid)?;
    let (id, secret) = token
        .strip_prefix(HUB_TOKEN_PREFIX)
        .and_then(|t| t.split_once('.'))
        .ok_or_else(invalid)?;
    let hub = st.db.hub(id).await.map_err(ApiError::internal)?;
    let key: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    let cached = st
        .hub_tokens
        .lock()
        .map_err(|_| ApiError::internal("hub token cache poisoned"))?
        .get(&key)
        .is_some_and(|until| *until > now);
    let verified = cached || {
        let hash = hub
            .as_ref()
            .map_or_else(|| dummy_hash().to_owned(), |h| h.token_hash.clone());
        let secret = secret.to_owned();
        blocking(move || Ok(verify_secret(&secret, &hash))).await? && hub.is_some()
    };
    let Some(hub) = hub.filter(|_| verified) else {
        return Err(invalid());
    };
    if !cached {
        let mut cache = st
            .hub_tokens
            .lock()
            .map_err(|_| ApiError::internal("hub token cache poisoned"))?;
        cache.retain(|_, until| *until > now);
        cache.insert(key, now + TOKEN_CACHE_MS);
    }
    if hub.revoked_at.is_some() {
        return Err(refused(
            ErrorCode::TokenRevoked,
            "This Hub is no longer paired.",
        ));
    }
    Ok(hub)
}

async fn heartbeat(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<HeartbeatRequest>,
) -> Result<Json<HeartbeatResponse>, ApiError> {
    let now = st.now();
    let hub = authenticate(&st, &headers, now).await?;
    check_field("epoch", &body.epoch)?;
    check_field("version", &body.version)?;
    let key = format!("{RATE_PREFIX}{}", hub.id);
    let rule = RateRule {
        key: &key,
        prune_prefix: RATE_PREFIX,
        bucket_ms: 1,
        window_ms: HEARTBEAT_MIN_GAP_MS - 1,
        limit: 1,
    };
    if let Err(wait_ms) = st
        .db
        .rate_reserve(&[rule], now)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::rate_limited(ceil_s(wait_ms)));
    }
    let Some(fence) = st
        .db
        .heartbeat(&Beat {
            hub_id: &hub.id,
            branch_id: &hub.branch_id,
            epoch: &body.epoch,
            version: &body.version,
            schema_version: body.schema_version,
            now,
        })
        .await
        .map_err(ApiError::internal)?
    else {
        return Err(refused(
            ErrorCode::TokenRevoked,
            "This Hub is no longer paired.",
        ));
    };
    let mut changed = hub.last_seen_at.is_none();
    if changed {
        tracing::info!(hub_id = %hub.id, branch_id = %hub.branch_id, "first hub heartbeat");
    }
    if let Some(setup) = body.setup.as_ref().filter(|s| s.valid()) {
        let json = serde_json::to_string(setup).map_err(ApiError::internal)?;
        let before = st
            .db
            .branch_setup(&hub.branch_id)
            .await
            .map_err(ApiError::internal)?;
        st.db
            .record_setup(&hub.branch_id, &json, setup.grouped(), now)
            .await
            .map_err(ApiError::internal)?;
        changed |= before.is_none_or(|(b, _)| b != json);
    }
    if changed {
        let _ = st.wizard_tx.send(hub.branch_id.clone());
    }
    let branch = st
        .db
        .branch(&hub.branch_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::internal("hub without its branch"))?;
    let entitlement = st
        .db
        .entitlement(&branch.id)
        .await
        .map_err(ApiError::internal)?;
    let claims =
        crate::licence::claims_for(&branch.tenant_id, &branch.id, entitlement.as_ref(), now);
    let licence = st.licence.sign(&claims).map_err(ApiError::internal)?;
    let pending = st
        .db
        .pending_commands(&hub.branch_id)
        .await
        .inspect_err(|e| tracing::warn!(error = %e, "heartbeat: pending commands unavailable"))
        .unwrap_or(0);
    Ok(Json(HeartbeatResponse {
        licence,
        fence: HeartbeatFence {
            confirmed: fence == body.epoch,
            epoch: fence,
        },
        cloud_time: now,
        pending_commands: Some(pending).filter(|n| *n > 0),
    }))
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/heartbeat", post(heartbeat))
        .with_state(st)
}
