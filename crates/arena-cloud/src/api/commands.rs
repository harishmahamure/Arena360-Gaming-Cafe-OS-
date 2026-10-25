use std::sync::Arc;

use arena_proto::{
    AppError, COMMAND_PAYLOAD_MAX, COMMAND_SCHEMA_VERSION, COMMANDS_PAGE, CommandResult,
    CommandResultAck, CommandWire, CommandsPage, ErrorCode, SETTINGS_COMMAND_KEYS,
    is_setup_command,
};
use arena_store::control::{CommandRow, NewCommand};
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use super::error::ApiError;
use super::heartbeat::authenticate;
use super::json::ApiJson;
use super::pair::check_field;
use crate::auth::AuthState;

pub async fn submit_command(
    st: &AuthState,
    branch_id: &str,
    idempotency_key: &str,
    kind: &str,
    payload: &serde_json::Value,
    issued_by: &str,
) -> Result<CommandRow, ApiError> {
    if !is_setup_command(kind) {
        return Err(AppError::with_field(
            ErrorCode::Unsupported,
            "kind",
            "This change can't be sent to the branch yet.",
        )
        .into());
    }
    if ulid::Ulid::from_string(idempotency_key).is_err() {
        return Err(ApiError::validation(
            "idempotency_key",
            "The key must be a ULID.",
        ));
    }
    let Some(fields) = payload.as_object() else {
        return Err(ApiError::validation(
            "payload",
            "Send the payload as an object.",
        ));
    };
    if kind == "settings.set"
        && !fields
            .get("key")
            .and_then(|k| k.as_str())
            .is_some_and(|k| SETTINGS_COMMAND_KEYS.contains(&k))
    {
        return Err(ApiError::validation(
            "key",
            "Only the branch name and timezone can be set from here.",
        ));
    }
    let payload = serde_json::to_string(payload).map_err(ApiError::internal)?;
    if payload.len() > COMMAND_PAYLOAD_MAX {
        return Err(ApiError::validation("payload", "The payload is too large."));
    }
    st.db
        .submit_command(&NewCommand {
            branch_id,
            idempotency_key,
            kind,
            schema_version: COMMAND_SCHEMA_VERSION,
            payload: &payload,
            issued_by,
            now: st.now(),
        })
        .await
        .map_err(ApiError::internal)
}

#[derive(Debug, Deserialize)]
struct FetchQuery {
    #[serde(default)]
    after: i64,
    epoch: String,
}

fn wire(row: CommandRow) -> Result<CommandWire, ApiError> {
    Ok(CommandWire {
        id: row.id,
        idempotency_key: row.idempotency_key,
        kind: row.kind,
        schema_version: row.schema_version,
        payload: serde_json::from_str(&row.payload).map_err(ApiError::internal)?,
        issued_by: row.issued_by,
    })
}

async fn fetch(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    Query(q): Query<FetchQuery>,
) -> Result<Json<CommandsPage>, ApiError> {
    let now = st.now();
    let hub = authenticate(&st, &headers, now).await?;
    check_field("epoch", &q.epoch)?;
    let rows = st
        .db
        .fetch_commands(&hub.branch_id, q.after.max(0), COMMANDS_PAGE, now)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(CommandsPage {
        commands: rows.into_iter().map(wire).collect::<Result<_, _>>()?,
    }))
}

async fn result(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    ApiJson(body): ApiJson<CommandResult>,
) -> Result<Json<CommandResultAck>, ApiError> {
    let now = st.now();
    let hub = authenticate(&st, &headers, now).await?;
    check_field("epoch", &body.epoch)?;
    let error = body
        .error
        .as_ref()
        .map(|e| serde_json::to_string(e).map_err(ApiError::internal))
        .transpose()?;
    let status = st
        .db
        .resolve_command(&hub.branch_id, id, body.applied_seq, error.as_deref(), now)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(CommandResultAck { status }))
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/commands", get(fetch))
        .route("/commands/{id}/result", post(result))
        .with_state(st)
}
