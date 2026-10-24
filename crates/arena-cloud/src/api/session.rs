//! `/v1/session*`: the owner's browser session.
//!
//! - `arena_access`: an opaque 32-byte token (hex), stored as SHA-256, 15 min, `Path=/v1`.
//! - `arena_refresh`: the same kind of token, 30 days, `Path=/v1/session/refresh`.
//!
//! Both are `HttpOnly; Secure; SameSite=Lax` and host-only (no `Domain`). `GET /v1/session`
//! answers the signed-in owner or `AUTH_FAILED`. `POST /v1/session/refresh` rotates the
//! refresh token (new access and refresh, the old one marked rotated, its access no longer
//! valid). A rotated token presented again within 10 s (two tabs refreshing at once) is
//! `AUTH_FAILED` without side effects; later it revokes the whole family.

use std::sync::Arc;

use arena_proto::{SessionInfo, SessionOwner};
use arena_store::control::{NewOwnerSession, RefreshOutcome};
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};

use super::error::ApiError;
use crate::auth::{AuthState, new_token, token_hash};

pub const ACCESS_COOKIE: &str = "arena_access";
pub const REFRESH_COOKIE: &str = "arena_refresh";
pub const ACCESS_TTL_S: i64 = 15 * 60;
pub const REFRESH_TTL_S: i64 = 30 * 24 * 60 * 60;
const ACCESS_PATH: &str = "/v1";
const REFRESH_PATH: &str = "/v1/session/refresh";

/// A new session: its row (hashes only) and the plain tokens for the cookies.
pub(super) struct IssuedSession {
    pub row: NewOwnerSession,
    access: String,
    refresh: String,
}

/// Fresh tokens and a session row for a new family (`owner_id` is filled in by the store).
pub(super) fn issue_session(now: i64) -> Result<IssuedSession, String> {
    let access = new_token()?;
    let refresh = new_token()?;
    Ok(IssuedSession {
        row: NewOwnerSession {
            id: ulid::Ulid::generate().to_string(),
            owner_id: String::new(),
            family_id: ulid::Ulid::generate().to_string(),
            access_hash: token_hash(&access),
            access_expires_at: now + ACCESS_TTL_S * 1000,
            refresh_hash: token_hash(&refresh),
            refresh_expires_at: now + REFRESH_TTL_S * 1000,
            created_at: now,
        },
        access,
        refresh,
    })
}

fn cookie(name: &str, value: &str, path: &str, max_age_s: i64) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{name}={value}; Path={path}; Max-Age={max_age_s}; HttpOnly; Secure; SameSite=Lax"
    ))
    .expect("cookie values are hex")
}

/// Adds both session cookies to `res`.
pub(super) fn with_session_cookies(mut res: Response, s: &IssuedSession) -> Response {
    let h = res.headers_mut();
    h.append(
        header::SET_COOKIE,
        cookie(ACCESS_COOKIE, &s.access, ACCESS_PATH, ACCESS_TTL_S),
    );
    h.append(
        header::SET_COOKIE,
        cookie(REFRESH_COOKIE, &s.refresh, REFRESH_PATH, REFRESH_TTL_S),
    );
    res
}

/// Expires both session cookies.
fn clear_cookies(mut res: Response) -> Response {
    let h = res.headers_mut();
    h.append(
        header::SET_COOKIE,
        cookie(ACCESS_COOKIE, "", ACCESS_PATH, 0),
    );
    h.append(
        header::SET_COOKIE,
        cookie(REFRESH_COOKIE, "", REFRESH_PATH, 0),
    );
    res
}

/// The value of cookie `name` in the request's `Cookie` headers.
pub(super) fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, v)| *k == name && !v.is_empty())
        .map(|(_, v)| v.to_owned())
}

fn info(owner: &arena_store::control::Owner) -> SessionInfo {
    SessionInfo {
        owner: SessionOwner {
            name: owner.name.clone(),
            mfa_state: owner.mfa_state,
        },
    }
}

async fn current(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
) -> Result<axum::Json<SessionInfo>, ApiError> {
    let token = read_cookie(&headers, ACCESS_COOKIE).ok_or_else(ApiError::auth_failed)?;
    let live = st
        .db
        .owner_session_by_access(&token_hash(&token), st.now())
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::auth_failed)?;
    Ok(axum::Json(info(&live.owner)))
}

async fn refresh(State(st): State<Arc<AuthState>>, headers: HeaderMap) -> Response {
    let Some(token) = read_cookie(&headers, REFRESH_COOKIE) else {
        return ApiError::auth_failed().into_response();
    };
    let now = st.now();
    let issued = match issue_session(now) {
        Ok(i) => i,
        Err(e) => return ApiError::internal(e).into_response(),
    };
    let outcome = match st
        .db
        .rotate_owner_session(&token_hash(&token), &issued.row, now)
        .await
    {
        Ok(o) => o,
        Err(e) => return ApiError::internal(e).into_response(),
    };
    match outcome {
        RefreshOutcome::Rotated(owner) => {
            with_session_cookies(axum::Json(info(&owner)).into_response(), &issued)
        }
        // A concurrent refresh (another tab) rotated it a moment ago and set the new cookies:
        // refuse without revoking, and without clearing the cookies it set.
        RefreshOutcome::RecentlyRotated => ApiError::auth_failed().into_response(),
        RefreshOutcome::Reused => {
            tracing::warn!("rotated refresh token presented again: session family revoked");
            clear_cookies(ApiError::auth_failed().into_response())
        }
        RefreshOutcome::Invalid => clear_cookies(ApiError::auth_failed().into_response()),
    }
}

/// The session routes, relative to `/v1`.
pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/session", get(current))
        .route("/session/refresh", post(refresh))
        .with_state(st)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookies_are_read_from_any_cookie_header() {
        let mut h = HeaderMap::new();
        h.append(header::COOKIE, "a=1; arena_access=abc".parse().unwrap());
        h.append(header::COOKIE, "arena_refresh=def".parse().unwrap());
        assert_eq!(read_cookie(&h, ACCESS_COOKIE).as_deref(), Some("abc"));
        assert_eq!(read_cookie(&h, REFRESH_COOKIE).as_deref(), Some("def"));
        assert_eq!(read_cookie(&h, "missing"), None);
    }
}
