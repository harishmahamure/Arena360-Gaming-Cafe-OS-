//! `POST /v1/kek/escrow` (Story 9.2, AD-24): a Hub of any tier, Free included, leaves its branch
//! key wrapped for the cloud escrow public key.
//!
//! The branch and the Hub come from the device token. The cloud checks that the body is an age
//! file of a sane size (so a bare key can never be stored), stores it in `gz_control.kek_escrow`
//! and writes an `admin_audit` row in the same transaction. Nothing in this release reads the
//! wrapped key back, and the answer says only that it was stored.

use std::sync::Arc;

use arena_proto::{AGE_MAGIC, KEK_ESCROW_MAX_BYTES, KekEscrowRequest, KekEscrowResponse};
use arena_store::control::{NewAdminAudit, RateRule};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use super::error::ApiError;
use super::heartbeat::authenticate;
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

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/kek/escrow", post(escrow))
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
