//! The ordering rules of the kiosk's offline intent queue (the storage lives in the service).
//!
//! - Every queued intent gets its ULID idempotency key and `issued_at` when the event happens,
//!   never at send time, so a replay is the same intent and the Hub's dedupe applies it once.
//! - On every connect the queue is replayed oldest first (by queue position), one at a time.
//! - An entry is deleted once the Hub answers with `Applied` (a fresh apply or a stored outcome)
//!   or with a deterministic rejection (retrying would fail the same way). A transient failure
//!   stops the replay; the entry stays first in line for the next connect. `UNSUPPORTED` and
//!   `BAD_FRAME` count as transient (an older Hub may not know the intent yet), and so does a
//!   `VALIDATION_FAILED` on `issued_at` (this kiosk's clock is ahead: fixing the clock fixes it).
//! - An entry older than [`QUEUE_TTL_MS`] is dropped without sending (the Hub would answer
//!   `EXPIRED`).

use arena_proto::ErrorCode;

use crate::client::ClientError;

/// How long a queued intent is kept: the Hub's intent TTL (7 days).
pub const QUEUE_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// What to do with the entry just sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayStep {
    /// Applied: delete it and send the next one.
    Applied,
    /// Refused for good: delete it (and log it) and send the next one.
    Rejected,
    /// Not decided: keep it and stop until the next connect.
    Stop,
}

/// Whether `code` would come back the same on every retry of the same intent.
pub fn is_deterministic(code: ErrorCode) -> bool {
    match code {
        ErrorCode::Forbidden
        | ErrorCode::Expired
        | ErrorCode::Stale
        | ErrorCode::ValidationFailed
        | ErrorCode::AlreadyBootstrapped
        | ErrorCode::BranchCodeLocked
        | ErrorCode::IdempotencyConflict
        | ErrorCode::PermissionDenied
        | ErrorCode::PinRequired
        | ErrorCode::PinRejected
        | ErrorCode::ReasonRequired
        | ErrorCode::NotFound
        | ErrorCode::BadRequest
        | ErrorCode::OtpInvalid
        | ErrorCode::OtpExpired
        | ErrorCode::PasswordTooShort
        | ErrorCode::PasswordCommon
        | ErrorCode::AlreadySet
        | ErrorCode::PasswordRequired
        | ErrorCode::TotpInvalid
        | ErrorCode::SetupIncomplete
        | ErrorCode::BranchCodeTaken
        | ErrorCode::HubIncompatible
        | ErrorCode::BillingDisabled
        | ErrorCode::OneFreeBranch
        | ErrorCode::StepNotReady
        | ErrorCode::SetupCodeInvalid
        | ErrorCode::PairCodeInvalid
        | ErrorCode::PairCodeExpired
        | ErrorCode::FeatureNotEntitled
        | ErrorCode::GroupNotEmpty
        | ErrorCode::GroupKindMismatch
        | ErrorCode::ZoneNotEmpty
        | ErrorCode::CellTaken
        | ErrorCode::TierLocked
        | ErrorCode::DeviceLimit => true,
        ErrorCode::BadFrame
        | ErrorCode::HsnRequired
        | ErrorCode::BarcodeTaken
        | ErrorCode::Unsupported
        | ErrorCode::NotPrimary
        | ErrorCode::SchemaTooNew
        | ErrorCode::TokenRevoked
        | ErrorCode::TokenInvalid
        | ErrorCode::HubSlow
        | ErrorCode::Internal
        | ErrorCode::AuthFailed
        | ErrorCode::AuthLocked
        | ErrorCode::SessionLocked
        | ErrorCode::RateLimited
        | ErrorCode::NoHub
        | ErrorCode::PairPinPaused
        | ErrorCode::CloudUnreachable
        | ErrorCode::HubNotPaired => false,
    }
}

/// Whether the Hub refused the intent because its `issued_at` is ahead of the Hub's clock.
pub fn is_clock_skew(err: &ClientError) -> bool {
    matches!(err, ClientError::Hub(e)
        if e.code == ErrorCode::ValidationFailed && e.field.as_deref() == Some("issued_at"))
}

/// Classifies the result of sending one queued intent.
pub fn classify<T>(result: &Result<T, ClientError>) -> ReplayStep {
    match result {
        Ok(_) => ReplayStep::Applied,
        Err(e) if is_clock_skew(e) => ReplayStep::Stop,
        Err(ClientError::Hub(e)) if is_deterministic(e.code) => ReplayStep::Rejected,
        Err(_) => ReplayStep::Stop,
    }
}

/// Whether an intent issued at `issued_at_ms` has outlived the queue TTL at `now_ms`.
pub fn is_expired(issued_at_ms: i64, now_ms: i64) -> bool {
    now_ms.saturating_sub(issued_at_ms) > QUEUE_TTL_MS
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_proto::{AppError, AppliedSummary};

    #[test]
    fn a_step_not_ready_refusal_is_deterministic() {
        assert!(is_deterministic(ErrorCode::StepNotReady));
    }

    #[test]
    fn applied_and_deterministic_refusals_delete_transient_ones_stop() {
        let ok: Result<AppliedSummary, ClientError> = Ok(AppliedSummary {
            intent_id: "I".into(),
            changes: vec![],
        });
        assert_eq!(classify(&ok), ReplayStep::Applied);
        let hub =
            |code| -> Result<(), ClientError> { Err(ClientError::Hub(AppError::new(code, "x"))) };
        for code in [
            ErrorCode::ValidationFailed,
            ErrorCode::Expired,
            ErrorCode::Forbidden,
            ErrorCode::PermissionDenied,
            ErrorCode::IdempotencyConflict,
        ] {
            assert_eq!(classify(&hub(code)), ReplayStep::Rejected, "{code:?}");
        }
        for code in [
            ErrorCode::Unsupported,
            ErrorCode::BadFrame,
            ErrorCode::NotPrimary,
            ErrorCode::HubSlow,
            ErrorCode::Internal,
            ErrorCode::TokenRevoked,
        ] {
            assert_eq!(classify(&hub(code)), ReplayStep::Stop, "{code:?}");
        }
        // A fast clock: kept, not dropped.
        let skew: Result<(), ClientError> = Err(ClientError::Hub(AppError::with_field(
            ErrorCode::ValidationFailed,
            "issued_at",
            "ahead",
        )));
        assert!(is_clock_skew(skew.as_ref().unwrap_err()));
        assert_eq!(classify(&skew), ReplayStep::Stop);
        for e in [
            ClientError::HubSlow,
            ClientError::Closed,
            ClientError::Io("x".into()),
        ] {
            assert_eq!(classify::<()>(&Err(e)), ReplayStep::Stop);
        }
    }

    #[test]
    fn the_ttl_is_seven_days() {
        let day = 24 * 60 * 60 * 1000;
        assert!(!is_expired(0, 7 * day));
        assert!(is_expired(0, 7 * day + 1));
        assert!(!is_expired(10, 0), "a future issued_at is not expired");
    }
}
