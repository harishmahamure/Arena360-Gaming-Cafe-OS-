//! `arena-proto`: protocol types shared by the Hub, the Kiosk and every UI surface.
//!
//! TypeScript bindings are generated from these types with ts-rs (`pnpm gen:types`) into
//! `packages/ui/src/generated`.

mod account;
mod actor;
mod admin;
mod alert;
mod audit;
mod billing;
mod catalog;
mod commands;
mod connectivity;
pub mod currency;
mod device_spec;
mod discovery;
mod display;
mod entitlement;
mod error;
mod frame;
mod intent;
mod kiosk;
mod kiosk_pairing;
mod kitchen;
pub mod licence;
mod operating;
pub mod pairing;
mod pc_command;
mod permission;
mod plans;
mod players;
mod pricing;
mod printer;
mod process;
mod scope;
mod session;
mod setup_devices;
mod signin;
mod signup;
mod snapshot;
mod staff_invite;
mod staff_list;
mod stock;
mod tax;
mod test_session;
pub mod tier;
mod ui_auth;
mod undo;
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
pub use alert::{AlertAck, AlertKind, AlertRaise, SnapshotAlert};
pub use audit::{
    AUDIT_DEFAULT_LIMIT, AUDIT_MAX_LIMIT, AuditActor, AuditActorKind, AuditAdminExit, AuditDetail,
    AuditElevatedBy, AuditElevationKind, AuditElevationMode, AuditFacets, AuditImage, AuditItem,
    AuditPage, AuditStaffOption, AuditValue, ELEVATION_HEADER,
};
pub use billing::{
    BILLING_GRACE_RANGE, BILLING_MINIMUM_RANGE, BILLING_ROUNDING_RANGE, BillingRules,
    DEFAULT_BILLING_CREDIT_AUTO_PAUSE, DEFAULT_BILLING_GRACE_MIN, DEFAULT_BILLING_MINIMUM_MIN,
    DEFAULT_BILLING_ROUNDING_MIN, DEFAULT_BILLING_TIME_UP_MODE, KEY_BILLING_CREDIT_AUTO_PAUSE,
    KEY_BILLING_GRACE_MIN, KEY_BILLING_MINIMUM_MIN, KEY_BILLING_ROUNDING_MIN,
    KEY_BILLING_TIME_UP_MODE, KIOSK_BILLING_KEYS, TIME_UP_MODES, TimeUpMode,
};
pub use catalog::{
    BundleComponent, CatalogEntry, ComponentView, IMAGE_PREFIX, MAX_BARCODE_CHARS,
    MAX_BUNDLE_COMPONENTS, MAX_CATEGORY_CHARS, MAX_COMPONENT_QTY, MAX_IMAGE_BYTES, MAX_IMAGE_PX,
    MAX_PRICE_MINOR, MAX_PRODUCT_NAME_CHARS, Product, ProductDelete, ProductUpsert,
    REASON_COMPONENT_INACTIVE, availability, bundle_quantity, hsn_valid, image_problem,
};
pub use commands::{
    COMMAND_PAYLOAD_MAX, COMMAND_SCHEMA_VERSION, COMMANDS_PAGE, CommandResult, CommandResultAck,
    CommandStatus, CommandWire, CommandsPage, SETTINGS_COMMAND_KEYS, SETUP_COMMAND_KINDS,
    is_setup_command,
};
pub use connectivity::{Connectivity, ConnectivityState};
pub use device_spec::{
    DeviceSpec, MIN_FREE_BYTES, MIN_FREE_GB, MIN_RAM_BYTES, MIN_RAM_GB, SpecFailure,
    SpecRequirement, check_device, meminfo_total_bytes,
};
pub use discovery::{
    DEFAULT_WEB_PORT, DISCOVERY_WINDOW_MS, DiscoveredHub, DiscoveryFound, HUB_PRODUCT, HubInfo,
    MAX_BRANCH_NAME_BYTES, MAX_HUB_NAME_BYTES, MDNS_SERVICE, PROBE_TIMEOUT_MS, ProbeOk,
    SelectedHub, TXT_BRANCH, TXT_FP, TXT_HUB, TXT_PROTO, TXT_WEB, is_fingerprint,
    sanitize_branch_name, sanitize_hub_name, txt_records,
};
pub use display::{
    HC_DEFAULT, HC_PREFIX, HcTarget, KEY_HC_HUB, hc_key, parse_hc_key, parse_hc_value,
};
pub use entitlement::{
    CLOCK_SKEW_LIMIT_MS, Entitlement, LicenceApply, LicenceStanding, clock_wrong,
};
pub use error::{AppError, ErrorCode};
pub use frame::{
    AppliedSummary, ChangeOpKind, ChangeSummary, Envelope, ErrorFrame, EventFrame, FRAME_KINDS,
    Frame, FrameBody, FrameError, Hello, HelloOk, HubEvent, IntentResult, IntentSubmit,
    MAX_RID_LEN, MAX_SETTINGS_GET_KEYS, PROTOCOL_VERSION, SettingsGet, SettingsValues, Versions,
    check_envelope, parse_frame, parse_frame_value, parse_json,
};
pub use intent::{
    AuditReadGranted, BootstrapOnboarding, BranchBootstrap, BranchSeedDefaults, DeviceAssign,
    DeviceMaintenance, DevicePlace, DeviceRegister, DeviceRevoke, DeviceVersionReported,
    FLOOR_COLS, FLOOR_ROWS, GROUP_COLOURS, GROUP_KINDS, GroupDelete, GroupUpsert, Intent,
    IntentCategory, IntentKind, IntentPayload, MAINTENANCE_NOTE_MAX, PermEffect, RATE_MAX_MINOR,
    RateUpsert, ReasonCode, RolePerm, RoleUpsert, SettingsSet, StaffDeactivate,
    StaffSetCredentials, StaffUpsert, ZoneDelete, ZoneUpsert, reasons_json,
};
pub use kiosk::{
    KioskHubState, KioskHubStatus, KioskLog, KioskLogLevel, MAX_KIOSK_LOG_CODE_LEN,
    MAX_KIOSK_LOG_DETAIL_CHARS,
};
pub use kiosk_pairing::{
    DeviceKind, KioskPairInfo, KioskPaired, KioskPairingCode, MAX_PC_NAME_CHARS, PAIR_CODE_DIGITS,
    PAIR_CODE_TTL_MS, PAIR_PATH, PAIR_PIN_MAX_FAILURES, PAIR_PIN_PAUSE_S, PAIR_PIN_WINDOW_MS,
    PAIR_SYSTEM_ACTOR, PairRequest, PairResponse, PairSecret, is_pair_code, is_pc_name,
    sanitize_pc_name,
};
pub use kitchen::{
    DEFAULT_KITCHEN_KOT_MODE, DEFAULT_KITCHEN_KOT_PAPER_MM, DEFAULT_KITCHEN_KOT_PRINTER,
    DEFAULT_LICENCE_FLAGS, FLAG_KITCHEN, KEY_DEVICE_CAP, KEY_KITCHEN_KOT_MODE,
    KEY_KITCHEN_KOT_PAPER_MM, KEY_KITCHEN_KOT_PRINTER, KEY_LICENCE_CLOCK, KEY_LICENCE_FLAGS,
    KEY_LICENCE_STATUS, KEY_USER_CAP, KITCHEN_GATED_KEYS, KITCHEN_KOT_MODES, TierGate, has_flag,
    kitchen_gate,
};
pub use licence::{LicenceClaims, LicenceError, VerifyKey};
pub use operating::{
    ADS_POLICY_VERSION, AdConsentState, AdsConsent, AdsConsentSet, BACKUP_PROBE_PREFIX,
    BackupDirProblem, DAYS_PER_WEEK, DEFAULT_ADS_CONSENT, DEFAULT_BACKUP_DIR, DEFAULT_FULFILMENT,
    DEFAULT_KIOSK_VOLUME, DEFAULT_OPENING_HOURS, DEFAULT_OPENING_HOURS_SET, DEFAULT_SETUP_LIVE,
    FULFILMENT_MODES, KEY_ADS_CONSENT, KEY_BACKUP_DIR, KEY_FULFILMENT, KEY_KIOSK_VOLUME_DEFAULT,
    KEY_OPENING_HOURS, KEY_OPENING_HOURS_SET, KEY_SETUP_LIVE, KIOSK_OPERATING_KEYS,
    KIOSK_VOLUME_MAX, LAST_MINUTE, OpeningDay, OpeningHoursError, SetupStatus, TestSessionState,
    has_open_day, is_absolute_path, is_backup_location, is_backup_tree_uri, opening_hours_json,
    parse_opening_hours,
};
pub use pairing::{
    ActivePairingCode, BOOTSTRAP_DAY_START_MIN, BootstrapOwner, BootstrapProfile,
    BranchBootstrapCommand, HEARTBEAT_BOOT_INTERVAL_S, HEARTBEAT_LIVE_INTERVAL_S,
    HEARTBEAT_MIN_GAP_MS, HUB_TOKEN_PREFIX, HeartbeatFence, HeartbeatRequest, HeartbeatResponse,
    HubPairRequest, HubPairResponse, HubPlatform, InstallHubState, IssuePairingCode, OWNER_ROLE,
    PAIR_ATTEMPTS_PER_IP_PER_HOUR, PAIR_FAILURES_PER_CODE, PAIRING_CODE_ALPHABET, PAIRING_CODE_LEN,
    PAIRING_CODE_TTL_MS, PairHubOk, PairHubRequest, PairingCodeIssued, SETUP_CODE_HUB_TTL_MS,
    SETUP_CODE_PLAINTEXT_TTL_MS, SetupCodeReveal, check_bootstrap, format_pairing_code,
    normalize_pairing_code, valid_pairing_code,
};
pub use pc_command::{
    DevicePower, DeviceScreen, LockReason, PC_MESSAGE_MAX, PcAck, PcAckEvent, PcAction,
};
pub use permission::{Permission, Requirement};
pub use plans::{
    MINUTES_PER_DAY, OwnedPlanView, PLAN_KINDS, PLAN_MAX_GROUPS, PLAN_MAX_MINUTES,
    PLAN_MAX_VALIDITY_DAYS, PlanSell, PlanSellReverted, PlanUpsert, PlanView,
};
pub use players::{
    CONSENT_TRANSACTIONAL, PLAYER_EMAIL_MAX_CHARS, PLAYER_NAME_MAX_CHARS, PLAYER_NOTES_MAX_CHARS,
    PLAYER_PASSWORD_MIN_CHARS, PLAYER_PHONE_MAX_DIGITS, PLAYER_PHONE_MIN_DIGITS,
    PLAYER_TAG_MAX_CHARS, PLAYER_TAGS_MAX, PLAYER_USERNAME_MAX_CHARS, PLAYER_USERNAME_MIN_CHARS,
    POLICY_VERSION_MAX_CHARS, PlayerConsent, PlayerCreate, PlayerEditForm, PlayerForm, PlayerHit,
    PlayerPasswordForm, PlayerPasswordReset, PlayerProfile, PlayerUpdate, UiPasswordResetRequest,
    UiPlayerEditRequest, UiPlayerRequest, normalize_phone, normalize_username, valid_phone,
    valid_policy_version, valid_tags, valid_username,
};
pub use pricing::{
    RULE_ADJUST_KINDS, RULE_ALL_DAYS, RULE_KINDS, RULE_MAX_DATE_SPAN_DAYS, RULE_MAX_PERCENT_BP,
    RULE_MAX_PRIORITY, RULE_MIN_PERCENT_BP, RuleConflict, RuleUpsert, RuleView,
};
pub use printer::{
    BluetoothAccess, DEFAULT_PRINTER_PAPER_MM, DEFAULT_PRINTER_PORT, DEFAULT_PRINTER_SELECTED,
    DEFAULT_PRINTER_STATUS, FoundPrinter, KEY_PRINTER_PAPER_MM, KEY_PRINTER_SELECTED,
    KEY_PRINTER_STATUS, MAX_PRINTER_ERROR_CHARS, MAX_PRINTER_NAME_CHARS, PRINTER_IP_FIELD,
    PRINTER_PAPER_WIDTHS_MM, PrinterErrorKind, PrinterFindResult, PrinterHealth, PrinterJobFailure,
    PrinterKind, PrinterOutcome, PrinterSelected, PrinterSelectedError, PrinterState,
    PrinterStatus, PrinterStatusReply, PrinterStatusSet, parse_bluetooth_address,
    parse_printer_addr, printer_error_text, valid_printer_error, valid_printer_outcome,
};
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
pub use session::{
    BillLine, BillRevertedEvent, BillSettledEvent, CARD_REFERENCE_MAX, CREDIT_DAY_MAX,
    CREDIT_DAYS_MAX, CREDIT_LIMIT_MAX_MINOR, CREDIT_OVERDUE_DAYS, ClearedSpend, ControllerCheck,
    ControllerResult, CreditAging, CreditAutoPause, CreditCollect, CreditCollectReverted,
    CreditCollection, CreditSetLimit, CreditView, DEFAULT_REMINDER_DAYS, EXTEND_OPTIONS_MIN,
    EndOutcome, ExtendSource, GroupPayer, PAUSE_NOTE_MAX, PauseReason, PaymentMethod, PaymentRow,
    SESSION_WARNING_MARKS_MIN, SessionAssignPlayer, SessionEnd, SessionEndReverted, SessionExpire,
    SessionExtend, SessionForceEnd, SessionForceEndReverted, SessionGroup, SessionListRow,
    SessionListState, SessionListView, SessionMode, SessionPause, SessionRateFlip, SessionResume,
    SessionScreen, SessionSettle, SessionSettleReverted, SessionStart, SessionStartedEvent,
    SessionTransfer, SettleOutcome, SnapshotBill, SnapshotGroupBill, SnapshotSession,
    UNDO_WINDOW_MS, UiSessionConvertRequest, WALLET_TOPUP_MAX_MINOR, WalletChangedEvent,
    WalletTopup, WalletTopupReverted, WalletTxnView, WalletView,
};
pub use setup_devices::{
    BranchSetup, SETUP_ITEMS_MAX, SetupCommandRequest, SetupCommandView, SetupDevice,
    SetupDevicesView, SetupGroup,
};
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
    AppliedEvent, DeviceStatus, EntitlementChangedEvent, HubSnapshot, LocalePending, NumberFormat,
    PendingNumber, PendingText, SettingsChangedEvent, SnapshotActiveRule, SnapshotBranch,
    SnapshotDevice, SnapshotGroup, SnapshotLan, SnapshotLocale, SnapshotUi, SnapshotZone,
    SymbolPosition,
};
pub use staff_invite::{
    InviteRole, InviteStatus, StaffInvite, StaffInviteView, StaffInvited, StaffInvites,
};
pub use staff_list::{StaffList, StaffStatus, StaffView, UiStaffDeactivateRequest};
pub use stock::{
    LotView, MAX_STOCK_IN_QTY, MAX_STOCK_QTY, MAX_SUPPLIER_CHARS, MOVE_STOCK_IN,
    MOVE_STOCK_IN_REVERTED, STOCK_UNDO_MS, StockIn, StockInReverted, consume_plan, valid_date,
    weighted_avg_cost,
};
pub use tax::{
    DEFAULT_TAX, GST_MAX_BP, GSTIN_INVALID, KEY_TAX, RATE_RANGE, SIMPLE_MAX_BP, TaxMode,
    TaxSettings, gstin_valid,
};
pub use test_session::{
    DEFAULT_TEST_SESSION, DeviceTestSessionCompleted, DeviceTestSessionStart, KEY_TEST_SESSION,
    TEST_SESSION_CONFIRM_MS, TEST_SESSION_MS, TestSessionOutcome, TestSessionStatus,
};
pub use tier::{EntitlementSource, TIERS, Tier, TierDefaults, TierLimits};
pub use ui_auth::{
    ElevateDrop, ElevateOk, ElevateRequest, ElevationItem, MAX_STAFF_PASSWORD_CHARS,
    MAX_TICKET_USES, MIN_STAFF_PASSWORD_CHARS, SessionStaff, SettingEntry, SetupCodeRequest,
    SignInOk, SignInRequest, UiError, UiIntentRequest, UiIntentResult, UiMe, UnlockOk,
    UnlockRequest,
};
pub use undo::{EffectKind, EffectRecordedEvent};
pub use wizard::{
    AddBranch, BRANCH_CODE_MAX, BRANCH_CODE_MIN, BranchCodeSet, BranchHub, COUNTRIES, CafeDetails,
    ChooseTier, Country, CountryRow, GoLiveBlocker, GoLiveItem, GoLiveItemView, GoLiveView,
    KEY_BRANCH_CODE, KEY_BRANCH_CODE_LOCKED_AT, KEY_BRANCH_NAME, OwnerBranch, SetBranchCode,
    StepState, TierChoices, TierOption, TierUnavailable, UpdateBranch, WizardBranch, WizardState,
    WizardStep, WizardStepInfo, WizardTenant, country, current_step, valid_branch_code,
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
