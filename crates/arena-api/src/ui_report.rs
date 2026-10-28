//! The Reports screen's reads (Story 5.27): `GET /ui/reports/daily-sales?from=&to=` and
//! `GET /ui/reports/sessions?from=&to=` (`report.view`), over inclusive business days
//! (`YYYY-MM-DD`, at most 90 days). They only read, off the writer's thread, so a long range
//! never holds up `apply()`. Story 5.28 adds `GET /ui/reports/shifts` and
//! `GET /ui/reports/shifts/{id}` (the Z-report of a closed shift).

use std::sync::Arc;

use arena_proto::{AppError, ErrorCode, Permission};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::ui_auth::{UiAuth, auth_failed, bearer, fail, internal, ui_error};

#[derive(Deserialize)]
pub(crate) struct RangeQuery {
    from: String,
    to: String,
}

/// Which report a request asks for.
#[derive(Clone, Copy)]
enum Report {
    DailySales,
    Sessions,
}

/// `GET /ui/reports/daily-sales?from=&to=`.
pub(crate) async fn daily_sales(
    state: State<Arc<UiAuth>>,
    headers: HeaderMap,
    q: Query<RangeQuery>,
) -> Response {
    run(state, headers, q, Report::DailySales).await
}

/// `GET /ui/reports/sessions?from=&to=`.
pub(crate) async fn sessions(
    state: State<Arc<UiAuth>>,
    headers: HeaderMap,
    q: Query<RangeQuery>,
) -> Response {
    run(state, headers, q, Report::Sessions).await
}

async fn run(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    Query(q): Query<RangeQuery>,
    report: Report,
) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    let me = match auth.acting_session(&token).await {
        Ok(me) => me,
        Err(e) => return fail(e),
    };
    if !me.permissions.contains(&Permission::ReportView) {
        return fail(ui_error(AppError::new(
            ErrorCode::PermissionDenied,
            "you do not have permission to view reports",
        )));
    }
    let reader = Arc::clone(auth.reader());
    let now = auth.clock().now_ms();
    let built = tokio::task::spawn_blocking(move || match report {
        Report::DailySales => reader
            .daily_sales(&q.from, &q.to, now)
            .map(|v| Json(v).into_response()),
        Report::Sessions => reader
            .sessions_report(&q.from, &q.to, now)
            .map(|v| Json(v).into_response()),
    })
    .await;
    match built {
        Ok(Ok(view)) => view,
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

/// Checks the bearer token and `report.view`.
async fn gate(auth: &UiAuth, headers: &HeaderMap) -> Result<(), Response> {
    let Some(token) = bearer(headers).map(str::to_owned) else {
        return Err(fail(auth_failed()));
    };
    let me = auth.acting_session(&token).await.map_err(fail)?;
    if !me.permissions.contains(&Permission::ReportView) {
        return Err(fail(ui_error(AppError::new(
            ErrorCode::PermissionDenied,
            "you do not have permission to view reports",
        ))));
    }
    Ok(())
}

/// `GET /ui/reports/shifts` (`report.view`): the closed shifts, newest first (Story 5.28).
pub(crate) async fn shifts(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    if let Err(denied) = gate(&auth, &headers).await {
        return denied;
    }
    let reader = Arc::clone(auth.reader());
    match tokio::task::spawn_blocking(move || reader.shift_list()).await {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

/// `GET /ui/reports/shifts/{id}` (`report.view`): the Z-report of a closed shift, as stored at
/// close (built from the rows for a shift closed before they were stored). `NOT_FOUND` for an
/// unknown shift, `STALE` while it is still open.
pub(crate) async fn shift(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(denied) = gate(&auth, &headers).await {
        return denied;
    }
    let reader = Arc::clone(auth.reader());
    let now = auth.clock().now_ms();
    match tokio::task::spawn_blocking(move || reader.z_report(&id, now)).await {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}
