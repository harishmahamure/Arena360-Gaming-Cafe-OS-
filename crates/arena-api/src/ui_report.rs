//! The Reports screen's reads (Story 5.27): `GET /ui/reports/daily-sales?from=&to=` and
//! `GET /ui/reports/sessions?from=&to=` (`report.view`), over inclusive business days
//! (`YYYY-MM-DD`, at most 90 days). They only read, off the writer's thread, so a long range
//! never holds up `apply()`. Story 5.28 adds `GET /ui/reports/shifts` and
//! `GET /ui/reports/shifts/{id}` (the Z-report of a closed shift). Story 5.29 adds
//! `GET /ui/reports/gst` and `POST /ui/reports/gst/export` (the GSTR-1 ZIP, written to a folder).
//! Story 5.30 adds `GET /ui/reports/players` and `GET /ui/reports/staff` (same range rules).

use std::sync::Arc;

use arena_proto::{AppError, ErrorCode, Permission};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::ui_auth::{
    UiAuth, auth_failed, bad_frame, bearer, fail, internal, player_kek, ui_error,
};

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
    Players,
    Staff,
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

/// `GET /ui/reports/players?from=&to=` (Story 5.30). Player names are opened with the branch
/// key, so a Hub with no key store answers `UNSUPPORTED`.
pub(crate) async fn players(
    state: State<Arc<UiAuth>>,
    headers: HeaderMap,
    q: Query<RangeQuery>,
) -> Response {
    run(state, headers, q, Report::Players).await
}

/// `GET /ui/reports/staff?from=&to=` (Story 5.30).
pub(crate) async fn staff(
    state: State<Arc<UiAuth>>,
    headers: HeaderMap,
    q: Query<RangeQuery>,
) -> Response {
    run(state, headers, q, Report::Staff).await
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
    let kek = match report {
        Report::Players => match player_kek(&auth) {
            Ok(kek) => Some(kek),
            Err(e) => return fail(e),
        },
        _ => None,
    };
    let reader = Arc::clone(auth.reader());
    let now = auth.clock().now_ms();
    let built = tokio::task::spawn_blocking(move || match report {
        Report::DailySales => reader
            .daily_sales(&q.from, &q.to, now)
            .map(|v| Json(v).into_response()),
        Report::Sessions => reader
            .sessions_report(&q.from, &q.to, now)
            .map(|v| Json(v).into_response()),
        Report::Players => {
            let kek = kek.ok_or_else(|| {
                AppError::new(ErrorCode::Unsupported, "this hub has no key store")
            })?;
            reader
                .players_report(&q.from, &q.to, now, &kek.kek()?)
                .map(|v| Json(v).into_response())
        }
        Report::Staff => reader
            .staff_report(&q.from, &q.to)
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

#[derive(Deserialize)]
pub(crate) struct MonthQuery {
    month: String,
}

/// `GET /ui/reports/gst?month=YYYY-MM` (`report.view`): a month of the GST return as the screen
/// shows it (Story 5.29). `VALIDATION_FAILED` on a bad month, or `tax` in a branch that is not
/// GST registered.
pub(crate) async fn gst(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    Query(q): Query<MonthQuery>,
) -> Response {
    if let Err(denied) = gate(&auth, &headers).await {
        return denied;
    }
    let reader = Arc::clone(auth.reader());
    match tokio::task::spawn_blocking(move || reader.gst_month(&q.month)).await {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

const EXPORT_SHAPE: &str = "the body must be {\"month\": \"YYYY-MM\", \"folder\": string}";

/// Writes `bytes` as `folder/file_name` through a temporary name, so a reader never sees half a
/// file. `folder` must be an absolute path of a writable directory; an Android tree URI is not
/// one (`VALIDATION_FAILED` on `folder`).
fn write_export(
    folder: &str,
    file_name: &str,
    bytes: &[u8],
) -> Result<std::path::PathBuf, AppError> {
    let bad = |reason: &str| AppError::with_field(ErrorCode::ValidationFailed, "folder", reason);
    let dir = std::path::Path::new(folder);
    if arena_proto::is_backup_tree_uri(folder) {
        return Err(bad("unsupported"));
    }
    arena_core::setup::probe_backup_dir(dir).map_err(|p| bad(p.as_str()))?;
    let target = dir.join(file_name);
    let part = dir.join(format!(".{file_name}.part"));
    let written = std::fs::write(&part, bytes).and_then(|()| std::fs::rename(&part, &target));
    if written.is_err() {
        let _ = std::fs::remove_file(&part);
        return Err(bad("not_writable"));
    }
    Ok(target)
}

/// `POST /ui/reports/gst/export {month, folder}` (`report.view`): writes the month's ZIP of CSVs
/// to `folder` and answers what was written (Story 5.29). Reads the database and writes one file;
/// it makes no network call.
pub(crate) async fn gst_export(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Err(denied) = gate(&auth, &headers).await {
        return denied;
    }
    let Ok(req) = serde_json::from_slice::<arena_proto::GstExportRequest>(&body) else {
        return fail(bad_frame(EXPORT_SHAPE));
    };
    let reader = Arc::clone(auth.reader());
    let job = tokio::task::spawn_blocking(move || {
        let (file_name, zip, month) = reader.gst_export(&req.month)?;
        let path = write_export(&req.folder, &file_name, &zip)?;
        Ok::<_, AppError>(arena_proto::GstExported {
            file_name,
            path: path.to_string_lossy().into_owned(),
            bytes: i64::try_from(zip.len()).unwrap_or(i64::MAX),
            month,
        })
    })
    .await;
    match job {
        Ok(Ok(done)) => Json(done).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}
