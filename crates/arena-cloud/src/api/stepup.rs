//! `POST /v1/session/stepup` and `POST /v1/session/stepup/sms`: a second factor proves, for 5
//! minutes, that the person at the signed-in session is the owner (Story 8.25). The proof is
//! `owner_sessions.stepup_at`; a sensitive owner command is refused `MFA_REQUIRED` without a fresh
//! one ([`super::commands`]).
//!
//! - `stepup {method, code}` takes a TOTP code, a backup code or (only when the owner turned
//!   text-message codes on) the code sent by `stepup/sms`. A TOTP step or backup code is single use
//!   exactly as at sign-in. Answers 204.
//! - Wrong codes: 5 in 15 minutes per owner, whatever the factor, then `RATE_LIMITED` with
//!   `Retry-After` until the oldest leaves the window. The count is the step-up's own (not the
//!   sign-in's) and the session is untouched. A wrong code is `TOTP_INVALID` on `code` with the
//!   same text for every method, so it never says which factor failed.
//! - `stepup/sms` sends the code (202), at most one a minute; the code lives on the session row for
//!   5 minutes and is cleared by a successful step-up.
//!
//! No code is ever logged.

use std::sync::Arc;

use arena_proto::{
    AppError, ErrorCode, OTP_RESEND_AFTER_S, SigninMethod, StepupRequest, StepupSms, StepupSmsSent,
};
use arena_store::control::{RateRule, SigninFactor};
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;

use super::error::ApiError;
use super::gate::RequireSetupComplete;
use super::json::ApiJson;
use super::mfa::open_secret;
use super::signin::{
    CHALLENGE_TTL_MS, MAX_CODE_CHARS, RESEND_AFTER_MS, backup_factor, sms_off, too_long,
};
use super::signup::{ceil_s, release, reserve_send, send_code};
use crate::auth::{AuthState, new_code};
use crate::totp;

const PREFIX: &str = "stepup:";
/// Wrong codes allowed in [`WINDOW_MS`].
pub const STEPUP_FAILURES_PER_WINDOW: u64 = 5;
pub const WINDOW_MS: i64 = 15 * 60_000;

/// The same text for every method: it never says which factor failed.
fn wrong_code() -> ApiError {
    AppError::with_field(
        ErrorCode::TotpInvalid,
        "code",
        "That code didn't work. Check it and try again.",
    )
    .into()
}

/// What a code is hashed under: the session, so it opens nothing else.
fn sms_scope(session_id: &str) -> String {
    format!("{PREFIX}{session_id}")
}

async fn factor(
    st: &AuthState,
    session_id: &str,
    owner_id: &str,
    body: &StepupRequest,
    now: i64,
) -> Result<Option<SigninFactor>, ApiError> {
    let code = body.code.trim();
    match body.method {
        SigninMethod::Totp => {
            let Some(m) = st
                .db
                .owner_mfa(owner_id)
                .await
                .map_err(ApiError::internal)?
                .filter(|m| m.confirmed_at.is_some())
            else {
                return Ok(None);
            };
            let secret = open_secret(st, &m)?;
            Ok(totp::matching_step(&secret, code, now, m.last_step)
                .map(|step| SigninFactor::Totp { step }))
        }
        SigninMethod::BackupCode => backup_factor(st, owner_id, code).await,
        SigninMethod::Sms => {
            let sent_after = now - CHALLENGE_TTL_MS;
            let stored = st
                .db
                .stepup_sms(session_id)
                .await
                .map_err(ApiError::internal)?;
            Ok(match stored {
                Some((hash, at))
                    if at > sent_after && st.code_matches(&sms_scope(session_id), code, &hash) =>
                {
                    Some(SigninFactor::Sms {
                        code_hash: hash,
                        sent_after,
                    })
                }
                _ => None,
            })
        }
    }
}

async fn stepup(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    ApiJson(body): ApiJson<StepupRequest>,
) -> Result<StatusCode, ApiError> {
    if body.code.chars().count() > MAX_CODE_CHARS {
        return Err(too_long("code"));
    }
    let owner = signed.owner;
    if body.method == SigninMethod::Sms && !owner.sms_fallback {
        return Err(sms_off());
    }
    let now = st.now();
    let key = format!("{PREFIX}{}", owner.id);
    let rule = RateRule {
        key: &key,
        prune_prefix: PREFIX,
        bucket_ms: 60_000,
        window_ms: WINDOW_MS,
        limit: STEPUP_FAILURES_PER_WINDOW,
    };
    // Every attempt takes a slot; a correct one gives it back.
    let reservation = match st
        .db
        .rate_reserve(&[rule], now)
        .await
        .map_err(ApiError::internal)?
    {
        Ok(r) => r,
        Err(wait_ms) => {
            tracing::warn!(owner_id = %owner.id, "step-up paused after repeated wrong codes");
            return Err(ApiError::rate_limited(ceil_s(wait_ms)));
        }
    };
    let method = match body.method {
        SigninMethod::Totp => "totp",
        SigninMethod::BackupCode => "backup_code",
        SigninMethod::Sms => "sms",
    };
    let Some(found) = factor(&st, &signed.session.id, &owner.id, &body, now).await? else {
        tracing::info!(owner_id = %owner.id, method, "step-up refused: wrong code");
        return Err(wrong_code());
    };
    let done = st
        .db
        .complete_stepup(&signed.session.id, &owner.id, &found, now)
        .await
        .map_err(ApiError::internal)?;
    if !done {
        tracing::info!(owner_id = %owner.id, method, "step-up refused: code already used");
        return Err(wrong_code());
    }
    st.db
        .rate_release(&reservation)
        .await
        .map_err(ApiError::internal)?;
    tracing::info!(owner_id = %owner.id, session_id = %signed.session.id, method, "step-up passed");
    Ok(StatusCode::NO_CONTENT)
}

async fn sms(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    ApiJson(_body): ApiJson<StepupSms>,
) -> Result<Response, ApiError> {
    let owner = signed.owner;
    if !owner.sms_fallback {
        return Err(sms_off());
    }
    let now = st.now();
    let session_id = signed.session.id;
    let previous = st
        .db
        .stepup_sms(&session_id)
        .await
        .map_err(ApiError::internal)?;
    if let Some((_, sent)) = &previous
        && now - sent < RESEND_AFTER_MS
    {
        return Err(ApiError::rate_limited(ceil_s(sent + RESEND_AFTER_MS - now)));
    }
    let phone = owner.phone_e164.clone();
    let reservation = reserve_send(&st, &phone, now).await?;
    let code = new_code().map_err(ApiError::internal)?;
    let hash = st.code_hash(&sms_scope(&session_id), &code);
    let seen = previous.as_ref().map(|(_, at)| *at);
    match st
        .db
        .set_stepup_sms(&session_id, seen, Some((&hash, now)))
        .await
    {
        Ok(true) => {}
        Ok(false) => {
            release(&st, &reservation).await;
            return Err(ApiError::rate_limited(ceil_s(RESEND_AFTER_MS)));
        }
        Err(e) => {
            release(&st, &reservation).await;
            return Err(ApiError::internal(e));
        }
    }
    if let Err(e) = send_code(&st, &phone, &code, &reservation).await {
        let back = previous.as_ref().map(|(h, at)| (h.as_str(), *at));
        if let Err(d) = st.db.set_stepup_sms(&session_id, Some(now), back).await {
            tracing::warn!(detail = %d, "could not restore a step-up code after a failed send");
        }
        return Err(e);
    }
    tracing::info!(phone_tag = %st.phone_tag(&phone), owner_id = %owner.id, "step-up code sent");
    Ok((
        StatusCode::ACCEPTED,
        axum::Json(StepupSmsSent {
            resend_after_s: OTP_RESEND_AFTER_S,
        }),
    )
        .into_response())
}

/// The step-up routes, relative to `/v1`.
pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/session/stepup", post(stepup))
        .route("/session/stepup/sms", post(sms))
        .with_state(st)
}
