//! `/v1/account/mfa/*`: the signed-in owner enrols an authenticator app (TOTP) and keeps 10
//! single-use backup codes.
//!
//! - `POST /v1/account/mfa/totp/start` needs a password (else 403 `PASSWORD_REQUIRED`) and
//!   `mfa_state = pending` (else 409 `ALREADY_SET`). Every call mints a new 20-byte secret,
//!   replacing the unconfirmed one, and answers `{secret, otpauth_uri, qr_svg}`.
//! - `POST /v1/account/mfa/totp/confirm {code}`: a valid code (this step or one either side)
//!   enrols the owner and answers the 10 backup codes, this once; anything else is 400
//!   `TOTP_INVALID` and the owner stays `pending`.
//! - `POST /v1/account/mfa/backup-codes/ack` records that the codes were saved (204).
//! - `POST /v1/account/mfa/backup-codes/regenerate {code}` (enrolled only, else 403
//!   `FORBIDDEN`) replaces every backup code after a valid TOTP code that is not a replay.
//!
//! A code for a step at or before the last accepted one is refused (`TOTP_INVALID`). The
//! secret is sealed with AES-256-GCM under `ARENA_MFA_KEY`; one that doesn't open (another
//! key) is 500 `INTERNAL`, logged. Answers carrying a secret or codes are `no-store`. No
//! password, secret, code or otpauth URI is ever logged.

use std::sync::Arc;

use arena_proto::{AppError, BackupCodes, ErrorCode, MfaState, TotpCode, TotpStarted};
use arena_store::control::{NewBackupCode, OwnerMfa, RateRule};
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;

use super::account::{already_set, blocking};
use super::error::ApiError;
use super::json::ApiJson;
use super::session::SignedIn;
use crate::auth::AuthState;
use crate::passwords::{display_backup_code, hash_secret, new_backup_codes};
use crate::totp;

fn password_required() -> ApiError {
    AppError::new(ErrorCode::PasswordRequired, "Set your password first.").into()
}

fn totp_invalid() -> ApiError {
    AppError::with_field(
        ErrorCode::TotpInvalid,
        "code",
        "That code didn't work. Enter the current code from your authenticator app.",
    )
    .into()
}

const TOTP_PREFIX: &str = "totp:";
pub const TOTP_FAILURES_PER_WINDOW: u64 = 5;
pub const TOTP_WINDOW_MS: i64 = 15 * 60_000;

async fn check_code(
    st: &AuthState,
    owner_id: &str,
    secret: &[u8],
    code: &str,
    now: i64,
    last_step: Option<i64>,
) -> Result<i64, ApiError> {
    let key = format!("{TOTP_PREFIX}{owner_id}");
    let rule = RateRule {
        key: &key,
        prune_prefix: TOTP_PREFIX,
        bucket_ms: 60_000,
        window_ms: TOTP_WINDOW_MS,
        limit: TOTP_FAILURES_PER_WINDOW,
    };
    let reservation = match st
        .db
        .rate_reserve(&[rule], now)
        .await
        .map_err(ApiError::internal)?
    {
        Ok(r) => r,
        Err(wait_ms) => {
            tracing::warn!(owner_id = %owner_id, "totp failure limit reached");
            let retry = u64::try_from((wait_ms + 999) / 1000).unwrap_or(1);
            return Err(ApiError::rate_limited(retry));
        }
    };
    let Some(step) = totp::matching_step(secret, code, now, last_step) else {
        tracing::info!(owner_id = %owner_id, "totp code wrong or reused");
        return Err(totp_invalid());
    };
    st.db
        .rate_release(&reservation)
        .await
        .map_err(ApiError::internal)?;
    Ok(step)
}

fn not_enrolled() -> ApiError {
    ApiError::forbidden("Set up your authenticator app first.")
}

/// `body` as JSON with `Cache-Control: no-store`.
fn no_store(body: impl serde::Serialize) -> Response {
    let mut res = axum::Json(body).into_response();
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

/// The owner's secret, opened with the MFA key: 500 `INTERNAL` (logged) when it doesn't open.
fn open_secret(st: &AuthState, m: &OwnerMfa) -> Result<Vec<u8>, ApiError> {
    st.mfa_key
        .open(&m.owner_id, &m.secret_ct, &m.secret_nonce)
        .map_err(|e| {
            tracing::error!(owner_id = %m.owner_id, "mfa secret did not decrypt");
            ApiError::internal(e)
        })
}

/// 10 new codes: their display form (for the answer) and their stored rows (argon2id hashes,
/// computed on the blocking pool).
async fn mint_backup_codes() -> Result<(Vec<String>, Vec<NewBackupCode>), ApiError> {
    blocking(|| {
        let codes = new_backup_codes()?;
        let mut rows = Vec::with_capacity(codes.len());
        for c in &codes {
            rows.push(NewBackupCode {
                id: ulid::Ulid::generate().to_string(),
                code_hash: hash_secret(c)?,
            });
        }
        Ok((codes.iter().map(|c| display_backup_code(c)).collect(), rows))
    })
    .await
}

async fn start(State(st): State<Arc<AuthState>>, signed: SignedIn) -> Result<Response, ApiError> {
    let o = signed.owner;
    if !o.has_password() {
        return Err(password_required());
    }
    if o.mfa_state != MfaState::Pending {
        return Err(already_set("Your authenticator app is already set up."));
    }
    let secret = totp::new_secret().map_err(ApiError::internal)?;
    let sealed = st
        .mfa_key
        .seal(&o.id, &secret)
        .map_err(ApiError::internal)?;
    if !st
        .db
        .start_owner_mfa(&o.id, &sealed.ciphertext, &sealed.nonce, st.now())
        .await
        .map_err(ApiError::internal)?
    {
        return Err(already_set("Your authenticator app is already set up."));
    }
    let b32 = totp::base32(&secret);
    let uri = totp::otpauth_uri(&o.phone_e164, &b32);
    let qr_svg = totp::qr_svg(&uri).map_err(ApiError::internal)?;
    tracing::info!(owner_id = %o.id, "totp enrolment started");
    Ok(no_store(TotpStarted {
        secret: b32,
        otpauth_uri: uri,
        qr_svg,
    }))
}

async fn confirm(
    State(st): State<Arc<AuthState>>,
    signed: SignedIn,
    ApiJson(body): ApiJson<TotpCode>,
) -> Result<Response, ApiError> {
    let o = signed.owner;
    if !o.has_password() {
        return Err(password_required());
    }
    if o.mfa_state != MfaState::Pending {
        return Err(already_set("Your authenticator app is already set up."));
    }
    let now = st.now();
    let Some(m) = st
        .db
        .owner_mfa(&o.id)
        .await
        .map_err(ApiError::internal)?
        .filter(|m| m.confirmed_at.is_none())
    else {
        return Err(totp_invalid());
    };
    let secret = open_secret(&st, &m)?;
    let step = check_code(&st, &o.id, &secret, body.code.trim(), now, m.last_step).await?;
    let (shown, rows) = mint_backup_codes().await?;
    if !st
        .db
        .confirm_owner_mfa(&o.id, &m.secret_nonce, step, &rows, now)
        .await
        .map_err(ApiError::internal)?
    {
        // Another start replaced the secret, or the step was used meanwhile.
        return Err(totp_invalid());
    }
    tracing::info!(owner_id = %o.id, "totp enrolled; backup codes issued");
    Ok(no_store(BackupCodes {
        backup_codes: shown,
    }))
}

async fn ack(State(st): State<Arc<AuthState>>, signed: SignedIn) -> Result<StatusCode, ApiError> {
    let o = signed.owner;
    if o.mfa_state != MfaState::Enrolled
        || !st
            .db
            .ack_backup_codes(&o.id, st.now())
            .await
            .map_err(ApiError::internal)?
    {
        return Err(not_enrolled());
    }
    tracing::info!(owner_id = %o.id, "backup codes saved");
    Ok(StatusCode::NO_CONTENT)
}

async fn regenerate(
    State(st): State<Arc<AuthState>>,
    signed: SignedIn,
    ApiJson(body): ApiJson<TotpCode>,
) -> Result<Response, ApiError> {
    let o = signed.owner;
    if o.mfa_state != MfaState::Enrolled {
        return Err(not_enrolled());
    }
    let now = st.now();
    let Some(m) = st
        .db
        .owner_mfa(&o.id)
        .await
        .map_err(ApiError::internal)?
        .filter(|m| m.confirmed_at.is_some())
    else {
        return Err(not_enrolled());
    };
    let secret = open_secret(&st, &m)?;
    let step = check_code(&st, &o.id, &secret, body.code.trim(), now, m.last_step).await?;
    let (shown, rows) = mint_backup_codes().await?;
    if !st
        .db
        .regenerate_backup_codes(&o.id, step, &rows, now)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(totp_invalid());
    }
    tracing::info!(owner_id = %o.id, "backup codes regenerated");
    Ok(no_store(BackupCodes {
        backup_codes: shown,
    }))
}

/// The MFA routes, relative to `/v1`.
pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/account/mfa/totp/start", post(start))
        .route("/account/mfa/totp/confirm", post(confirm))
        .route("/account/mfa/backup-codes/ack", post(ack))
        .route("/account/mfa/backup-codes/regenerate", post(regenerate))
        .with_state(st)
}
