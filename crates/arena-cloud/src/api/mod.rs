//! The `/v1` API surface and its error envelope.
//!
//! Every failure leaves as a flat [`arena_proto::AppError`] JSON body with a stable code: an
//! unknown path (`NOT_FOUND`, 404), a wrong method (`UNSUPPORTED`, 405), a body that is not
//! valid JSON or doesn't match the request (`BAD_REQUEST`, 400, or 413 over [`BODY_LIMIT`]),
//! and any internal failure or panic (`INTERNAL`, 500, a fixed message; the detail is only
//! logged).
//!
//! With an [`AuthState`] the owner routes are mounted too: `/v1/signup/*` ([`signup`]),
//! `/v1/session*` ([`session`]), `/v1/account/password` ([`account`]) and
//! `/v1/account/mfa/*` ([`mfa`]), behind the CORS layer ([`cors`]); with `--dev` also
//! `GET /v1/dev/sms`, the fake SMS outbox, and `GET /v1/dev/setup-gate`, which passes only
//! [`gate::RequireSetupComplete`].

pub mod account;
pub mod branches;
pub mod commands;
mod cors;
mod dev;
mod error;
pub mod gate;
mod heartbeat;
mod json;
mod meta;
pub mod mfa;
pub mod owned;
mod pair;
mod panic;
pub mod security;
pub mod session;
pub mod signin;
pub mod signup;
pub mod wizard;
pub mod wizard_devices;
mod wizard_events;
pub mod wizard_golive;
pub mod wizard_install;
pub mod wizard_staff;
pub mod wizard_tier;

use std::sync::Arc;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};

use crate::auth::AuthState;
pub use error::{ApiError, INTERNAL_MESSAGE, status_of};
pub use json::ApiJson;

/// The largest request body any `/v1` route accepts.
pub const BODY_LIMIT: usize = 64 * 1024;

/// The `/v1` routes. The dev-only routes (`/v1/dev/*`) are mounted only when `dev` is set.
fn v1(dev: bool, auth: Option<&Arc<AuthState>>, commands_only: bool) -> Router {
    let mut v1 = Router::new();
    if let (true, Some(st)) = (commands_only, auth) {
        return v1.merge(commands::router(Arc::clone(st)));
    }
    if let Some(st) = auth {
        v1 = v1
            .merge(signup::router(Arc::clone(st)))
            .merge(session::router(Arc::clone(st)))
            .merge(account::router(Arc::clone(st)))
            .merge(mfa::router(Arc::clone(st)))
            .merge(signin::router(Arc::clone(st)))
            .merge(security::router(Arc::clone(st)))
            .merge(wizard::router(Arc::clone(st)))
            .merge(wizard_tier::router(Arc::clone(st)))
            .merge(wizard_install::router(Arc::clone(st)))
            .merge(wizard_staff::router(Arc::clone(st)))
            .merge(wizard_devices::router(Arc::clone(st)))
            .merge(wizard_golive::router(Arc::clone(st)))
            .merge(branches::router(Arc::clone(st)))
            .merge(pair::router(Arc::clone(st)))
            .merge(heartbeat::router(Arc::clone(st)))
            .merge(commands::router(Arc::clone(st)))
            .merge(wizard_events::router(Arc::clone(st)))
            .merge(meta::router());
    }
    if dev {
        v1 = v1.merge(dev::router());
        if let Some(st) = auth {
            v1 = v1
                .merge(dev::sms_router(Arc::clone(st)))
                .merge(dev::gate_router(Arc::clone(st)));
        }
    }
    v1
}

/// Any error response (≥ 400) that is not already JSON, such as a rejection from an extractor
/// other than [`ApiJson`], becomes the envelope ([`ApiError::for_status`]).
async fn envelope_plain_errors(res: Response) -> Response {
    let status = res.status();
    let json = res
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"));
    if status.as_u16() < 400 || json {
        return res;
    }
    ApiError::for_status(status).into_response()
}

/// Wraps `base` (routes outside `/v1`, such as `/healthz`) and the `/v1` routes in the error
/// envelope: the `NOT_FOUND` fallback, the wrong-method mapping, the body limit, the mapping of
/// any other non-JSON error response and the panic-catching layer.
pub fn with_envelope(base: Router, dev: bool) -> Router {
    with_envelope_and_auth(base, dev, None)
}

/// [`with_envelope`] plus, when `auth` is set, the owner sign-up and session routes and the
/// CORS layer for `auth.web_origins` (outermost, so error envelopes carry the CORS headers).
pub fn with_envelope_and_auth(base: Router, dev: bool, auth: Option<Arc<AuthState>>) -> Router {
    with_envelope_for(base, dev, auth, false)
}

pub fn with_envelope_commands(base: Router, dev: bool, auth: Option<Arc<AuthState>>) -> Router {
    with_envelope_for(base, dev, auth, true)
}

fn with_envelope_for(
    base: Router,
    dev: bool,
    auth: Option<Arc<AuthState>>,
    commands_only: bool,
) -> Router {
    let app = base
        .nest("/v1", v1(dev, auth.as_ref(), commands_only))
        .method_not_allowed_fallback(error::method_not_allowed)
        .fallback(error::not_found)
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(axum::middleware::map_response(envelope_plain_errors))
        .layer(axum::middleware::from_fn(panic::catch_panic));
    match auth {
        Some(st) => app.layer(axum::middleware::from_fn_with_state(
            cors::Origins(Arc::new(st.web_origins.clone())),
            cors::cors,
        )),
        None => app,
    }
}
