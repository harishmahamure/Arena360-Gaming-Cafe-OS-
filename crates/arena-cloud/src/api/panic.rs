//! A panic in a handler becomes `INTERNAL` and the server keeps serving.

use std::panic::AssertUnwindSafe;

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use futures_util::FutureExt;

use super::error::ApiError;

/// Runs the rest of the stack, catching a panic: it is logged (its message only) and answered
/// with the fixed `INTERNAL` envelope.
pub(super) async fn catch_panic(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_owned();
    match AssertUnwindSafe(next.run(req)).catch_unwind().await {
        Ok(res) => res,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "non-string panic payload".to_owned());
            ApiError::internal(format_args!("handler panicked on {method} {path}: {msg}"))
                .into_response()
        }
    }
}
