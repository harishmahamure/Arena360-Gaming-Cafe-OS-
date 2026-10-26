use std::sync::Arc;

use arena_proto::{
    AppError, BranchSetup, CommandStatus, IssuePairingCode, SetupCommandRequest, SetupCommandView,
    SetupDevicesView, WizardState, WizardStep,
};
use arena_store::control::{CommandRow, SETUP_KINDS};
use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use super::commands::submit_command;
use super::error::ApiError;
use super::gate::RequireSetupComplete;
use super::json::ApiJson;
use super::wizard_staff::{ready_branch, skip};
use crate::auth::AuthState;

const HUB_ONLINE_MS: i64 = 3 * 60 * 1000;
const OPEN_WINDOW_MS: i64 = 10 * 60 * 1000;
const REJECTED_WINDOW_MS: i64 = 5 * 60 * 1000;

#[derive(Debug, Deserialize)]
struct DevicesQuery {
    branch_id: Option<String>,
}

fn command_view(r: CommandRow) -> Result<SetupCommandView, ApiError> {
    Ok(SetupCommandView {
        key: r.idempotency_key,
        kind: r.kind,
        payload: serde_json::from_str(&r.payload).map_err(ApiError::internal)?,
        status: r.status,
        error: r
            .error
            .as_deref()
            .filter(|_| r.status == CommandStatus::Rejected)
            .map(serde_json::from_str::<AppError>)
            .transpose()
            .map_err(ApiError::internal)?,
    })
}

async fn get_devices(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    Query(q): Query<DevicesQuery>,
) -> Result<Json<SetupDevicesView>, ApiError> {
    let branch = ready_branch(&st, &signed.owner.id, q.branch_id.as_deref()).await?;
    let now = st.now();
    let stored = st
        .db
        .branch_setup(&branch.id)
        .await
        .map_err(ApiError::internal)?;
    let reported_at = stored.as_ref().map(|(_, at)| *at);
    let setup = stored
        .map(|(body, _)| serde_json::from_str::<BranchSetup>(&body))
        .transpose()
        .map_err(ApiError::internal)?;
    let hub_online = st
        .db
        .active_hub(&branch.id)
        .await
        .map_err(ApiError::internal)?
        .and_then(|h| h.last_seen_at)
        .is_some_and(|at| now - at < HUB_ONLINE_MS);
    let commands = st
        .db
        .setup_commands(
            &branch.id,
            reported_at.unwrap_or(now - OPEN_WINDOW_MS),
            now - REJECTED_WINDOW_MS,
        )
        .await
        .map_err(ApiError::internal)?
        .into_iter()
        .map(command_view)
        .collect::<Result<_, _>>()?;
    Ok(Json(SetupDevicesView {
        setup,
        reported_at,
        now,
        hub_online,
        commands,
    }))
}

async fn post_command(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    ApiJson(body): ApiJson<SetupCommandRequest>,
) -> Result<Json<SetupCommandView>, ApiError> {
    if !SETUP_KINDS.contains(&body.kind.as_str()) {
        return Err(ApiError::validation(
            "kind",
            "Choose a group or device change.",
        ));
    }
    let branch = ready_branch(&st, &signed.owner.id, body.branch_id.as_deref()).await?;
    let row = submit_command(
        &st,
        &branch.id,
        &ulid::Ulid::generate().to_string(),
        &body.kind,
        &body.payload,
        &signed.owner.id,
    )
    .await?;
    tracing::info!(owner_id = %signed.owner.id, branch_id = %branch.id, kind = %body.kind, "device setup command queued");
    Ok(Json(command_view(row)?))
}

async fn skip_products(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    ApiJson(body): ApiJson<IssuePairingCode>,
) -> Result<Json<WizardState>, ApiError> {
    skip(&st, &signed.owner.id, body, WizardStep::Products).await
}

async fn skip_printer(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    ApiJson(body): ApiJson<IssuePairingCode>,
) -> Result<Json<WizardState>, ApiError> {
    skip(&st, &signed.owner.id, body, WizardStep::PrinterPayments).await
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/wizard/devices", get(get_devices).post(post_command))
        .route("/wizard/products/skip", post(skip_products))
        .route("/wizard/printer-payments/skip", post(skip_printer))
        .with_state(st)
}
