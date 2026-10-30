use std::net::SocketAddr;
use std::sync::Arc;

use arena_proto::{
    AppError, BOOTSTRAP_DAY_START_MIN, BootstrapOwner, BootstrapProfile, BranchBootstrapCommand,
    ErrorCode, HUB_TOKEN_PREFIX, HubPairRequest, HubPairResponse, HubPlatform, HubRole, OWNER_ROLE,
    PAIR_ATTEMPTS_PER_IP_PER_HOUR, PAIR_FAILURES_PER_CODE, SETUP_CODE_HUB_TTL_MS, TaxSettings,
    Tier, check_bootstrap, normalize_pairing_code, valid_pairing_code,
};
use arena_store::control::{
    Branch, Entitlement, NewHub, NewSetupCode, Owner, PairOutcome, PairingCodeRow, RateRule,
};
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
use crate::pairing_code::new_pairing_code;
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

fn wrong_role(code: HubRole) -> ApiError {
    let message = match code {
        HubRole::Standby => "That is a standby code. Enter it on the standby PC.",
        HubRole::Main => {
            "That is a Hub code, not a standby code. Get a standby code from your browser."
        }
    };
    AppError::new(ErrorCode::PairCodeInvalid, message).into()
}

fn not_entitled() -> ApiError {
    AppError::with_field(
        ErrorCode::FeatureNotEntitled,
        "platform",
        "The Free plan runs on the Android Hub.",
    )
    .into()
}

pub(super) fn check_field(field: &str, v: &str) -> Result<(), ApiError> {
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
    role: HubRole,
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
    if code.role != role {
        return Err(wrong_role(code.role));
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
    if role == HubRole::Standby {
        // A standby is a second Windows PC on a plan that includes it (Story 9.18).
        if platform != HubPlatform::Windows {
            return Err(AppError::new(
                ErrorCode::PlatformUnsupported,
                "A standby needs the Windows Hub.",
            )
            .into());
        }
        if !tier.defaults().flags.contains(&"standby") {
            return Err(AppError::new(
                ErrorCode::TierLocked,
                "A standby Hub is available on Premium.",
            )
            .into());
        }
    }
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

fn bootstrap_key(branch_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(format!("bootstrap\0{branch_id}").as_bytes());
    let mut b = [0u8; 16];
    b.copy_from_slice(&d[..16]);
    b[0] &= 0x7f;
    ulid::Ulid::from(u128::from_be_bytes(b)).to_string()
}

fn bootstrap_of(
    branch: &Branch,
    owner: &Owner,
    setup_code_hash: String,
    setup_expires_at: i64,
) -> Result<BranchBootstrapCommand, ApiError> {
    let gstin = branch.gstin.clone().filter(|_| branch.gst_registered);
    let tax = TaxSettings {
        country: branch.country.clone(),
        gst: branch.gst_registered && branch.country == "IN",
        gstin: gstin.unwrap_or_default(),
        ..TaxSettings::parse(arena_proto::DEFAULT_TAX).ok_or_else(|| ApiError::internal("tax"))?
    };
    let command = BranchBootstrapCommand {
        idempotency_key: bootstrap_key(&branch.id),
        profile: BootstrapProfile {
            name: branch.name.clone(),
            branch_code: branch.branch_code.clone(),
            currency: branch.currency.clone(),
            tz: branch.timezone.clone(),
            day_start_min: BOOTSTRAP_DAY_START_MIN,
            country: branch.country.clone(),
            gst_registered: tax.gst,
            gstin: tax.gstin.clone(),
            tax_mode: tax.tax_mode(),
        },
        owner_staff: BootstrapOwner {
            name: owner.name.clone(),
            login: owner.phone_e164.clone(),
            role: OWNER_ROLE.to_owned(),
            setup_code_hash,
            setup_expires_at,
        },
        template: None,
    };
    let p = &command.profile;
    check_bootstrap(
        &p.branch_code,
        &p.country,
        p.gst_registered,
        &p.gstin,
        p.tax_mode,
        &command.owner_staff,
    )
    .map_err(|(field, message)| ApiError::internal(format!("bootstrap {field}: {message}")))?;
    Ok(command)
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
    let c = classify(&st, &code_hash, body.platform, body.role, now).await?;

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

    let standby = body.role == HubRole::Standby;
    // One PC is one role: a standby joins beside a live Windows main Hub that is another machine,
    // and a main Hub cannot be the machine that is already the standby. Refused before the code
    // is spent.
    let main = st
        .db
        .active_hub(&c.branch.id)
        .await
        .map_err(ApiError::internal)?;
    if standby {
        let Some(main) = main else {
            return Err(AppError::new(ErrorCode::StepNotReady, "Pair the main Hub first.").into());
        };
        if main.platform != HubPlatform::Windows {
            return Err(AppError::new(
                ErrorCode::PlatformUnsupported,
                "A standby needs the Windows Hub.",
            )
            .into());
        }
        if main.hub_instance_id == body.hub_instance_id {
            return Err(AppError::new(
                ErrorCode::PairCodeInvalid,
                "This PC is already the main Hub. Use a second PC as the standby.",
            )
            .into());
        }
    } else if st
        .db
        .standby_hub(&c.branch.id)
        .await
        .map_err(ApiError::internal)?
        .is_some_and(|h| h.hub_instance_id == body.hub_instance_id)
    {
        return Err(AppError::new(
            ErrorCode::PairCodeInvalid,
            "This PC is already the standby. Use another PC as the main Hub.",
        )
        .into());
    }
    // A standby makes no owner sign-in code and applies no bootstrap: it only joins the branch
    // (Story 9.18). The main Hub's owner code and bootstrap are made first, so a branch that cannot
    // bootstrap refuses before the code is spent.
    let main_parts = if standby {
        None
    } else {
        let setup_code = new_pairing_code().map_err(ApiError::internal)?;
        let setup_hash = {
            let code = setup_code.clone();
            blocking(move || hash_secret(&code)).await?
        };
        let sealed = st
            .mfa_key
            .seal(&c.branch.id, setup_code.as_bytes())
            .map_err(ApiError::internal)?;
        let owner = st
            .db
            .owner_of_branch(&c.branch.id)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(|| ApiError::internal("branch without an owner"))?;
        bootstrap_of(
            &c.branch,
            &owner,
            setup_hash.clone(),
            now + SETUP_CODE_HUB_TTL_MS,
        )?;
        Some((setup_hash, sealed, owner))
    };

    // Decided before the pairing revokes the earlier Hub: a branch with history is restored on the
    // new Hub, not started again.
    let restore_pending = !standby
        && st
            .db
            .branch_has_history(&c.branch.id, &body.hub_instance_id)
            .await
            .map_err(ApiError::internal)?;
    let outcome = st
        .db
        .pair_hub(&NewHub {
            id: &hub_id,
            branch_id: &c.branch.id,
            platform: body.platform,
            role: body.role,
            hub_instance_id: &body.hub_instance_id,
            version: &body.version,
            token_hash: &token_hash,
            code_hash: &code_hash,
            setup: match &main_parts {
                Some((setup_hash, sealed, _)) => NewSetupCode {
                    code_hash: setup_hash,
                    hub_expires_at: now + SETUP_CODE_HUB_TTL_MS,
                    ciphertext: &sealed.ciphertext,
                    nonce: &sealed.nonce,
                },
                None => NewSetupCode {
                    code_hash: "",
                    hub_expires_at: 0,
                    ciphertext: &[],
                    nonce: &[],
                },
            },
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
            return Err(classify(&st, &code_hash, body.platform, body.role, now)
                .await
                .err()
                .unwrap_or_else(invalid));
        }
    };
    let bootstrap = match &main_parts {
        Some((_, _, owner)) if !restore_pending => {
            let setup = st
                .db
                .setup_code(&c.branch.id)
                .await
                .map_err(ApiError::internal)?
                .ok_or_else(|| ApiError::internal("pairing left no owner setup code"))?;
            let mut bootstrap =
                bootstrap_of(&c.branch, owner, setup.code_hash, setup.hub_expires_at)?;
            bootstrap.template = super::setup_start::bootstrap_link(&st, &c.branch, now).await;
            Some(bootstrap)
        }
        _ => None,
    };
    tracing::info!(
        hub_id = %hub_id,
        tenant_id = %c.branch.tenant_id,
        branch_id = %c.branch.id,
        pairing_code_id = %c.code.id,
        platform = body.platform.as_str(),
        role = body.role.as_str(),
        tier = c.tier.as_str(),
        revoked_hubs = revoked.len(),
        restore_pending,
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
        bootstrap,
        restore_pending,
        branch_name: c.branch.name,
        role: body.role,
    }))
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new().route("/pair", post(pair)).with_state(st)
}
