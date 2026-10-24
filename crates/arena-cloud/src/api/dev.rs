//! Dev-only routes (`--dev`): one per envelope failure, for the integration suite. Production
//! routing never mounts them.

use axum::Json;
use axum::Router;
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
