//! [`ApiError`]: an [`AppError`] with its HTTP status.

use std::fmt::Display;

use arena_proto::{AppError, ErrorCode};
use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// The one message an `INTERNAL` error ever carries.
pub const INTERNAL_MESSAGE: &str = "Something went wrong. Try again in a moment.";

/// The HTTP status for an error code on the cloud API.
pub fn status_of(code: ErrorCode) -> StatusCode {
    match code {
        ErrorCode::NotFound => StatusCode::NOT_FOUND,
        // A wrong method is 405 through [`ApiError::method_not_allowed`] only.
        ErrorCode::Unsupported | ErrorCode::BadRequest | ErrorCode::BadFrame => {
            StatusCode::BAD_REQUEST
        }
        ErrorCode::ValidationFailed | ErrorCode::ReasonRequired | ErrorCode::Expired => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        ErrorCode::AuthFailed | ErrorCode::TokenRevoked | ErrorCode::TokenInvalid => {
            StatusCode::UNAUTHORIZED
        }
        ErrorCode::AuthLocked => StatusCode::TOO_MANY_REQUESTS,
        ErrorCode::Forbidden
        | ErrorCode::PermissionDenied
        | ErrorCode::PinRequired
        | ErrorCode::PinRejected => StatusCode::FORBIDDEN,
        ErrorCode::SessionLocked => StatusCode::LOCKED,
        ErrorCode::Stale | ErrorCode::AlreadyBootstrapped | ErrorCode::IdempotencyConflict => {
            StatusCode::CONFLICT
        }
        ErrorCode::NotPrimary | ErrorCode::HubSlow => StatusCode::SERVICE_UNAVAILABLE,
        ErrorCode::SchemaTooNew | ErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// A failed `/v1` request: the flat [`AppError`] body and its status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    status: StatusCode,
    error: AppError,
}

impl ApiError {
    /// The status this error is sent with.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// The body this error is sent with.
    pub fn body(&self) -> &AppError {
        &self.error
    }

    /// `NOT_FOUND`, 404.
    pub fn not_found() -> Self {
        AppError::new(ErrorCode::NotFound, "There is nothing at this address.").into()
    }

    /// `UNSUPPORTED`, 405 (the wrong-method fallback).
    pub fn method_not_allowed() -> Self {
        Self {
            status: StatusCode::METHOD_NOT_ALLOWED,
            error: AppError::new(
                ErrorCode::Unsupported,
                "This address doesn't accept that method.",
            ),
        }
    }

    /// The envelope for a non-JSON error response with `status` (from an extractor or a layer
    /// other than [`super::ApiJson`]): 404 `NOT_FOUND`, 405 `UNSUPPORTED`, 413 and other 4xx
    /// `BAD_REQUEST` with the same status, 5xx `INTERNAL`.
    pub fn for_status(status: StatusCode) -> Self {
        match status {
            StatusCode::NOT_FOUND => Self::not_found(),
            StatusCode::METHOD_NOT_ALLOWED => Self::method_not_allowed(),
            StatusCode::PAYLOAD_TOO_LARGE => Self::too_large(),
            s if s.is_server_error() => {
                Self::internal(format_args!("{s} response without a JSON body"))
            }
            s => Self {
                status: s,
                error: AppError::new(ErrorCode::BadRequest, "The request could not be read."),
            },
        }
    }

    /// `BAD_REQUEST`, 400, optionally naming `field`.
    pub fn bad_request(message: impl Into<String>, field: Option<String>) -> Self {
        let mut error = AppError::new(ErrorCode::BadRequest, message);
        error.field = field;
        error.into()
    }

    /// `BAD_REQUEST`, 413: the body is over the limit.
    pub fn too_large() -> Self {
        Self {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            error: AppError::new(ErrorCode::BadRequest, "The request body is too large."),
        }
    }

    /// `INTERNAL`, 500 with the fixed message. `detail` goes to the log only.
    pub fn internal(detail: impl Display) -> Self {
        tracing::error!(code = "INTERNAL", detail = %detail, "request failed");
        Self::generic_internal()
    }

    fn generic_internal() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: AppError::new(ErrorCode::Internal, INTERNAL_MESSAGE),
        }
    }
}

impl From<AppError> for ApiError {
    /// Pairs `error` with its status. An `INTERNAL` error's message may carry storage detail,
    /// so it is logged and replaced with the fixed message.
    fn from(error: AppError) -> Self {
        if error.code == ErrorCode::Internal {
            return Self::internal(error.message);
        }
        Self {
            status: status_of(error.code),
            error,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.error)).into_response()
    }
}

/// The router fallback: `NOT_FOUND`.
pub(super) async fn not_found() -> ApiError {
    ApiError::not_found()
}

/// A route exists but not for this method: `UNSUPPORTED`.
pub(super) async fn method_not_allowed() -> ApiError {
    ApiError::method_not_allowed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_never_carries_its_detail() {
        let e = ApiError::from(AppError::new(
            ErrorCode::Internal,
            "rusqlite::Error at src/lib.rs:1",
        ));
        assert_eq!(e.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(e.body().message, INTERNAL_MESSAGE);
        assert_eq!(e.body().field, None);
    }

    #[test]
    fn the_spec_codes_map_to_their_statuses() {
        for (code, status) in [
            (ErrorCode::NotFound, 404),
            (ErrorCode::Unsupported, 400),
            (ErrorCode::BadRequest, 400),
            (ErrorCode::ValidationFailed, 422),
            (ErrorCode::Internal, 500),
        ] {
            assert_eq!(status_of(code).as_u16(), status, "{code:?}");
        }
        assert_eq!(ApiError::method_not_allowed().status().as_u16(), 405);
        assert_eq!(ApiError::too_large().status().as_u16(), 413);
        assert_eq!(ApiError::too_large().body().code, ErrorCode::BadRequest);
    }
}

#[cfg(test)]
mod for_status_tests {
    use super::*;

    #[test]
    fn plain_error_statuses_map_to_envelope_codes() {
        for (status, code, out) in [
            (404, ErrorCode::NotFound, 404),
            (405, ErrorCode::Unsupported, 405),
            (413, ErrorCode::BadRequest, 413),
            (415, ErrorCode::BadRequest, 415),
            (400, ErrorCode::BadRequest, 400),
            (503, ErrorCode::Internal, 500),
        ] {
            let e = ApiError::for_status(StatusCode::from_u16(status).unwrap());
            assert_eq!(
                (e.body().code, e.status().as_u16()),
                (code, out),
                "{status}"
            );
        }
    }
}
