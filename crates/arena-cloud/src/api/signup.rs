//! `/v1/signup/*`: owner sign-up with a phone one-time code.
//!
//! - `POST /v1/signup/start` validates the form (`VALIDATION_FAILED` naming the field),
//!   reserves a send (5 per phone per rolling hour, and `ARENA_SMS_GLOBAL_PER_MIN` across
//!   every phone per rolling minute; else `RATE_LIMITED` with `Retry-After`), stores a hashed
//!   challenge and sends the code. It answers 202 with the same body whether or not the phone
//!   already has an owner, and never reads `owners`.
//! - `POST /v1/signup/resend` puts a new code on the same challenge (30 s after its previous
//!   send), resetting expiry and attempts; it counts against the same limits. Two resends of
//!   one challenge race on its expiry: only one reissues it, the other is `RATE_LIMITED`.
//! - `POST /v1/signup/verify` checks the code. A wrong one is `OTP_INVALID` (the 5th voids the
//!   challenge: `OTP_EXPIRED`); a void, expired or used challenge is `OTP_EXPIRED`. A correct
//!   one consumes the challenge, creates the owner (`pending`) if the phone has none and
//!   signs a `pending` owner in (session cookies, `next: account_setup`). An `enrolled` owner
//!   gets no session (`next: sign_in`): the code alone never bypasses their second factor. A
//!   repeated verify of a used challenge that already carries that owner's session answers
//!   `account_setup` again (a double submit).
//!
//! Sends are reserved atomically before the SMS goes out. A provider failure answers 503
//! `INTERNAL` and gives the reservation back, unless delivery is uncertain (a timeout after
//! the request went out), when it stays counted. A failed first send deletes the new
//! challenge; a failed resend puts the previous code back. Logs name a phone only by its
//! `phone_tag`; never the name, email, code or tokens.

use std::sync::Arc;

use arena_proto::{
    OTP_DIGITS, OTP_MAX_ATTEMPTS, OTP_RESEND_AFTER_S, OTP_SENDS_PER_HOUR, OTP_TTL_S,
    SignupChallenge, SignupNext, SignupResend, SignupStart, SignupVerified, SignupVerify,
};
use arena_store::control::{
    NewOtpChallenge, RateReservation, RateRule, SignupCompletion, SignupOutcome,
};
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;

use super::error::ApiError;
use super::json::ApiJson;
use super::session::{ACCESS_COOKIE, issue_session, read_cookie, with_session_cookies};
use crate::auth::{AuthState, new_code, token_hash, validate_start};
use crate::messaging::Sms;

const SECOND_MS: i64 = 1000;
const MINUTE_MS: i64 = 60 * SECOND_MS;
const HOUR_MS: i64 = 60 * MINUTE_MS;
const TTL_MS: i64 = OTP_TTL_S as i64 * 1000;
const RESEND_AFTER_MS: i64 = OTP_RESEND_AFTER_S as i64 * 1000;
/// Key prefix of the per-phone send limiter.
const PHONE_PREFIX: &str = "otp_send:";
/// Key of the all-phones SMS limiter.
pub const GLOBAL_SMS_KEY: &str = "sms:global";

/// The SMS text for `code`: the DLT-registered template, its minutes from [`OTP_TTL_S`].
pub fn sms_text(code: &str) -> String {
    let minutes = OTP_TTL_S / 60;
    let unit = if minutes == 1 { "minute" } else { "minutes" };
    format!("{code} is your Arena 360 code. It expires in {minutes} {unit}.")
}

/// The rate-limit key of a phone's sends.
pub fn send_key(phone: &str) -> String {
    format!("{PHONE_PREFIX}{phone}")
}

fn ceil_s(ms: i64) -> u64 {
    u64::try_from((ms + 999) / 1000).unwrap_or(1).max(1)
}

/// Reserves one send to `phone` under both limits, or `RATE_LIMITED` with the wait.
async fn reserve_send(st: &AuthState, phone: &str, now: i64) -> Result<RateReservation, ApiError> {
    let key = send_key(phone);
    let rules = [
        RateRule {
            key: &key,
            prune_prefix: PHONE_PREFIX,
            bucket_ms: MINUTE_MS,
            window_ms: HOUR_MS,
            limit: u64::from(OTP_SENDS_PER_HOUR),
        },
        RateRule {
            key: GLOBAL_SMS_KEY,
            prune_prefix: GLOBAL_SMS_KEY,
            bucket_ms: SECOND_MS,
            window_ms: MINUTE_MS,
            limit: st.sms_global_per_min,
        },
    ];
    match st
        .db
        .rate_reserve(&rules, now)
        .await
        .map_err(ApiError::internal)?
    {
        Ok(r) => Ok(r),
        Err(wait_ms) => {
            let retry = ceil_s(wait_ms);
            tracing::warn!(
                phone_tag = %st.phone_tag(phone),
                retry_after_s = retry,
                "signup code send limit reached"
            );
            Err(ApiError::rate_limited(retry))
        }
    }
}

/// Gives a reservation back; a failure is only logged (the slot then stays counted).
async fn release(st: &AuthState, r: &RateReservation) {
    if let Err(e) = st.db.rate_release(r).await {
        tracing::warn!(detail = %e, "could not release an SMS send reservation");
    }
}

/// Sends `code` to `phone`: 503 `INTERNAL` when the provider fails, after giving the
/// reservation back unless delivery is uncertain.
async fn send_code(
    st: &AuthState,
    phone: &str,
    code: &str,
    reservation: &RateReservation,
) -> Result<(), ApiError> {
    let Err(e) = st.sms.send(phone, &sms_text(code)).await else {
        return Ok(());
    };
    tracing::warn!(
        phone_tag = %st.phone_tag(phone),
        provider = st.sms.name(),
        uncertain = e.uncertain,
        detail = %e,
        "signup code send failed"
    );
    if !e.uncertain {
        release(st, reservation).await;
    }
    Err(ApiError::unavailable(e))
}

fn accepted(challenge_id: String) -> Response {
    (
        StatusCode::ACCEPTED,
        axum::Json(SignupChallenge {
            challenge_id,
            expires_in_s: OTP_TTL_S,
            resend_after_s: OTP_RESEND_AFTER_S,
        }),
    )
        .into_response()
}

async fn start(
    State(st): State<Arc<AuthState>>,
    ApiJson(body): ApiJson<SignupStart>,
) -> Result<Response, ApiError> {
    let owner = validate_start(&body).map_err(|e| ApiError::validation(e.field, e.message))?;
    let phone = owner.phone_e164.clone();
    let now = st.now();
    let id = ulid::Ulid::generate().to_string();
    let code = new_code().map_err(ApiError::internal)?;
    let reservation = reserve_send(&st, &phone, now).await?;
    let stored = st
        .db
        .insert_otp_challenge(&NewOtpChallenge {
            id: id.clone(),
            phone_e164: phone.clone(),
            code_hash: st.code_hash(&id, &code),
            payload: owner,
            created_at: now,
            expires_at: now + TTL_MS,
        })
        .await;
    if let Err(e) = stored {
        release(&st, &reservation).await;
        return Err(ApiError::internal(e));
    }
    if let Err(e) = send_code(&st, &phone, &code, &reservation).await {
        if let Err(d) = st.db.delete_otp_challenge(&id).await {
            tracing::warn!(detail = %d, "could not delete an unsent signup challenge");
        }
        return Err(e);
    }
    tracing::info!(phone_tag = %st.phone_tag(&phone), challenge_id = %id, "signup code sent");
    Ok(accepted(id))
}

fn expired() -> ApiError {
    arena_proto::AppError::new(
        arena_proto::ErrorCode::OtpExpired,
        "This code no longer works. Request a new code.",
    )
    .into()
}

fn invalid() -> ApiError {
    arena_proto::AppError::new(
        arena_proto::ErrorCode::OtpInvalid,
        "That code is not right.",
    )
    .into()
}

async fn resend(
    State(st): State<Arc<AuthState>>,
    ApiJson(body): ApiJson<SignupResend>,
) -> Result<Response, ApiError> {
    let now = st.now();
    let Some(c) = st
        .db
        .otp_challenge(&body.challenge_id)
        .await
        .map_err(ApiError::internal)?
    else {
        return Err(expired());
    };
    // Used, or voided by wrong codes (its form is gone): start again.
    if c.consumed_at.is_some() || c.payload.is_none() {
        return Err(expired());
    }
    // The previous send of this challenge is when its current expiry started.
    let last_sent = c.expires_at - TTL_MS;
    if now - last_sent < RESEND_AFTER_MS {
        return Err(ApiError::rate_limited(ceil_s(
            last_sent + RESEND_AFTER_MS - now,
        )));
    }
    let reservation = reserve_send(&st, &c.phone_e164, now).await?;
    let code = new_code().map_err(ApiError::internal)?;
    let hash = st.code_hash(&c.id, &code);
    let new_expires = now + TTL_MS;
    let reissued = match st
        .db
        .reissue_otp_challenge(&c.id, c.expires_at, &hash, new_expires)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            release(&st, &reservation).await;
            return Err(ApiError::internal(e));
        }
    };
    if !reissued {
        // Another resend (or a verify) got there first.
        release(&st, &reservation).await;
        return Err(ApiError::rate_limited(ceil_s(RESEND_AFTER_MS)));
    }
    if let Err(e) = send_code(&st, &c.phone_e164, &code, &reservation).await {
        // Keep the challenge as it was: the previous code still works.
        if let Err(d) = st
            .db
            .revert_otp_reissue(&c.id, &hash, new_expires, &c)
            .await
        {
            tracing::warn!(detail = %d, "could not restore a signup challenge after a failed resend");
        }
        return Err(e);
    }
    tracing::info!(phone_tag = %st.phone_tag(&c.phone_e164), challenge_id = %c.id, "signup code resent");
    Ok(accepted(c.id))
}

/// Whether the request already carries a live session of the owner of `phone` (a repeated
/// verify after the first one signed them in).
async fn signed_in_as(
    st: &AuthState,
    headers: &HeaderMap,
    phone: &str,
    now: i64,
) -> Result<bool, ApiError> {
    let Some(token) = read_cookie(headers, ACCESS_COOKIE) else {
        return Ok(false);
    };
    let live = st
        .db
        .owner_session_by_access(&token_hash(&token), now)
        .await
        .map_err(ApiError::internal)?;
    Ok(live.is_some_and(|l| l.owner.phone_e164 == phone))
}

fn next(next: SignupNext) -> axum::Json<SignupVerified> {
    axum::Json(SignupVerified { next })
}

async fn verify(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<SignupVerify>,
) -> Result<Response, ApiError> {
    let code = body.code.trim();
    if code.len() != OTP_DIGITS || !code.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ApiError::validation("code", "Enter the 6-digit code."));
    }
    let now = st.now();
    let Some(c) = st
        .db
        .otp_challenge(&body.challenge_id)
        .await
        .map_err(ApiError::internal)?
    else {
        return Err(expired());
    };
    let tag = st.phone_tag(&c.phone_e164);
    if c.consumed_at.is_some() {
        if signed_in_as(&st, &headers, &c.phone_e164, now).await? {
            return Ok(next(SignupNext::AccountSetup).into_response());
        }
        tracing::info!(phone_tag = %tag, challenge_id = %c.id, "signup code already used");
        return Err(expired());
    }
    let Some(owner) = c.payload.as_ref() else {
        return Err(expired());
    };
    if c.attempts >= OTP_MAX_ATTEMPTS || now >= c.expires_at {
        tracing::info!(phone_tag = %tag, challenge_id = %c.id, "signup code no longer valid");
        return Err(expired());
    }
    if !st.code_matches(&c.id, code, &c.code_hash) {
        let attempts = st
            .db
            .record_otp_failure(&c.id, OTP_MAX_ATTEMPTS)
            .await
            .map_err(ApiError::internal)?;
        tracing::info!(phone_tag = %tag, challenge_id = %c.id, attempts = ?attempts, "signup code wrong");
        return Err(match attempts {
            Some(n) if n < OTP_MAX_ATTEMPTS => invalid(),
            _ => expired(),
        });
    }
    let issued = issue_session(now).map_err(ApiError::internal)?;
    let owner_id = ulid::Ulid::generate().to_string();
    let outcome = st
        .db
        .complete_signup(&SignupCompletion {
            challenge_id: &c.id,
            code_hash: &c.code_hash,
            max_attempts: OTP_MAX_ATTEMPTS,
            now,
            new_owner_id: &owner_id,
            owner,
            session: &issued.row,
        })
        .await
        .map_err(ApiError::internal)?;
    match outcome {
        SignupOutcome::NotConsumed => {
            // Lost a race to a concurrent verify that signed this browser in already.
            if signed_in_as(&st, &headers, &c.phone_e164, now).await? {
                return Ok(next(SignupNext::AccountSetup).into_response());
            }
            Err(expired())
        }
        SignupOutcome::Session(owner) => {
            tracing::info!(phone_tag = %tag, owner_id = %owner.id, "owner signed up");
            Ok(with_session_cookies(
                next(SignupNext::AccountSetup).into_response(),
                &issued,
            ))
        }
        SignupOutcome::Enrolled(owner) => {
            tracing::info!(phone_tag = %tag, owner_id = %owner.id, "signup code for an enrolled owner: sign in");
            Ok(next(SignupNext::SignIn).into_response())
        }
    }
}

/// The sign-up routes, relative to `/v1`.
pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/signup/start", post(start))
        .route("/signup/resend", post(resend))
        .route("/signup/verify", post(verify))
        .with_state(st)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sms_text_is_the_dlt_template_with_the_ttl_in_minutes() {
        assert_eq!(OTP_TTL_S, 300);
        assert_eq!(
            sms_text("123456"),
            "123456 is your Arena 360 code. It expires in 5 minutes."
        );
    }
}
