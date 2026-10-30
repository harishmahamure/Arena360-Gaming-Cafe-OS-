//! `arena-tasks`: the Hub's background tasks.

pub mod catalog;
pub mod cloud;
pub mod commands;
pub mod connectivity;
pub mod credit_reminder;
pub mod daily_export;
pub mod disk;
pub mod fence;
pub mod heartbeat;
pub mod housekeeping;
pub mod invoice;
pub mod keys;
pub mod keystore;
pub mod lease;
pub mod mdns;
pub mod notify;
pub mod notify_trigger;
pub mod printer;
pub mod receipts;
pub mod restore_fetch;
pub mod restore_wait;
pub mod session_timer;
pub mod template;
pub mod template_apply;
pub mod test_session;
pub mod tunnel;
pub mod upload;

pub use catalog::{CATALOG_INTERVAL, CatalogPullTask};
pub use cloud::{CloudClient, CloudEndpoint, CloudFailure, CloudPairer, MediaFailure};
pub use commands::{COMMAND_POLL_INTERVAL, CommandTask};
pub use connectivity::{
    CloudHost, ConnectivityTask, DEFAULT_CLOUD_HOST, DEFAULT_INTERNET_ENDPOINTS, NetProber,
    PROBE_INTERVAL, PROBE_TIMEOUT, Prober, decide, default_internet_endpoints,
    parse_internet_endpoints,
};
pub use credit_reminder::{CREDIT_REMINDER_TICK, CreditReminderTask};
pub use daily_export::{
    DAILY_EXPORT_RETRY, DAILY_EXPORT_TICK, DailyExportTask, Pass as DailyExportPass,
};
pub use fence::{Claim, claim_fence};
pub use heartbeat::HeartbeatTask;
pub use housekeeping::{
    DEDUPE_RETENTION_MS, HOUSEKEEPING_INTERVAL, Housekeeping, HousekeepingTask,
};
pub use invoice::{INVOICE_BATCH, INVOICE_RETRY_MS, INVOICE_TICK, InvoiceTask};
pub use keys::{KeyBackupTask, wrap_kek};
pub use keystore::{
    AndroidKeystore, AndroidSecrets, FileKeystore, HUB_TOKEN_FILE, KEK_FILE, KeyLost, Keystore,
    KeystoreKek, RESTORE_KEY_FILE, SecretWrapper, WrapError, WrappedKeystore, is_key_lost,
    key_lost_error, mint_kek, named_keystore, platform_keystore,
};
pub use lease::{LEASE_TTL_MS, LeaseTask, RENEW_INTERVAL, Tick};
pub use mdns::{
    Advert, Advertiser, BranchSource, Interfaces, MDNS_FAILURE_LOG_INTERVAL, MDNS_POLL_INTERVAL,
    MdnsAdvertiser, MdnsTask, NetworkName, SystemInterfaces, lan_ipv4_addresses, mdns_host_name,
};
pub use notify::{IN_DOUBT_RETRY_MS, NOTIFY_TICK, NotifyDrainTask};
pub use notify_trigger::{NOTIFY_TRIGGER_TICK, NotifyTriggerTask};
pub use receipts::{PRINT_BATCH, PRINT_TICK, ReceiptPrintTask};
pub use restore_fetch::{RestoreFetcher, STAGING_DIR as RESTORE_STAGING_DIR};
pub use restore_wait::{KeyRequestTask, restore_public_key};
pub use session_timer::{SessionTimerTask, TIMER_EVENTS_DEPTH, TIMER_TICK};
pub use template::{NOT_ENTITLED_RETRY, SetupTemplateTask, TIER_RECHECK, UPLOAD_DEBOUNCE};
pub use template_apply::{SetupApplyTask, SetupTemplateInbox};
pub use test_session::TestSessionTask;
pub use tunnel::{
    Connector, MAX_RESTART_BACKOFF, ProcessConnector, Running, TUNNEL_CREDENTIAL_FILE,
    TunnelCredential, TunnelInbox, TunnelTask,
};
pub use upload::{
    BACKLOG_TRIGGER_ROWS, BACKUP_NOW_WAIT, BackupRequester, BackupResponder, MIN_UPLOAD_INTERVAL,
    ResendInbox, UploadTask, backup_channel,
};
