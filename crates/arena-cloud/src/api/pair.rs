use std::net::SocketAddr;
use std::sync::Arc;

use arena_proto::{
    AppError, ErrorCode, HUB_TOKEN_PREFIX, HubPairRequest, HubPairResponse, HubPlatform,
    PAIR_ATTEMPTS_PER_IP_PER_HOUR, PAIR_FAILURES_PER_CODE, Tier, normalize_pairing_code,
    valid_pairing_code,
};
use arena_store::control::{Branch, Entitlement, NewHub, PairOutcome, PairingCodeRow, RateRule};
use axum::extract::connect_info::ConnectInfo;
use axum::extract::{Extension, State};
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use super::account::blocking;
use super::error::ApiError;
use super::json::ApiJson;
use super::signup::ceil_s;
use crate::auth::{AuthState, random_bytes};
use crate::client_ip::client_ip;
use crate::passwords::hash_secret;

const MINUTE_MS: i64 = 60 * 1000;
const HOUR_MS: i64 = 60 * MINUTE_MS;
const IP_PREFIX: &str = "pair_ip:";
const CODE_PREFIX: &str = "pair_code:";
const MAX_FIELD_CHARS: usize = 128;

pub fn ip_key(ip: &str) -> String {
    format!("{IP_PREFIX}{ip}")
}

pub fn code_failures_key(code_hash: &str) -> String {
    format!("{CODE_PREFIX}{code_hash}")
}

fn code_rule(key: &str) -> RateRule<'_> {
    RateRule {
        key,
        prune_prefix: CODE_PREFIX,
        bucket_ms: MINUTE_MS,
        window_ms: HOUR_MS,
        limit: u64::from(PAIR_FAILURES_PER_CODE),
    }
}

fn invalid() -> ApiError {
    AppError::new(
        ErrorCode::PairCodeInvalid,
        "That code didn't work. Get a new code from your browser.",
    )
    .into()
}

fn expired() -> ApiError {
    AppError::new(
        ErrorCode::PairCodeExpired,
        "That code has expired. Get a new code from your browser.",
    )
    .into()
}

fn not_entitled() -> ApiError {
    AppError::with_field(
        ErrorCode::FeatureNotEntitled,
        "platform",
        "The Free plan runs on the Android Hub.",
    )
    .into()
}

fn check_field(field: &str, v: &str) -> Result<(), ApiError> {
    let n = v.chars().count();
    if n == 0 || n > MAX_FIELD_CHARS || v.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(ApiError::validation(
            field,
            format!("Send 1–{MAX_FIELD_CHARS} characters without spaces."),
        ));
    }
    Ok(())
}

async fn reserve_ip(st: &AuthState, ip: &str, now: i64) -> Result<(), ApiError> {
    let key = ip_key(ip);
    let rule = RateRule {
        key: &key,
        prune_prefix: IP_PREFIX,
        bucket_ms: MINUTE_MS,
        window_ms: HOUR_MS,
        limit: u64::from(PAIR_ATTEMPTS_PER_IP_PER_HOUR),
    };
    match st
        .db
        .rate_reserve(&[rule], now)
        .await
        .map_err(ApiError::internal)?
    {
        Ok(_) => Ok(()),
        Err(wait_ms) => {
            let retry = ceil_s(wait_ms);
            tracing::warn!(retry_after_s = retry, "hub pairing ip limit reached");
            Err(ApiError::rate_limited(retry))
        }
    }
}

async fn record_failure(st: &AuthState, code: &PairingCodeRow, now: i64) {
    let key = code_failures_key(&code.code_hash);
    let rule = code_rule(&key);
    let since = now - HOUR_MS - MINUTE_MS;
    let result = async {
        let _ = st.db.rate_reserve(&[rule], now).await?;
        st.db
            .invalidate_failed_pairing_code(&code.code_hash, &key, since, rule.limit, now)
            .await
    }
    .await;
    match result {
        Ok(true) => tracing::warn!(
            branch_id = %code.branch_id,
            pairing_code_id = %code.id,
            "hub pairing code invalidated after repeated failures"
        ),
        Ok(false) => {}
        Err(e) => tracing::warn!(detail = %e, "could not record a hub pairing failure"),
    }
}

struct Candidate {
    code: PairingCodeRow,
    branch: Branch,
    entitlement: Option<Entitlement>,
    tier: Tier,
}

async fn classify(
    st: &AuthState,
    code_hash: &str,
    platform: HubPlatform,
    now: i64,
) -> Result<Candidate, ApiError> {
    let Some(code) = st
        .db
        .pairing_code_by_hash(code_hash)
        .await
        .map_err(ApiError::internal)?
    else {
        return Err(invalid());
    };
    if code.used_at.is_some() || code.invalidated_at.is_some() {
        return Err(invalid());
    }
    if now >= code.expires_at {
        return Err(expired());
    }
    let branch = st
        .db
        .branch(&code.branch_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::internal("pairing code without its branch"))?;
    let entitlement = st
        .db
        .entitlement(&branch.id)
        .await
        .map_err(ApiError::internal)?;
    let tier = entitlement.as_ref().map_or(Tier::Free, |e| e.tier_at(now));
    if platform == HubPlatform::Windows && tier == Tier::Free {
        record_failure(st, &code, now).await;
        tracing::info!(
            branch_id = %branch.id,
            pairing_code_id = %code.id,
            platform = platform.as_str(),
            "hub pairing refused: windows hub on free"
        );
        return Err(not_entitled());
    }
    Ok(Candidate {
        code,
        branch,
        entitlement,
        tier,
    })
}

async fn pair(
    State(st): State<Arc<AuthState>>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<HubPairRequest>,
) -> Result<Json<HubPairResponse>, ApiError> {
    let now = st.now();
    let ip = client_ip(peer.map(|Extension(ConnectInfo(a))| a), &headers).map_or_else(
        || "unknown".to_owned(),
        |ip| match ip.to_canonical() {
            std::net::IpAddr::V6(v6) => {
                std::net::Ipv6Addr::from(u128::from(v6) & (!0u128 << 64)).to_string()
            }
            v4 => v4.to_string(),
        },
    );
    reserve_ip(&st, &ip, now).await?;
    check_field("hub_instance_id", &body.hub_instance_id)?;
    check_field("version", &body.version)?;
    let code = normalize_pairing_code(&body.code);
    if !valid_pairing_code(&code) {
        return Err(invalid());
    }
    let code_hash = st.pairing_code_hash(&code);
    let c = classify(&st, &code_hash, body.platform, now).await?;

    let hub_id = ulid::Ulid::generate().to_string();
    let secret = URL_SAFE_NO_PAD.encode(random_bytes::<32>().map_err(ApiError::internal)?);
    let token = format!("{HUB_TOKEN_PREFIX}{hub_id}.{secret}");
    let token_hash = blocking(move || hash_secret(&secret)).await?;
    let claims = crate::licence::claims_for(
        &c.branch.tenant_id,
        &c.branch.id,
        c.entitlement.as_ref(),
        now,
    );
    let licence = st.licence.sign(&claims).map_err(ApiError::internal)?;

    let outcome = st
        .db
        .pair_hub(&NewHub {
            id: &hub_id,
            branch_id: &c.branch.id,
            platform: body.platform,
            hub_instance_id: &body.hub_instance_id,
            version: &body.version,
            token_hash: &token_hash,
            code_hash: &code_hash,
            now,
        })
        .await
        .map_err(ApiError::internal)?;
    let revoked = match outcome {
        PairOutcome::Paired { revoked } => revoked,
        PairOutcome::CodeRefused => {
            tracing::info!(
                branch_id = %c.branch.id,
                pairing_code_id = %c.code.id,
                "hub pairing lost to a concurrent use of the code"
            );
            return Err(classify(&st, &code_hash, body.platform, now)
                .await
                .err()
                .unwrap_or_else(invalid));
        }
    };
    tracing::info!(
        hub_id = %hub_id,
        tenant_id = %c.branch.tenant_id,
        branch_id = %c.branch.id,
        pairing_code_id = %c.code.id,
        platform = body.platform.as_str(),
        tier = c.tier.as_str(),
        revoked_hubs = revoked.len(),
        "hub paired"
    );
    Ok(Json(HubPairResponse {
        hub_id,
        token,
        tenant_id: c.branch.tenant_id,
        branch_id: c.branch.id,
        tier: claims.tier,
        branch_code: c.branch.branch_code,
        licence,
    }))
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new().route("/pair", post(pair)).with_state(st)
}
