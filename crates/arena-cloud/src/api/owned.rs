use std::sync::Arc;

use arena_store::control::Branch;
use axum::extract::{FromRequestParts, Path};
use axum::http::request::Parts;

use super::error::ApiError;
use super::gate::RequireSetupComplete;
use super::session::SignedIn;
use crate::auth::AuthState;

#[derive(Debug, Clone)]
pub struct OwnedBranch {
    pub signed: SignedIn,
    pub branch: Branch,
}

pub fn not_your_branch() -> ApiError {
    ApiError::forbidden("You don't have access to this branch.")
}

impl FromRequestParts<Arc<AuthState>> for OwnedBranch {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        st: &Arc<AuthState>,
    ) -> Result<Self, Self::Rejection> {
        let RequireSetupComplete(signed) =
            RequireSetupComplete::from_request_parts(parts, st).await?;
        let Path(id) = Path::<String>::from_request_parts(parts, st)
            .await
            .map_err(|_| not_your_branch())?;
        let branch = st
            .db
            .owned_branch(&signed.owner.id, &id)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(|| {
                tracing::warn!(owner_id = %signed.owner.id, "branch outside the owner's tenant refused");
                not_your_branch()
            })?;
        Ok(Self { signed, branch })
    }
}
