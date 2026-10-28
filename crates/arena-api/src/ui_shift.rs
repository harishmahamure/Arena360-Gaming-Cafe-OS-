//! The Close shift screen's reveal (Story 5.26): `GET /ui/shift/count?counted_minor=<n>` returns
//! the expected cash, the signed difference and the breakdown for a stated count. The count is
//! blind: there is no way to read the expected cash without giving a count first. It only reads;
//! the close itself is the `shift.close` intent.

use std::sync::Arc;

use arena_proto::{AppError, ErrorCode, MAX_CASH_MOVE_MINOR, Permission};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::ui_auth::{UiAuth, auth_failed, bearer, fail, internal, ui_error};

#[derive(Deserialize)]
pub(crate) struct CountQuery {
    counted_minor: i64,
}

/// `GET /ui/shift/count?counted_minor=` (`shift.close`): the reveal, or `SHIFT_NOT_OPEN`.
pub(crate) async fn count(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    Query(q): Query<CountQuery>,
) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    let me = match auth.acting_session(&token).await {
        Ok(me) => me,
        Err(e) => return fail(e),
    };
    if !me.permissions.contains(&Permission::ShiftClose) {
        return fail(ui_error(AppError::new(
            ErrorCode::PermissionDenied,
            "you do not have permission to close a shift",
        )));
    }
    if !(0..=MAX_CASH_MOVE_MINOR).contains(&q.counted_minor) {
        return fail(ui_error(AppError::with_field(
            ErrorCode::ValidationFailed,
            "counted_minor",
            "enter the cash you counted",
        )));
    }
    let reader = Arc::clone(auth.reader());
    match tokio::task::spawn_blocking(move || reader.shift_count(q.counted_minor)).await {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}
