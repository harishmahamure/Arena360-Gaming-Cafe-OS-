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

use std::sync::Arc;

use arena_proto::{
    AppError, AuditBuildRequest, AuditBuildStatus, ErrorCode, OWNER_ALL_BRANCHES, is_audit_month,
};
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;

use super::error::ApiError;
use super::gate::RequireSetupComplete;
use super::json::ApiJson;
use super::owner_read::branches_for;
use super::signup::ceil_s;
use crate::audit_build::{Requested, is_fresh, status_of};
use crate::auth::AuthState;

#[derive(Debug, Deserialize)]
struct MonthQuery {
    month: Option<String>,
}

fn month_error() -> ApiError {
    ApiError::validation("month", "Send a month like 2026-10.")
}

/// The month key, which must be a real month no later than the current one.
fn checked_month(month: &str, now: i64) -> Result<(), ApiError> {
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
    let requested = crate::audit_build::request_build(
        &st.db,
        &branch.tenant_id,
        &branch.id,
        &req.month,
        &format!("owner:{}", signed.owner.id),
        None,
        now,
    )
    .await
    .map_err(ApiError::internal)?;
    match requested {
        Requested::Existing(job) | Requested::Queued(job) => Ok(answer(&status_of(&job))),
        Requested::RateLimited { wait_ms } => Err(ApiError::rate_limited(ceil_s(wait_ms))),
    }
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
        return Err(AppError::new(
            ErrorCode::AuditBuildFailed,
            "That month could not be rebuilt. Ask for it again.",
        )
        .into());
    }
    Ok(answer(&status))
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/branches/{id}/audit/build", post(build).get(status))
        .with_state(st)
}
