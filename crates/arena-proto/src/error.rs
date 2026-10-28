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
    /// The branch code can no longer change: an invoice has used it.
    BranchCodeLocked,
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
    PairCodeExpired,
    FeatureNotEntitled,
    CloudUnreachable,
    HubNotPaired,
    /// The group still has devices; move them out first.
    GroupNotEmpty,
    /// The device's kind does not fit the group's kind.
    GroupKindMismatch,
    /// The zone still has devices; move them out first.
    ZoneNotEmpty,
    /// Another device already holds that cell.
    CellTaken,
    TierLocked,
    /// The branch's device cap is reached.
    DeviceLimit,
    HsnRequired,
    BarcodeTaken,
    SetupCodeInvalid,
    LoginTaken,
    UserLimit,
    UsernameTaken,
    PhoneTaken,
    PcBusy,
    PcUnavailable,
    NoRate,
    DeviceUnassigned,
    /// A console has an ended session whose controllers staff have not yet marked.
    ControllersUnchecked,
    /// The payments do not add up to the bill total.
    PaymentShort,
    /// A UPI or card payment is missing its reference.
    ReferenceRequired,
    /// A WhatsApp receipt for a guest, or for a player whose current transactional consent is not
    /// granted.
    ConsentRequired,
    /// A coupon that cannot be applied to the bill; the message says why (Story 5.21).
    CouponInvalid,
    /// A void of an order that is already settled; settled orders are refunded (Story 5.23).
    OrderSettled,
    /// A refund names an invoice number the branch never issued (Story 5.24).
    InvoiceNotFound,
    /// A refund above what was paid on the invoice less earlier refunds (Story 5.24).
    RefundExceedsPaid,
    /// An Undo that is late, from other staff, or blocked by an irreversible effect or a changed
    /// state.
    UndoNotAllowed,
    /// A wallet payment above the player's balance.
    InsufficientBalance,
    /// A credit spend above the player's limit.
    CreditLimit,
    /// A credit limit set below what the player already owes.
    CreditBelowOutstanding,
    /// A second credit override for the same player on one business day.
    OverrideUsedToday,
    /// A credit spend for a player whose credit is paused for being 15+ days overdue.
    CreditPaused,
    /// A second pass sold to a player who already holds an unexpired one.
    PassAlreadyActive,
    /// A pack session started on a pack past its expiry or with no minutes left.
    PackExpired,
    /// A pack session started on a device group the pack does not allow.
    PlanNotForGroup,
    /// A pass or block session started outside the window of the plan.
    PassOutsideWindow,
    /// An extend that would run past the branch closing time.
    PastClosing,
    /// A stock decrement (waste, sale or write-off) above the quantity on hand.
    OutOfStock,
    /// A `shift.open` while a shift is already open at the branch; the message names whose.
    ShiftAlreadyOpen,
    /// An intent with a cash payment row while no shift is open.
    ShiftNotOpen,
    /// A counter sale of a bundle with a component short of stock; the message names it.
    BundleShort,
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
            Self::BranchCodeLocked => "BRANCH_CODE_LOCKED",
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
            Self::SetupCodeInvalid => "SETUP_CODE_INVALID",
            Self::PairPinPaused => "PAIR_PIN_PAUSED",
            Self::PairCodeExpired => "PAIR_CODE_EXPIRED",
            Self::FeatureNotEntitled => "FEATURE_NOT_ENTITLED",
            Self::CloudUnreachable => "CLOUD_UNREACHABLE",
            Self::HubNotPaired => "HUB_NOT_PAIRED",
            Self::GroupNotEmpty => "GROUP_NOT_EMPTY",
            Self::GroupKindMismatch => "GROUP_KIND_MISMATCH",
            Self::ZoneNotEmpty => "ZONE_NOT_EMPTY",
            Self::CellTaken => "CELL_TAKEN",
            Self::TierLocked => "TIER_LOCKED",
            Self::DeviceLimit => "DEVICE_LIMIT",
            Self::HsnRequired => "HSN_REQUIRED",
            Self::BarcodeTaken => "BARCODE_TAKEN",
            Self::LoginTaken => "LOGIN_TAKEN",
            Self::UserLimit => "USER_LIMIT",
            Self::UsernameTaken => "USERNAME_TAKEN",
            Self::PhoneTaken => "PHONE_TAKEN",
            Self::PcBusy => "PC_BUSY",
            Self::PcUnavailable => "PC_UNAVAILABLE",
            Self::NoRate => "NO_RATE",
            Self::DeviceUnassigned => "DEVICE_UNASSIGNED",
            Self::ControllersUnchecked => "CONTROLLERS_UNCHECKED",
            Self::PaymentShort => "PAYMENT_SHORT",
            Self::ReferenceRequired => "REFERENCE_REQUIRED",
            Self::ConsentRequired => "CONSENT_REQUIRED",
            Self::CouponInvalid => "COUPON_INVALID",
            Self::OrderSettled => "ORDER_SETTLED",
            Self::InvoiceNotFound => "INVOICE_NOT_FOUND",
            Self::RefundExceedsPaid => "REFUND_EXCEEDS_PAID",
            Self::UndoNotAllowed => "UNDO_NOT_ALLOWED",
            Self::InsufficientBalance => "INSUFFICIENT_BALANCE",
            Self::CreditLimit => "CREDIT_LIMIT",
            Self::CreditBelowOutstanding => "CREDIT_BELOW_OUTSTANDING",
            Self::OverrideUsedToday => "OVERRIDE_USED_TODAY",
            Self::CreditPaused => "CREDIT_PAUSED",
            Self::PassAlreadyActive => "PASS_ALREADY_ACTIVE",
            Self::PackExpired => "PACK_EXPIRED",
            Self::PlanNotForGroup => "PLAN_NOT_FOR_GROUP",
            Self::PassOutsideWindow => "PASS_OUTSIDE_WINDOW",
            Self::PastClosing => "PAST_CLOSING",
            Self::OutOfStock => "OUT_OF_STOCK",
            Self::ShiftAlreadyOpen => "SHIFT_ALREADY_OPEN",
            Self::ShiftNotOpen => "SHIFT_NOT_OPEN",
            Self::BundleShort => "BUNDLE_SHORT",
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
            (ErrorCode::BranchCodeLocked, "BRANCH_CODE_LOCKED"),
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
            (ErrorCode::SetupCodeInvalid, "SETUP_CODE_INVALID"),
            (ErrorCode::PairPinPaused, "PAIR_PIN_PAUSED"),
            (ErrorCode::PairCodeExpired, "PAIR_CODE_EXPIRED"),
            (ErrorCode::FeatureNotEntitled, "FEATURE_NOT_ENTITLED"),
            (ErrorCode::CloudUnreachable, "CLOUD_UNREACHABLE"),
            (ErrorCode::HubNotPaired, "HUB_NOT_PAIRED"),
            (ErrorCode::GroupNotEmpty, "GROUP_NOT_EMPTY"),
            (ErrorCode::GroupKindMismatch, "GROUP_KIND_MISMATCH"),
            (ErrorCode::ZoneNotEmpty, "ZONE_NOT_EMPTY"),
            (ErrorCode::CellTaken, "CELL_TAKEN"),
            (ErrorCode::TierLocked, "TIER_LOCKED"),
            (ErrorCode::DeviceLimit, "DEVICE_LIMIT"),
            (ErrorCode::HsnRequired, "HSN_REQUIRED"),
            (ErrorCode::BarcodeTaken, "BARCODE_TAKEN"),
            (ErrorCode::LoginTaken, "LOGIN_TAKEN"),
            (ErrorCode::UserLimit, "USER_LIMIT"),
            (ErrorCode::UsernameTaken, "USERNAME_TAKEN"),
            (ErrorCode::PhoneTaken, "PHONE_TAKEN"),
            (ErrorCode::PcBusy, "PC_BUSY"),
            (ErrorCode::PcUnavailable, "PC_UNAVAILABLE"),
            (ErrorCode::NoRate, "NO_RATE"),
            (ErrorCode::DeviceUnassigned, "DEVICE_UNASSIGNED"),
            (ErrorCode::ControllersUnchecked, "CONTROLLERS_UNCHECKED"),
            (ErrorCode::PaymentShort, "PAYMENT_SHORT"),
            (ErrorCode::ReferenceRequired, "REFERENCE_REQUIRED"),
            (ErrorCode::ConsentRequired, "CONSENT_REQUIRED"),
            (ErrorCode::CouponInvalid, "COUPON_INVALID"),
            (ErrorCode::OrderSettled, "ORDER_SETTLED"),
            (ErrorCode::InvoiceNotFound, "INVOICE_NOT_FOUND"),
            (ErrorCode::RefundExceedsPaid, "REFUND_EXCEEDS_PAID"),
            (ErrorCode::UndoNotAllowed, "UNDO_NOT_ALLOWED"),
            (ErrorCode::InsufficientBalance, "INSUFFICIENT_BALANCE"),
            (ErrorCode::CreditLimit, "CREDIT_LIMIT"),
            (
                ErrorCode::CreditBelowOutstanding,
                "CREDIT_BELOW_OUTSTANDING",
            ),
            (ErrorCode::OverrideUsedToday, "OVERRIDE_USED_TODAY"),
            (ErrorCode::CreditPaused, "CREDIT_PAUSED"),
            (ErrorCode::PassAlreadyActive, "PASS_ALREADY_ACTIVE"),
            (ErrorCode::PackExpired, "PACK_EXPIRED"),
            (ErrorCode::PlanNotForGroup, "PLAN_NOT_FOR_GROUP"),
            (ErrorCode::PassOutsideWindow, "PASS_OUTSIDE_WINDOW"),
            (ErrorCode::PastClosing, "PAST_CLOSING"),
            (ErrorCode::OutOfStock, "OUT_OF_STOCK"),
            (ErrorCode::ShiftAlreadyOpen, "SHIFT_ALREADY_OPEN"),
            (ErrorCode::ShiftNotOpen, "SHIFT_NOT_OPEN"),
            (ErrorCode::BundleShort, "BUNDLE_SHORT"),
        ];
        for (code, text) in cases {
            let json = serde_json::to_string(&code).unwrap();
            assert_eq!(json, format!("\"{text}\""));
            assert_eq!(serde_json::from_str::<ErrorCode>(&json).unwrap(), code);
            assert_eq!(code.as_str(), text);
        }
    }
}
