//! Account -> Subscription (Story 10.8): `GET /v1/account/subscription` (every branch's plan),
//! `GET /v1/account/invoices?branch=&fy=` (invoices and credit notes, newest first) and
//! `GET /v1/account/invoices/{id}/pdf` (a short-lived signed link to one PDF). Everything is read
//! through the signed-in owner's own tenant: a branch or invoice of another tenant answers
//! `FORBIDDEN`, and nothing is read for it.

use std::collections::HashMap;
use std::sync::Arc;

use arena_proto::{
    AccountSubscription, AppError, ErrorCode, InvoiceDownload, InvoiceList, PRESIGN_TTL_S,
};
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::HeaderValue;
use axum::http::header::CACHE_CONTROL;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use super::branches::tier_of;
use super::error::ApiError;
use super::gate::RequireSetupComplete;
use crate::account_plan::{self, PlanError};
use crate::auth::AuthState;

/// The most invoices one list answers with.
const LIST_LIMIT: u32 = 500;

#[derive(Debug, Deserialize)]
struct AccountQuery {
    branch: Option<String>,
    fy: Option<String>,
}

fn not_yours() -> ApiError {
    ApiError::forbidden("You don't have access to this.")
}

fn query_of(q: Result<Query<AccountQuery>, QueryRejection>) -> Result<AccountQuery, ApiError> {
    q.map(|Query(q)| q)
        .map_err(|_| ApiError::bad_request("The filter is not valid.", None))
}

/// Never cached: the card and the list change when a payment lands.
fn fresh<T: serde::Serialize>(body: T) -> Response {
    (
        [(CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        Json(body),
    )
        .into_response()
}

/// The owner's tenant id, or `None` when the owner has no café yet.
async fn tenant_id(st: &AuthState, owner_id: &str) -> Result<Option<String>, ApiError> {
    Ok(st
        .db
        .tenant_of_owner(owner_id)
        .await
        .map_err(ApiError::internal)?
        .map(|t| t.id))
}

async fn subscription(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    q: Result<Query<AccountQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let q = query_of(q)?;
    let Some(tenant) = tenant_id(&st, &signed.owner.id).await? else {
        return Ok(fresh(AccountSubscription {
            branches: Vec::new(),
        }));
    };
    let branches = st
        .db
        .branches_of_tenant(&tenant)
        .await
        .map_err(ApiError::internal)?;
    if let Some(wanted) = &q.branch
        && !branches.iter().any(|b| &b.id == wanted)
    {
        tracing::warn!(owner_id = %signed.owner.id, "subscription of a branch outside the tenant refused");
        return Err(not_yours());
    }
    let mut out = Vec::new();
    for b in branches
        .iter()
        .filter(|b| q.branch.as_ref().is_none_or(|w| &b.id == w))
    {
        let tier = tier_of(&st, &b.id).await?;
        let subs = st
            .db
            .subscriptions_of_branch(&b.id)
            .await
            .map_err(ApiError::internal)?;
        let plan = account_plan::plan_of(&st.db, b, tier, &subs)
            .await
            .map_err(|PlanError::Db(e)| ApiError::internal(e))?;
        out.push(plan);
    }
    Ok(fresh(AccountSubscription { branches: out }))
}

async fn invoices(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    q: Result<Query<AccountQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let q = query_of(q)?;
    if let Some(fy) = &q.fy
        && !account_plan::valid_fy(fy)
    {
        return Err(ApiError::bad_request(
            "The financial year looks like 2026-27.",
            Some("fy".to_owned()),
        ));
    }
    let Some(tenant) = tenant_id(&st, &signed.owner.id).await? else {
        return Ok(fresh(InvoiceList {
            invoices: Vec::new(),
            years: Vec::new(),
        }));
    };
    let branches = st
        .db
        .branches_of_tenant(&tenant)
        .await
        .map_err(ApiError::internal)?;
    if let Some(wanted) = &q.branch
        && !branches.iter().any(|b| &b.id == wanted)
    {
        tracing::warn!(owner_id = %signed.owner.id, "invoices of a branch outside the tenant refused");
        return Err(not_yours());
    }
    let names: HashMap<_, _> = branches
        .iter()
        .map(|b| (b.id.as_str(), b.name.as_str()))
        .collect();
    let rows = st
        .db
        .invoices_of_tenant(&tenant, q.branch.as_deref(), q.fy.as_deref(), LIST_LIMIT)
        .await
        .map_err(ApiError::internal)?;
    let years = st
        .db
        .invoice_years_of_tenant(&tenant)
        .await
        .map_err(ApiError::internal)?;
    Ok(fresh(InvoiceList {
        invoices: rows
            .iter()
            .map(|i| {
                account_plan::invoice_row(i, names.get(i.branch_id.as_str()).copied().unwrap_or(""))
            })
            .collect(),
        years,
    }))
}

async fn download(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let tenant = tenant_id(&st, &signed.owner.id)
        .await?
        .ok_or_else(not_yours)?;
    // An id that is not the tenant's answers exactly as one that does not exist.
    let invoice = st
        .db
        .invoice_by_id(&id)
        .await
        .map_err(ApiError::internal)?
        .filter(|i| i.tenant_id == tenant)
        .ok_or_else(|| {
            tracing::warn!(owner_id = %signed.owner.id, "invoice outside the tenant refused");
            not_yours()
        })?;
    let Some(key) = invoice.pdf_key else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            "This invoice's PDF is still being prepared.",
        )
        .into());
    };
    let presigner = st
        .presigner
        .get()
        .ok_or_else(|| ApiError::unavailable("no data bucket for invoice links"))?;
    let url = presigner
        .presign_get(&key)
        .await
        .map_err(ApiError::internal)?;
    Ok(fresh(InvoiceDownload {
        url,
        expires_in_s: PRESIGN_TTL_S,
    }))
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/account/subscription", get(subscription))
        .route("/account/invoices", get(invoices))
        .route("/account/invoices/{id}/pdf", get(download))
        .with_state(st)
}
