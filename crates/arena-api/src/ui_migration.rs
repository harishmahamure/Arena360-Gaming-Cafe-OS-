//! `POST /ui/migration/join` (Story 10.17): "Waiting for your Android Hub" on the Windows Hub of a
//! Hub move takes the Android Hub's address and the 6-digit code on its screen and starts the
//! handover.
//!
//! A Hub waiting for a move has no staff and no session, so the route sits behind the loopback
//! guard only, like `/ui/pair-hub`. The code is what authorises the handover, and the Android Hub
//! limits wrong ones.

use std::sync::Arc;

use arena_core::handover::HandoverJoinControl;
use arena_proto::{ErrorCode, HandoverJoinRequest, UiError};
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};

use crate::ui_auth::{bad_frame, fail};

type Control = Arc<dyn HandoverJoinControl>;

async fn join(State(control): State<Control>, body: axum::body::Bytes) -> Response {
    let Ok(request) = serde_json::from_slice::<HandoverJoinRequest>(&body) else {
        return fail(bad_frame("the body must be {\"address\", \"code\"}"));
    };
    if request.address.trim().is_empty() {
        return fail(UiError {
            code: ErrorCode::ValidationFailed,
            message: "Type the tablet's address.".into(),
            field: Some("address".into()),
            retry_after_s: None,
        });
    }
    if !arena_proto::valid_handover_code(request.code.trim()) {
        return fail(UiError {
            code: ErrorCode::ValidationFailed,
            message: "The code is 6 digits.".into(),
            field: Some("code".into()),
            retry_after_s: None,
        });
    }
    match control.join(request).await {
        Ok(view) => Json(view).into_response(),
        Err(e) => fail(e),
    }
}

pub fn ui_migration_router(control: Control) -> Router {
    Router::new()
        .route("/ui/migration/join", post(join))
        .with_state(control)
        .layer(axum::middleware::from_fn(crate::health::ui_guard))
}
