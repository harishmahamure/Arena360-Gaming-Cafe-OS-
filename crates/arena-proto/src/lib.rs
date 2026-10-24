//! `arena-proto`: protocol types shared by the Hub, the Kiosk and every UI surface.
//!
//! TypeScript bindings are generated from these types with ts-rs (`pnpm gen:types`) into
//! `packages/ui/src/generated`.

mod account;
mod actor;
mod admin;
mod audit;
mod connectivity;
pub mod currency;
mod display;
mod error;
mod frame;
mod intent;
mod kiosk;
mod operating;
mod permission;
mod process;
mod scope;
mod signin;
mod signup;
mod snapshot;
mod ui_auth;
mod wizard;

pub use account::{
    BACKUP_CODE_CHARS, BACKUP_CODE_COUNT, BackupCodes, PASSWORD_MAX_CHARS, PASSWORD_MIN_CHARS,
    SetPassword, TOTP_DIGITS, TOTP_PERIOD_S, TotpCode, TotpStarted,
};
pub use actor::{Actor, ActorKind, ElevatedBy, ElevationKind, ElevationMode};
pub use admin::{
    ADMIN_EXIT_FAILED_CODE, ADMIN_EXIT_MS, AdminExitMode, AdminPinOk, AdminUnlockAck,
    AdminUnlockAckEvent, DeviceAdminExit, DeviceAdminRelock, DeviceAdminUnlock,
    FLAG_STALE_VERIFIER, MAX_VERIFIER_ENTRIES, ManagerVerifier, VerifierEntry, VerifyAdminPin,
    validate_exit_id,
};
pub use audit::{
    AUDIT_DEFAULT_LIMIT, AUDIT_MAX_LIMIT, AuditActor, AuditActorKind, AuditAdminExit, AuditDetail,
    AuditElevatedBy, AuditElevationKind, AuditElevationMode, AuditFacets, AuditImage, AuditItem,
    AuditPage, AuditStaffOption, AuditValue, ELEVATION_HEADER,
};
pub use connectivity::{Connectivity, ConnectivityState};
pub use display::{
    HC_DEFAULT, HC_PREFIX, HcTarget, KEY_HC_HUB, hc_key, parse_hc_key, parse_hc_value,
};
pub use error::{AppError, ErrorCode};
pub use frame::{
    AppliedSummary, ChangeOpKind, ChangeSummary, Envelope, ErrorFrame, EventFrame, FRAME_KINDS,
    Frame, FrameBody, FrameError, Hello, HelloOk, HubEvent, IntentResult, IntentSubmit,
    MAX_RID_LEN, MAX_SETTINGS_GET_KEYS, PROTOCOL_VERSION, SettingsGet, SettingsValues, Versions,
    check_envelope, parse_frame, parse_frame_value, parse_json,
};
pub use intent::{
    AuditReadGranted, BranchBootstrap, BranchSeedDefaults, DeviceRegister, DeviceRevoke,
    DeviceVersionReported, Intent, IntentCategory, IntentKind, IntentPayload, PermEffect,
    ReasonCode, RolePerm, RoleUpsert, SettingsSet, StaffUpsert, reasons_json,
};
pub use kiosk::{
    KioskHubState, KioskHubStatus, KioskLog, KioskLogLevel, MAX_KIOSK_LOG_CODE_LEN,
    MAX_KIOSK_LOG_DETAIL_CHARS,
};
pub use operating::{
    ADS_POLICY_VERSION, AdConsentState, AdsConsent, AdsConsentSet, BACKUP_PROBE_PREFIX,
    BackupDirProblem, DAYS_PER_WEEK, DEFAULT_ADS_CONSENT, DEFAULT_BACKUP_DIR, DEFAULT_FULFILMENT,
    DEFAULT_KIOSK_VOLUME, DEFAULT_OPENING_HOURS, DEFAULT_OPENING_HOURS_SET, FULFILMENT_MODES,
    KEY_ADS_CONSENT, KEY_BACKUP_DIR, KEY_FULFILMENT, KEY_KIOSK_VOLUME_DEFAULT, KEY_OPENING_HOURS,
    KEY_OPENING_HOURS_SET, KIOSK_OPERATING_KEYS, KIOSK_VOLUME_MAX, LAST_MINUTE, OpeningDay,
    OpeningHoursError, SetupStatus, has_open_day, is_absolute_path, opening_hours_json,
    parse_opening_hours,
};
pub use permission::{Permission, Requirement};
pub use process::{
    ARENA_SIGNER_CN, BACKFILL_PROCESS_MODE, BOOTSTRAP_PROCESS_MODE, DEFAULT_PROCESS_ALLOW,
    DEFAULT_PROCESS_BLOCK, INSTALL_DIR_TOKEN, KEY_PROCESS_ALLOW, KEY_PROCESS_BLOCK,
    KEY_PROCESS_MODE, KioskProcessBlocked, MAX_EXE_NAME_CHARS, MAX_PATH_GLOB_CHARS,
    MAX_PROCESS_PATH_CHARS, MAX_PROCESS_RULES, MAX_PROCESS_RULES_CHARS, MAX_SIGNER_CHARS,
    PATH_ELLIPSIS, PATH_TOKENS, ProcessBlockReason, ProcessList, ProcessMode, ProcessRule,
    SYSTEM_DIR_TOKEN, is_catch_all_glob, is_file_name_glob, is_full_path_glob,
    is_user_writable_glob, parse_process_mode, parse_process_rules, truncate_path_left,
};
pub use scope::{ClientClass, PAIRED_SETTINGS_PREFIX, class_scope, in_scope, may_read_setting};
pub use signin::{
    BACKUP_CODES_LOW, DEVICE_LABEL_MAX_CHARS, SIGNIN_CHALLENGE_TTL_S, SIGNIN_LOCK_S,
    SIGNIN_MAX_ATTEMPTS, SIGNIN_MAX_FAILURES, Signin, SigninMethod, SigninNext, SigninReply,
    SigninSms, SigninSmsSent, SigninVerified, SigninVerify, SmsFallback, TRUSTED_DEVICE_TTL_S,
    TrustedDevice,
};
pub use signup::{
    MfaState, OTP_DIGITS, OTP_MAX_ATTEMPTS, OTP_RESEND_AFTER_S, OTP_SENDS_PER_HOUR, OTP_TTL_S,
    SessionInfo, SessionOwner, SignupChallenge, SignupNext, SignupResend, SignupStart,
    SignupVerified, SignupVerify,
};
pub use snapshot::{
    AppliedEvent, HubSnapshot, LocalePending, NumberFormat, PendingNumber, PendingText,
    SettingsChangedEvent, SnapshotBranch, SnapshotDevice, SnapshotLocale, SnapshotUi,
    SymbolPosition,
};
pub use ui_auth::{
    ElevateDrop, ElevateOk, ElevateRequest, ElevationItem, MAX_TICKET_USES, SessionStaff,
    SettingEntry, SignInOk, SignInRequest, UiError, UiIntentRequest, UiMe, UnlockOk, UnlockRequest,
};
pub use wizard::{
    BRANCH_CODE_MAX, BRANCH_CODE_MIN, BranchCodeSet, COUNTRIES, CafeDetails, Country, CountryRow,
    SetBranchCode, StepState, WizardBranch, WizardState, WizardStep, WizardStepInfo, WizardTenant,
    country, current_step, valid_branch_code,
};

/// Returns this crate's name.
pub fn crate_name() -> &'static str {
    "arena-proto"
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name_is_set() {
        assert_eq!(super::crate_name(), "arena-proto");
    }
}
