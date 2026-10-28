//! `GET /v1/catalog/manifest?since=<version>` (Story 6.1): the published games changed since a
//! Hub's catalogue version, with their signatures and media. Device-token auth like the
//! heartbeat; a revoked Hub is 403, and a tenant is held to [`CATALOG_MANIFEST_PER_MIN`] calls a
//! minute. Auth and the limit run before any catalogue row is read.

use std::sync::Arc;

use arena_proto::{CATALOG_MANIFEST_PATH, CATALOG_MANIFEST_PER_MIN, ErrorCode};
use arena_store::control::RateRule;
use axum::Json;
use axum::Router;
use axum::extract::{Query, State};
use axum::http::header::{CACHE_CONTROL, ETAG, IF_NONE_MATCH};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::Deserialize;

use super::error::ApiError;
use super::heartbeat::authenticate;
use super::signup::ceil_s;
use crate::auth::AuthState;

const RATE_PREFIX: &str = "catalog:";
const WINDOW_MS: i64 = 60_000;

#[derive(Deserialize)]
struct ManifestQuery {
    since: Option<u64>,
}

fn etag(since: i64, version: i64) -> String {
    format!("\"{since}-{version}\"")
}

async fn manifest(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    Query(q): Query<ManifestQuery>,
) -> Result<Response, ApiError> {
    let now = st.now();
    let hub = match authenticate(&st, &headers, now).await {
        Ok(hub) => hub,
        // A revoked device token is 403 here; no token or an unknown one stays 401.
        Err(e) if e.body().code == ErrorCode::TokenRevoked => {
            return Err(ApiError::forbidden("This Hub is no longer paired."));
        }
        Err(e) => return Err(e),
    };
    let branch = st
        .db
        .branch(&hub.branch_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::internal("hub without its branch"))?;
    let key = format!("{RATE_PREFIX}{}", branch.tenant_id);
    let rule = RateRule {
        key: &key,
        prune_prefix: RATE_PREFIX,
        bucket_ms: 1_000,
        window_ms: WINDOW_MS,
        limit: CATALOG_MANIFEST_PER_MIN,
    };
    if let Err(wait_ms) = st
        .db
        .rate_reserve(&[rule], now)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::rate_limited(ceil_s(wait_ms)));
    }
    let Some(cdn) = st.catalog_cdn.get() else {
        return Err(ApiError::unavailable("no CDN base URL for catalogue media"));
    };
    let since = i64::try_from(q.since.unwrap_or(0)).unwrap_or(i64::MAX);
    let version = st.db.catalog_version().await.map_err(ApiError::internal)?;
    let tag = etag(since.min(version), version);
    let tag_value = HeaderValue::from_str(&tag).map_err(ApiError::internal)?;
    let matches = headers
        .get(IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == tag));
    if since >= version || matches {
        return Ok((
            StatusCode::NOT_MODIFIED,
            [(ETAG, tag_value), (CACHE_CONTROL, no_cache())],
        )
            .into_response());
    }
    let rows = st
        .db
        .catalog_between(since, version)
        .await
        .map_err(ApiError::internal)?;
    let body = crate::catalog::manifest(rows, version, since, cdn);
    Ok(([(ETAG, tag_value), (CACHE_CONTROL, no_cache())], Json(body)).into_response())
}

fn no_cache() -> HeaderValue {
    HeaderValue::from_static("private, max-age=0, must-revalidate")
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route(CATALOG_MANIFEST_PATH, get(manifest))
        .with_state(st)
}
