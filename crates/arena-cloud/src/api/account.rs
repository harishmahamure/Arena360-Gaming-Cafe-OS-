//! `POST /v1/account/password {password}`: the signed-in owner sets their password, once.
//!
//! The password is 10–128 Unicode scalar values (`PASSWORD_TOO_SHORT`, or `VALIDATION_FAILED`
//! when too long) and its lowercase form is not on the bundled common-password list
//! (`PASSWORD_COMMON`); every refusal names `field: "password"`. It is stored only as an
//! argon2id PHC string. A second set is 409 `ALREADY_SET`: changing a password is not here.
//! The password never reaches a log.

use std::sync::Arc;

use arena_proto::{AppError, ErrorCode, PASSWORD_MAX_CHARS, SetPassword};
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;

use super::error::ApiError;
use super::json::ApiJson;
use super::session::SignedIn;
use crate::auth::AuthState;
use crate::passwords::{PasswordRejection, check_password, hash_secret};

pub(super) const ARGON2_CONCURRENCY: usize = 4;
static ARGON2_PERMITS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(ARGON2_CONCURRENCY);

/// `ALREADY_SET`, 409.
pub(super) fn already_set(message: &str) -> ApiError {
    AppError::new(ErrorCode::AlreadySet, message).into()
}

fn rejection(r: PasswordRejection) -> ApiError {
    match r {
        PasswordRejection::TooShort => AppError::with_field(
            ErrorCode::PasswordTooShort,
            "password",
            "Use at least 10 characters.",
        )
        .into(),
        PasswordRejection::TooLong => ApiError::validation(
            "password",
            format!("Use at most {PASSWORD_MAX_CHARS} characters."),
        ),
        PasswordRejection::Common => AppError::with_field(
            ErrorCode::PasswordCommon,
            "password",
            "That password is too common. Choose another.",
        )
        .into(),
    }
}

/// Runs a CPU-heavy hash on the blocking pool.
pub(super) async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, ApiError> {
    let permit = ARGON2_PERMITS
        .acquire()
        .await
        .map_err(|e| ApiError::internal(format_args!("hash permits closed: {e}")))?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        f()
    })
    .await
    .map_err(|e| ApiError::internal(format_args!("hash task failed: {e}")))?
    .map_err(ApiError::internal)
}

async fn set_password(
    State(st): State<Arc<AuthState>>,
    signed: SignedIn,
    ApiJson(body): ApiJson<SetPassword>,
) -> Result<StatusCode, ApiError> {
    let owner_id = signed.owner.id;
    if signed.owner.password_set_at.is_some() {
        return Err(already_set("Your password is already set."));
    }
    check_password(&body.password).map_err(rejection)?;
    let password = body.password;
    let hash = blocking(move || hash_secret(&password)).await?;
    if !st
        .db
        .set_owner_password(&owner_id, &hash, st.now())
        .await
        .map_err(ApiError::internal)?
    {
        return Err(already_set("Your password is already set."));
    }
    tracing::info!(owner_id = %owner_id, "owner password set");
    Ok(StatusCode::NO_CONTENT)
}

/// The account routes, relative to `/v1`.
pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/account/password", post(set_password))
        .with_state(st)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn at_most_four_hashes_run_at_once() {
        static NOW: AtomicUsize = AtomicUsize::new(0);
        static PEAK: AtomicUsize = AtomicUsize::new(0);
        let tasks: Vec<_> = (0..10)
            .map(|_| {
                tokio::spawn(blocking(|| {
                    let n = NOW.fetch_add(1, Ordering::SeqCst) + 1;
                    PEAK.fetch_max(n, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    NOW.fetch_sub(1, Ordering::SeqCst);
                    Ok(())
                }))
            })
            .collect();
        for t in tasks {
            t.await.unwrap().unwrap();
        }
        assert_eq!(PEAK.load(Ordering::SeqCst), ARGON2_CONCURRENCY);
        assert_eq!(ARGON2_PERMITS.available_permits(), ARGON2_CONCURRENCY);
    }
}
