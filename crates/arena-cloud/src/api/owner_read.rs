//! The owner read API (Story 8.9, AD-35): `GET /v1/branches/{id}/live` and
//! `GET /v1/branches/{id}/summary?from=&to=&group_by=`, with `all` in place of the id for every
//! paid branch of the owner's café.
//!
//! The cloud reads the Hub's own files from the data bucket (`state/latest.json`,
//! `rollups/<date>.json`) through [`crate::owner_read::ReadCache`] and sums them in memory; it
//! never queries branch rows. The bucket key is built from the session's tenant and the branch
//! row found under it, never from the request. A branch that is not the owner's café's answers
//! `NOT_FOUND` (nothing tells it apart from one that does not exist), a Free branch
//! `FEATURE_NOT_ENTITLED`, and a tenant over [`OWNER_API_LIMIT_PER_MIN`] calls a minute
//! `RATE_LIMITED` with `Retry-After`.

use std::collections::BTreeMap;
use std::sync::Arc;

use arena_proto::{
    AppError, BRANCH_ONLINE_WINDOW_MS, DataAgeReport, ErrorCode, GroupBy, OWNER_ALL_BRANCHES,
    OWNER_API_LIMIT_PER_MIN, OWNER_RANGE_MAX_DAYS, OwnerLive, OwnerLiveAll, OwnerRange,
    OwnerRangeAll, OwnerRangeRow, OwnerRangeTotals, Tier, dates_between, is_rollup_date,
};
use arena_store::control::{Branch, RateRule};
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use super::branches::tier_of;
use super::error::ApiError;
use super::gate::RequireSetupComplete;
use super::json::ApiJson;
use super::signup::ceil_s;
use crate::auth::AuthState;
use crate::presign::{Presigner, has_path_chars};

const RATE_PREFIX: &str = "ownerread:";
const WINDOW_MS: i64 = 60 * 1000;
const BUCKET_MS: i64 = 1000;

#[derive(Debug, Deserialize)]
struct RangeQuery {
    from: Option<String>,
    to: Option<String>,
    group_by: Option<String>,
}

fn not_entitled() -> ApiError {
    AppError::new(
        ErrorCode::FeatureNotEntitled,
        "Branch figures are available on Basic.",
    )
    .into()
}

/// The branches a call is about, after the tenant's rate limit and the entitlement check.
async fn branches_for(
    st: &AuthState,
    owner_id: &str,
    id: &str,
    now: i64,
) -> Result<Vec<Branch>, ApiError> {
    let tenant = st
        .db
        .tenant_of_owner(owner_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    let key = format!("{RATE_PREFIX}{}", tenant.id);
    let rule = RateRule {
        key: &key,
        prune_prefix: RATE_PREFIX,
        bucket_ms: BUCKET_MS,
        window_ms: WINDOW_MS,
        limit: OWNER_API_LIMIT_PER_MIN,
    };
    if let Err(wait_ms) = st
        .db
        .rate_reserve(&[rule], now)
        .await
        .map_err(ApiError::internal)?
    {
        tracing::warn!(tenant_id = %tenant.id, "owner read refused: rate limited");
        return Err(ApiError::rate_limited(ceil_s(wait_ms)));
    }
    let branches = st
        .db
        .branches_of_tenant(&tenant.id)
        .await
        .map_err(ApiError::internal)?;
    let mut picked = Vec::new();
    if id == OWNER_ALL_BRANCHES {
        for b in branches {
            if tier_of(st, &b.id).await? != Tier::Free {
                picked.push(b);
            }
        }
        if picked.is_empty() {
            return Err(not_entitled());
        }
    } else {
        let b = branches.into_iter().find(|b| b.id == id).ok_or_else(|| {
            tracing::warn!(owner_id, "owner read: branch outside the owner's tenant");
            ApiError::not_found()
        })?;
        if tier_of(st, &b.id).await? == Tier::Free {
            return Err(not_entitled());
        }
        picked.push(b);
    }
    for b in &picked {
        if has_path_chars(&b.tenant_id) || has_path_chars(&b.id) {
            return Err(ApiError::internal(
                "tenant or branch id with path characters",
            ));
        }
    }
    Ok(picked)
}

fn bucket(st: &AuthState) -> Result<&Presigner, ApiError> {
    st.presigner
        .get()
        .ok_or_else(|| ApiError::unavailable("no data bucket for owner reads"))
}

async fn live_of(
    st: &AuthState,
    presigner: &Presigner,
    b: &Branch,
    now: i64,
) -> Result<OwnerLive, ApiError> {
    let summary = st
        .read_cache
        .live(presigner, &b.tenant_id, &b.id, now)
        .await
        .map_err(ApiError::unavailable)?
        // A summary in another currency than the branch's is not this branch's figure (as with
        // a rollup day): it is not served, so `all` never totals it under the wrong currency.
        .filter(|s| s.currency == b.currency && s.branch_id == b.id);
    let last_heartbeat_at = st
        .db
        .active_hub(&b.id)
        .await
        .map_err(ApiError::internal)?
        .and_then(|h| h.last_seen_at);
    Ok(OwnerLive {
        branch_id: b.id.clone(),
        currency: b.currency.clone(),
        as_of: summary.as_ref().map(|s| s.as_of),
        branch_online: last_heartbeat_at.is_some_and(|t| now - t <= BRANCH_ONLINE_WINDOW_MS),
        last_heartbeat_at,
        summary: summary.map(|s| (*s).clone()),
    })
}

async fn live(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let now = st.now();
    let branches = branches_for(&st, &signed.owner.id, &id, now).await?;
    let presigner = bucket(&st)?;
    let mut out = Vec::with_capacity(branches.len());
    for b in &branches {
        out.push(live_of(&st, presigner, b, now).await?);
    }
    Ok(if id == OWNER_ALL_BRANCHES {
        Json(OwnerLiveAll::new(out)).into_response()
    } else {
        Json(out.remove(0)).into_response()
    })
}

/// A valid range: both dates real, `from` not after `to`, at most [`OWNER_RANGE_MAX_DAYS`].
struct Range {
    from: String,
    to: String,
    group_by: GroupBy,
    dates: Vec<String>,
}

fn parse_range(q: Result<Query<RangeQuery>, QueryRejection>) -> Result<Range, ApiError> {
    let Query(q) = q.map_err(|_| ApiError::validation("from", "Send from, to and group_by."))?;
    let date = |field: &str, v: Option<String>| match v {
        Some(d) if is_rollup_date(&d) => Ok(d),
        _ => Err(ApiError::validation(field, "Send a date like 2026-10-29.")),
    };
    let from = date("from", q.from)?;
    let to = date("to", q.to)?;
    let group_by = match q.group_by.as_deref() {
        None => GroupBy::Day,
        Some(g) => GroupBy::parse(g)
            .ok_or_else(|| ApiError::validation("group_by", "Choose day, week or month."))?,
    };
    let dates = dates_between(&from, &to);
    if dates.is_empty() {
        return Err(ApiError::validation(
            "to",
            "The end date can't be before the start date.",
        ));
    }
    if i64::try_from(dates.len()).unwrap_or(i64::MAX) > OWNER_RANGE_MAX_DAYS {
        return Err(ApiError::validation(
            "to",
            format!("Choose a range of up to {OWNER_RANGE_MAX_DAYS} days."),
        ));
    }
    Ok(Range {
        from,
        to,
        group_by,
        dates,
    })
}

async fn range_of(
    st: &AuthState,
    presigner: &Presigner,
    b: &Branch,
    r: &Range,
    now: i64,
) -> Result<OwnerRange, ApiError> {
    let files = st
        .read_cache
        .rollups(
            presigner,
            &st.rollup_metrics,
            &b.tenant_id,
            &b.id,
            &r.dates,
            now,
        )
        .await
        .map_err(ApiError::unavailable)?;
    let mut totals = OwnerRangeTotals::default();
    let mut periods: BTreeMap<String, OwnerRangeTotals> = BTreeMap::new();
    let mut missing_days = Vec::new();
    for d in &r.dates {
        let period = r
            .group_by
            .period_of(d)
            .ok_or_else(|| ApiError::internal("a range date without a period"))?;
        let row = periods.entry(period).or_default();
        // A day written in another currency is not this branch's figure: it counts as missing
        // rather than being added to the wrong one.
        match files
            .get(d)
            .filter(|f| f.currency == b.currency && f.branch_id == b.id && f.business_day == *d)
        {
            Some(f) => {
                row.add_rollup(f);
                totals.add_rollup(f);
            }
            None => missing_days.push(d.clone()),
        }
    }
    Ok(OwnerRange {
        branch_id: b.id.clone(),
        currency: b.currency.clone(),
        from: r.from.clone(),
        to: r.to.clone(),
        group_by: r.group_by,
        totals,
        rows: periods
            .into_iter()
            .map(|(period, totals)| OwnerRangeRow { period, totals })
            .collect(),
        missing_days,
    })
}

async fn summary(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    Path(id): Path<String>,
    query: Result<Query<RangeQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let range = parse_range(query)?;
    let now = st.now();
    let branches = branches_for(&st, &signed.owner.id, &id, now).await?;
    let presigner = bucket(&st)?;
    let mut out = Vec::with_capacity(branches.len());
    for b in &branches {
        out.push(range_of(&st, presigner, b, &range, now).await?);
    }
    Ok(if id == OWNER_ALL_BRANCHES {
        Json(OwnerRangeAll::new(out)).into_response()
    } else {
        Json(out.remove(0)).into_response()
    })
}

/// Seconds a figure's age may be before the report is refused as nonsense (a year).
const DATA_AGE_MAX_S: u64 = 365 * 24 * 60 * 60;

/// `POST /v1/branches/{id}/data-age`: the app reports how old a branch's figures were when Today
/// opened. Only the branch, the age and the tier (the cloud's own) are kept.
async fn data_age(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<DataAgeReport>,
) -> Result<StatusCode, ApiError> {
    if id == OWNER_ALL_BRANCHES {
        return Err(ApiError::not_found());
    }
    if body.age_s > DATA_AGE_MAX_S {
        return Err(ApiError::validation("age_s", "That age is too large."));
    }
    let branches = branches_for(&st, &signed.owner.id, &id, st.now()).await?;
    let b = branches
        .first()
        .ok_or_else(|| ApiError::internal("no branch after the tenant check"))?;
    let tier = tier_of(&st, &b.id).await?;
    st.data_age.record(&b.tenant_id, &b.id, tier, body.age_s);
    Ok(StatusCode::NO_CONTENT)
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/branches/{id}/live", get(live))
        .route("/branches/{id}/summary", get(summary))
        .route("/branches/{id}/data-age", post(data_age))
        .with_state(st)
}
