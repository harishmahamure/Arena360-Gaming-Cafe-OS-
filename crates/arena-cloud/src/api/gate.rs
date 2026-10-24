//! [`RequireSetupComplete`]: the extractor every setup-wizard route (Story 2.7 on) takes. It
//! passes only a signed-in owner who has set a password, enrolled an authenticator app and
//! confirmed saving their backup codes; anyone else gets 403 `SETUP_INCOMPLETE` (or 401
//! `AUTH_FAILED` without a session).

use std::sync::Arc;

use arena_proto::{AppError, ErrorCode, MfaState};
use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use super::error::ApiError;
use super::session::{SignedIn, backup_codes_acked};
use crate::auth::AuthState;

/// A signed-in owner whose account setup is complete.
#[derive(Debug, Clone)]
pub struct RequireSetupComplete(pub SignedIn);

/// `SETUP_INCOMPLETE`, 403.
pub fn setup_incomplete() -> ApiError {
    AppError::new(
        ErrorCode::SetupIncomplete,
        "Finish setting up your account first.",
    )
    .into()
}

impl FromRequestParts<Arc<AuthState>> for RequireSetupComplete {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        st: &Arc<AuthState>,
    ) -> Result<Self, Self::Rejection> {
        let signed = SignedIn::from_request_parts(parts, st).await?;
        let o = &signed.owner;
        if !o.has_password()
            || o.mfa_state != MfaState::Enrolled
            || !backup_codes_acked(st, &o.id).await?
        {
            return Err(setup_incomplete());
        }
        Ok(Self(signed))
    }
}
