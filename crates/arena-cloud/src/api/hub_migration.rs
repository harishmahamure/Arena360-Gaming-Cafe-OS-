//! The owner's side of moving a branch from its Android Hub to a Windows Hub (Story 10.16): start
//! the move (a `migrate` pairing code for the Windows install), get a fresh code while the move is
//! still waiting for its PC, and cancel it before the handover.

use std::sync::Arc;

use arena_proto::{
    ActivePairingCode, AppError, ErrorCode, HubPlatform, MigrationPhase, OwnerMigration,
    PAIRING_CODE_TTL_MS, PairingCodeIssued, Tier,
};
use arena_store::control::{HubMigration, MigrationStart, MigrationState, NewMigration, RateRule};
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};

use super::branches::tier_of;
use super::error::ApiError;
use super::owned::OwnedBranch;
use crate::auth::AuthState;
use crate::pairing_code::new_pairing_code;

const ISSUE_ATTEMPTS: usize = 5;
const ISSUE_PREFIX: &str = "migrateissue:";
const ISSUES_PER_HOUR: u64 = 30;
const HOUR_MS: i64 = 60 * 60 * 1000;

/// The move as the owner reads it on Account -> Branches; `None` when the branch has none.
pub(super) async fn migration_of(
    st: &AuthState,
    branch_id: &str,
    now: i64,
) -> Result<Option<OwnerMigration>, ApiError> {
    let Some(m) = st
        .db
        .live_hub_migration(branch_id)
        .await
        .map_err(ApiError::internal)?
    else {
        return Ok(None);
    };
    Ok(Some(match m.state {
        MigrationState::AwaitingWindows => OwnerMigration {
            phase: MigrationPhase::AwaitingWindows,
            target_hub_name: None,
            code: st
                .db
                .active_migration_code(branch_id, now)
                .await
                .map_err(ApiError::internal)?
                .map(|expires_at| ActivePairingCode { expires_at }),
        },
        _ => OwnerMigration {
            phase: MigrationPhase::AwaitingHandover,
            target_hub_name: m.target_hub_name,
            code: None,
        },
    }))
}

fn in_progress(m: &HubMigration) -> ApiError {
    AppError::new(
        ErrorCode::HubMigrationActive,
        match &m.target_hub_name {
            Some(name) => {
                format!("{name} has paired. Confirm the move on the tablet, or cancel it.")
            }
            None => "The move is already under way. Confirm it on the tablet, or cancel it.".into(),
        },
    )
    .into()
}

async fn start(
    State(st): State<Arc<AuthState>>,
    owned: OwnedBranch,
) -> Result<impl IntoResponse, ApiError> {
    let branch = &owned.branch;
    let now = st.now();
    if tier_of(&st, &branch.id).await? == Tier::Free {
        return Err(AppError::new(
            ErrorCode::TierLocked,
            "Moving to a Windows Hub is available on Basic and Premium.",
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
                "Pair the Android Hub first.",
            ))
        })?;
    if hub.platform != HubPlatform::Android {
        return Err(AppError::new(
            ErrorCode::PlatformUnsupported,
            "This branch already runs on a Windows Hub.",
        )
        .into());
    }
    let key = format!("{ISSUE_PREFIX}{}", branch.id);
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
        tracing::warn!(branch_id = %branch.id, "migrate code issue limit reached");
        return Err(ApiError::rate_limited(
            u64::try_from((wait_ms + 999) / 1000).unwrap_or(1),
        ));
    }
    let expires_at = now + PAIRING_CODE_TTL_MS;
    let migration_id = ulid::Ulid::generate().to_string();
    for _ in 0..ISSUE_ATTEMPTS {
        let code = new_pairing_code().map_err(ApiError::internal)?;
        let hash = st.pairing_code_hash(&code);
        let code_id = ulid::Ulid::generate().to_string();
        let started = st
            .db
            .start_hub_migration(&NewMigration {
                id: &migration_id,
                branch_id: &branch.id,
                from_hub_id: &hub.id,
                code_id: &code_id,
                code_hash: &hash,
                now,
                expires_at,
            })
            .await
            .map_err(ApiError::internal)?;
        match started {
            MigrationStart::Started | MigrationStart::CodeReissued => {
                tracing::info!(
                    owner_id = %owned.signed.owner.id,
                    branch_id = %branch.id,
                    from_hub_id = %hub.id,
                    pairing_code_id = %code_id,
                    "hub migration code issued"
                );
                return Ok((
                    [(header::CACHE_CONTROL, "no-store")],
                    Json(PairingCodeIssued {
                        code,
                        expires_at,
                        server_now: now,
                    }),
                ));
            }
            MigrationStart::InProgress(m) => return Err(in_progress(&m)),
            MigrationStart::HubChanged => {
                return Err(AppError::new(
                    ErrorCode::StepNotReady,
                    "Your Hub changed. Refresh and try again.",
                )
                .into());
            }
            MigrationStart::HashTaken => {
                tracing::warn!(branch_id = %branch.id, "migrate code collided; drawing another");
            }
        }
    }
    Err(ApiError::internal("no free pairing code"))
}

async fn cancel(
    State(st): State<Arc<AuthState>>,
    owned: OwnedBranch,
) -> Result<StatusCode, ApiError> {
    let branch = &owned.branch;
    match st
        .db
        .cancel_hub_migration(&branch.id, st.now())
        .await
        .map_err(ApiError::internal)?
    {
        Some(m) => {
            tracing::info!(
                owner_id = %owned.signed.owner.id,
                branch_id = %branch.id,
                migration_id = %m.id,
                "hub migration cancelled"
            );
            Ok(StatusCode::NO_CONTENT)
        }
        None => Err(AppError::new(ErrorCode::NotFound, "There is no move to cancel.").into()),
    }
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/branches/{id}/hub-migration", post(start).delete(cancel))
        .with_state(st)
}
