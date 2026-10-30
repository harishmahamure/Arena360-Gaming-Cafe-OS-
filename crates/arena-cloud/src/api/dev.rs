//! Dev-only routes (`--dev`): one per envelope failure, for the integration suite. Production
//! routing never mounts them.

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};

use super::error::ApiError;
use super::json::ApiJson;

#[derive(Debug, Serialize, Deserialize)]
struct Echo {
    text: String,
}

/// `POST /v1/dev/echo` `{text}` → `{text}`.
async fn echo(ApiJson(body): ApiJson<Echo>) -> Json<Echo> {
    Json(body)
}

/// `GET /v1/dev/fail`: always `INTERNAL`.
async fn fail() -> Result<Json<Echo>, ApiError> {
    Err(ApiError::internal(
        "dev: deliberate failure (this detail stays in the log)",
    ))
}

/// `GET /v1/dev/panic`: panics.
async fn panic() -> Json<Echo> {
    panic!("dev: deliberate panic");
}

/// The dev routes, relative to `/v1`.
pub(super) fn router() -> Router {
    Router::new()
        .route("/dev/echo", post(echo))
        .route("/dev/fail", get(fail))
        .route("/dev/panic", get(panic))
}

/// `GET /v1/dev/sms`: every message the fake SMS provider recorded, oldest first.
async fn sms(
    State(st): State<Arc<crate::auth::AuthState>>,
) -> Json<Vec<crate::messaging::SentSms>> {
    Json(st.sms.fake().map(|f| f.sent()).unwrap_or_default())
}

/// The fake SMS outbox route, relative to `/v1`.
pub(super) fn sms_router(st: Arc<crate::auth::AuthState>) -> Router {
    Router::new().route("/dev/sms", get(sms)).with_state(st)
}

/// `GET /v1/dev/setup-gate`: `{"setup_complete": true}` for an owner who passes
/// [`super::gate::RequireSetupComplete`]; otherwise its rejection.
async fn setup_gate(_ok: super::gate::RequireSetupComplete) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "setup_complete": true }))
}

/// The setup-gate probe route, relative to `/v1`.
pub(super) fn gate_router(st: Arc<crate::auth::AuthState>) -> Router {
    Router::new()
        .route("/dev/setup-gate", get(setup_gate))
        .with_state(st)
}

/// `GET /v1/dev/notify`: every player message the recorded fake captured, oldest first.
async fn notify_outbox(
    State(st): State<Arc<crate::auth::AuthState>>,
) -> Json<Vec<crate::messaging::SentMessage>> {
    Json(
        st.notifier
            .get()
            .and_then(crate::messaging::Notifier::fake)
            .map(|f| f.sent())
            .unwrap_or_default(),
    )
}

/// The recorded player-message route, relative to `/v1`.
pub(super) fn notify_router(st: Arc<crate::auth::AuthState>) -> Router {
    Router::new()
        .route("/dev/notify", get(notify_outbox))
        .with_state(st)
}

/// `POST /v1/dev/owner-alerts {id?, branch_id, kind, params}`: raises an owner alert for a branch
/// (Story 8.33's source of alerts until the Hub's thresholds and the offline detector exist). An
/// unknown branch is `NOT_FOUND`, an unknown kind or a missing parameter `VALIDATION_FAILED`.
async fn raise_owner_alert(
    State(st): State<Arc<crate::auth::AuthState>>,
    ApiJson(body): ApiJson<arena_proto::DevOwnerAlert>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let kind = arena_proto::OwnerAlertKind::parse(&body.kind)
        .ok_or_else(|| ApiError::validation("kind", "This kind of alert isn't supported."))?;
    let branch = st
        .db
        .branch(&body.branch_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    let raised = crate::owner_alerts::raise(
        &st,
        crate::owner_alerts::Raise {
            id: body.id.as_deref(),
            branch: &branch,
            kind,
            params: &body.params,
        },
    )
    .await?;
    Ok(Json(
        serde_json::json!({ "id": raised.id, "created": raised.created }),
    ))
}

/// The owner-alert fixture route, relative to `/v1`.
pub(super) fn owner_alert_router(st: Arc<crate::auth::AuthState>) -> Router {
    Router::new()
        .route("/dev/owner-alerts", post(raise_owner_alert))
        .with_state(st)
}
