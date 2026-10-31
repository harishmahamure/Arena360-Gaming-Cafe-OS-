//! `POST /ui/standby/promote` (Story 9.21): "Promote to main" on the standby screen.
//!
//! A Manager (or the Owner) enters their PIN in place (`ops.promote`). The route checks the
//! PIN and hands the manager's staff id to the promote hook the Hub set when it started as a
//! standby ([`UiAuth::set_promote`]); the hook runs the steps (lease check, schema check, epoch
//! claim, erasures, writer) and answers the new epoch. Everything the steps refuse comes back as
//! it is: `LEASE_FRESH` ("The main Hub is still running. Check it before promoting."),
//! `SCHEMA_BEHIND`, `FENCED`. Nothing changes on a refusal before the hook starts.
//!
//! A refusal a forced promote could get past (Story 9.22) carries `force_offer` (the warning and the
//! lag). The manager then sends the request again with `force` and `data_loss_acknowledged`, and
//! the PIN again; the hook runs the same steps without the freshness check.

use std::sync::Arc;

use arena_proto::{
    AppError, ErrorCode, Permission, PromoteFailure, PromoteRefused, PromoteRequest,
};
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};

use crate::ui_auth::{UiAuth, auth_failed, bad_frame, bearer, fail, status_of, ui_error};

fn refused(f: PromoteFailure) -> Response {
    let status = status_of(f.error.code);
    let body = PromoteRefused {
        error: ui_error(f.error),
        force_offer: f.force_offer,
    };
    (status, Json(body)).into_response()
}

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
        return fail(bad_frame(
            "the body must be {\"pin\"?, \"force\"?, \"data_loss_acknowledged\"?}",
        ));
    };
    let Some(hook) = auth.promote_hook() else {
        return failed(AppError::new(
            ErrorCode::Unsupported,
            "this PC is not a standby Hub",
        ));
    };
    // A forced promote goes past the checks that protect the old main Hub's last writes, so the
    // manager must have been shown the warning and said yes.
    if request.force && !request.data_loss_acknowledged {
        return failed(AppError::with_field(
            ErrorCode::ValidationFailed,
            "data_loss_acknowledged",
            "Confirm that data since the last sync may be lost.",
        ));
    }
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
    match hook(manager, request.force).await {
        Ok(done) => Json(done).into_response(),
        Err(e) => {
            tracing::warn!(
                code = e.error.code.as_str(),
                force = request.force,
                "promote to main refused"
            );
            refused(e)
        }
    }
}
