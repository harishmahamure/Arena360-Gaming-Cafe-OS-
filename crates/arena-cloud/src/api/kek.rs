//! `POST /v1/kek/escrow` (Story 9.2, AD-24): a Hub of any tier, Free included, leaves its branch
//! key wrapped for the cloud escrow public key.
//!
//! The branch and the Hub come from the device token. The cloud checks that the body is an age
//! file of a sane size (so a bare key can never be stored), stores it in `gz_control.kek_escrow`
//! and writes an `admin_audit` row in the same transaction. No endpoint returns the wrapped key
//! and the answer says only that it was stored; the one reader is the owner's key release
//! (`backup.restore`, Story 9.10), which re-wraps it for a replacement Hub.
//! `POST /v1/kek/request` and `POST /v1/kek/fetch` serve that Hub (Stories 9.9 and 9.10).

use std::sync::Arc;

use arena_proto::{
    AGE_MAGIC, AppError, ErrorCode, KEK_ESCROW_MAX_BYTES, KekEscrowRequest, KekEscrowResponse,
    KekFetchRequest, KekFetchResponse, KekRequest, KekRequestResponse, KeyRequestState,
    sanitise_pc_name, valid_age_recipient,
};
use arena_store::control::{KeyFetch, NewAdminAudit, RateRule};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use super::error::ApiError;
use super::heartbeat::{authenticate, authenticate_key_holder};
use super::json::ApiJson;
use super::signup::ceil_s;
use crate::auth::AuthState;

const RATE_PREFIX: &str = "kek:";
const WINDOW_MS: i64 = 60 * 60 * 1000;
/// Escrows an hour per branch: one per pairing, with room for retries.
const HOURLY_LIMIT: u64 = 12;

/// The wrapped key as bytes, or why it was refused.
fn parse(body: &KekEscrowRequest) -> Result<Vec<u8>, ApiError> {
    let bad = |message: &str| ApiError::validation("wrapped_kek", message);
    let bytes = STANDARD
        .decode(body.wrapped_kek.trim())
        .map_err(|_| bad("Send the wrapped key as base64."))?;
    if bytes.is_empty() || bytes.len() > KEK_ESCROW_MAX_BYTES {
        return Err(bad("That is not a wrapped branch key."));
    }
    if !bytes.starts_with(AGE_MAGIC) {
        return Err(bad("The branch key must be wrapped for the escrow key."));
    }
    Ok(bytes)
}

async fn escrow(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<KekEscrowRequest>,
) -> Result<Json<KekEscrowResponse>, ApiError> {
    let now = st.now();
    let hub = authenticate(&st, &headers, now).await?;
    let bytes = parse(&body)?;
    let key = format!("{RATE_PREFIX}{}", hub.branch_id);
    let rule = RateRule {
        key: &key,
        prune_prefix: RATE_PREFIX,
        bucket_ms: 1,
        window_ms: WINDOW_MS,
        limit: HOURLY_LIMIT,
    };
    if let Err(wait_ms) = st
        .db
        .rate_reserve(&[rule], now)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::rate_limited(ceil_s(wait_ms)));
    }
    let summary = serde_json::json!({
        "hub_id": hub.id,
        "hub_instance_id": hub.hub_instance_id,
        "bytes": bytes.len(),
    })
    .to_string();
    let id = ulid::Ulid::generate().to_string();
    let request_id = ulid::Ulid::generate().to_string();
    st.db
        .put_kek_escrow(
            &hub.branch_id,
            &hub.hub_instance_id,
            &STANDARD.encode(&bytes),
            now,
            &NewAdminAudit {
                id: &id,
                at: now,
                actor_kind: "system",
                actor_id: &hub.id,
                action: "kek.escrow.write",
                target_kind: "branch",
                target_id: &hub.branch_id,
                reason: None,
                summary: &summary,
                request_id: &request_id,
            },
        )
        .await
        .map_err(ApiError::internal)?;
    tracing::info!(hub_id = %hub.id, branch_id = %hub.branch_id, "branch key escrowed");
    Ok(Json(KekEscrowResponse { stored: true }))
}

/// Requests a Hub may make an hour: one per start and one per WAN return, with room to spare.
const REQUEST_HOURLY_LIMIT: u64 = 30;
const REQUEST_RATE_PREFIX: &str = "kekreq:";

/// `POST /v1/kek/request` (Story 9.9): a replacement Hub, still waiting to be restored, names the
/// one-time public key the owner may release the branch key to. One `pending` row per Hub
/// instance, refreshed by asking again; Story 9.10 decides it. Nothing is released here.
async fn request(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<KekRequest>,
) -> Result<Json<KekRequestResponse>, ApiError> {
    let now = st.now();
    let hub = authenticate_key_holder(&st, &headers, now).await?;
    if body.hub_instance_id != hub.hub_instance_id {
        return Err(ApiError::validation(
            "hub_instance_id",
            "That is not this Hub's instance.",
        ));
    }
    if !valid_age_recipient(&body.public_key) {
        return Err(ApiError::validation(
            "public_key",
            "Send an age public key (age1…).",
        ));
    }
    if !st
        .db
        .branch_has_history(&hub.branch_id, &hub.hub_instance_id)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::forbidden(
            "This Hub does not replace another Hub of the branch.",
        ));
    }
    let key = format!("{REQUEST_RATE_PREFIX}{}", hub.branch_id);
    let rule = RateRule {
        key: &key,
        prune_prefix: REQUEST_RATE_PREFIX,
        bucket_ms: 1,
        window_ms: WINDOW_MS,
        limit: REQUEST_HOURLY_LIMIT,
    };
    if let Err(wait_ms) = st
        .db
        .rate_reserve(&[rule], now)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::rate_limited(ceil_s(wait_ms)));
    }
    let state = st
        .db
        .put_key_request(
            &ulid::Ulid::generate().to_string(),
            &hub.branch_id,
            &hub.hub_instance_id,
            &body.public_key,
            body.pc_name
                .as_deref()
                .and_then(sanitise_pc_name)
                .as_deref(),
            now,
        )
        .await
        .map_err(ApiError::internal)?;
    tracing::info!(
        hub_id = %hub.id,
        branch_id = %hub.branch_id,
        state = state.as_str(),
        "branch key requested by a replacement hub"
    );
    Ok(Json(KekRequestResponse { state }))
}

/// Fetches an hour per Hub: the waiting Hub asks about every minute, with room to spare.
const FETCH_HOURLY_LIMIT: u64 = 120;
const FETCH_RATE_PREFIX: &str = "kekfetch:";

/// `POST /v1/kek/fetch` (Story 9.10): the waiting Hub asks how its request stands. The re-wrapped
/// key is handed out once, for an `approved` request not fetched before; asking again is
/// `KEY_ALREADY_RELEASED`. A pending, denied or expired request answers its state and no key.
async fn fetch(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<KekFetchRequest>,
) -> Result<Json<KekFetchResponse>, ApiError> {
    let now = st.now();
    let hub = authenticate_key_holder(&st, &headers, now).await?;
    if body.hub_instance_id != hub.hub_instance_id {
        return Err(ApiError::validation(
            "hub_instance_id",
            "That is not this Hub's instance.",
        ));
    }
    let key = format!("{FETCH_RATE_PREFIX}{}", hub.branch_id);
    let rule = RateRule {
        key: &key,
        prune_prefix: FETCH_RATE_PREFIX,
        bucket_ms: 1,
        window_ms: WINDOW_MS,
        limit: FETCH_HOURLY_LIMIT,
    };
    if let Err(wait_ms) = st
        .db
        .rate_reserve(&[rule], now)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::rate_limited(ceil_s(wait_ms)));
    }
    st.db
        .expire_key_requests(&hub.branch_id, now)
        .await
        .map_err(ApiError::internal)?;
    let found = st
        .db
        .fetch_key_release(&hub.branch_id, &hub.hub_instance_id, now)
        .await
        .map_err(ApiError::internal)?;
    match found {
        KeyFetch::NoRequest => Err(ApiError::not_found()),
        KeyFetch::AlreadyFetched => Err(AppError::new(
            ErrorCode::KeyAlreadyReleased,
            "The branch key was already sent to this Hub.",
        )
        .into()),
        KeyFetch::State(state) => Ok(Json(KekFetchResponse {
            state,
            wrapped_kek: None,
        })),
        KeyFetch::Released(wrapped) => {
            tracing::info!(
                hub_id = %hub.id,
                branch_id = %hub.branch_id,
                "branch key handed to the replacement hub"
            );
            Ok(Json(KekFetchResponse {
                state: KeyRequestState::Approved,
                wrapped_kek: Some(wrapped),
            }))
        }
    }
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/kek/escrow", post(escrow))
        .route("/kek/request", post(request))
        .route("/kek/fetch", post(fetch))
        .with_state(st)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(bytes: &[u8]) -> KekEscrowRequest {
        KekEscrowRequest {
            wrapped_kek: STANDARD.encode(bytes),
        }
    }

    #[test]
    fn only_a_small_age_file_in_base64_is_taken() {
        let mut file = AGE_MAGIC.to_vec();
        file.extend_from_slice(b"-> X25519 stanza\n");
        assert_eq!(parse(&req(&file)).unwrap(), file);
        for refused in [
            req(b""),
            req(&[7u8; 32]),
            req(&[AGE_MAGIC, &[0u8; KEK_ESCROW_MAX_BYTES]].concat()),
            KekEscrowRequest {
                wrapped_kek: "not base64!".into(),
            },
        ] {
            assert!(parse(&refused).is_err());
        }
    }
}
