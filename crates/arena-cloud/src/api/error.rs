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
        ErrorCode::NotFound | ErrorCode::InvoiceNotFound => StatusCode::NOT_FOUND,
        // A wrong method is 405 through [`ApiError::method_not_allowed`] only.
        ErrorCode::Unsupported | ErrorCode::BadRequest | ErrorCode::BadFrame => {
            StatusCode::BAD_REQUEST
        }
        ErrorCode::ValidationFailed
        | ErrorCode::FieldTooLong
        | ErrorCode::BadStage
        | ErrorCode::ReasonRequired
        | ErrorCode::Expired
        | ErrorCode::HsnRequired
        | ErrorCode::ReferenceRequired
        | ErrorCode::ConsentRequired
        | ErrorCode::CouponInvalid
        | ErrorCode::PasswordTooShort
        | ErrorCode::PasswordCommon => StatusCode::UNPROCESSABLE_ENTITY,
        ErrorCode::AuthFailed | ErrorCode::TokenRevoked | ErrorCode::TokenInvalid => {
            StatusCode::UNAUTHORIZED
        }
        ErrorCode::AuthLocked | ErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        ErrorCode::OtpInvalid | ErrorCode::OtpExpired | ErrorCode::TotpInvalid => {
            StatusCode::BAD_REQUEST
        }
        ErrorCode::Forbidden
        | ErrorCode::PasswordRequired
        | ErrorCode::SetupIncomplete
        | ErrorCode::PermissionDenied
        | ErrorCode::PinRequired
        | ErrorCode::PinRejected => StatusCode::FORBIDDEN,
        ErrorCode::SessionLocked => StatusCode::LOCKED,
        ErrorCode::Stale
        | ErrorCode::OrderSettled
        | ErrorCode::RefundExceedsPaid
        | ErrorCode::AlreadyBootstrapped
        | ErrorCode::BranchCodeLocked
        | ErrorCode::IdempotencyConflict
        | ErrorCode::AlreadySet
        | ErrorCode::BranchCodeTaken
        | ErrorCode::BillingDisabled
        | ErrorCode::OneFreeBranch
        | ErrorCode::StepNotReady
        | ErrorCode::HubNotPaired
        | ErrorCode::PlatformUnsupported
        | ErrorCode::GroupNotEmpty
        | ErrorCode::GroupKindMismatch
        | ErrorCode::ZoneNotEmpty
        | ErrorCode::CellTaken
        | ErrorCode::BarcodeTaken
        | ErrorCode::LoginTaken
        | ErrorCode::UsernameTaken
        | ErrorCode::PhoneTaken
        | ErrorCode::PcBusy
        | ErrorCode::PcUnavailable
        | ErrorCode::NoRate
        | ErrorCode::PaymentShort
        | ErrorCode::UndoNotAllowed
        | ErrorCode::InsufficientBalance
        | ErrorCode::CreditLimit
        | ErrorCode::CreditBelowOutstanding
        | ErrorCode::OverrideUsedToday
        | ErrorCode::CreditPaused
        | ErrorCode::PassAlreadyActive
        | ErrorCode::PackExpired
        | ErrorCode::PlanNotForGroup
        | ErrorCode::PassOutsideWindow
        | ErrorCode::PastClosing
        | ErrorCode::OutOfStock
        | ErrorCode::ShiftAlreadyOpen
        | ErrorCode::ShiftNotOpen
        | ErrorCode::DrawerShort
        | ErrorCode::VarianceOverThreshold
        | ErrorCode::BundleShort
        | ErrorCode::ControllersUnchecked
        | ErrorCode::EraseBlocked
        | ErrorCode::KeyAlreadyReleased
        | ErrorCode::RestoreGap
        | ErrorCode::DeviceUnassigned => StatusCode::CONFLICT,
        ErrorCode::PairCodeInvalid | ErrorCode::PairCodeExpired | ErrorCode::SetupCodeInvalid => {
            StatusCode::UNAUTHORIZED
        }
        ErrorCode::FeatureNotEntitled
        | ErrorCode::MfaRequired
        | ErrorCode::TierLocked
        | ErrorCode::FeatureNotInTier
        | ErrorCode::KeyNotReleased
        | ErrorCode::DeviceLimit
        | ErrorCode::UserLimit => StatusCode::FORBIDDEN,
        ErrorCode::CloudUnreachable => StatusCode::SERVICE_UNAVAILABLE,
        ErrorCode::NotPrimary
        | ErrorCode::HubSlow
        | ErrorCode::NoHub
        | ErrorCode::RestorePending => StatusCode::SERVICE_UNAVAILABLE,
        ErrorCode::HubIncompatible => StatusCode::CONFLICT,
        ErrorCode::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        ErrorCode::Fenced
        | ErrorCode::RollupSchemaOld
        | ErrorCode::BranchUpdateRequired
        | ErrorCode::CommandExpired => StatusCode::CONFLICT,
        ErrorCode::PairPinPaused => StatusCode::TOO_MANY_REQUESTS,
        ErrorCode::AuditBuilding => StatusCode::ACCEPTED,
        ErrorCode::AuditBuildFailed => StatusCode::BAD_GATEWAY,
        ErrorCode::KeyErased => StatusCode::GONE,
        ErrorCode::RestoreWrongBranch | ErrorCode::RestoreCorrupt => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        ErrorCode::SchemaTooNew | ErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// A failed `/v1` request: the flat [`AppError`] body and its status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    status: StatusCode,
    error: AppError,
    /// Sent as `Retry-After` (seconds).
    retry_after_s: Option<u64>,
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
            retry_after_s: None,
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
                retry_after_s: None,
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
            retry_after_s: None,
        }
    }

    /// `PAYLOAD_TOO_LARGE`, 413: the body is over this route's own limit.
    pub fn payload_too_large(message: impl Into<String>) -> Self {
        AppError::new(ErrorCode::PayloadTooLarge, message).into()
    }

    /// `VALIDATION_FAILED`, 422, naming `field`.
    pub fn validation(field: &str, message: impl Into<String>) -> Self {
        AppError::with_field(ErrorCode::ValidationFailed, field, message).into()
    }

    /// `RATE_LIMITED`, 429, with `Retry-After: retry_after_s` (at least 1).
    pub fn rate_limited(retry_after_s: u64) -> Self {
        let mut e: Self = AppError::new(
            ErrorCode::RateLimited,
            "Too many requests. Try again after the time in Retry-After.",
        )
        .into();
        e.retry_after_s = Some(retry_after_s.max(1));
        e
    }

    /// `error` (a `RATE_LIMITED` with its own message, 429) with `Retry-After: retry_after_s`.
    pub fn retry_in(error: AppError, retry_after_s: u64) -> Self {
        let mut e: Self = error.into();
        e.retry_after_s = Some(retry_after_s.max(1));
        e
    }

    pub fn locked(retry_after_s: u64) -> Self {
        let mut e: Self = AppError::new(
            ErrorCode::AuthLocked,
            "Sign-in is paused. Try again after the time in Retry-After.",
        )
        .into();
        e.retry_after_s = Some(retry_after_s.max(1));
        e
    }

    /// `AUTH_FAILED`, 401: no valid session.
    pub fn auth_failed() -> Self {
        AppError::new(ErrorCode::AuthFailed, "Sign in to continue.").into()
    }

    /// `FORBIDDEN`, 403.
    pub fn forbidden(message: impl Into<String>) -> Self {
        AppError::new(ErrorCode::Forbidden, message).into()
    }

    /// `INTERNAL` with 503: a dependency (such as the SMS provider) is down. `detail` goes to
    /// the log only.
    pub fn unavailable(detail: impl Display) -> Self {
        tracing::error!(code = "INTERNAL", detail = %detail, "dependency unavailable");
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            error: AppError::new(ErrorCode::Internal, INTERNAL_MESSAGE),
            retry_after_s: None,
        }
    }

    /// The `Retry-After` seconds this error carries.
    pub fn retry_after_s(&self) -> Option<u64> {
        self.retry_after_s
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
            retry_after_s: None,
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
            retry_after_s: None,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut res = (self.status, Json(self.error)).into_response();
        if let Some(s) = self.retry_after_s {
            res.headers_mut()
                .insert(axum::http::header::RETRY_AFTER, s.into());
        }
        res
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
            (ErrorCode::RateLimited, 429),
            (ErrorCode::OtpInvalid, 400),
            (ErrorCode::OtpExpired, 400),
            (ErrorCode::AuthFailed, 401),
            (ErrorCode::Forbidden, 403),
            (ErrorCode::PasswordTooShort, 422),
            (ErrorCode::PasswordCommon, 422),
            (ErrorCode::AlreadySet, 409),
            (ErrorCode::PasswordRequired, 403),
            (ErrorCode::TotpInvalid, 400),
            (ErrorCode::SetupIncomplete, 403),
            (ErrorCode::BranchCodeTaken, 409),
            (ErrorCode::BillingDisabled, 409),
            (ErrorCode::OneFreeBranch, 409),
            (ErrorCode::StepNotReady, 409),
            (ErrorCode::PairCodeInvalid, 401),
            (ErrorCode::PairCodeExpired, 401),
            (ErrorCode::FeatureNotEntitled, 403),
            (ErrorCode::PlatformUnsupported, 409),
            (ErrorCode::MfaRequired, 403),
            (ErrorCode::PayloadTooLarge, 413),
            (ErrorCode::Fenced, 409),
            (ErrorCode::RollupSchemaOld, 409),
            (ErrorCode::BranchUpdateRequired, 409),
            (ErrorCode::CommandExpired, 409),
            (ErrorCode::AuditBuilding, 202),
            (ErrorCode::AuditBuildFailed, 502),
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
