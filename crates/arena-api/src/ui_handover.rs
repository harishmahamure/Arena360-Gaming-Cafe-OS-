//! The Android Hub's side of handing the branch to a Windows Hub, as the manager drives it from
//! the Hub UI (Story 10.17):
//!
//! * `POST /ui/handover/start {pin}`   confirm the move named by the last heartbeat: a 6-digit code
//! * `POST /ui/handover/file {pin}`    "Use a file instead": write the snapshot to the backup folder
//! * `POST /ui/handover/cancel {pin}`  "Cancel move": serve again as main, nothing set aside
//!
//! Each takes the PIN of an Owner or Manager (`ops.promote`, the permission that moves the main
//! role between Hubs), even from a signed-in Manager: the PIN is what confirms the move. The
//! answers are the [`arena_proto::HandoverView`]; the Hub snapshot carries the same view.

use std::sync::Arc;

use arena_proto::{AppError, ErrorCode, HandoverPinRequest, HandoverView, Permission};
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};

use crate::ui_auth::{UiAuth, auth_failed, bad_frame, bearer, fail, ui_error};

/// Checks the session and the manager's PIN; the handover to act on.
async fn confirmed(
    auth: &Arc<UiAuth>,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<(Arc<arena_core::handover::Handover>, String), Response> {
    let Some(token) = bearer(headers).map(str::to_owned) else {
        return Err(fail(auth_failed()));
    };
    let me = auth.acting_session(&token).await.map_err(fail)?;
    let request =
        serde_json::from_slice::<HandoverPinRequest>(if body.is_empty() { b"{}" } else { body })
            .map_err(|_| fail(bad_frame("the body must be {\"pin\"}")))?;
    let Some(handover) = auth.handover() else {
        return Err(fail(ui_error(AppError::new(
            ErrorCode::Unsupported,
            "this hub does not hand over",
        ))));
    };
    let Some(pin) = request.pin else {
        return Err(fail(ui_error(arena_core::rbac::denied())));
    };
    let manager = auth
        .pin_holder(&me, Some(pin), Permission::OpsPromote)
        .await
        .map_err(fail)?;
    Ok((handover, manager))
}

fn answer(view: Option<HandoverView>) -> Response {
    Json(view).into_response()
}

pub(crate) async fn start(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let (handover, manager) = match confirmed(&auth, &headers, &body).await {
        Ok(ok) => ok,
        Err(r) => return r,
    };
    match handover.begin().await {
        Ok(view) => {
            tracing::warn!(staff_id = %manager, "handover to a Windows Hub confirmed");
            answer(Some(view))
        }
        Err(e) => fail(ui_error(e)),
    }
}

pub(crate) async fn file(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let (handover, manager) = match confirmed(&auth, &headers, &body).await {
        Ok(ok) => ok,
        Err(r) => return r,
    };
    match handover.use_file().await {
        Ok(view) => {
            tracing::warn!(staff_id = %manager, "handover snapshot written to the backup folder");
            answer(Some(view))
        }
        Err(e) => fail(ui_error(e)),
    }
}

pub(crate) async fn cancel(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let (handover, manager) = match confirmed(&auth, &headers, &body).await {
        Ok(ok) => ok,
        Err(r) => return r,
    };
    match handover.cancel().await {
        Ok(()) => {
            tracing::warn!(staff_id = %manager, "handover cancelled; this hub serves again");
            answer(None)
        }
        Err(e) => fail(ui_error(e)),
    }
}
