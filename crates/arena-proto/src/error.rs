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
        ];
        for (code, text) in cases {
            let json = serde_json::to_string(&code).unwrap();
            assert_eq!(json, format!("\"{text}\""));
            assert_eq!(serde_json::from_str::<ErrorCode>(&json).unwrap(), code);
            assert_eq!(code.as_str(), text);
        }
    }
}
