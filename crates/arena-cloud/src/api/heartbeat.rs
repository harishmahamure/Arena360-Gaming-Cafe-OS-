use std::sync::Arc;

use arena_proto::{
    AppError, ErrorCode, HEARTBEAT_MIN_GAP_MS, HUB_TOKEN_PREFIX, HeartbeatFence, HeartbeatRequest,
    HeartbeatResponse, SeqRange, Tier,
};
use arena_store::control::{AdMetric, Beat, HubRow, HubUploadView, RateRule};
use axum::extract::State;
use axum::http::{HeaderMap, header::AUTHORIZATION};
use axum::routing::post;
use axum::{Json, Router};
use sha2::{Digest, Sha256};

use super::account::blocking;
use super::error::ApiError;
use super::json::ApiJson;
use super::pair::check_field;
use super::signup::ceil_s;
use crate::auth::AuthState;
use crate::gaps::{first_missing, parse_batch_key};
use crate::passwords::{dummy_hash, verify_secret};
use crate::presign::{Epoch, MAX_SEQ, branch_prefix};

const TOKEN_CACHE_MS: i64 = 5 * 60 * 1000;
const RATE_PREFIX: &str = "hb:";

fn refused(code: ErrorCode, message: &str) -> ApiError {
    AppError::new(code, message).into()
}

fn invalid() -> ApiError {
    refused(ErrorCode::TokenInvalid, "This Hub's token isn't valid.")
}

pub(super) async fn authenticate(
    st: &AuthState,
    headers: &HeaderMap,
    now: i64,
) -> Result<HubRow, ApiError> {
    let token = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(invalid)?;
    let (id, secret) = token
        .strip_prefix(HUB_TOKEN_PREFIX)
        .and_then(|t| t.split_once('.'))
        .ok_or_else(invalid)?;
    let hub = st.db.hub(id).await.map_err(ApiError::internal)?;
    let key: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    let cached = st
        .hub_tokens
        .lock()
        .map_err(|_| ApiError::internal("hub token cache poisoned"))?
        .get(&key)
        .is_some_and(|until| *until > now);
    let verified = cached || {
        let hash = hub
            .as_ref()
            .map_or_else(|| dummy_hash().to_owned(), |h| h.token_hash.clone());
        let secret = secret.to_owned();
        blocking(move || Ok(verify_secret(&secret, &hash))).await? && hub.is_some()
    };
    let Some(hub) = hub.filter(|_| verified) else {
        return Err(invalid());
    };
    if !cached {
        let mut cache = st
            .hub_tokens
            .lock()
            .map_err(|_| ApiError::internal("hub token cache poisoned"))?;
        cache.retain(|_, until| *until > now);
        cache.insert(key, now + TOKEN_CACHE_MS);
    }
    if hub.revoked_at.is_some() {
        return Err(refused(
            ErrorCode::TokenRevoked,
            "This Hub is no longer paired.",
        ));
    }
    Ok(hub)
}

async fn heartbeat(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<HeartbeatRequest>,
) -> Result<Json<HeartbeatResponse>, ApiError> {
    let now = st.now();
    let hub = authenticate(&st, &headers, now).await?;
    check_field("epoch", &body.epoch)?;
    check_field("version", &body.version)?;
    let key = format!("{RATE_PREFIX}{}", hub.id);
    let rule = RateRule {
        key: &key,
        prune_prefix: RATE_PREFIX,
        bucket_ms: 1,
        window_ms: HEARTBEAT_MIN_GAP_MS - 1,
        limit: 1,
    };
    if let Err(wait_ms) = st
        .db
        .rate_reserve(&[rule], now)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::rate_limited(ceil_s(wait_ms)));
    }
    let Some(fence) = st
        .db
        .heartbeat(&Beat {
            hub_id: &hub.id,
            branch_id: &hub.branch_id,
            epoch: &body.epoch,
            version: &body.version,
            schema_version: body.schema_version,
            now,
        })
        .await
        .map_err(ApiError::internal)?
    else {
        return Err(refused(
            ErrorCode::TokenRevoked,
            "This Hub is no longer paired.",
        ));
    };
    if let Some(schema) = body.command_schema
        && let Err(e) = st.db.record_command_schema(&hub.id, schema).await
    {
        tracing::warn!(error = %e, "heartbeat: command schema not recorded");
    }
    let mut changed = hub.last_seen_at.is_none();
    if changed {
        tracing::info!(hub_id = %hub.id, branch_id = %hub.branch_id, "first hub heartbeat");
    }
    if let Some(setup) = body.setup.as_ref().filter(|s| s.valid()) {
        let json = serde_json::to_string(setup).map_err(ApiError::internal)?;
        let before = st
            .db
            .branch_setup(&hub.branch_id)
            .await
            .map_err(ApiError::internal)?;
        st.db
            .record_setup(&hub.branch_id, &json, setup, now)
            .await
            .map_err(ApiError::internal)?;
        changed |= before.is_none_or(|(b, _)| b != json);
    }
    if changed {
        let _ = st.wizard_tx.send(hub.branch_id.clone());
    }
    let branch = st
        .db
        .branch(&hub.branch_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::internal("hub without its branch"))?;
    let entitlement = st
        .db
        .entitlement(&branch.id)
        .await
        .map_err(ApiError::internal)?;
    let claims =
        crate::licence::claims_for(&branch.tenant_id, &branch.id, entitlement.as_ref(), now);
    let licence = st.licence.sign(&claims).map_err(ApiError::internal)?;
    // The opening hours of a paid branch, for the offline watch (Story 8.35). A block that does
    // not parse is ignored and never fails the heartbeat.
    if claims.tier != Tier::Free
        && let Some(opening) = body.opening.as_ref().filter(|o| o.parse().is_some())
        && let Err(e) = st
            .db
            .record_branch_opening(
                &hub.branch_id,
                &opening.opening_hours,
                &opening.timezone,
                now,
            )
            .await
    {
        tracing::warn!(error = %e, hub_id = %hub.id, "heartbeat: opening hours not recorded");
    }
    let resend = if claims.tier == Tier::Free {
        None
    } else {
        check_upload(&st, &hub, &branch.tenant_id, &body).await
    };
    // Ad totals never hold the heartbeat back: a block that cannot be recorded is simply not
    // acknowledged, and the Hub sends it again.
    let ad_impressions_through = record_ads(
        &st,
        &hub,
        AdMetric::Impressions,
        body.ad_impressions.as_ref(),
    )
    .await;
    let ad_requests_through =
        record_ads(&st, &hub, AdMetric::Requests, body.ad_requests.as_ref()).await;
    let pending = st
        .db
        .pending_commands(&hub.branch_id)
        .await
        .inspect_err(|e| tracing::warn!(error = %e, "heartbeat: pending commands unavailable"))
        .unwrap_or(0);
    let setup_template = super::setup_start::retry_link(&st, &hub.branch_id, now).await;
    Ok(Json(HeartbeatResponse {
        licence,
        fence: HeartbeatFence {
            confirmed: fence == body.epoch,
            epoch: fence,
        },
        cloud_time: now,
        pending_commands: Some(pending).filter(|n| *n > 0),
        ad_impressions_through,
        ad_requests_through,
        setup_template,
        resend,
    }))
}

/// Compares the heartbeat with the cloud's cached view of the Hub's upload (Story 8.3). A
/// match costs nothing. A mismatch lists `changes/<epoch>/` and answers the first run of seqs the
/// Hub says it uploaded that the bucket does not hold. Nothing here fails the heartbeat.
async fn check_upload(
    st: &AuthState,
    hub: &HubRow,
    tenant_id: &str,
    body: &HeartbeatRequest,
) -> Option<SeqRange> {
    let epoch = Epoch::parse(&body.epoch).ok()?;
    if !(0..=MAX_SEQ).contains(&body.last_seq_uploaded) {
        return None;
    }
    let before = st
        .db
        .hub_upload_view(&hub.id)
        .await
        .inspect_err(|e| tracing::warn!(error = %e, hub_id = %hub.id, "heartbeat: no upload view"))
        .ok()?;
    let mut view = before.clone();
    view.last_upload_at = body.last_upload_at.or(view.last_upload_at);
    view.last_summary_at = body.last_summary_at.or(view.last_summary_at);
    if let Some(day) = body.last_rollup_date.as_ref().filter(|d| d.len() <= 10) {
        view.last_rollup_date = Some(day.clone());
    }
    if let Some(u) = body
        .resend_unavailable
        .as_ref()
        .filter(|u| u.epoch == body.epoch && u.range().valid() && u.to <= MAX_SEQ)
        && view
            .unavailable
            .as_ref()
            .is_none_or(|held| held.epoch != u.epoch || held.from > u.from || held.to < u.to)
    {
        view.unavailable = Some(match view.unavailable.take() {
            Some(held) => held.merged(u),
            None => u.clone(),
        });
        st.gap_metrics
            .unavailable(tenant_id, &hub.branch_id, &body.epoch, u.range());
    }
    let resend = find_gap(st, tenant_id, &hub.branch_id, &epoch, body, &mut view).await;
    if view != before
        && let Err(e) = st.db.save_hub_upload_view(&hub.id, &view).await
    {
        tracing::warn!(error = %e, hub_id = %hub.id, "heartbeat: upload view not saved");
    }
    resend
}

async fn find_gap(
    st: &AuthState,
    tenant_id: &str,
    branch_id: &str,
    epoch: &Epoch,
    body: &HeartbeatRequest,
    view: &mut HubUploadView,
) -> Option<SeqRange> {
    if view.epoch_seen.as_deref() == Some(epoch.as_str())
        && view.seq_seen == Some(body.last_seq_uploaded)
    {
        return None;
    }
    let presigner = st.presigner.get()?;
    let prefix = format!(
        "{}changes/{}/",
        branch_prefix(tenant_id, branch_id),
        epoch.as_str()
    );
    let keys = presigner
        .list(&prefix)
        .await
        .inspect_err(
            |e| tracing::warn!(error = %e, branch_id, "heartbeat: could not list the change log"),
        )
        .ok()?;
    let mut held: Vec<(i64, i64)> = keys
        .iter()
        .filter_map(|k| k.strip_prefix(&prefix).and_then(parse_batch_key))
        .collect();
    if let Some(u) = view.unavailable.as_ref().filter(|u| u.epoch == body.epoch) {
        // A run the Hub no longer has is not asked for again.
        held.push((u.from, u.to));
    }
    let base = body
        .fork_seq
        .filter(|s| (0..MAX_SEQ).contains(s))
        .unwrap_or(0)
        + 1;
    let Some(gap) = first_missing(held, base, body.last_seq_uploaded) else {
        view.epoch_seen = Some(epoch.as_str().to_owned());
        view.seq_seen = Some(body.last_seq_uploaded);
        view.resend = None;
        return None;
    };
    let asked = (epoch.as_str().to_owned(), gap);
    // A hole that only shrank because part of a resend landed is still the same gap.
    let shrunk = view
        .resend
        .as_ref()
        .is_some_and(|(e, r)| e == &asked.0 && r.from <= gap.from && gap.to <= r.to);
    if view.resend.as_ref() != Some(&asked) && !shrunk {
        view.gaps_found += 1;
        st.gap_metrics
            .found(tenant_id, branch_id, epoch.as_str(), gap);
        view.resend = Some(asked);
    }
    Some(gap)
}

/// Adds a well-formed block under the branch's cursor and answers the stored cursor, which the
/// Hub moves to (Story 4.14).
async fn record_ads(
    st: &AuthState,
    hub: &HubRow,
    metric: AdMetric,
    block: Option<&arena_proto::AdCounts>,
) -> Option<i64> {
    let block = block.filter(|b| b.valid())?;
    st.db
        .record_ads(&hub.branch_id, metric, block)
        .await
        .inspect_err(
            |e| tracing::warn!(error = %e, hub_id = %hub.id, "heartbeat: ad totals not recorded"),
        )
        .ok()
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/heartbeat", post(heartbeat))
        .with_state(st)
}
