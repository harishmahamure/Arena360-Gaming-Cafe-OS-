//! A second branch starts from the tenant's existing setup (Story 5.38).
//!
//! The owner's choice ("Start from <branch>'s setup", on by default when another paid branch has
//! a stored file, Story 5.37) lives in `gz_control.branch_template_choice`. When the new Hub
//! pairs, `branch.bootstrap` carries a short-lived signed link to the chosen file
//! ([`bootstrap_link`]); the Hub downloads it from `GET /v1/setup-template/{token}`, imports it
//! and reports `POST /v1/branches/{id}/setup-template/outcome`. A failed try shows in the wizard
//! with Retry, which has the next heartbeat carry a fresh link ([`retry_link`]).

use std::sync::Arc;

use arena_proto::{
    AppError, ErrorCode, SETUP_LINK_TTL_MS, SetSetupStart, SetupStartSource, SetupStartView,
    SetupTemplateLink, SetupTemplateOutcome, TemplateStatus, Tier,
};
use arena_store::control::Branch;
use axum::extract::{Path, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, HeaderValue};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use super::branches::tier_of;
use super::error::ApiError;
use super::heartbeat::authenticate;
use super::json::ApiJson;
use super::owned::OwnedBranch;
use crate::auth::AuthState;
use crate::templates::{TemplateStore, file_key};

const MESSAGE_MAX_CHARS: usize = 200;
const LINK_PATH: &str = "/v1/setup-template/";

fn store_of(st: &AuthState) -> Option<&TemplateStore> {
    st.templates.get()
}

/// The tenant's other branches that are on a paid tier and have a stored setup file, oldest
/// first.
async fn sources_of(st: &AuthState, branch: &Branch) -> Result<Vec<SetupStartSource>, ApiError> {
    let Some(store) = store_of(st) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for other in st
        .db
        .branches_of_tenant(&branch.tenant_id)
        .await
        .map_err(ApiError::internal)?
    {
        if other.id == branch.id || tier_of(st, &other.id).await? == Tier::Free {
            continue;
        }
        let stored = store
            .describe(&other.id)
            .await
            .map_err(|e| ApiError::unavailable(format!("setup template unreadable: {e}")))?;
        if let Some(stored) = stored {
            out.push(SetupStartSource {
                branch_id: other.id,
                name: other.name,
                uploaded_at: stored.uploaded_at,
            });
        }
    }
    Ok(out)
}

/// The branch the new branch starts from: the owner's pick if it is still a source, the first
/// source when the owner has not chosen, none when the option is off.
fn selected(
    sources: &[SetupStartSource],
    choice: Option<&arena_store::control::TemplateChoice>,
) -> Option<String> {
    match choice {
        None => sources.first().map(|s| s.branch_id.clone()),
        Some(c) => c
            .source_branch_id
            .clone()
            .filter(|id| sources.iter().any(|s| &s.branch_id == id)),
    }
}

/// A signed link to `source`'s stored file, valid for [`SETUP_LINK_TTL_MS`].
async fn mint_link(
    st: &AuthState,
    source: &str,
    now: i64,
) -> Result<Option<SetupTemplateLink>, ApiError> {
    let Some(store) = store_of(st) else {
        return Ok(None);
    };
    let Some(stored) = store
        .describe(source)
        .await
        .map_err(|e| ApiError::unavailable(format!("setup template unreadable: {e}")))?
    else {
        return Ok(None);
    };
    let expires_at = now + SETUP_LINK_TTL_MS;
    let data = format!("{source}.{expires_at}.{}", stored.sha256);
    let token = format!("{data}.{}", st.link_signature(&data));
    Ok(Some(SetupTemplateLink {
        path: format!("{LINK_PATH}{token}"),
        version: stored.version,
        sha256: stored.sha256,
        bytes: stored.bytes,
        expires_at,
    }))
}

/// The link `branch.bootstrap` carries when `branch` pairs for the first time, and the choice
/// it records: `pending` until the Hub answers. A pairing that finds the setup already applied
/// carries none. Never fails the pairing: anything that goes wrong is logged and the branch
/// bootstraps without the template.
pub(super) async fn bootstrap_link(
    st: &AuthState,
    branch: &Branch,
    now: i64,
) -> Option<SetupTemplateLink> {
    let attempt = async {
        let choice = st
            .db
            .template_choice(&branch.id)
            .await
            .map_err(ApiError::internal)?;
        if choice
            .as_ref()
            .and_then(|c| TemplateStatus::parse(c.status.as_deref().unwrap_or_default()))
            == Some(TemplateStatus::Applied)
        {
            return Ok(None);
        }
        let sources = sources_of(st, branch).await?;
        let Some(source) = selected(&sources, choice.as_ref()) else {
            return Ok(None);
        };
        let Some(link) = mint_link(st, &source, now).await? else {
            return Ok(None);
        };
        st.db
            .set_template_choice(&branch.id, Some(&source), Some("pending"), now)
            .await
            .map_err(ApiError::internal)?;
        Ok::<_, ApiError>(Some(link))
    };
    match attempt.await {
        Ok(link) => link,
        Err(e) => {
            tracing::warn!(branch_id = %branch.id, error = ?e.body().code, "setup template not offered at pairing");
            None
        }
    }
}

/// The fresh link a heartbeat carries after the owner pressed Retry; one heartbeat wins it.
pub(super) async fn retry_link(
    st: &AuthState,
    branch_id: &str,
    now: i64,
) -> Option<SetupTemplateLink> {
    let attempt = async {
        let choice = st
            .db
            .template_choice(branch_id)
            .await
            .map_err(ApiError::internal)?;
        let Some(c) = choice.filter(|c| c.status.as_deref() == Some("retry")) else {
            return Ok(None);
        };
        let Some(source) = c.source_branch_id else {
            return Ok(None);
        };
        let Some(link) = mint_link(st, &source, now).await? else {
            st.db
                .set_template_status(
                    branch_id,
                    "failed",
                    Some("The saved setup is no longer available."),
                    now,
                )
                .await
                .map_err(ApiError::internal)?;
            let _ = st.wizard_tx.send(branch_id.to_owned());
            return Ok(None);
        };
        let won = st
            .db
            .move_template_status(branch_id, "retry", "pending", now)
            .await
            .map_err(ApiError::internal)?;
        Ok::<_, ApiError>(won.then_some(link))
    };
    match attempt.await {
        Ok(link) => link,
        Err(e) => {
            tracing::warn!(branch_id, error = ?e.body().code, "setup template retry not offered");
            None
        }
    }
}

async fn view_of(st: &AuthState, branch: &Branch) -> Result<SetupStartView, ApiError> {
    let sources = sources_of(st, branch).await?;
    let others = st
        .db
        .branches_of_tenant(&branch.tenant_id)
        .await
        .map_err(ApiError::internal)?
        .iter()
        .filter(|b| b.id != branch.id)
        .count();
    let choice = st
        .db
        .template_choice(&branch.id)
        .await
        .map_err(ApiError::internal)?;
    let selected = selected(&sources, choice.as_ref());
    let status = choice
        .as_ref()
        .filter(|_| selected.is_some())
        .and_then(|c| c.status.as_deref())
        .and_then(TemplateStatus::parse);
    let error = choice
        .and_then(|c| c.error)
        .filter(|_| status == Some(TemplateStatus::Failed));
    Ok(SetupStartView {
        other_branches: u32::try_from(others).unwrap_or(u32::MAX),
        enabled: selected.is_some(),
        sources,
        selected,
        status,
        error,
    })
}

async fn get_choice(
    State(st): State<Arc<AuthState>>,
    owned: OwnedBranch,
) -> Result<Json<SetupStartView>, ApiError> {
    Ok(Json(view_of(&st, &owned.branch).await?))
}

async fn put_choice(
    State(st): State<Arc<AuthState>>,
    owned: OwnedBranch,
    ApiJson(body): ApiJson<SetSetupStart>,
) -> Result<Json<SetupStartView>, ApiError> {
    let branch = &owned.branch;
    let now = st.now();
    let existing = st
        .db
        .template_choice(&branch.id)
        .await
        .map_err(ApiError::internal)?;
    if !body.enabled {
        st.db
            .set_template_choice(&branch.id, None, None, now)
            .await
            .map_err(ApiError::internal)?;
        tracing::info!(branch_id = %branch.id, "branch no longer starts from another branch's setup");
        return Ok(Json(view_of(&st, branch).await?));
    }
    let sources = sources_of(&st, branch).await?;
    let source = match body.source_branch_id {
        Some(id) if sources.iter().any(|s| s.branch_id == id) => id,
        None if !sources.is_empty() => selected(&sources, existing.as_ref())
            .or_else(|| sources.first().map(|s| s.branch_id.clone()))
            .ok_or_else(|| ApiError::internal("a source vanished"))?,
        _ => {
            return Err(ApiError::validation(
                "source_branch_id",
                "Choose one of your other branches that has a saved setup.",
            ));
        }
    };
    let unchanged = existing.as_ref().is_some_and(|c| {
        c.source_branch_id.as_deref() == Some(source.as_str()) && c.status.is_some()
    });
    if !unchanged {
        // A Hub that is already paired gets the file on its next heartbeat; one that is not
        // gets it with the bootstrap.
        let paired = st
            .db
            .active_hub(&branch.id)
            .await
            .map_err(ApiError::internal)?
            .is_some();
        st.db
            .set_template_choice(&branch.id, Some(&source), paired.then_some("retry"), now)
            .await
            .map_err(ApiError::internal)?;
        tracing::info!(branch_id = %branch.id, source = %source, paired, "branch starts from another branch's setup");
    }
    Ok(Json(view_of(&st, branch).await?))
}

async fn retry(
    State(st): State<Arc<AuthState>>,
    owned: OwnedBranch,
) -> Result<Json<SetupStartView>, ApiError> {
    let branch = &owned.branch;
    if st
        .db
        .move_template_status(&branch.id, "failed", "retry", st.now())
        .await
        .map_err(ApiError::internal)?
    {
        tracing::info!(branch_id = %branch.id, "setup template retry requested");
    }
    Ok(Json(view_of(&st, branch).await?))
}

async fn outcome(
    State(st): State<Arc<AuthState>>,
    Path(branch_id): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<SetupTemplateOutcome>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let now = st.now();
    let hub = authenticate(&st, &headers, now).await?;
    if hub.branch_id != branch_id {
        return Err(ApiError::forbidden(
            "This Hub can't report for that branch.",
        ));
    }
    let message = body
        .message
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(|m| {
            m.chars()
                .filter(|c| !c.is_control())
                .take(MESSAGE_MAX_CHARS)
                .collect::<String>()
        });
    let (status, error) = if body.applied {
        ("applied", None)
    } else {
        (
            "failed",
            Some(message.unwrap_or_else(|| "The Hub could not set this up.".to_owned())),
        )
    };
    st.db
        .set_template_status(&branch_id, status, error.as_deref(), now)
        .await
        .map_err(ApiError::internal)?;
    let _ = st.wizard_tx.send(branch_id.clone());
    tracing::info!(branch_id = %branch_id, status, "hub reported the starting setup");
    Ok(Json(serde_json::json!({ "status": status })))
}

/// `GET /v1/setup-template/{token}`: the stored file a signed, unexpired link leads to. Every
/// refusal is the same `NOT_FOUND`, so a link says nothing about why it stopped working.
async fn download(
    State(st): State<Arc<AuthState>>,
    Path(token): Path<String>,
) -> Result<Response, ApiError> {
    let gone = || -> ApiError {
        AppError::new(
            ErrorCode::NotFound,
            "This link has expired. Ask for a new one.",
        )
        .into()
    };
    let mut parts = token.splitn(4, '.');
    let (Some(source), Some(exp), Some(sha), Some(sig)) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(gone());
    };
    let data = format!("{source}.{exp}.{sha}");
    if !st.link_signed(&data, sig) || exp.parse::<i64>().map_or(true, |e| e <= st.now()) {
        return Err(gone());
    }
    let Some(store) = store_of(&st) else {
        return Err(ApiError::unavailable("no object store for setup templates"));
    };
    let described = store
        .describe(source)
        .await
        .map_err(|e| ApiError::unavailable(format!("setup template unreadable: {e}")))?;
    if described.is_none_or(|d| d.sha256 != sha) {
        return Err(gone());
    }
    let bytes = store
        .get(&file_key(source))
        .await
        .map_err(|e| ApiError::unavailable(format!("setup template unreadable: {e}")))?;
    let mut res = (StatusCode::OK, bytes).into_response();
    res.headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    res.headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(res)
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route(
            "/branches/{id}/setup-start",
            get(get_choice).put(put_choice),
        )
        .route("/branches/{id}/setup-start/retry", post(retry))
        .route("/branches/{id}/setup-template/outcome", post(outcome))
        .route("/setup-template/{token}", get(download))
        .with_state(st)
}
