//! `POST /ui/migration/join` (Story 10.17): "Waiting for your Android Hub" on the Windows Hub of a
//! Hub move takes the Android Hub's address and the 6-digit code on its screen and starts the
//! handover.
//!
//! A Hub waiting for a move has no staff and no session, so the route sits behind the loopback
//! guard only, like `/ui/pair-hub`. The code is what authorises the handover, and the Android Hub
//! limits wrong ones. The file and import routes (Story 10.18) are guarded the same way: the owner
//! released the branch key to this PC in the cloud, which is what authorises the import.

use std::sync::Arc;

use arena_core::handover::{HandoverJoinControl, MigrationImportControl};
use arena_proto::{ErrorCode, HandoverFileRequest, HandoverJoinRequest, UiError};
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

type Import = Arc<dyn MigrationImportControl>;

/// `POST /ui/migration/file` (Story 10.18): takes the snapshot from a file on this PC, when the
/// tablet could not be reached.
async fn file(State(control): State<Import>, body: axum::body::Bytes) -> Response {
    let Ok(request) = serde_json::from_slice::<HandoverFileRequest>(&body) else {
        return fail(bad_frame("the body must be {\"path\"}"));
    };
    match control.use_file(request).await {
        Ok(view) => Json(view).into_response(),
        Err(e) => fail(e),
    }
}

/// `POST /ui/migration/import` (Story 10.18): imports the received snapshot and restarts the Hub
/// onto it. Needs the branch key the owner released to this PC.
async fn import(State(control): State<Import>) -> Response {
    match control.import().await {
        Ok(done) => Json(done).into_response(),
        Err(e) => fail(e),
    }
}

pub fn ui_migration_router(control: Control, importer: Import) -> Router {
    let join = Router::new()
        .route("/ui/migration/join", post(join))
        .with_state(control);
    let import = Router::new()
        .route("/ui/migration/file", post(file))
        .route("/ui/migration/import", post(import))
        .with_state(importer);
    join.merge(import)
        .layer(axum::middleware::from_fn(crate::health::ui_guard))
}
