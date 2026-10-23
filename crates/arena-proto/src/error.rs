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
    /// The Hub did not answer in time.
    HubSlow,
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
            (ErrorCode::HubSlow, "HUB_SLOW"),
        ];
        for (code, text) in cases {
            let json = serde_json::to_string(&code).unwrap();
            assert_eq!(json, format!("\"{text}\""));
            assert_eq!(serde_json::from_str::<ErrorCode>(&json).unwrap(), code);
        }
    }
}
