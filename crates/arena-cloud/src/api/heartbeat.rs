use std::sync::Arc;

use arena_proto::{
    AppError, ErrorCode, HEARTBEAT_MIN_GAP_MS, HUB_TOKEN_PREFIX, HeartbeatFence, HeartbeatRequest,
    HeartbeatResponse, HubPlatform, LicenceClaims, SeqRange, TUNNEL_FLAG, Tier, TunnelAnswer,
    TunnelReport, tunnel_hostname,
};
use arena_store::control::{
    AdMetric, Beat, BranchTunnel, FenceMove, HubRow, HubUploadView, NewBranchTunnel, RateRule,
    TunnelRowState,
};
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
/// The most batches a heartbeat's backup block may report waiting (a bound on what is stored).
const MAX_PENDING_BATCHES: i64 = 1_000_000;
/// How long the tunnel step may hold a heartbeat answer (the Hub's request times out at 15 s).
const TUNNEL_BUDGET: std::time::Duration = std::time::Duration::from_secs(8);

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
            fork_seq: body.fork_seq.filter(|s| (0..=MAX_SEQ).contains(s)),
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
    let fence = settle_fence(&st, &hub, &body, fence, now).await?;
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
    // Cloudflare is slow now and then; the licence, fence and commands in this answer must not
    // wait on it past the Hub's own request timeout. A late answer is "change nothing".
    let tunnel = tokio::time::timeout(
        TUNNEL_BUDGET,
        tunnel_answer(&st, &hub, &claims, body.tunnel, now),
    )
    .await
    .unwrap_or_else(|_| {
        tracing::warn!(hub_id = %hub.id, "heartbeat: the tunnel step ran out of time");
        None
    });
    Ok(Json(HeartbeatResponse {
        licence,
        fence,
        cloud_time: now,
        pending_commands: Some(pending).filter(|n| *n > 0),
        ad_impressions_through,
        ad_requests_through,
        setup_template,
        resend,
        tunnel,
    }))
}

/// Settles the Hub's epoch against the branch fence (Story 9.3). `fence` is what the beat found
/// (the first epoch reported, when there was none).
///
/// * The fence is the Hub's epoch: confirmed.
/// * The Hub's counter is above the fence's: the fence moves to the Hub's epoch by compare-and-set
///   from exactly the fence read (with the beat's `fork_seq`), and the Hub is confirmed.
/// * The same counter with another instance, a Hub counter below the fence's, or a lost race for
///   the move: not confirmed, and the answer carries the fence (the winner's, when it moved).
async fn settle_fence(
    st: &AuthState,
    hub: &HubRow,
    body: &HeartbeatRequest,
    fence: String,
    now: i64,
) -> Result<HeartbeatFence, ApiError> {
    if fence == body.epoch {
        return Ok(HeartbeatFence {
            confirmed: true,
            epoch: fence,
        });
    }
    let above = match (Epoch::parse(&body.epoch), Epoch::parse(&fence)) {
        (Ok(own), Ok(held)) => own.counter() > held.counter(),
        _ => false,
    };
    if !above {
        return Ok(HeartbeatFence {
            confirmed: false,
            epoch: fence,
        });
    }
    let fork_seq = body.fork_seq.filter(|s| (0..=MAX_SEQ).contains(s));
    match st
        .db
        .claim_branch_fence(&hub.branch_id, &hub.id, &fence, &body.epoch, fork_seq, now)
        .await
        .map_err(ApiError::internal)?
    {
        FenceMove::Moved { requeued } => {
            tracing::info!(
                hub_id = %hub.id,
                branch_id = %hub.branch_id,
                epoch = %body.epoch,
                previous = %fence,
                requeued,
                "fence advanced to the hub's epoch"
            );
            Ok(HeartbeatFence {
                confirmed: true,
                epoch: body.epoch.clone(),
            })
        }
        FenceMove::Stale { current } => {
            let epoch = current.unwrap_or(fence);
            Ok(HeartbeatFence {
                confirmed: epoch == body.epoch,
                epoch,
            })
        }
    }
}

/// What the Hub is to do about its tunnel (Story 8.39), decided from the signed licence flags and
/// the paired Hub's platform. A provider failure never fails the heartbeat: the answer is `off`
/// and the next heartbeat tries again (the row's state makes create and delete repeatable). The
/// credential is fetched when the tunnel is created or when the Hub reports it holds none, handed
/// out in the answer and never stored or logged. `None` only when the cloud's own database could
/// not be read, which tells the Hub to change nothing.
async fn tunnel_answer(
    st: &AuthState,
    hub: &HubRow,
    claims: &LicenceClaims,
    report: Option<TunnelReport>,
    now: i64,
) -> Option<TunnelAnswer> {
    let row = st
        .db
        .branch_tunnel(&hub.branch_id)
        .await
        .inspect_err(|e| tracing::warn!(error = %e, hub_id = %hub.id, "heartbeat: no tunnel row"))
        .ok()?;
    let entitled = claims.tier != Tier::Free && claims.flags.iter().any(|f| f == TUNNEL_FLAG);
    if !entitled || hub.platform != HubPlatform::Windows {
        if let Some(row) = row {
            remove_tunnel(st, &row, now).await;
        }
        return Some(if entitled {
            TunnelAnswer::UnsupportedPlatform
        } else {
            TunnelAnswer::Off
        });
    }
    // A failure on the cloud's side is no reason to take down a connector the Hub already runs:
    // a Hub that holds its credential is told nothing, one that does not is told `off`.
    let unsettled = || {
        if report.is_some_and(|r| r.has_credential) {
            None
        } else {
            Some(TunnelAnswer::Off)
        }
    };
    let Some(provider) = st.tunnels.get() else {
        tracing::warn!(hub_id = %hub.id, "heartbeat: a branch is entitled to a tunnel and none is configured");
        return unsettled();
    };
    // The flag came back while a delete was still being retried: finish it first.
    let row = match row {
        Some(row) if row.state == TunnelRowState::Deleting => {
            if !remove_tunnel(st, &row, now).await {
                return Some(TunnelAnswer::Off);
            }
            None
        }
        other => other,
    };
    let (hostname, tunnel_id, delivered) = match row {
        Some(row) => (row.hostname, row.tunnel_id, row.delivered_at.is_some()),
        None => {
            let hostname = tunnel_hostname(&hub.branch_id);
            let made = match provider.ensure(&hub.branch_id, &hostname).await {
                Ok(made) => made,
                Err(e) => {
                    tracing::warn!(error = %e, hub_id = %hub.id, "heartbeat: tunnel not created");
                    return unsettled();
                }
            };
            if let Err(e) = st
                .db
                .insert_branch_tunnel(&NewBranchTunnel {
                    branch_id: &hub.branch_id,
                    hostname: &hostname,
                    tunnel_id: &made.tunnel_id,
                    access_app_id: &made.access_app_id,
                    now,
                })
                .await
            {
                tracing::warn!(error = %e, hub_id = %hub.id, "heartbeat: tunnel not recorded");
                return unsettled();
            }
            tracing::info!(branch_id = %hub.branch_id, "tunnel created");
            (hostname, made.tunnel_id, false)
        }
    };
    // A Hub that sends no report (an older build) is not asked to take the credential again.
    if delivered && report.is_none_or(|r| r.has_credential) {
        return Some(TunnelAnswer::On { hostname });
    }
    match provider.credential(&tunnel_id).await {
        Ok(credential) => {
            if let Err(e) = st.db.mark_tunnel_delivered(&hub.branch_id, now).await {
                tracing::warn!(error = %e, hub_id = %hub.id, "heartbeat: tunnel delivery not recorded");
            }
            Some(TunnelAnswer::Grant {
                hostname,
                credential,
            })
        }
        Err(e) => {
            tracing::warn!(error = %e, hub_id = %hub.id, "heartbeat: no tunnel credential");
            unsettled()
        }
    }
}

/// Deletes the Access application, the DNS name and the tunnel of `row`, then the row. The row
/// stays `deleting` when the provider fails (or there is none), and the next heartbeat retries.
/// Answers whether the tunnel is gone.
async fn remove_tunnel(st: &AuthState, row: &BranchTunnel, now: i64) -> bool {
    if row.state != TunnelRowState::Deleting
        && let Err(e) = st
            .db
            .set_tunnel_state(&row.branch_id, TunnelRowState::Deleting, now)
            .await
    {
        tracing::warn!(error = %e, branch_id = %row.branch_id, "heartbeat: tunnel not marked deleting");
        return false;
    }
    let Some(provider) = st.tunnels.get() else {
        return false;
    };
    let what = crate::tunnel::Provisioned {
        tunnel_id: row.tunnel_id.clone(),
        access_app_id: row.access_app_id.clone(),
    };
    if let Err(e) = provider.delete(&row.branch_id, &row.hostname, &what).await {
        tracing::warn!(error = %e, branch_id = %row.branch_id, "heartbeat: tunnel not deleted");
        return false;
    }
    match st.db.delete_branch_tunnel(&row.branch_id).await {
        Ok(()) => {
            tracing::info!(branch_id = %row.branch_id, "tunnel deleted");
            true
        }
        Err(e) => {
            tracing::warn!(error = %e, branch_id = %row.branch_id, "heartbeat: tunnel row not removed");
            false
        }
    }
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
    // A paid Hub's backup block replaces the three fields each time, so a failure the Hub no
    // longer reports leaves with it (Story 9.17). An older Hub or a Free one sends none.
    if let Some(b) = &body.backup {
        view.last_snapshot_at = b.last_snapshot_at.filter(|t| *t >= 0);
        view.pending_batches = Some(b.pending_batches.clamp(0, MAX_PENDING_BATCHES));
        view.failed_at = b.failed_at.filter(|t| *t >= 0);
    }
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
    // History before the fork belongs to the parent epoch; history before the upgrade snapshot
    // (Story 9.11) is in the snapshot, not in batches.
    let base = [body.fork_seq, body.base_seq]
        .into_iter()
        .flatten()
        .filter(|s| (0..MAX_SEQ).contains(s))
        .max()
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
