//! The owner's audit build routes (Story 8.36, AD-37): `POST /v1/branches/{id}/audit/build`
//! asks for a month of a branch's history to be rebuilt, and
//! `GET /v1/branches/{id}/audit/build?month=` follows it.
//!
//! Nothing is built here: the route writes a `builder_jobs` row and the `builder` role does the
//! work. A request that finds a queued, building or fresh job reuses it. The answer is the job's
//! [`AuditBuildStatus`]: 202 with `code: AUDIT_BUILDING` and the progress while it waits, 200
//! when it is ready. A failed job answers `AUDIT_BUILD_FAILED` on the `GET`; the next `POST` starts
//! a new attempt. A tenant over [`arena_proto::AUDIT_BUILDS_PER_HOUR`] new builds an hour gets
//! `RATE_LIMITED`. Free branches upload nothing to build from: `FEATURE_NOT_ENTITLED`.
//!
//! `GET /v1/branches/{id}/audit?month=&q=` (Story 8.37) searches a built month with one of the fixed
//! queries of the `arena-proto` catalogue; see [`crate::audit_query`].

use std::collections::HashMap;
use std::sync::Arc;

use arena_proto::{
    AppError, AuditBuildRequest, AuditBuildState, AuditBuildStatus, AuditSearch, ErrorCode,
    OWNER_ALL_BRANCHES, is_audit_month,
};
use arena_store::control::Branch;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use super::error::ApiError;
use super::gate::RequireSetupComplete;
use super::json::ApiJson;
use super::owner_read::branches_for;
use super::signup::ceil_s;
use crate::audit_build::{
    Requested, cache_key, is_fresh, request_build, request_rebuild, status_of,
};
use crate::audit_query::{QueryError, search_month};
use crate::auth::AuthState;
use crate::presign::{branch_prefix, has_path_chars};

#[derive(Debug, Deserialize)]
struct MonthQuery {
    month: Option<String>,
}

fn month_error() -> ApiError {
    ApiError::validation("month", "Send a month like 2026-10.")
}

/// The month key, which must be a real month no later than the current one.
pub(super) fn checked_month(month: &str, now: i64) -> Result<(), ApiError> {
    if !is_audit_month(month) {
        return Err(month_error());
    }
    let current = chrono::DateTime::from_timestamp_millis(now)
        .map(|t| t.format("%Y-%m").to_string())
        .ok_or_else(|| ApiError::internal("the clock is out of range"))?;
    if month > current.as_str() {
        return Err(ApiError::validation(
            "month",
            "That month has not happened yet.",
        ));
    }
    Ok(())
}

/// The answer for a job: 200 when ready, 202 while it waits.
fn answer(status: &AuditBuildStatus) -> Response {
    let code = if status.code.is_some() {
        StatusCode::ACCEPTED
    } else {
        StatusCode::OK
    };
    (code, Json(status)).into_response()
}

async fn build(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<AuditBuildRequest>,
) -> Result<Response, ApiError> {
    let now = st.now();
    if id == OWNER_ALL_BRANCHES {
        return Err(ApiError::validation("id", "Pick one branch."));
    }
    checked_month(&req.month, now)?;
    let branches = branches_for(&st, &signed.owner.id, &id, now).await?;
    let branch = &branches[0];
    answer_build(
        &st,
        branch,
        &req.month,
        &format!("owner:{}", signed.owner.id),
        None,
    )
    .await
}

/// Queues `month` of `branch` unless a usable job exists, and answers the job's state: the one entry
/// point of the owner's and the support path's build requests.
pub(super) async fn answer_build(
    st: &AuthState,
    branch: &Branch,
    month: &str,
    requested_by: &str,
    reason: Option<&str>,
) -> Result<Response, ApiError> {
    queue_again(st, branch, month, requested_by, reason, false).await
}

async fn status(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    Path(id): Path<String>,
    q: Result<Query<MonthQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let now = st.now();
    if id == OWNER_ALL_BRANCHES {
        return Err(ApiError::validation("id", "Pick one branch."));
    }
    let month = q
        .ok()
        .and_then(|Query(q)| q.month)
        .ok_or_else(month_error)?;
    if !is_audit_month(&month) {
        return Err(month_error());
    }
    let branches = branches_for(&st, &signed.owner.id, &id, now).await?;
    let branch = &branches[0];
    let job = st
        .db
        .builder_job(&branch.id, &month)
        .await
        .map_err(ApiError::internal)?
        // A build past its cache lifetime is gone, as if never built.
        .filter(|j| j.state != arena_proto::AuditBuildState::Ready || is_fresh(j, now))
        .ok_or_else(ApiError::not_found)?;
    let status = status_of(&job);
    if status.code == Some(ErrorCode::AuditBuildFailed) {
        tracing::warn!(branch_id = %branch.id, month = %month, "audit build failed, reported to owner");
        return Err(failed_error());
    }
    Ok(answer(&status))
}

fn failed_error() -> ApiError {
    AppError::new(
        ErrorCode::AuditBuildFailed,
        "That month could not be rebuilt. Ask for it again.",
    )
    .into()
}

/// Queues the month again (`force`: although it is `ready` and fresh) and answers its state.
async fn queue_again(
    st: &AuthState,
    branch: &Branch,
    month: &str,
    requested_by: &str,
    reason: Option<&str>,
    force: bool,
) -> Result<Response, ApiError> {
    let now = st.now();
    let (db, tenant, id) = (&st.db, &branch.tenant_id, &branch.id);
    let requested = if force {
        request_rebuild(db, tenant, id, month, requested_by, reason, now).await
    } else {
        request_build(db, tenant, id, month, requested_by, reason, now).await
    }
    .map_err(ApiError::internal)?;
    match requested {
        Requested::Existing(job) | Requested::Queued(job) => Ok(answer(&status_of(&job))),
        Requested::RateLimited { wait_ms } => Err(ApiError::rate_limited(ceil_s(wait_ms))),
    }
}

/// The answer to a search over `branch`'s `month` (Story 8.37): the page when the month is built
/// and fresh, otherwise the state of its build. A month that was never built is `NOT_FOUND`; one
/// past its cache lifetime, or whose object is gone, is queued again and answers
/// `AUDIT_BUILDING`. Shared with the support path, which passes who asked and why.
pub(super) async fn search_answer(
    st: &AuthState,
    branch: &Branch,
    month: &str,
    search: AuditSearch,
    requested_by: &str,
    reason: Option<&str>,
) -> Result<Response, ApiError> {
    let now = st.now();
    let job = st
        .db
        .builder_job(&branch.id, month)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| {
            ApiError::from(AppError::new(
                ErrorCode::NotFound,
                "Build that month before searching it.",
            ))
        })?;
    match job.state {
        AuditBuildState::Queued | AuditBuildState::Building => {
            return Ok(answer(&status_of(&job)));
        }
        AuditBuildState::Failed => {
            tracing::warn!(branch_id = %branch.id, month, "audit search on a failed build");
            return Err(failed_error());
        }
        AuditBuildState::Ready => {}
    }
    let Some(built_at) = job.built_at.filter(|_| is_fresh(&job, now)) else {
        return queue_again(st, branch, month, requested_by, reason, false).await;
    };
    let (Some(cache), Some(data)) = (st.audit_cache.get(), st.presigner.get()) else {
        return Err(ApiError::unavailable("no audit cache or data bucket"));
    };
    if has_path_chars(&branch.id) || has_path_chars(&branch.tenant_id) {
        return Err(ApiError::internal("branch id with path characters"));
    }
    let key = cache_key(&branch_prefix(&branch.tenant_id, &branch.id), month);
    match search_month(cache, data, &key, &branch.id, month, built_at, search).await {
        Ok(page) => Ok((StatusCode::OK, Json(page)).into_response()),
        Err(QueryError::Gone) => {
            tracing::warn!(branch_id = %branch.id, month, "built month is gone from the bucket");
            queue_again(st, branch, month, requested_by, reason, true).await
        }
        Err(QueryError::BadCursor) => Err(ApiError::validation(
            "cursor",
            "That page marker is not one this search issued.",
        )),
        Err(QueryError::Failed(e)) => Err(ApiError::internal(e)),
    }
}

/// `GET /v1/branches/{id}/audit?month=&q=`.
async fn search(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    Path(id): Path<String>,
    q: Result<Query<HashMap<String, String>>, QueryRejection>,
) -> Result<Response, ApiError> {
    let now = st.now();
    if id == OWNER_ALL_BRANCHES {
        return Err(ApiError::validation("id", "Pick one branch."));
    }
    let params = q.map(|Query(q)| q).unwrap_or_default();
    let month = params
        .get("month")
        .filter(|m| is_audit_month(m))
        .ok_or_else(month_error)?
        .clone();
    let search = AuditSearch::parse(&params)?;
    let branches = branches_for(&st, &signed.owner.id, &id, now).await?;
    search_answer(
        &st,
        &branches[0],
        &month,
        search,
        &format!("owner:{}", signed.owner.id),
        None,
    )
    .await
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/branches/{id}/audit/build", post(build).get(status))
        .route("/branches/{id}/audit", get(search))
        .with_state(st)
}
