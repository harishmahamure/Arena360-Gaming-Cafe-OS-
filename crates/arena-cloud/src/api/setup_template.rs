//! `PUT /v1/branches/{id}/setup-template` (Story 5.37): a paid Hub keeps its setup file (the
//! Story 5.36 export) in the cloud so a second branch can start from it. Device-token auth like
//! the heartbeat; the branch must be the token's own and entitled to a paid tier.

use std::sync::Arc;

use arena_proto::{AppError, ErrorCode, SETUP_FORMAT, SETUP_MAX_BYTES, SetupTemplateStored, Tier};
use axum::body::{Body, to_bytes};
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::http::header::CONTENT_LENGTH;
use axum::routing::put;
use axum::{Json, Router};

use super::error::ApiError;
use super::heartbeat::authenticate;
use crate::auth::AuthState;

const TOO_LARGE: &str = "The setup file is larger than 1 MB.";

async fn put_template(
    State(st): State<Arc<AuthState>>,
    Path(branch_id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<SetupTemplateStored>, ApiError> {
    let now = st.now();
    let hub = authenticate(&st, &headers, now).await?;
    if hub.branch_id != branch_id {
        tracing::warn!(hub_id = %hub.id, "setup template for another branch refused");
        return Err(ApiError::forbidden(
            "This Hub can't upload for that branch.",
        ));
    }
    let tier = st
        .db
        .entitlement(&branch_id)
        .await
        .map_err(ApiError::internal)?
        .map_or(Tier::Free, |e| e.tier_at(now));
    if tier == Tier::Free {
        return Err(AppError::new(
            ErrorCode::FeatureNotEntitled,
            "Keeping a setup template in the cloud needs a paid plan.",
        )
        .into());
    }
    let Some(store) = st.templates.get() else {
        return Err(ApiError::unavailable("no object store for setup templates"));
    };
    let declared = headers
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok());
    if declared.is_some_and(|n| n > SETUP_MAX_BYTES) {
        return Err(ApiError::payload_too_large(TOO_LARGE));
    }
    let bytes = to_bytes(body, SETUP_MAX_BYTES)
        .await
        .map_err(|_| ApiError::payload_too_large(TOO_LARGE))?;
    let version = setup_version(&bytes)?;
    let stored = store
        .store(&branch_id, bytes.to_vec(), version, now)
        .await
        .map_err(|e| ApiError::unavailable(format!("setup template not stored: {e}")))?;
    tracing::info!(branch_id = %branch_id, bytes = stored.bytes, version, "setup template stored");
    Ok(Json(stored))
}

/// The format version of a setup file, or `VALIDATION_FAILED` on `file` for anything that is
/// not one. Only the envelope is read: the Hub that imports the file checks the rest.
fn setup_version(bytes: &[u8]) -> Result<u32, ApiError> {
    let not_setup = || ApiError::validation("file", "This is not an Arena 360 setup file.");
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| not_setup())?;
    if value.get("format").and_then(|f| f.as_str()) != Some(SETUP_FORMAT) {
        return Err(not_setup());
    }
    value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(not_setup)
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/branches/{id}/setup-template", put(put_template))
        .with_state(st)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_setup_file_envelope_is_accepted() {
        assert_eq!(
            setup_version(br#"{"format":"arena360-setup","version":3}"#).unwrap(),
            3
        );
        for bad in [
            &b"not json"[..],
            br#"{"format":"other","version":1}"#,
            br#"{"format":"arena360-setup"}"#,
            br#"{"format":"arena360-setup","version":-1}"#,
            br#"{"format":"arena360-setup","version":4294967296}"#,
            b"[]",
        ] {
            let e = setup_version(bad).unwrap_err();
            assert_eq!(e.body().field.as_deref(), Some("file"), "{bad:?}");
        }
    }
}
