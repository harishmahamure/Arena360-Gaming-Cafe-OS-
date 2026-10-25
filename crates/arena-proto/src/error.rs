//! The single error shape returned by every failing request.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Machine-readable error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum ErrorCode {
    /// A frame was malformed or missing a required field.
    BadFrame,
    /// The frame kind is not supported.
    Unsupported,
    /// The caller's class or role may not perform this request.
    Forbidden,
    /// This Hub is not the primary writer for the branch.
    NotPrimary,
    /// The database schema is newer than this binary.
    SchemaTooNew,
    /// The intent is older than its TTL.
    Expired,
    /// The edit's `base_rev` does not match the current row.
    Stale,
    /// The device token was revoked.
    TokenRevoked,
    /// The device token is wrong, malformed or unknown for its class.
    TokenInvalid,
    /// The Hub did not answer in time.
    HubSlow,
    /// An unexpected storage or I/O failure on the Hub.
    Internal,
    /// The intent's payload failed validation; `field` names the offending field.
    ValidationFailed,
    /// The branch has already been bootstrapped.
    AlreadyBootstrapped,
    /// The idempotency key was already used for a different intent body.
    IdempotencyConflict,
    /// Sign-in failed, or the request carries no valid session. Never says which field failed.
    AuthFailed,
    /// Too many failed sign-ins for this login: paused for a while.
    AuthLocked,
    /// The actor does not hold the permission (or implicit principal) the intent requires.
    PermissionDenied,
    /// The intent needs a manager PIN (again): no elevation, an unusable ticket, or the PIN
    /// changed since it was checked.
    PinRequired,
    /// The PIN entered approves nothing. One fixed message; it never names anyone.
    PinRejected,
    /// The intent kind needs a reason and none was given.
    ReasonRequired,
    /// The Hub UI session is locked (idle lock): unlock it with a PIN first.
    SessionLocked,
    /// The cloud has no such resource or route.
    NotFound,
    /// The body is not valid JSON or doesn't match the request; `field` names the offending
    /// field when known.
    BadRequest,
    /// Too many requests (an OTP send limit, or a resend sooner than allowed): wait for the
    /// `Retry-After` seconds and try again.
    RateLimited,
    /// The one-time code is wrong; attempts are left on the same challenge.
    OtpInvalid,
    /// The one-time code challenge expired, ran out of attempts or was already used: request
    /// a new code.
    OtpExpired,
    /// The new password is shorter than 10 characters; `field` is `password`.
    PasswordTooShort,
    /// The new password is on the common-password list; `field` is `password`.
    PasswordCommon,
    /// The thing being set (a password, an authenticator app) is already set and cannot be
    /// set again here.
    AlreadySet,
    /// The request needs the owner's password to be set first.
    PasswordRequired,
    /// The authenticator code is wrong, too old or already used; `field` is `code`.
    TotpInvalid,
    /// Account setup (password, authenticator app, saved backup codes) is not finished.
    SetupIncomplete,
    BranchCodeTaken,
    NoHub,
    HubIncompatible,
    BillingDisabled,
    OneFreeBranch,
    StepNotReady,
    PairCodeInvalid,
    PairPinPaused,
}

/// An error returned to a client: a code, a message and optionally the offending field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub field: Option<String>,
}

impl ErrorCode {
    /// The wire form of the code, e.g. `NOT_PRIMARY`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BadFrame => "BAD_FRAME",
            Self::Unsupported => "UNSUPPORTED",
            Self::Forbidden => "FORBIDDEN",
            Self::NotPrimary => "NOT_PRIMARY",
            Self::SchemaTooNew => "SCHEMA_TOO_NEW",
            Self::Expired => "EXPIRED",
            Self::Stale => "STALE",
            Self::TokenRevoked => "TOKEN_REVOKED",
            Self::TokenInvalid => "TOKEN_INVALID",
            Self::HubSlow => "HUB_SLOW",
            Self::Internal => "INTERNAL",
            Self::ValidationFailed => "VALIDATION_FAILED",
            Self::AlreadyBootstrapped => "ALREADY_BOOTSTRAPPED",
            Self::IdempotencyConflict => "IDEMPOTENCY_CONFLICT",
            Self::AuthFailed => "AUTH_FAILED",
            Self::AuthLocked => "AUTH_LOCKED",
            Self::PermissionDenied => "PERMISSION_DENIED",
            Self::PinRequired => "PIN_REQUIRED",
            Self::PinRejected => "PIN_REJECTED",
            Self::ReasonRequired => "REASON_REQUIRED",
            Self::SessionLocked => "SESSION_LOCKED",
            Self::NotFound => "NOT_FOUND",
            Self::BadRequest => "BAD_REQUEST",
            Self::RateLimited => "RATE_LIMITED",
            Self::OtpInvalid => "OTP_INVALID",
            Self::OtpExpired => "OTP_EXPIRED",
            Self::PasswordTooShort => "PASSWORD_TOO_SHORT",
            Self::PasswordCommon => "PASSWORD_COMMON",
            Self::AlreadySet => "ALREADY_SET",
            Self::PasswordRequired => "PASSWORD_REQUIRED",
            Self::TotpInvalid => "TOTP_INVALID",
            Self::SetupIncomplete => "SETUP_INCOMPLETE",
            Self::BranchCodeTaken => "BRANCH_CODE_TAKEN",
            Self::NoHub => "NO_HUB",
            Self::HubIncompatible => "HUB_INCOMPATIBLE",
            Self::BillingDisabled => "BILLING_DISABLED",
            Self::OneFreeBranch => "ONE_FREE_BRANCH",
            Self::StepNotReady => "STEP_NOT_READY",
            Self::PairCodeInvalid => "PAIR_CODE_INVALID",
            Self::PairPinPaused => "PAIR_PIN_PAUSED",
        }
    }
}

impl AppError {
    /// Builds an error with no field.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            field: None,
        }
    }
}

impl AppError {
    /// Builds an error naming the offending field.
    pub fn with_field(
        code: ErrorCode,
        field: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            field: Some(field.into()),
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for AppError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_without_field() {
        let err = AppError {
            code: ErrorCode::BadFrame,
            message: "missing idempotency_key".into(),
            field: None,
        };
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(
            json,
            r#"{"code":"BAD_FRAME","message":"missing idempotency_key"}"#
        );
        assert_eq!(serde_json::from_str::<AppError>(&json).unwrap(), err);
    }

    #[test]
    fn round_trips_with_field() {
        let err = AppError {
            code: ErrorCode::Stale,
            message: "row changed".into(),
            field: Some("base_rev".into()),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(
            json,
            r#"{"code":"STALE","message":"row changed","field":"base_rev"}"#
        );
        assert_eq!(serde_json::from_str::<AppError>(&json).unwrap(), err);
    }

    #[test]
    fn codes_serialize_screaming_snake_case() {
        let cases = [
            (ErrorCode::BadFrame, "BAD_FRAME"),
            (ErrorCode::Unsupported, "UNSUPPORTED"),
            (ErrorCode::Forbidden, "FORBIDDEN"),
            (ErrorCode::NotPrimary, "NOT_PRIMARY"),
            (ErrorCode::SchemaTooNew, "SCHEMA_TOO_NEW"),
            (ErrorCode::Expired, "EXPIRED"),
            (ErrorCode::Stale, "STALE"),
            (ErrorCode::TokenRevoked, "TOKEN_REVOKED"),
            (ErrorCode::TokenInvalid, "TOKEN_INVALID"),
            (ErrorCode::HubSlow, "HUB_SLOW"),
            (ErrorCode::Internal, "INTERNAL"),
            (ErrorCode::ValidationFailed, "VALIDATION_FAILED"),
            (ErrorCode::AlreadyBootstrapped, "ALREADY_BOOTSTRAPPED"),
            (ErrorCode::IdempotencyConflict, "IDEMPOTENCY_CONFLICT"),
            (ErrorCode::AuthFailed, "AUTH_FAILED"),
            (ErrorCode::AuthLocked, "AUTH_LOCKED"),
            (ErrorCode::PermissionDenied, "PERMISSION_DENIED"),
            (ErrorCode::PinRequired, "PIN_REQUIRED"),
            (ErrorCode::PinRejected, "PIN_REJECTED"),
            (ErrorCode::ReasonRequired, "REASON_REQUIRED"),
            (ErrorCode::SessionLocked, "SESSION_LOCKED"),
            (ErrorCode::NotFound, "NOT_FOUND"),
            (ErrorCode::BadRequest, "BAD_REQUEST"),
            (ErrorCode::RateLimited, "RATE_LIMITED"),
            (ErrorCode::OtpInvalid, "OTP_INVALID"),
            (ErrorCode::OtpExpired, "OTP_EXPIRED"),
            (ErrorCode::PasswordTooShort, "PASSWORD_TOO_SHORT"),
            (ErrorCode::PasswordCommon, "PASSWORD_COMMON"),
            (ErrorCode::AlreadySet, "ALREADY_SET"),
            (ErrorCode::PasswordRequired, "PASSWORD_REQUIRED"),
            (ErrorCode::TotpInvalid, "TOTP_INVALID"),
            (ErrorCode::SetupIncomplete, "SETUP_INCOMPLETE"),
            (ErrorCode::BranchCodeTaken, "BRANCH_CODE_TAKEN"),
            (ErrorCode::NoHub, "NO_HUB"),
            (ErrorCode::HubIncompatible, "HUB_INCOMPATIBLE"),
            (ErrorCode::BillingDisabled, "BILLING_DISABLED"),
            (ErrorCode::OneFreeBranch, "ONE_FREE_BRANCH"),
            (ErrorCode::StepNotReady, "STEP_NOT_READY"),
            (ErrorCode::PairCodeInvalid, "PAIR_CODE_INVALID"),
            (ErrorCode::PairPinPaused, "PAIR_PIN_PAUSED"),
        ];
        for (code, text) in cases {
            let json = serde_json::to_string(&code).unwrap();
            assert_eq!(json, format!("\"{text}\""));
            assert_eq!(serde_json::from_str::<ErrorCode>(&json).unwrap(), code);
            assert_eq!(code.as_str(), text);
        }
    }
}
