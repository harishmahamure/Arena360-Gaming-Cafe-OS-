//! `POST /ui/standby/promote` (Story 9.21): "Promote to main" on the standby screen.
//!
//! A Manager (or the Owner) enters their PIN in place (`ops.promote`). The route checks the
//! PIN and hands the manager's staff id to the promote hook the Hub set when it started as a
//! standby ([`UiAuth::set_promote`]); the hook runs the steps (lease check, schema check, epoch
//! claim, erasures, writer) and answers the new epoch. Everything the steps refuse comes back as
//! it is: `LEASE_FRESH` ("The main Hub is still running. Check it before promoting."),
//! `SCHEMA_BEHIND`, `FENCED`. Nothing changes on a refusal before the hook starts.

use std::sync::Arc;

use arena_proto::{AppError, ErrorCode, Permission, PromoteRequest};
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};

use crate::ui_auth::{UiAuth, auth_failed, bad_frame, bearer, fail, status_of, ui_error};

fn failed(e: AppError) -> Response {
    (status_of(e.code), Json(ui_error(e))).into_response()
}

pub(crate) async fn promote(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    let me = match auth.acting_session(&token).await {
        Ok(me) => me,
        Err(e) => return fail(e),
    };
    let Ok(request) =
        serde_json::from_slice::<PromoteRequest>(if body.is_empty() { b"{}" } else { &body })
    else {
        return fail(bad_frame("the body must be {\"pin\"?}"));
    };
    let Some(hook) = auth.promote_hook() else {
        return failed(AppError::new(
            ErrorCode::Unsupported,
            "this PC is not a standby Hub",
        ));
    };
    // The PIN is what confirms a promote, so it is always required, even from a Manager: theirs,
    // or another Manager's when a Counter is signed in.
    let Some(pin) = request.pin else {
        return fail(ui_error(arena_core::rbac::denied()));
    };
    let manager = match auth
        .pin_holder(&me, Some(pin), Permission::OpsPromote)
        .await
    {
        Ok(id) => id,
        Err(e) => return fail(e),
    };
    tracing::warn!(staff_id = %manager, "promote to main requested");
    match hook(manager).await {
        Ok(done) => Json(done).into_response(),
        Err(e) => {
            tracing::warn!(code = e.code.as_str(), "promote to main refused");
            failed(e)
        }
    }
}
