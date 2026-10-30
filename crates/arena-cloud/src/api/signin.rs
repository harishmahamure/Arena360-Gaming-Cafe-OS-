use std::sync::Arc;
use std::sync::atomic::Ordering;

use arena_proto::{
    AppError, DEVICE_LABEL_MAX_CHARS, ErrorCode, MfaState, OTP_RESEND_AFTER_S,
    SIGNIN_CHALLENGE_TTL_S, SIGNIN_LOCK_S, SIGNIN_MAX_ATTEMPTS, SIGNIN_MAX_FAILURES, Signin,
    SigninMethod, SigninNext, SigninReply, SigninSms, SigninSmsSent, SigninVerified, SigninVerify,
    TRUSTED_DEVICE_TTL_S,
};
use arena_store::control::{
    NewOwnerDevice, NewSigninChallenge, Owner, SigninChallenge, SigninCompletion, SigninFactor,
    SigninFailure,
};
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;

use super::account::blocking;
use super::error::ApiError;
use super::json::ApiJson;
use super::mfa::{TotpCheck, check_totp, open_secret};
use super::session::{
    IssuedSession, TRUSTED_COOKIE, TRUSTED_PATH, cookie, issue_session, read_cookie,
    with_session_cookies,
};
use super::signup::{ceil_s, release, reserve_send, send_code};
use crate::auth::{AuthState, new_code, new_token, token_hash, valid_email, valid_phone};
use crate::passwords::{dummy_hash, normalize_backup_code, verify_secret};
use crate::useragent;

pub(super) const CHALLENGE_TTL_MS: i64 = SIGNIN_CHALLENGE_TTL_S as i64 * 1000;
const LOCK_MS: i64 = SIGNIN_LOCK_S as i64 * 1000;
pub(super) const RESEND_AFTER_MS: i64 = OTP_RESEND_AFTER_S as i64 * 1000;
const TRUSTED_TTL_MS: i64 = TRUSTED_DEVICE_TTL_S as i64 * 1000;

pub const SIGNIN_FAILED_MESSAGE: &str = "That didn't work. Check your details and try again.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Login {
    Phone(String),
    Email(String),
}

pub fn parse_login(raw: &str) -> Option<Login> {
    let t = raw.trim();
    if t.contains('@') {
        let e = t.to_lowercase();
        return valid_email(&e).then_some(Login::Email(e));
    }
    let compact: String = t
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '.' | '(' | ')'))
        .collect();
    let compact = match compact.strip_prefix('0') {
        Some(rest)
            if rest.len() == 10
                && !rest.starts_with('0')
                && rest.bytes().all(|b| b.is_ascii_digit()) =>
        {
            rest.to_owned()
        }
        _ => compact,
    };
    let phone = if compact.starts_with('+') {
        compact
    } else if let Some(rest) = compact.strip_prefix("00") {
        format!("+{rest}")
    } else if compact.len() == 10 && compact.bytes().all(|b| b.is_ascii_digit()) {
        format!("+91{compact}")
    } else {
        return None;
    };
    valid_phone(&phone).then_some(Login::Phone(phone))
}

const MAX_LOGIN_CHARS: usize = 254;
const MAX_PASSWORD_CHARS: usize = 128;
pub(super) const MAX_CODE_CHARS: usize = 32;

pub(super) fn too_long(field: &str) -> ApiError {
    ApiError::validation(field, "That is too long.")
}

pub fn signin_failed() -> ApiError {
    AppError::new(ErrorCode::AuthFailed, SIGNIN_FAILED_MESSAGE).into()
}

fn locked_for(until: i64, now: i64) -> ApiError {
    ApiError::locked(ceil_s(until - now))
}

fn lock_of(owner: &Owner, now: i64) -> Option<ApiError> {
    owner
        .locked_until
        .filter(|&u| u > now)
        .map(|u| locked_for(u, now))
}

async fn record_failure(
    st: &AuthState,
    owner_id: &str,
    challenge_id: Option<&str>,
    now: i64,
) -> Result<(), ApiError> {
    let locked = st
        .db
        .record_signin_failure(&SigninFailure {
            owner_id,
            challenge_id,
            max_attempts: SIGNIN_MAX_ATTEMPTS,
            max_failures: SIGNIN_MAX_FAILURES,
            lock_ms: LOCK_MS,
            now,
        })
        .await
        .map_err(ApiError::internal)?;
    if locked.is_some() {
        tracing::warn!(owner_id = %owner_id, lock_s = SIGNIN_LOCK_S, "sign-in paused after repeated failures");
    }
    Ok(())
}

async fn dummy_verify(st: &AuthState, password: String) {
    st.dummy_verifies.fetch_add(1, Ordering::SeqCst);
    let _ = blocking(move || Ok(verify_secret(&password, dummy_hash()))).await;
}

fn trusted_cookie(res: &mut Response, token: &str, max_age_s: i64) {
    res.headers_mut().append(
        header::SET_COOKIE,
        cookie(TRUSTED_COOKIE, token, TRUSTED_PATH, max_age_s),
    );
}

pub(super) fn clear_trusted_cookie(res: &mut Response) {
    trusted_cookie(res, "", 0);
}

fn signed_in(reply: impl serde::Serialize, issued: &IssuedSession) -> Response {
    with_session_cookies(axum::Json(reply).into_response(), issued)
}

async fn signin(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Signin>,
) -> Result<Response, ApiError> {
    if body.login.chars().count() > MAX_LOGIN_CHARS {
        return Err(too_long("login"));
    }
    if body.password.chars().count() > MAX_PASSWORD_CHARS {
        return Err(too_long("password"));
    }
    let login = parse_login(&body.login)
        .ok_or_else(|| ApiError::validation("login", "Enter your mobile number or email."))?;
    if body.password.is_empty() {
        return Err(ApiError::validation("password", "Enter your password."));
    }
    let now = st.now();
    let (owner, tag) = match &login {
        Login::Phone(p) => (
            st.db.owner_by_phone(p).await.map_err(ApiError::internal)?,
            st.phone_tag(p),
        ),
        Login::Email(e) => (
            st.db.owner_by_email(e).await.map_err(ApiError::internal)?,
            st.email_tag(e),
        ),
    };
    let hash = match &owner {
        Some(o) => st
            .db
            .owner_password_hash(&o.id)
            .await
            .map_err(ApiError::internal)?,
        None => None,
    };
    let password = body.password;
    let ok = match hash {
        Some(h) => blocking(move || Ok(verify_secret(&password, &h))).await?,
        None => {
            dummy_verify(&st, password).await;
            false
        }
    };
    let Some(owner) = owner else {
        tracing::info!(login_tag = %tag, "sign-in refused: unknown login");
        return Err(signin_failed());
    };
    if !ok {
        tracing::info!(login_tag = %tag, owner_id = %owner.id, "sign-in refused: wrong password");
        if owner.locked_until.is_none_or(|u| u <= now) {
            record_failure(&st, &owner.id, None, now).await?;
        }
        return Err(signin_failed());
    }
    if let Some(e) = lock_of(&owner, now) {
        tracing::info!(login_tag = %tag, owner_id = %owner.id, "sign-in refused: paused");
        return Err(e);
    }
    if owner.mfa_state == MfaState::Pending {
        let issued = issue_session(now).map_err(ApiError::internal)?;
        if !st
            .db
            .signin_with_password(&owner.id, &issued.row, None, now)
            .await
            .map_err(ApiError::internal)?
        {
            return Err(signin_failed());
        }
        tracing::info!(login_tag = %tag, owner_id = %owner.id, "owner signed in: account setup pending");
        return Ok(signed_in(
            SigninReply {
                next: Some(SigninNext::AccountSetup),
                challenge_id: None,
                methods: None,
            },
            &issued,
        ));
    }
    if let Some(token) = read_cookie(&headers, TRUSTED_COOKIE) {
        let device = st
            .db
            .trusted_device(&owner.id, &token_hash(&token), now)
            .await
            .map_err(ApiError::internal)?;
        if let Some(d) = device {
            let issued = issue_session(now).map_err(ApiError::internal)?;
            if st
                .db
                .signin_with_password(&owner.id, &issued.row, Some(&d.id), now)
                .await
                .map_err(ApiError::internal)?
            {
                tracing::info!(login_tag = %tag, owner_id = %owner.id, device_id = %d.id, "owner signed in on a trusted device");
                return Ok(signed_in(
                    SigninReply {
                        next: Some(SigninNext::Done),
                        challenge_id: None,
                        methods: None,
                    },
                    &issued,
                ));
            }
        }
    }
    let id = ulid::Ulid::generate().to_string();
    st.db
        .insert_signin_challenge(&NewSigninChallenge {
            id: id.clone(),
            owner_id: owner.id.clone(),
            created_at: now,
            expires_at: now + CHALLENGE_TTL_MS,
        })
        .await
        .map_err(ApiError::internal)?;
    let mut methods = vec![SigninMethod::Totp, SigninMethod::BackupCode];
    if owner.sms_fallback {
        methods.push(SigninMethod::Sms);
    }
    tracing::info!(login_tag = %tag, owner_id = %owner.id, challenge_id = %id, "sign-in: second factor needed");
    Ok(axum::Json(SigninReply {
        next: None,
        challenge_id: Some(id),
        methods: Some(methods),
    })
    .into_response())
}

async fn live_challenge(
    st: &AuthState,
    id: &str,
    now: i64,
) -> Result<(SigninChallenge, Owner), ApiError> {
    let c = st
        .db
        .signin_challenge(id)
        .await
        .map_err(ApiError::internal)?
        .filter(|c| {
            c.consumed_at.is_none() && c.attempts < SIGNIN_MAX_ATTEMPTS && c.expires_at > now
        })
        .ok_or_else(signin_failed)?;
    let owner = st
        .db
        .owner_by_id(&c.owner_id)
        .await
        .map_err(ApiError::internal)?
        .filter(|o| o.mfa_state == MfaState::Enrolled)
        .ok_or_else(signin_failed)?;
    Ok((c, owner))
}

pub(super) fn sms_off() -> ApiError {
    ApiError::forbidden("Text-message codes are turned off for this account.")
}

async fn sms(
    State(st): State<Arc<AuthState>>,
    ApiJson(body): ApiJson<SigninSms>,
) -> Result<Response, ApiError> {
    let now = st.now();
    let (c, owner) = live_challenge(&st, &body.challenge_id, now).await?;
    if !owner.sms_fallback {
        return Err(sms_off());
    }
    if let Some(e) = lock_of(&owner, now) {
        return Err(e);
    }
    if let Some(sent) = c.sms_sent_at
        && now - sent < RESEND_AFTER_MS
    {
        return Err(ApiError::rate_limited(ceil_s(sent + RESEND_AFTER_MS - now)));
    }
    let phone = owner.phone_e164.clone();
    let reservation = reserve_send(&st, &phone, now).await?;
    let code = new_code().map_err(ApiError::internal)?;
    let hash = st.code_hash(&c.id, &code);
    let stored = st
        .db
        .set_signin_sms(
            &c.id,
            c.sms_sent_at,
            &hash,
            now,
            now + CHALLENGE_TTL_MS,
            SIGNIN_MAX_ATTEMPTS,
        )
        .await;
    match stored {
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
        if let Err(d) = st.db.revert_signin_sms(&c.id, &hash, &c).await {
            tracing::warn!(detail = %d, "could not restore a sign-in challenge after a failed send");
        }
        return Err(e);
    }
    tracing::info!(phone_tag = %st.phone_tag(&phone), challenge_id = %c.id, "sign-in code sent");
    Ok((
        StatusCode::ACCEPTED,
        axum::Json(SigninSmsSent {
            resend_after_s: OTP_RESEND_AFTER_S,
        }),
    )
        .into_response())
}

fn device_label(given: Option<&str>, headers: &HeaderMap) -> Result<String, ApiError> {
    let given = given.map(str::trim).filter(|l| !l.is_empty());
    if let Some(l) = given {
        if l.chars().count() > DEVICE_LABEL_MAX_CHARS || l.chars().any(char::is_control) {
            return Err(ApiError::validation(
                "device_label",
                format!("Use at most {DEVICE_LABEL_MAX_CHARS} characters."),
            ));
        }
        return Ok(l.to_owned());
    }
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok());
    Ok(useragent::device_label(ua))
}

pub(super) async fn backup_factor(
    st: &AuthState,
    owner_id: &str,
    code: &str,
) -> Result<Option<SigninFactor>, ApiError> {
    let Some(code) = normalize_backup_code(code) else {
        return Ok(None);
    };
    let unused: Vec<(String, String)> = st
        .db
        .backup_codes(owner_id)
        .await
        .map_err(ApiError::internal)?
        .into_iter()
        .filter(|c| c.used_at.is_none())
        .map(|c| (c.id, c.code_hash))
        .collect();
    let found = blocking(move || {
        let mut found = None;
        for (id, hash) in unused {
            if verify_secret(&code, &hash) && found.is_none() {
                found = Some(id);
            }
        }
        Ok(found)
    })
    .await?;
    Ok(found.map(|id| SigninFactor::BackupCode { id }))
}

async fn factor(
    st: &AuthState,
    c: &SigninChallenge,
    owner: &Owner,
    body: &SigninVerify,
    now: i64,
) -> Result<Option<SigninFactor>, ApiError> {
    let code = body.code.trim();
    match body.method {
        SigninMethod::Totp => {
            let Some(m) = st
                .db
                .owner_mfa(&owner.id)
                .await
                .map_err(ApiError::internal)?
                .filter(|m| m.confirmed_at.is_some())
            else {
                return Ok(None);
            };
            let secret = open_secret(st, &m)?;
            match check_totp(st, &owner.id, &secret, code, now, m.last_step).await? {
                TotpCheck::Accepted(step) => Ok(Some(SigninFactor::Totp { step })),
                TotpCheck::Wrong => Ok(None),
                TotpCheck::Limited(retry) => Err(ApiError::locked(retry)),
            }
        }
        SigninMethod::BackupCode => backup_factor(st, &owner.id, code).await,
        SigninMethod::Sms => {
            let sent_after = now - CHALLENGE_TTL_MS;
            let ok = match (&c.sms_code_hash, c.sms_sent_at) {
                (Some(hash), Some(at)) if at > sent_after => st.code_matches(&c.id, code, hash),
                _ => false,
            };
            Ok(ok.then(|| SigninFactor::Sms {
                code_hash: c.sms_code_hash.clone().unwrap_or_default(),
                sent_after,
            }))
        }
    }
}

fn method_name(m: SigninMethod) -> &'static str {
    match m {
        SigninMethod::Totp => "totp",
        SigninMethod::BackupCode => "backup_code",
        SigninMethod::Sms => "sms",
    }
}

async fn verify(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<SigninVerify>,
) -> Result<Response, ApiError> {
    if body.code.chars().count() > MAX_CODE_CHARS {
        return Err(too_long("code"));
    }
    let label = if body.trust_device {
        Some(device_label(body.device_label.as_deref(), &headers)?)
    } else {
        None
    };
    let now = st.now();
    let (c, owner) = live_challenge(&st, &body.challenge_id, now).await?;
    if let Some(e) = lock_of(&owner, now) {
        return Err(e);
    }
    if body.method == SigninMethod::Sms && !owner.sms_fallback {
        return Err(sms_off());
    }
    let method = method_name(body.method);
    let Some(factor) = factor(&st, &c, &owner, &body, now).await? else {
        tracing::info!(owner_id = %owner.id, challenge_id = %c.id, method, "sign-in refused: second factor wrong");
        record_failure(&st, &owner.id, Some(&c.id), now).await?;
        return Err(signin_failed());
    };
    let issued = issue_session(now).map_err(ApiError::internal)?;
    let device_token = match label {
        Some(_) => Some(new_token().map_err(ApiError::internal)?),
        None => None,
    };
    let existing = match (&device_token, read_cookie(&headers, TRUSTED_COOKIE)) {
        (Some(_), Some(old)) => st
            .db
            .trusted_device(&owner.id, &token_hash(&old), now)
            .await
            .map_err(ApiError::internal)?,
        _ => None,
    };
    let device = label
        .zip(device_token.as_ref())
        .map(|(label, token)| NewOwnerDevice {
            id: existing
                .as_ref()
                .map_or_else(|| ulid::Ulid::generate().to_string(), |d| d.id.clone()),
            token_hash: token_hash(token),
            label,
            expires_at: now + TRUSTED_TTL_MS,
            refresh: existing.is_some(),
        });
    let device_id = device.as_ref().map(|d| d.id.clone());
    let backup = matches!(factor, SigninFactor::BackupCode { .. });
    let done = st
        .db
        .complete_signin(&SigninCompletion {
            challenge_id: &c.id,
            owner_id: &owner.id,
            max_attempts: SIGNIN_MAX_ATTEMPTS,
            now,
            factor,
            device,
            session: &issued.row,
        })
        .await
        .map_err(ApiError::internal)?;
    if !done {
        let consumed = st
            .db
            .signin_challenge(&c.id)
            .await
            .map_err(ApiError::internal)?
            .is_some_and(|c| c.consumed_at.is_some());
        if consumed {
            tracing::info!(owner_id = %owner.id, challenge_id = %c.id, method, "sign-in refused: challenge already used");
            return Err(signin_failed());
        }
        tracing::info!(owner_id = %owner.id, challenge_id = %c.id, method, "sign-in refused: factor already used");
        record_failure(&st, &owner.id, Some(&c.id), now).await?;
        return Err(signin_failed());
    }
    let remaining = if backup {
        match st.db.backup_codes_remaining(&owner.id).await {
            Ok(n) => Some(n),
            Err(e) => {
                tracing::warn!(owner_id = %owner.id, detail = %e, "could not count the backup codes left");
                None
            }
        }
    } else {
        None
    };
    let mut res = signed_in(
        SigninVerified {
            next: SigninNext::Done,
            backup_codes_remaining: remaining,
        },
        &issued,
    );
    if let Some(token) = &device_token {
        trusted_cookie(&mut res, token, i64::from(TRUSTED_DEVICE_TTL_S));
    }
    tracing::info!(
        owner_id = %owner.id,
        method,
        device_id = device_id.as_deref().unwrap_or(""),
        "owner signed in with a second factor"
    );
    Ok(res)
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/signin", post(signin))
        .route("/signin/sms", post(sms))
        .route("/signin/verify", post(verify))
        .with_state(st)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logins_are_normalised() {
        let phone = |s: &str| Some(Login::Phone(s.to_owned()));
        assert_eq!(parse_login(" +91 98765-43210 "), phone("+919876543210"));
        assert_eq!(parse_login("98765 43210"), phone("+919876543210"));
        assert_eq!(parse_login("0091 9876543210"), phone("+919876543210"));
        assert_eq!(parse_login("+44 (20) 7946 0958"), phone("+442079460958"));
        assert_eq!(
            parse_login(" Asha@Example.IN "),
            Some(Login::Email("asha@example.in".into()))
        );
        assert_eq!(parse_login("09876543210"), phone("+919876543210"));
        assert_eq!(parse_login("098765 43210"), phone("+919876543210"));
        assert_eq!(parse_login("12345"), None);
        assert_eq!(parse_login("+91123"), None);
        assert_eq!(parse_login("asha@"), None);
        assert_eq!(parse_login(""), None);
    }
}
