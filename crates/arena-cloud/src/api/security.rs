use std::sync::Arc;

use arena_proto::{MfaState, SmsFallback, TrustedDevice};
use axum::Router;
use axum::extract::{FromRequestParts, Path, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};

use super::error::ApiError;
use super::gate::setup_incomplete;
use super::json::ApiJson;
use super::session::{SignedIn, clear_cookies};
use super::signin::clear_trusted_cookie;
use crate::auth::AuthState;

#[derive(Debug, Clone)]
pub struct EnrolledOwner(pub SignedIn);

impl FromRequestParts<Arc<AuthState>> for EnrolledOwner {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        st: &Arc<AuthState>,
    ) -> Result<Self, Self::Rejection> {
        let signed = SignedIn::from_request_parts(parts, st).await?;
        if signed.owner.mfa_state != MfaState::Enrolled {
            return Err(setup_incomplete());
        }
        Ok(Self(signed))
    }
}

async fn sms_fallback(
    State(st): State<Arc<AuthState>>,
    EnrolledOwner(signed): EnrolledOwner,
    ApiJson(body): ApiJson<SmsFallback>,
) -> Result<StatusCode, ApiError> {
    if !st
        .db
        .set_sms_fallback(&signed.owner.id, body.enabled)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::auth_failed());
    }
    tracing::info!(owner_id = %signed.owner.id, enabled = body.enabled, "sms fallback changed");
    Ok(StatusCode::NO_CONTENT)
}

async fn devices(
    State(st): State<Arc<AuthState>>,
    EnrolledOwner(signed): EnrolledOwner,
) -> Result<axum::Json<Vec<TrustedDevice>>, ApiError> {
    let current = signed.session.device_id.as_deref();
    let list = st
        .db
        .owner_devices(&signed.owner.id, st.now())
        .await
        .map_err(ApiError::internal)?
        .into_iter()
        .map(|d| TrustedDevice {
            current: current == Some(d.id.as_str()),
            id: d.id,
            label: d.label,
            last_seen_at: d.last_seen_at,
            created_at: d.created_at,
        })
        .collect();
    Ok(axum::Json(list))
}

async fn remove_device(
    State(st): State<Arc<AuthState>>,
    EnrolledOwner(signed): EnrolledOwner,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    if !st
        .db
        .revoke_owner_device(&signed.owner.id, &id, st.now())
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::not_found());
    }
    tracing::info!(owner_id = %signed.owner.id, device_id = %id, "trusted device removed");
    Ok(StatusCode::NO_CONTENT)
}

async fn sign_out_everywhere(
    State(st): State<Arc<AuthState>>,
    EnrolledOwner(signed): EnrolledOwner,
) -> Result<Response, ApiError> {
    st.db
        .sign_out_everywhere(&signed.owner.id, st.now())
        .await
        .map_err(ApiError::internal)?;
    tracing::info!(owner_id = %signed.owner.id, "signed out everywhere");
    let mut res = clear_cookies(StatusCode::NO_CONTENT.into_response());
    clear_trusted_cookie(&mut res);
    Ok(res)
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/account/security/sms-fallback", put(sms_fallback))
        .route("/account/devices", get(devices))
        .route("/account/devices/{id}", delete(remove_device))
        .route("/account/sign-out-everywhere", post(sign_out_everywhere))
        .with_state(st)
}
