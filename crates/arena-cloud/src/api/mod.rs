//! The `/v1` API surface and its error envelope.
//!
//! Every failure leaves as a flat [`arena_proto::AppError`] JSON body with a stable code: an
//! unknown path (`NOT_FOUND`, 404), a wrong method (`UNSUPPORTED`, 405), a body that is not
//! valid JSON or doesn't match the request (`BAD_REQUEST`, 400, or 413 over [`BODY_LIMIT`]),
//! and any internal failure or panic (`INTERNAL`, 500, a fixed message; the detail is only
//! logged).

mod dev;
mod error;
mod json;
mod panic;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};

pub use error::{ApiError, INTERNAL_MESSAGE, status_of};
pub use json::ApiJson;

/// The largest request body any `/v1` route accepts.
pub const BODY_LIMIT: usize = 64 * 1024;

/// The `/v1` routes. The dev-only routes (`/v1/dev/*`) are mounted only when `dev` is set.
fn v1(dev: bool) -> Router {
    let v1 = Router::new();
    if dev { v1.merge(dev::router()) } else { v1 }
}

/// Any error response (≥ 400) that is not already JSON, such as a rejection from an extractor
/// other than [`ApiJson`], becomes the envelope ([`ApiError::for_status`]).
async fn envelope_plain_errors(res: Response) -> Response {
    let status = res.status();
    let json = res
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"));
    if status.as_u16() < 400 || json {
        return res;
    }
    ApiError::for_status(status).into_response()
}

/// Wraps `base` (routes outside `/v1`, such as `/healthz`) and the `/v1` routes in the error
/// envelope: the `NOT_FOUND` fallback, the wrong-method mapping, the body limit, the mapping of
/// any other non-JSON error response and the panic-catching layer.
pub fn with_envelope(base: Router, dev: bool) -> Router {
    base.nest("/v1", v1(dev))
        .method_not_allowed_fallback(error::method_not_allowed)
        .fallback(error::not_found)
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(axum::middleware::map_response(envelope_plain_errors))
        .layer(axum::middleware::from_fn(panic::catch_panic))
}
