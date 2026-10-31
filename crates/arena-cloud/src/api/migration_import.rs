//! The Windows Hub's side of a Hub move once it holds the Android Hub's final snapshot (Story
//! 10.18): claim the epoch after the Android Hub's, and say that the imported branch is serving.
//!
//! `POST /v1/migration/claim` is the one compare-and-set of [`super::fence`] for a Hub that is not
//! yet the branch's main Hub: it is allowed only for the Windows Hub of a move whose Android Hub
//! reported the handover, and only while the fence is still the snapshot's epoch. Asking again for
//! the epoch the Hub already holds answers it again, so a restart between the claim and the
//! import loses nothing. `POST /v1/migration/imported` then makes the Windows Hub the branch's
//! main Hub and revokes the Android Hub's token, in one transaction.

use std::sync::Arc;

use arena_proto::{
    AppError, ErrorCode, HubRole, MigrationClaimRequest, MigrationClaimResponse,
    MigrationImportedRequest,
};
use arena_store::control::{MigrationClaim, MigrationFinish, RateRule};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};

use super::error::ApiError;
use super::fence::fenced_message;
use super::heartbeat::authenticate_any;
use super::json::ApiJson;
use super::signup::ceil_s;
use crate::auth::AuthState;
use crate::presign::{Epoch, MAX_SEQ};

const RATE_PREFIX: &str = "migclaim:";
const WINDOW_MS: i64 = 60 * 60 * 1000;
/// Claims and reports an hour per branch: a move happens once, with room for retries.
const HOURLY_LIMIT: u64 = 60;

fn not_ready() -> ApiError {
    AppError::new(
        ErrorCode::StepNotReady,
        "The Android Hub has not handed the branch over yet.",
    )
    .into()
}

fn fenced(current: Option<&str>) -> ApiError {
    AppError::new(
        ErrorCode::Fenced,
        match current {
            Some(fence) => fenced_message(fence),
            None => "No Hub has reported to this branch yet.".to_owned(),
        },
    )
    .into()
}

/// The epoch after `parent`, minted for `instance`.
fn next_epoch(parent: &Epoch, instance: &str) -> Result<String, ApiError> {
    let counter = parent
        .counter()
        .checked_add(1)
        .filter(|c| *c <= i64::MAX as u64)
        .ok_or_else(|| {
            ApiError::validation("fork.parent_epoch", "That counter cannot be advanced.")
        })?;
    Ok(format!("{counter}:{instance}"))
}

async fn rate(st: &AuthState, branch_id: &str, now: i64) -> Result<(), ApiError> {
    let key = format!("{RATE_PREFIX}{branch_id}");
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
    Ok(())
}

async fn claim(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<MigrationClaimRequest>,
) -> Result<Json<MigrationClaimResponse>, ApiError> {
    let now = st.now();
    let hub = authenticate_any(&st, &headers, now).await?;
    if hub.migration_id.is_none() {
        return Err(ApiError::forbidden(
            "This Hub is not the Windows Hub of a move.",
        ));
    }
    if body.instance_id != hub.hub_instance_id {
        return Err(ApiError::validation(
            "instance_id",
            "That is not this Hub's instance.",
        ));
    }
    let parent = Epoch::parse(&body.fork.parent_epoch).map_err(|_| {
        ApiError::validation(
            "fork.parent_epoch",
            "Send the parent epoch like 3:HUBINSTANCE.",
        )
    })?;
    if !(0..=MAX_SEQ).contains(&body.fork.parent_seq) {
        return Err(ApiError::validation(
            "fork.parent_seq",
            "The parent seq cannot be negative.",
        ));
    }
    let epoch = next_epoch(&parent, &hub.hub_instance_id)?;
    rate(&st, &hub.branch_id, now).await?;
    match st
        .db
        .claim_migration_fence(
            &hub.id,
            &hub.branch_id,
            parent.as_str(),
            &epoch,
            body.fork.parent_seq,
            now,
        )
        .await
        .map_err(ApiError::internal)?
    {
        MigrationClaim::Moved => {
            tracing::info!(
                hub_id = %hub.id,
                branch_id = %hub.branch_id,
                epoch = %epoch,
                "hub migration: epoch claimed"
            );
            Ok(Json(MigrationClaimResponse { epoch }))
        }
        MigrationClaim::NotReady => Err(not_ready()),
        MigrationClaim::Stale { current } => Err(fenced(current.as_deref())),
    }
}

async fn imported(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<MigrationImportedRequest>,
) -> Result<Json<MigrationClaimResponse>, ApiError> {
    let now = st.now();
    let hub = authenticate_any(&st, &headers, now).await?;
    Epoch::parse(&body.epoch)
        .map_err(|_| ApiError::validation("epoch", "Send the epoch like 4:HUBINSTANCE."))?;
    rate(&st, &hub.branch_id, now).await?;
    if hub.migration_id.is_none() {
        // Reported before (the answer was lost): the Hub is the main Hub and holds the fence.
        let fence = st
            .db
            .branch_fence_holder(&hub.branch_id)
            .await
            .map_err(ApiError::internal)?;
        return match fence {
            Some((f, holder))
                if f == body.epoch && holder == hub.id && hub.role == HubRole::Main =>
            {
                Ok(Json(MigrationClaimResponse { epoch: body.epoch }))
            }
            other => Err(fenced(other.as_ref().map(|(f, _)| f.as_str()))),
        };
    }
    match st
        .db
        .finish_hub_migration(&hub.id, &body.epoch, now)
        .await
        .map_err(ApiError::internal)?
    {
        MigrationFinish::Done => {
            tracing::info!(
                hub_id = %hub.id,
                branch_id = %hub.branch_id,
                epoch = %body.epoch,
                "hub migration done; the Android hub is revoked"
            );
            Ok(Json(MigrationClaimResponse { epoch: body.epoch }))
        }
        MigrationFinish::NotReady => Err(not_ready()),
        MigrationFinish::Fenced => {
            let current = st
                .db
                .branch_fence_epoch(&hub.branch_id)
                .await
                .map_err(ApiError::internal)?;
            Err(fenced(current.as_deref()))
        }
    }
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/migration/claim", post(claim))
        .route("/migration/imported", post(imported))
        .with_state(st)
}
