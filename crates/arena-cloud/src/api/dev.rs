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
