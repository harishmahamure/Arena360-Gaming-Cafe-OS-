//! The support audit endpoints (Story 8.37, AD-21): `POST /v1/admin/branches/{id}/audit/build`
//! and `GET /v1/admin/branches/{id}/audit?month=&q=&reason=`.
//!
//! Only an Arena 360 staff credential gets in (`Authorization: Bearer`, see [`crate::admin`]);
//! with none configured the routes do not exist (`NOT_FOUND`). Every call needs a non-empty
//! `reason`, and writes one `admin_audit` row (`actor_kind = admin`, the staff id, the branch,
//! month, query and reason) before any data is returned. The answers are the owner's: a build
//! status, or a page of change metadata with players by id only.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use arena_proto::{AuditBuildRequest, AuditSearch, OWNER_ALL_BRANCHES, is_audit_month};
use arena_store::control::{Branch, NewAdminAudit};
use axum::Router;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;
use axum::response::Response;
use axum::routing::{get, post};
use serde::Deserialize;

use super::audit::{answer_build, checked_month, search_answer};
use super::error::ApiError;
use super::json::ApiJson;
use crate::auth::AuthState;

/// The longest reason kept.
const REASON_MAX_CHARS: usize = 500;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdminBuildRequest {
    #[serde(flatten)]
    build: AuditBuildRequest,
    reason: Option<String>,
}

/// The staff id behind the bearer token. With no staff credentials the API is not there.
fn staff_of(st: &AuthState, headers: &HeaderMap) -> Result<String, ApiError> {
    let tokens = st
        .admin_tokens
        .get()
        .filter(|t| !t.is_empty())
        .ok_or_else(ApiError::not_found)?;
    let token = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .unwrap_or_default();
    tokens
        .authenticate(token)
        .map(str::to_owned)
        .ok_or_else(ApiError::auth_failed)
}

fn checked_reason(reason: Option<&str>) -> Result<String, ApiError> {
    let reason = reason.map(str::trim).unwrap_or_default();
    if reason.is_empty() {
        return Err(ApiError::validation(
            "reason",
            "Say why this month is being read.",
        ));
    }
    if reason.chars().count() > REASON_MAX_CHARS {
        return Err(ApiError::validation("reason", "Keep the reason shorter."));
    }
    Ok(reason.to_owned())
}

async fn branch_of(st: &AuthState, id: &str) -> Result<Branch, ApiError> {
    if id == OWNER_ALL_BRANCHES {
        return Err(ApiError::validation("id", "Pick one branch."));
    }
    st.db
        .branch(id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)
}

/// Writes the audit row for one support call; no data is returned if this fails.
async fn record(
    st: &AuthState,
    staff: &str,
    action: &str,
    branch: &Branch,
    reason: &str,
    summary: serde_json::Value,
) -> Result<(), ApiError> {
    let (id, request_id) = (
        ulid::Ulid::generate().to_string(),
        ulid::Ulid::generate().to_string(),
    );
    let summary = summary.to_string();
    st.db
        .append_admin_audit(&NewAdminAudit {
            id: &id,
            at: st.now(),
            actor_kind: "admin",
            actor_id: staff,
            action,
            target_kind: "branch",
            target_id: &branch.id,
            reason: Some(reason),
            summary: &summary,
            request_id: &request_id,
        })
        .await
        .map_err(ApiError::internal)
}

async fn build(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<AdminBuildRequest>,
) -> Result<Response, ApiError> {
    let staff = staff_of(&st, &headers)?;
    let reason = checked_reason(req.reason.as_deref())?;
    let month = req.build.month;
    checked_month(&month, st.now())?;
    let branch = branch_of(&st, &id).await?;
    record(
        &st,
        &staff,
        "audit.build",
        &branch,
        &reason,
        serde_json::json!({"month": month}),
    )
    .await?;
    answer_build(
        &st,
        &branch,
        &month,
        &format!("admin:{staff}"),
        Some(&reason),
    )
    .await
}

async fn search(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    q: Result<Query<HashMap<String, String>>, QueryRejection>,
) -> Result<Response, ApiError> {
    let staff = staff_of(&st, &headers)?;
    let mut params = q.map(|Query(q)| q).unwrap_or_default();
    let reason = checked_reason(params.remove("reason").as_deref())?;
    let month = params
        .get("month")
        .filter(|m| is_audit_month(m))
        .ok_or_else(|| ApiError::validation("month", "Send a month like 2026-10."))?
        .clone();
    let search = AuditSearch::parse(&params)?;
    let branch = branch_of(&st, &id).await?;
    // Sorted, so the same question is recorded the same way.
    let query: BTreeMap<&String, &String> = params.iter().collect();
    record(
        &st,
        &staff,
        "audit.search",
        &branch,
        &reason,
        serde_json::json!({"month": month, "query": query}),
    )
    .await?;
    search_answer(
        &st,
        &branch,
        &month,
        search,
        &format!("admin:{staff}"),
        Some(&reason),
    )
    .await
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/admin/branches/{id}/audit/build", post(build))
        .route("/admin/branches/{id}/audit", get(search))
        .with_state(st)
}
