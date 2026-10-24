//! `arena-proto`: protocol types shared by the Hub, the Kiosk and every UI surface.
//!
//! TypeScript bindings are generated from these types with ts-rs (`pnpm gen:types`) into
//! `packages/ui/src/generated`.

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
mod permission;
mod process;
mod scope;
mod signup;
mod snapshot;
mod ui_auth;

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
