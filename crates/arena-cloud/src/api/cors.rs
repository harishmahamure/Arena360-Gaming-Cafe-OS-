//! CORS for the owner web apps: `ARENA_WEB_ORIGINS` is an exact-match origin list.
//!
//! - A preflight (`OPTIONS` with `Access-Control-Request-Method`) from a listed origin is
//!   answered 204 with the allowed methods and headers and
//!   `Access-Control-Allow-Credentials: true`; from any other origin, 403 `FORBIDDEN`.
//! - A state-changing request (not `GET`/`HEAD`/`OPTIONS`) with an `Origin` that isn't listed
//!   is 403 `FORBIDDEN` and never reaches a handler.
//! - Every response to a listed origin carries `Access-Control-Allow-Origin: <origin>`,
//!   `Access-Control-Allow-Credentials: true` and exposes `Retry-After`. Requests without an
//!   `Origin` (servers, curl) pass unchanged.
//! - Every `/v1` response carries `Vary: Origin`.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::error::ApiError;

/// The allowed origins, lowercase.
#[derive(Debug, Clone, Default)]
pub struct Origins(pub Arc<Vec<String>>);

fn allow_headers(res: &mut Response, origin: &HeaderValue) {
    let h = res.headers_mut();
    h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
    h.insert(
        header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
        HeaderValue::from_static("true"),
    );
    h.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("Retry-After"),
    );
}

/// The CORS middleware. Every `/v1` response carries `Vary: Origin`, whatever the origin, so
/// a cache never serves one origin's answer to another.
pub(super) async fn cors(State(origins): State<Origins>, req: Request, next: Next) -> Response {
    let v1 = req.uri().path() == "/v1" || req.uri().path().starts_with("/v1/");
    let mut res = cors_inner(&origins, req, next).await;
    if v1 {
        res.headers_mut()
            .append(header::VARY, HeaderValue::from_static("Origin"));
    }
    res
}

async fn cors_inner(origins: &Origins, req: Request, next: Next) -> Response {
    let Some(origin) = req.headers().get(header::ORIGIN).cloned() else {
        return next.run(req).await;
    };
    let listed = origin
        .to_str()
        .is_ok_and(|o| origins.0.iter().any(|a| a.eq_ignore_ascii_case(o)));
    let method = req.method().clone();
    let preflight = method == Method::OPTIONS
        && req
            .headers()
            .contains_key(header::ACCESS_CONTROL_REQUEST_METHOD);
    if preflight {
        if !listed {
            return ApiError::forbidden("This origin may not call the API.").into_response();
        }
        let mut res = StatusCode::NO_CONTENT.into_response();
        allow_headers(&mut res, &origin);
        let h = res.headers_mut();
        h.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, POST"),
        );
        h.insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("content-type"),
        );
        h.insert(
            header::ACCESS_CONTROL_MAX_AGE,
            HeaderValue::from_static("600"),
        );
        return res;
    }
    let safe = matches!(method, Method::GET | Method::HEAD | Method::OPTIONS);
    if !listed && !safe {
        tracing::warn!(method = %method, path = req.uri().path(), "request from an unlisted origin refused");
        return ApiError::forbidden("This origin may not call the API.").into_response();
    }
    let mut res = next.run(req).await;
    if listed {
        allow_headers(&mut res, &origin);
    }
    res
}
