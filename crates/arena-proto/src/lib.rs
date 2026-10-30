//! `arena-proto`: protocol types shared by the Hub, the Kiosk and every UI surface.
//!
//! TypeScript bindings are generated from these types with ts-rs (`pnpm gen:types`) into
//! `packages/ui/src/generated`.

mod account;
mod actor;
mod ad_metrics;
mod admin;
mod alert;
mod attract;
mod audit;
mod audit_build;
mod audit_query;
mod backup_export;
mod billing;
mod booster;
mod catalog;
mod commands;
mod connectivity;
mod coupon;
pub mod currency;
mod device_spec;
mod discount;
mod discovery;
mod display;
mod entitlement;
mod error;
mod fence;
mod frame;
mod game_catalog;
mod gst;
mod import;
mod intent;
mod kek;
mod kiosk;
mod kiosk_pairing;
mod kitchen;
mod library;
pub mod licence;
pub mod live;
mod matching;
mod notify;
mod operating;
mod owner_alert;
mod owner_commands;
mod owner_export;
mod owner_read;
pub mod pairing;
mod pc_command;
mod pc_settings;
mod permission;
mod plans;
mod player_login;
mod player_session;
mod player_signup;
mod players;
mod pos;
mod presign;
mod pricing;
mod printer;
mod process;
mod receipt;
mod refund;
mod report;
mod report_export;
mod restore;
mod restore_plan;
mod rollup;
mod scan;
mod scope;
mod session;
mod setup_devices;
mod setup_file;
mod shift;
mod shop;
mod signin;
mod signup;
mod snapshot;
mod staff_invite;
mod staff_list;
mod stock;
mod stock_report;
mod summary;
mod tax;
mod test_session;
pub mod tier;
mod tunnel;
mod ui_auth;
mod undo;
mod wizard;

pub use account::{
    BACKUP_CODE_CHARS, BACKUP_CODE_COUNT, BackupCodes, PASSWORD_MAX_CHARS, PASSWORD_MIN_CHARS,
    SetPassword, TOTP_DIGITS, TOTP_PERIOD_S, TotpCode, TotpStarted,
};
pub use actor::{Actor, ActorKind, ElevatedBy, ElevationKind, ElevationMode};
pub use ad_metrics::{
    AD_BATCH_ROWS, AdCounts, AdDayCount, AdEventKind, AdImpressionRecorded, AdRequestRecorded,
    AdSlot, UiAdEvent,
};
pub use admin::{
    ADMIN_EXIT_FAILED_CODE, ADMIN_EXIT_MS, AdminExitMode, AdminPinOk, AdminUnlockAck,
    AdminUnlockAckEvent, DeviceAdminExit, DeviceAdminRelock, DeviceAdminUnlock,
    FLAG_STALE_VERIFIER, MAX_VERIFIER_ENTRIES, ManagerVerifier, VerifierEntry, VerifyAdminPin,
    validate_exit_id,
};
pub use alert::{AlertAck, AlertKind, AlertRaise, SnapshotAlert};
pub use attract::{
    AttractMedia, AttractState, AttractView, DEFAULT_KIOSK_LOGO, KEY_KIOSK_LOGO, LOGO_PNG_PREFIX,
    LOGO_SVG_PREFIX, MAX_ATTRACT_MEDIA, MAX_LOGO_BYTES, MAX_LOGO_PX, MAX_LOGO_VALUE_CHARS,
    logo_problem,
};
pub use audit::{
    AUDIT_DEFAULT_LIMIT, AUDIT_MAX_LIMIT, AuditActor, AuditActorKind, AuditAdminExit, AuditDetail,
    AuditElevatedBy, AuditElevationKind, AuditElevationMode, AuditFacets, AuditImage, AuditItem,
    AuditPage, AuditStaffOption, AuditValue, ELEVATION_HEADER,
};
pub use audit_build::{
    AUDIT_BUILD_STALE_MS, AUDIT_BUILDS_PER_HOUR, AUDIT_CACHE_TTL_MS, AuditBuildRequest,
    AuditBuildState, AuditBuildStatus, audit_month_start, is_audit_month,
};
pub use audit_query::{
    AUDIT_LOCAL_CACHE_BYTES, AUDIT_QUERY_PAGE, AuditQueryKind, AuditSearch, AuditSearchPage,
};
pub use backup_export::{
    AUTO_EXPORT_ACTOR, AUTO_EXPORT_KEEP, BackupExport, DEFAULT_BACKUP_LAST_EXPORT,
    EXPORT_EXTENSION, EXPORT_FORMAT_REFERENCE_URL, EXPORT_FORMAT_VERSION, EXPORT_HISTORY,
    EXPORT_MAGIC, EverythingExported, ExportEntry, ExportEverythingRequest, ExportFormatError,
    ExportHeader, KEY_BACKUP_LAST_EXPORT, MAX_EXPORT_FILE_NAME_CHARS, MAX_EXPORT_HEADER_BYTES,
    auto_export_file_name, everything_folder_name, export_file_name, is_auto_export_file,
    is_auto_export_name,
};
pub use billing::{
    BILLING_GRACE_RANGE, BILLING_LOW_BALANCE_MAX_MINOR, BILLING_MINIMUM_RANGE,
    BILLING_ROUNDING_RANGE, BILLING_WARN_MARK_RANGE, BILLING_WARN_MARKS_MAX, BillingRules,
    DEFAULT_BILLING_CREDIT_AUTO_PAUSE, DEFAULT_BILLING_GRACE_MIN, DEFAULT_BILLING_LOW_BALANCE,
    DEFAULT_BILLING_MINIMUM_MIN, DEFAULT_BILLING_ROUNDING_MIN, DEFAULT_BILLING_TIME_UP_MODE,
    DEFAULT_BILLING_WARN_MARKS, KEY_BILLING_CREDIT_AUTO_PAUSE, KEY_BILLING_GRACE_MIN,
    KEY_BILLING_LOW_BALANCE, KEY_BILLING_MINIMUM_MIN, KEY_BILLING_ROUNDING_MIN,
    KEY_BILLING_TIME_UP_MODE, KEY_BILLING_WARN_MARKS, KIOSK_BILLING_KEYS, TIME_UP_MODES,
    TimeUpMode, WarnMarksProblem, low_balance_from, parse_low_balance, parse_warn_marks,
    warn_marks_from,
};
pub use booster::{
    BOOSTER_PRIORITIES, BOOSTER_PROGRAM_NAME_MAX, BOOSTER_PROGRAMS_MAX, Booster, BoosterPriority,
    BoosterProgramsProblem, DEFAULT_BOOSTER_CLOSE, DEFAULT_BOOSTER_POWER_PLAN,
    DEFAULT_BOOSTER_PRIORITY, DeviceLaunchTimes, GameLaunchMedian, KEY_BOOSTER_CLOSE,
    KEY_BOOSTER_POWER_PLAN, KEY_BOOSTER_PRIORITY, KIOSK_BOOSTER_KEYS, LAUNCH_MEDIAN_DAYS,
    LAUNCH_WINDOW_TIMEOUT_MS, LastLaunch, LaunchTimesView, median_ms, parse_booster_programs,
};
pub use catalog::{
    BundleComponent, CatalogEntry, ComponentView, IMAGE_PREFIX, MAX_BARCODE_CHARS,
    MAX_BUNDLE_COMPONENTS, MAX_CATEGORY_CHARS, MAX_COMPONENT_QTY, MAX_IMAGE_BYTES, MAX_IMAGE_PX,
    MAX_PRICE_MINOR, MAX_PRODUCT_NAME_CHARS, Product, ProductDelete, ProductUpsert,
    REASON_COMPONENT_INACTIVE, REASON_EXPIRED, availability, bundle_quantity, hsn_valid,
    image_problem,
};
pub use commands::{
    COMMAND_EXPIRY_MS, COMMAND_FETCH_TIMEOUT_MS, COMMAND_PAYLOAD_MAX, COMMAND_SCHEMA_VERSION,
    COMMANDS_PAGE, CommandResult, CommandResultAck, CommandStatus, CommandWire, CommandsPage,
    PRODUCT_COMMAND_PAYLOAD_MAX, SETTINGS_COMMAND_KEYS, SETUP_COMMAND_KINDS, command_payload_max,
    is_setup_command,
};
pub use connectivity::{Connectivity, ConnectivityState};
pub use coupon::{
    COUPON_CODE_MAX_CHARS, COUPON_CODE_MIN_CHARS, COUPON_MAX_PERCENT_BP, COUPON_MAX_TARGETS,
    COUPON_TARGET_KINDS, CouponPreview, CouponPreviewRequest, CouponTarget, CouponUpsert,
    CouponView, coupon_code_normal, coupon_code_valid,
};
pub use device_spec::{
    DeviceSpec, MIN_FREE_BYTES, MIN_FREE_GB, MIN_RAM_BYTES, MIN_RAM_GB, SpecFailure,
    SpecRequirement, check_device, meminfo_total_bytes,
};
pub use discount::{
    DISCOUNT_MAX_PERCENT_BP, DISCOUNT_NOTE_MAX_CHARS, DiscountPreview, DiscountPreviewRequest,
    DiscountReason, ManualDiscount,
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
pub use fence::{
    FenceClaimReason, FenceClaimRequest, FenceClaimResponse, FenceFork, epoch_parts, fenced_epoch,
};
pub use frame::{
    AppliedSummary, ChangeOpKind, ChangeSummary, Envelope, ErrorFrame, EventFrame, FRAME_KINDS,
    Frame, FrameBody, FrameError, Hello, HelloOk, HubEvent, IntentResult, IntentSubmit,
    MAX_RID_LEN, MAX_SETTINGS_GET_KEYS, PROTOCOL_VERSION, SettingsGet, SettingsValues, Versions,
    check_envelope, parse_frame, parse_frame_value, parse_json,
};
pub use game_catalog::{
    CATALOG_MANIFEST_PATH, CATALOG_MANIFEST_PER_MIN, CatalogGame, CatalogManifest, CatalogMedia,
    CatalogSignature,
};
pub use gst::{
    GST_B2B, GST_B2CS, GST_CDNR, GST_HSN, GstExportRequest, GstExported, GstMonth, GstSection,
    gst_file_name,
};
pub use import::{
    IMPORT_CHUNK_ROWS, IMPORT_CREDIT_REMINDER_DAYS, IMPORT_MAX_BYTES, IMPORT_MAX_FAILURES,
    IMPORT_MAX_ROWS, IMPORT_NO_PASSWORD, IMPORT_PLAN_COLUMNS, IMPORT_PLAYER_COLUMNS,
    IMPORT_PRODUCT_COLUMNS, ImportCommitRequest, ImportFailure, ImportPlanRow, ImportPlans,
    ImportPlayerRow, ImportPlayers, ImportPreviewRequest, ImportProductRow, ImportProducts,
    ImportTemplateRequest, ImportTemplateSaved, PlanImportPreview, PlanImportResult,
    PlayerImportPreview, PlayerImportResult, ProductImportCommitRequest, ProductImportPreview,
    ProductImportPreviewRequest, ProductImportResult, StarterCommitRequest, StarterItem,
    StarterPlanInfo, StarterPreview, StarterPreviewRequest, StarterProductInfo, StarterResult,
    StarterTemplateInfo,
};
pub use intent::{
    AuditReadGranted, BootstrapOnboarding, BranchBootstrap, BranchSeedDefaults, DeviceAssign,
    DeviceMaintenance, DevicePlace, DeviceRegister, DeviceRevoke, DeviceVersionReported,
    FLOOR_COLS, FLOOR_ROWS, GROUP_COLOURS, GROUP_KINDS, GroupDelete, GroupUpsert, Intent,
    IntentCategory, IntentKind, IntentPayload, MAINTENANCE_NOTE_MAX, PermEffect, RATE_MAX_MINOR,
    RateUpsert, ReasonCode, RolePerm, RoleUpsert, SettingsSet, StaffDeactivate,
    StaffSetCredentials, StaffUpsert, ZoneDelete, ZoneUpsert, reasons_json,
};
#[cfg(any(test, feature = "dev-keys"))]
pub use kek::DEV_KEK_ESCROW_RECIPIENTS;
pub use kek::{
    AGE_MAGIC, KEK_ESCROW_MAX_BYTES, KEK_ESCROW_RECIPIENTS, KEY_REQUEST_EXPIRY_MS,
    KEY_REQUEST_PC_NAME_MAX, KEY_REQUEST_PUBLIC_KEY_LEN, KEYS_OBJECT_KEY, KekEscrowRequest,
    KekEscrowResponse, KekFetchRequest, KekFetchResponse, KekRequest, KekRequestResponse,
    KeyRequestState, PRESIGN_KIND_KEYS, PendingKeyRequest, sanitise_pc_name, valid_age_recipient,
};
pub use kiosk::{
    GameMediaRef, KioskGameLaunched, KioskGamePlayRecorded, KioskHubState, KioskHubStatus,
    KioskLauncherOpened, KioskLog, KioskLogLevel, LastPlayed, MAX_GAME_ID_LEN, MAX_GAME_MEDIA,
    MAX_KIOSK_LOG_CODE_LEN, MAX_KIOSK_LOG_DETAIL_CHARS, MAX_PLAY_MINUTES, MOST_PLAYED_DAYS,
    MOST_PLAYED_LIMIT, MostPlayed, MostPlayedGame, OPENABLE_LAUNCHERS,
};
pub use kiosk_pairing::{
    DeviceKind, KioskPairInfo, KioskPaired, KioskPairingCode, MAX_PC_NAME_CHARS, PAIR_CODE_DIGITS,
    PAIR_CODE_TTL_MS, PAIR_PATH, PAIR_PIN_MAX_FAILURES, PAIR_PIN_PAUSE_S, PAIR_PIN_WINDOW_MS,
    PAIR_SYSTEM_ACTOR, PairRequest, PairResponse, PairSecret, is_pair_code, is_pc_name,
    sanitize_pc_name,
};
pub use kitchen::{
    BOARD_CANCELLED_KEEP_MS, BOARD_SERVED_KEEP_MS, DEFAULT_KITCHEN_KOT_MODE,
    DEFAULT_KITCHEN_KOT_PAPER_MM, DEFAULT_KITCHEN_KOT_PRINTER, DEFAULT_LICENCE_FLAGS, FLAG_KITCHEN,
    KDS_CODE_WRONG, KDS_DEFAULT_NAME, KDS_LIMIT_MESSAGE, KDS_MAX_DISPLAYS, KDS_PAGE_PATH,
    KDS_PAIR_PATH, KDS_SEEN_INTERVAL_MS, KDS_STATUS_PATH, KEY_DEVICE_CAP, KEY_KITCHEN_KOT_MODE,
    KEY_KITCHEN_KOT_PAPER_MM, KEY_KITCHEN_KOT_PRINTER, KEY_LICENCE_CLOCK, KEY_LICENCE_FLAGS,
    KEY_LICENCE_STATUS, KEY_LICENCE_TIER, KEY_USER_CAP, KITCHEN_GATED_KEYS, KITCHEN_KOT_MODES,
    KITCHEN_STAGES, KITCHEN_UNDO_WINDOW_MS, KdsPair, KdsPairRequest, KdsPairResponse,
    KdsPairingCode, KdsRevoke, KdsSeen, KdsStatus, KitchenAdvance, KitchenAdvanceReverted,
    KitchenBoard, KitchenBoardTicket, KitchenDisplayView, KitchenTicketLineView, KitchenTicketView,
    KotPrintFailed, KotPrinted, KotReprint, MAX_KITCHEN_PRESETS, MAX_LINE_NOTE_CHARS,
    MAX_LINE_NOTES, MAX_PRESET_NOTE_CHARS, STAGE_CANCELLED, STAGE_NEW, STAGE_PREPARING,
    STAGE_READY, STAGE_SERVED, TierGate, has_flag, is_display_name, kitchen_gate, next_stage,
    previous_stage,
};
pub use library::{
    DEFAULT_KIOSK_LIBRARY, KEY_KIOSK_LIBRARY, KioskInstall, KioskLibraryEntry,
    LIBRARY_AGE_RATING_MAX, LIBRARY_ARGS_MAX, LIBRARY_CATALOG_ID_MAX, LIBRARY_CATEGORY_MAX,
    LIBRARY_KINDS, LIBRARY_LAUNCHERS, LIBRARY_NAME_MAX, LIBRARY_PATH_MAX, LIBRARY_TAG_MAX,
    LIBRARY_TAGS_MAX, LIBRARY_URI_MAX, LibraryDelete, LibraryEntryView, LibraryInstall,
    LibraryUpsert, LibraryView, MAX_LIBRARY_ENTRIES, MAX_LIBRARY_INSTALLS, MAX_LIBRARY_VALUE_CHARS,
    is_launch_uri, is_library_path, launcher_from_catalogue, launcher_install_globs, library_rules,
    library_rules_with_installs, parse_kiosk_library, tags_from_json, tags_to_json,
};
pub use licence::{LicenceClaims, LicenceError, VerifyKey};
pub use live::{
    LIVE_PATH, LIVE_TICK_MS, LIVE_TOKEN_AUD, LIVE_TOKEN_TTL_S, LiveClaims, LiveFloor, LiveGrant,
    LiveTile, LiveTileSession, LiveTileState, LiveToken, LiveTokenError, LiveZone,
    verify_live_token,
};
pub use matching::{
    GameMatch, GameMatched, LibraryDismiss, LibraryDismissClear, MAX_DISMISS_KEYS, MAX_DISMISSED,
    MAX_MATCHES, MAX_PROPOSAL_KEY_CHARS, ProposalDevice, ProposalView, ProposalsView,
    is_proposal_key, proposal_key,
};
pub use notify::{
    CREDIT_REMINDER_INTERVAL_S, CREDIT_REMINDER_MINUTE, MESSAGE_KINDS, NOTIFY_DRAIN_INTERVAL_S,
    NOTIFY_EXPIRY_MS, NOTIFY_KIND_CONSOLE_5MIN, NOTIFY_KIND_CREDIT_REMINDER,
    NOTIFY_KIND_LOW_BALANCE, NOTIFY_KIND_SESSION_ENDING, NOTIFY_PARAM_MAX_CHARS,
    NOTIFY_PURPOSE_OWNER_ALERT, NOTIFY_STATE_EXPIRED, NOTIFY_STATE_FAILED, NOTIFY_STATE_SENT,
    NOTIFY_TRIGGER_INTERVAL_S, NotifyChannel, NotifyExpired, NotifyFailed, NotifyRearm,
    NotifyRequest, NotifyRequested, NotifyResponse, NotifySent, NotifySkipped, NotifyState,
    SESSION_NEAR_END_MS, SESSION_NOTIFY_KINDS, valid_e164,
};
pub use operating::{
    ADS_POLICY_VERSION, ADS_RATINGS, AdConsentState, AdRating, AdsConsent, AdsConsentSet,
    BACKUP_PROBE_PREFIX, BRANCH_OPENING_MAX_BYTES, BackupDirProblem, BranchOpening, DAYS_PER_WEEK,
    DEFAULT_ADS_CONSENT, DEFAULT_ADS_RATING, DEFAULT_BACKUP_DIR, DEFAULT_FULFILMENT,
    DEFAULT_KIOSK_VOLUME, DEFAULT_MEDIA_CAP_GB, DEFAULT_OPENING_HOURS, DEFAULT_OPENING_HOURS_SET,
    DEFAULT_SETUP_LIVE, FULFILMENT_MODES, KEY_ADS_CONSENT, KEY_ADS_RATING, KEY_BACKUP_DIR,
    KEY_FULFILMENT, KEY_KIOSK_VOLUME_DEFAULT, KEY_MEDIA_CAP_GB, KEY_OPENING_HOURS,
    KEY_OPENING_HOURS_SET, KEY_SETUP_LIVE, KIOSK_OPERATING_KEYS, KIOSK_VOLUME_MAX, LAST_MINUTE,
    MEDIA_CAP_GB_RANGE, MediaCacheStatus, OpeningDay, OpeningHoursError, SetupStatus,
    TestSessionState, has_open_day, is_absolute_path, is_backup_location, is_backup_tree_uri,
    opening_hours_json, opening_window_start, parse_opening_hours,
};
pub use owner_alert::{
    DEFAULT_OWNER_ALERT_THRESHOLDS, DevOwnerAlert, KEY_OWNER_ALERT_THRESHOLDS,
    OWNER_ALERT_MAX_MINOR, OWNER_ALERT_PARAM_MAX_CHARS, OWNER_ALERTS_PAGE, OWNER_ALERTS_PAGE_MAX,
    OwnerAlertKind, OwnerAlertList, OwnerAlertParamError, OwnerAlertPush, OwnerAlertSettings,
    OwnerAlertSettingsPatch, OwnerAlertThresholds, OwnerAlertView, PUSH_SUBSCRIPTIONS_MAX,
    PUSH_TTL_S, PushKeys, PushRemove, PushSubscribe,
};
pub use owner_commands::{
    CreditRemind, DEVICE_REVOKE_REASONS, KeyRelease, KeyReleaseDecision, OWNER_COMMAND_KINDS,
    OWNER_COMMAND_POSTS_PER_MIN, OWNER_COMMANDS_PAGE, OWNER_COMMANDS_PAGE_MAX, OWNER_UPDATES,
    OverwrittenChange, OwnerCommandKind, OwnerCommandList, OwnerCommandPost, OwnerCommandPosted,
    OwnerCommandPreview, OwnerCommandPreviewRequest, OwnerCommandSkip, OwnerCommandTarget,
    OwnerCommandView, OwnerDeviceRevoke, OwnerStaffUpsert, OwnerUpdate, StaffSetupCode,
    UpdatedByOwnerEvent, accepted_owner_kind, owner_command_kind, owner_update_for_table,
    validate_owner_payload,
};
pub use owner_export::{ExportReport, major, owner_csv, utc_minute};
pub use owner_read::{
    BRANCH_ONLINE_WINDOW_MS, DataAgeReport, GroupBy, OWNER_ALL_BRANCHES, OWNER_API_LIMIT_PER_MIN,
    OWNER_RANGE_MAX_DAYS, OWNER_READ_CACHE_MS, OwnerLive, OwnerLiveAll, OwnerLiveTotal, OwnerRange,
    OwnerRangeAll, OwnerRangeCredit, OwnerRangeCurrency, OwnerRangeExpiring, OwnerRangeRow,
    OwnerRangeShift, OwnerRangeStock, OwnerRangeTotals, OwnerRangeWaste, date_from_days,
    dates_between, days_from_date,
};
pub use pairing::{
    ActivePairingCode, BOOTSTRAP_DAY_START_MIN, BootstrapOwner, BootstrapProfile,
    BranchBootstrapCommand, HEARTBEAT_BOOT_INTERVAL_S, HEARTBEAT_LIVE_INTERVAL_S,
    HEARTBEAT_MIN_GAP_MS, HUB_TOKEN_PREFIX, HeartbeatFence, HeartbeatRequest, HeartbeatResponse,
    HubPairRequest, HubPairResponse, HubPlatform, InstallHubState, IssuePairingCode, OWNER_ROLE,
    PAIR_ATTEMPTS_PER_IP_PER_HOUR, PAIR_FAILURES_PER_CODE, PAIRING_CODE_ALPHABET, PAIRING_CODE_LEN,
    PAIRING_CODE_TTL_MS, PairHubOk, PairHubRequest, PairingCodeIssued, ResendUnavailable,
    SETUP_CODE_HUB_TTL_MS, SETUP_CODE_PLAINTEXT_TTL_MS, SeqRange, SetupCodeReveal, check_bootstrap,
    format_pairing_code, normalize_pairing_code, valid_pairing_code,
};
pub use pc_command::{
    DeviceMessage, DevicePower, DeviceScreen, LockReason, MessageScope, PC_MESSAGE_MAX, PcAck,
    PcAckEvent, PcAction, valid_message_text,
};
pub use pc_settings::{
    DEFAULT_DISPLAY_MODES, DISPLAY_MODES_MAX, DISPLAY_REVERT_SECS, DisplayMode,
    DisplayModesProblem, KEY_DISPLAY_MODES, MOUSE_SPEED_DEFAULT, MOUSE_SPEED_MAX, MOUSE_SPEED_MIN,
    PC_SETTING_TEXT_MAX, PcLayout, PcSettings, PcSettingsChanged, PcSettingsPanel,
    PlayerPcSettingsResult, PlayerPcSettingsSet, PlayerPcSettingsSetResult, parse_display_mode,
    parse_display_modes, valid_keyboard_layout, valid_mouse_speed,
};
pub use permission::{Permission, Requirement};
pub use plans::{
    MINUTES_PER_DAY, OwnedPlanView, PLAN_KINDS, PLAN_MAX_GROUPS, PLAN_MAX_MINUTES,
    PLAN_MAX_VALIDITY_DAYS, PlanSell, PlanSellReverted, PlanUpsert, PlanView,
};
pub use player_login::{
    LOGIN_LOCK_SECONDS, LOGIN_MAX_FAILURES, MAX_LOGIN_PASSWORD_BYTES, MAX_LOGIN_USERNAME_BYTES,
    PLAYER_LOGIN_CODE, PlayerAuthResult, PlayerAuthenticate, PlayerContext,
};
pub use player_session::{
    ACCOUNT_ORDER_LIMIT, AccountOrder, AccountOrderItem, AccountPack, AccountPackEntry,
    ExtendPackSource, ExtendQuote, MAX_PLAN_OPTIONS, PLAYER_LOGIN_RECORD_SECONDS,
    PLAYER_LOGIN_REQUIRED, PlanOption, PlanTag, PlayerAccountPage, PlayerAccountResult,
    PlayerChangePassword, PlayerChoice, PlayerEndResult, PlayerExtend, PlayerExtendOptions,
    PlayerExtendResult, PlayerPasswordChangeResult, PlayerPlansResult, PlayerStartResult,
    PlayerStartSession, PlayerUnlock, PlayerUnlockResult, PlayerWallet,
};
pub use player_signup::{
    MAX_SIGNUP_NAME_BYTES, MAX_SIGNUP_PASSWORD_BYTES, MAX_SIGNUP_PHONE_BYTES,
    MAX_SIGNUP_USERNAME_BYTES, PLAYER_POLICY_VERSION, PlayerSignup, PlayerSignupResult,
};
pub use players::{
    CONSENT_TRANSACTIONAL, ERASE_NOTE_MAX_CHARS, PLAYER_EMAIL_MAX_CHARS, PLAYER_NAME_MAX_CHARS,
    PLAYER_NOTES_MAX_CHARS, PLAYER_PASSWORD_MAX_CHARS, PLAYER_PASSWORD_MIN_CHARS,
    PLAYER_PHONE_MAX_DIGITS, PLAYER_PHONE_MIN_DIGITS, PLAYER_TAG_MAX_CHARS, PLAYER_TAGS_MAX,
    PLAYER_USERNAME_MAX_CHARS, PLAYER_USERNAME_MIN_CHARS, POLICY_VERSION_MAX_CHARS, PlayerConsent,
    PlayerCreate, PlayerEditForm, PlayerErase, PlayerForm, PlayerHit, PlayerPasswordChange,
    PlayerPasswordForm, PlayerPasswordReset, PlayerProfile, PlayerUpdate, UiPasswordResetRequest,
    UiPlayerEditRequest, UiPlayerEraseRequest, UiPlayerRequest, WalletDisposal, normalize_phone,
    normalize_username, valid_phone, valid_policy_version, valid_tags, valid_username,
};
pub use pos::{
    CounterOrder, InvoiceIssue, KioskSale, MAX_SALE_LINES, MAX_SALE_QTY, MOVE_COUNT_ADJUST,
    MOVE_RETURN, MOVE_SALE, MOVE_SALE_REVERTED, NOTE_SALE_OVERRIDE, OrderLineCancel,
    OrderLineEvent, OrderLineServed, OrderPlace, OrderPlaceReverted, OrderPlacedEvent, OrderReady,
    OrderVoid, OrderVoidEvent, PlaceOutcome, PosSale, PosSaleLine, PosSaleLineView,
    PosSalePaymentView, PosSaleReverted, PosSaleView, PosSalesView, VOID_NOTE_MAX_CHARS,
};
pub use presign::{
    PRESIGN_KIND_ROLLUP, PRESIGN_KIND_SNAPSHOT, PRESIGN_TTL_S, PresignBatch, PresignRequest,
    PresignResponse,
};
pub use pricing::{
    RULE_ADJUST_KINDS, RULE_ALL_DAYS, RULE_KINDS, RULE_MAX_DATE_SPAN_DAYS, RULE_MAX_PERCENT_BP,
    RULE_MAX_PRIORITY, RULE_MIN_PERCENT_BP, RuleConflict, RuleUpsert, RuleView,
};
pub use printer::{
    BluetoothAccess, DEFAULT_PRINTER_PAPER_MM, DEFAULT_PRINTER_PORT, DEFAULT_PRINTER_SELECTED,
    DEFAULT_PRINTER_STATUS, FoundPrinter, KEY_PRINTER_PAPER_MM, KEY_PRINTER_SELECTED,
    KEY_PRINTER_STATUS, MAX_PRINTER_ERROR_CHARS, MAX_PRINTER_NAME_CHARS, PRINTER_IP_FIELD,
    PRINTER_OFFLINE_HOLD_MS, PRINTER_PAPER_WIDTHS_MM, PrinterErrorKind, PrinterFindResult,
    PrinterHealth, PrinterJobFailure, PrinterKind, PrinterOutcome, PrinterReady, PrinterSelected,
    PrinterSelectedError, PrinterState, PrinterStatus, PrinterStatusReply, PrinterStatusSet,
    PrinterUnready, parse_bluetooth_address, parse_printer_addr, printer_error_text,
    valid_printer_error, valid_printer_outcome,
};
pub use process::{
    ARENA_SIGNER_CN, BACKFILL_PROCESS_MODE, BOOTSTRAP_PROCESS_MODE, DEFAULT_PROCESS_ALLOW,
    DEFAULT_PROCESS_BLOCK, INSTALL_DIR_TOKEN, KEY_PROCESS_ALLOW, KEY_PROCESS_BLOCK,
    KEY_PROCESS_MODE, KioskCleanupFailed, KioskProcessBlocked, MAX_CLEANUP_DETAIL_CHARS,
    MAX_CLEANUP_STEP_LEN, MAX_EXE_NAME_CHARS, MAX_PATH_GLOB_CHARS, MAX_PROCESS_PATH_CHARS,
    MAX_PROCESS_RULES, MAX_PROCESS_RULES_CHARS, MAX_SIGNER_CHARS, PATH_ELLIPSIS, PATH_TOKENS,
    ProcessBlockReason, ProcessList, ProcessMode, ProcessRule, SYSTEM_DIR_TOKEN, is_catch_all_glob,
    is_file_name_glob, is_full_path_glob, is_user_writable_glob, parse_process_mode,
    parse_process_rules, truncate_path_left,
};
pub use receipt::{
    DEFAULT_NOTIFY, DEFAULT_RECEIPT_ADDRESS, DEFAULT_RECEIPT_DEFAULT, DEFAULT_RECEIPT_DRAWER,
    DEFAULT_RECEIPT_PHONE, DrawerKickFailed, DrawerKicked, KEY_NOTIFY_CREDIT_REMINDER,
    KEY_NOTIFY_LOW_BALANCE, KEY_NOTIFY_SESSION_ENDING, KEY_NOTIFY_WHATSAPP_RECEIPT,
    KEY_RECEIPT_ADDRESS, KEY_RECEIPT_DEFAULT, KEY_RECEIPT_DRAWER, KEY_RECEIPT_FOOTER,
    KEY_RECEIPT_PHONE, NOTIFY_KEYS, NOTIFY_KIND_RECEIPT, NOTIFY_PURPOSE_TRANSACTIONAL,
    NOTIFY_STATE_QUEUED, NOTIFY_STATE_SKIPPED, RECEIPT_ADDRESS_MAX_LINES, RECEIPT_DEFAULT_CHOICES,
    RECEIPT_ERROR_MAX_CHARS, RECEIPT_FOOTER_MAX_LINES, RECEIPT_LINE_MAX_CHARS,
    RECEIPT_PHONE_MAX_LINES, ReceiptChoice, ReceiptPrintFailed, ReceiptPrinted, ReceiptReprint,
    WhatsappRefusal, ZReportReprint, notify_gate, tier_is_paid, valid_receipt_lines,
    whatsapp_refusal,
};
pub use refund::{
    MAX_REFUND_LINES, REFUND_NOTE_MAX_CHARS, RefundCreate, RefundInvoice, RefundLineReq,
    RefundLineView, RefundSummary,
};
pub use report::{
    CREDIT_BUCKET_0_7, CREDIT_BUCKET_8_14, CREDIT_BUCKET_15_PLUS, CreditBucketRow, CreditDebtor,
    DailySales, DeviceSessions, GroupSessions, GuestTotals, HourSales, NamedSales, NewReturning,
    OverdueRow, OverdueView, PLAYERS_DEBTOR_LIMIT, PLAYERS_TOP_LIMIT, PlayerSpend, PlayersReport,
    RECEIVED_CREDIT_COLLECT, RECEIVED_TOPUP, REPORT_MAX_DAYS, ReportAmount, SALES_DISCOUNTS,
    SALES_PLANS, SALES_PLAY_TIME, SALES_PRODUCTS, SALES_TAX_ADDED, SHIFT_LIST_LIMIT,
    SessionsReport, ShiftList, ShiftListRow, ShiftVariance, StaffReport, StaffRow, ZCashLine,
    ZReference, ZReferenceGroup, ZRefund, ZReport, ZVoid, ZWaste,
};
pub use report_export::{EXPORT_REPORTS, ExportFormat, ReportExportRequest, ReportExported};
pub use restore::{
    DEFAULT_BACKUP_LAST_RESTORE, EpochChangedEvent, KEY_BACKUP_LAST_RESTORE, ORPHAN_KEEP_DAYS,
    OpsRestore, RESTORE_SYSTEM_ACTOR, RestoreCheck, RestoreCheckRequest, RestoreRequest,
    RestoreStaged, restore_time_text, restore_warning,
};
pub use restore_plan::{
    RestoreFetchPhase, RestoreFetchView, RestoreGapView, RestorePlan, RestorePlanBatch,
    RestorePlanRequest, RestorePlanSnapshot, RestorePosition, RestoreReadyView,
};
pub use rollup::{
    ROLLUP_MIN_SCHEMA, ROLLUP_PLAYERS_CAP, ROLLUP_SCHEMA, RollupAged, RollupAmount, RollupByMethod,
    RollupByType, RollupCredit, RollupCreditPlayer, RollupExpiring, RollupPlayHours, RollupPublish,
    RollupRevenue, RollupSessions, RollupShift, RollupStock, RollupStockLevel, RollupV1,
    RollupWaste, emitted_rollup_schema, is_rollup_date, rollup_object_key, rollup_schema_accepted,
    snapshot_object_key,
};
pub use scan::{
    DeviceScanView, KEY_BRANCH_TZ, KioskScanReported, LibraryScansView, MAX_SCAN_ENTRIES,
    MAX_SCAN_FILE_NAME_CHARS, MAX_SCAN_PATH_CHARS, MAX_SCAN_TEXT_CHARS, SCAN_ENTRY_LAUNCHERS,
    SCAN_LAUNCHERS, ScanEntry, ScanLauncher,
};
pub use scope::{ClientClass, PAIRED_SETTINGS_PREFIX, class_scope, in_scope, may_read_setting};
pub use session::{
    BillItem, BillLine, BillRevertedEvent, BillSettledEvent, CARD_REFERENCE_MAX, CREDIT_DAY_MAX,
    CREDIT_DAYS_MAX, CREDIT_LIMIT_MAX_MINOR, CREDIT_OVERDUE_DAYS, ClearedSpend, ControllerCheck,
    ControllerResult, CreditAging, CreditAutoPause, CreditCollect, CreditCollectReverted,
    CreditCollection, CreditSetLimit, CreditView, DEFAULT_REMINDER_DAYS, EXTEND_OPTIONS_MIN,
    EndOutcome, ExtendSource, GroupPayer, OrderStatus, PAUSE_NOTE_MAX, PauseReason, PaymentMethod,
    PaymentRow, SESSION_WARNING_MARKS_MIN, SessionAssignPlayer, SessionEnd, SessionEndReverted,
    SessionExpire, SessionExtend, SessionForceEnd, SessionForceEndReverted, SessionGroup,
    SessionListRow, SessionListState, SessionListView, SessionMode, SessionPause, SessionRateFlip,
    SessionResume, SessionScreen, SessionSettle, SessionSettleReverted, SessionStart,
    SessionStartedEvent, SessionSync, SessionTransfer, SettleOutcome, SnapshotBill,
    SnapshotGroupBill, SnapshotSession, UNDO_WINDOW_MS, UiSessionConvertRequest,
    WALLET_TOPUP_MAX_MINOR, WalletChangedEvent, WalletTopup, WalletTopupReverted, WalletTxnView,
    WalletView,
};
pub use setup_devices::{
    BranchSetup, SETUP_ITEMS_MAX, SetupCommandRequest, SetupCommandView, SetupDevice,
    SetupDevicesView, SetupGroup,
};
pub use setup_file::{
    ImportSetup, SETUP_FILE_NAME, SETUP_FORMAT, SETUP_LINK_TTL_MS, SETUP_MAX_BYTES,
    SETUP_MAX_ENTRIES, SETUP_TEMPLATE_PATH, SETUP_UPDATE_HUB, SETUP_VERSION, SetSetupStart,
    SetupBundle, SetupCommitRequest, SetupComponent, SetupCoupon, SetupCouponTarget, SetupFile,
    SetupFileGroup, SetupFileResult, SetupItem, SetupKindCount, SetupPlan, SetupPreview,
    SetupPreviewRequest, SetupProduct, SetupRule, SetupStartSource, SetupStartView,
    SetupTemplateLink, SetupTemplateOutcome, SetupTemplateStored, TemplateStatus,
};
pub use shift::{
    CASH_DEPOSIT, CASH_DRAWER_OPEN, CASH_EXPENSE, CASH_FLOAT_IN, CASH_REFUND, CASH_SALE,
    CASH_SALE_REVERTED, CashDeposit, CloseDeposit, DEFAULT_SHIFT_VARIANCE_THRESHOLD,
    DEPOSIT_REFERENCE_MAX_CHARS, EXPENSE_NOTE_MAX_CHARS, ExpenseCategory, ExpenseCreate,
    ExpensePaidFrom, KEY_SHIFT_VARIANCE_THRESHOLD, MAX_CASH_MOVE_MINOR, MAX_FLOAT_MINOR,
    ShiftBreakdown, ShiftClose, ShiftCount, ShiftOpen, ShiftState, SnapshotShift,
    VARIANCE_NOTE_MAX_CHARS, VarianceReason, receipt_photo_problem,
};
pub use shop::{
    FLAG_KIOSK_ORDERS, MAX_CART_LINES, MAX_CART_QTY, MAX_SHOP_ID_LEN, POPULAR_DAYS, POPULAR_LIMIT,
    ShopComponent, ShopImageRequest, ShopImageResult, ShopItem, ShopMenu, ShopPay, ShopPayOptions,
    ShopPlace, ShopPlaceResult, kiosk_order_gate,
};
pub use signin::{
    BACKUP_CODES_LOW, DEVICE_LABEL_MAX_CHARS, SIGNIN_CHALLENGE_TTL_S, SIGNIN_LOCK_S,
    SIGNIN_MAX_ATTEMPTS, SIGNIN_MAX_FAILURES, STEPUP_TTL_MS, Signin, SigninMethod, SigninNext,
    SigninReply, SigninSms, SigninSmsSent, SigninVerified, SigninVerify, SmsFallback,
    StepupRequest, StepupSms, StepupSmsSent, TRUSTED_DEVICE_TTL_S, TrustedDevice,
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
    DEFAULT_INVENTORY_TAKE_STOCK_AT, DEFAULT_STOCK_EXPIRY_WARN_DAYS, ExpiringLot, ExpiryView,
    KEY_INVENTORY_TAKE_STOCK_AT, KEY_STOCK_EXPIRY_WARN_DAYS, LotView, LowStockItem, LowStockView,
    MAX_STOCK_IN_QTY, MAX_STOCK_QTY, MAX_SUPPLIER_CHARS, MOVE_EXPIRED, MOVE_STOCK_IN,
    MOVE_STOCK_IN_REVERTED, MOVE_WASTE, MOVE_WASTE_REVERTED, STOCK_EXPIRY_WARN_RANGE,
    STOCK_UNDO_MS, StockExpiryCheck, StockIn, StockInReverted, StockThreshold, StockWaste,
    StockWasteReverted, StockWriteOff, TAKE_STOCK_AT_MODES, TAKE_STOCK_AT_ORDER,
    TAKE_STOCK_AT_SERVED, add_days, consume_plan, lot_expired, lot_in_window, valid_date,
    weighted_avg_cost,
};
pub use stock_report::{
    STOCK_MOVES_LIMIT, StockLevel, StockMoveLine, StockMoves, StockReport, WASTE_EXPIRED,
    WASTE_REASONS, WasteProductRow, WasteReasonRow, WasteReport,
};
pub use summary::{
    ATTENTION_ALERT, ATTENTION_CREDIT_OVERDUE, ATTENTION_LOW_STOCK, ATTENTION_SHIFT_VARIANCE,
    BranchSummary, CatalogueDevice, CatalogueRow, CatalogueStaff, SUMMARY_LIST_CAP,
    SUMMARY_LOG_DATE, SUMMARY_OBJECT_KEY, SUMMARY_SCHEMA, SummaryAttention, SummaryCatalogue,
    SummaryPublish, SummaryShift,
};
pub use tax::{
    BUYER_NAME_MAX_CHARS, BusinessBuyer, DEFAULT_TAX, GST_MAX_BP, GSTIN_INVALID, KEY_TAX,
    RATE_RANGE, SIMPLE_MAX_BP, TaxMode, TaxSettings, gstin_valid,
};
pub use test_session::{
    DEFAULT_TEST_SESSION, DeviceTestSessionCompleted, DeviceTestSessionStart, KEY_TEST_SESSION,
    TEST_SESSION_CONFIRM_MS, TEST_SESSION_MS, TestSessionOutcome, TestSessionStatus,
};
pub use tier::{EntitlementSource, TIERS, Tier, TierDefaults, TierLimits};
pub use tunnel::{
    TUNNEL_DOMAIN, TUNNEL_FLAG, TUNNEL_ORIGIN, TUNNEL_PORT, TunnelAnswer, TunnelLine, TunnelReport,
    TunnelState, parse_tunnel_host, tunnel_hostname,
};
pub use ui_auth::{
    ElevateDrop, ElevateOk, ElevateRequest, ElevationItem, MAX_STAFF_PASSWORD_CHARS,
    MAX_TICKET_USES, MIN_STAFF_PASSWORD_CHARS, SessionStaff, SettingEntry, SetupCodeRequest,
    SignInOk, SignInRequest, UiError, UiIntentRequest, UiIntentResult, UiMe, UnlockOk,
    UnlockRequest,
};
pub use undo::{EffectKind, EffectRecordedEvent};
pub use wizard::{
    AddBranch, BRANCH_CODE_MAX, BRANCH_CODE_MIN, BranchCodeSet, BranchHub, BranchSetupProgress,
    COUNTRIES, CafeDetails, ChooseTier, Country, CountryRow, GoLiveBlocker, GoLiveItem,
    GoLiveItemView, GoLiveView, KEY_BRANCH_CODE, KEY_BRANCH_CODE_LOCKED_AT, KEY_BRANCH_NAME,
    OwnerBranch, SetBranchCode, StepState, TierChoices, TierOption, TierUnavailable, UpdateBranch,
    WizardBranch, WizardState, WizardStep, WizardStepInfo, WizardTenant, country, current_step,
    valid_branch_code,
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
