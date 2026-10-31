//! The nightly `gz_control` backup (`--role jobs --task control-backup`) and its restore
//! (`ops restore-control`).
//!
//! Backup: dump (`ControlDb::dump`), age-encrypt to the X25519 recipients in
//! `ARENA_BACKUP_AGE_RECIPIENTS` (the job never holds a private key), put
//! `gz_control/nightly/<UTC yyyy-mm-ddTHHMMSSZ>.sql.age`, then delete all but the newest 30
//! backups by key, never the one just written. Any failure is logged and, when
//! `ARENA_ALERT_WEBHOOK_URL` is set, POSTed as an alert naming the environment, the task and the
//! error code.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use arena_store::control::{ControlDb, ControlError, Dump, RestoreReport};

use crate::alert;
use crate::config::{Database, Decryption, JobConfig, RestoreConfig, RestoreSource, Task};
use crate::store::{ObjectStore, S3Config, S3Store, StoreError};

/// The comma-separated age X25519 public keys a backup is encrypted to.
pub const RECIPIENTS_ENV: &str = "ARENA_BACKUP_AGE_RECIPIENTS";
/// Where nightly backups live in the bucket.
pub const NIGHTLY_PREFIX: &str = "gz_control/nightly/";
/// The suffix of every backup object.
pub const BACKUP_SUFFIX: &str = ".sql.age";
/// How many backups retention keeps.
pub const KEEP: usize = 30;
/// The timestamp in a backup key (UTC).
const KEY_TIME_FORMAT: &str = "%Y-%m-%dT%H%M%SZ";
/// A run gives up (and alerts) after this long, inside the CronJob's 1800 s deadline, so a hung
/// run is reported rather than silently killed.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(25 * 60);

/// Stable failure codes (logs, alerts).
pub mod code {
    pub const CONFIG_INVALID: &str = "BACKUP_CONFIG_INVALID";
    pub const DUMP_FAILED: &str = "BACKUP_DUMP_FAILED";
    pub const ENCRYPT_FAILED: &str = "BACKUP_ENCRYPT_FAILED";
    pub const UPLOAD_FAILED: &str = "BACKUP_UPLOAD_FAILED";
    pub const RETENTION_FAILED: &str = "BACKUP_RETENTION_FAILED";
    pub const TIMEOUT: &str = "BACKUP_TIMEOUT";
    pub const SWEEP_FAILED: &str = "COMMAND_SWEEP_FAILED";
    pub const SWEEP_TIMEOUT: &str = "COMMAND_SWEEP_TIMEOUT";
    pub const WATCH_FAILED: &str = "OFFLINE_WATCH_FAILED";
    pub const WATCH_TIMEOUT: &str = "OFFLINE_WATCH_TIMEOUT";
    pub const RETENTION_SWEEP_FAILED: &str = "RETENTION_SWEEP_FAILED";
    pub const RETENTION_SWEEP_TIMEOUT: &str = "RETENTION_SWEEP_TIMEOUT";
    pub const BILLING_RECONCILE_FAILED: &str = "BILLING_RECONCILE_FAILED";
    pub const BILLING_RECONCILE_TIMEOUT: &str = "BILLING_RECONCILE_TIMEOUT";
    pub const INVOICE_RENDER_FAILED: &str = "INVOICE_RENDER_FAILED";
    pub const INVOICE_RENDER_TIMEOUT: &str = "INVOICE_RENDER_TIMEOUT";
    pub const SUBSCRIPTION_SWEEP_FAILED: &str = "SUBSCRIPTION_SWEEP_FAILED";
    pub const SUBSCRIPTION_SWEEP_TIMEOUT: &str = "SUBSCRIPTION_SWEEP_TIMEOUT";
    pub const SUBSCRIPTION_LIFECYCLE_FAILED: &str = "SUBSCRIPTION_LIFECYCLE_FAILED";
    pub const SUBSCRIPTION_LIFECYCLE_TIMEOUT: &str = "SUBSCRIPTION_LIFECYCLE_TIMEOUT";
    pub const RESTORE_CONFIG_INVALID: &str = "RESTORE_CONFIG_INVALID";
    pub const SOURCE_FAILED: &str = "RESTORE_SOURCE_FAILED";
    pub const DECRYPT_FAILED: &str = "RESTORE_DECRYPT_FAILED";
}

/// A failed backup or restore: a stable code and a detail for the log (never a secret).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobError {
    pub code: &'static str,
    pub detail: String,
}

impl JobError {
    pub(crate) fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for JobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl From<ControlError> for JobError {
    fn from(e: ControlError) -> Self {
        let code = e.code();
        let text = e.to_string();
        // Some `ControlError` texts already start with their code; say it once.
        let detail = text
            .strip_prefix(code)
            .and_then(|t| t.strip_prefix(": "))
            .map_or_else(|| text.clone(), str::to_owned);
        Self::new(code, detail)
    }
}

/// The key a backup taken at `now_ms` is stored under.
pub fn object_key(now_ms: i64) -> String {
    let at = chrono::DateTime::from_timestamp_millis(now_ms).unwrap_or_default();
    format!(
        "{NIGHTLY_PREFIX}{}{BACKUP_SUFFIX}",
        at.format(KEY_TIME_FORMAT)
    )
}

/// Is `key` a nightly backup written by [`object_key`]? Retention never touches anything else.
pub fn is_backup_key(key: &str) -> bool {
    key.strip_prefix(NIGHTLY_PREFIX)
        .and_then(|k| k.strip_suffix(BACKUP_SUFFIX))
        .is_some_and(|ts| chrono::NaiveDateTime::parse_from_str(ts, KEY_TIME_FORMAT).is_ok())
}

/// Parses comma-separated age X25519 public keys (`age1…`). Errors say which entry is wrong,
/// never its text (a private key pasted by mistake must not reach a log).
pub fn parse_recipients(text: &str) -> Result<Vec<age::x25519::Recipient>, String> {
    let entries: Vec<&str> = text
        .split(',')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .collect();
    if entries.is_empty() {
        return Err(format!("{RECIPIENTS_ENV} has no recipients"));
    }
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            if e.to_ascii_uppercase().starts_with("AGE-SECRET-KEY") {
                return Err(format!(
                    "{RECIPIENTS_ENV} entry {} is a private key; only public keys (age1…) belong there",
                    i + 1
                ));
            }
            age::x25519::Recipient::from_str(e).map_err(|_| {
                format!(
                    "{RECIPIENTS_ENV} entry {} is not an age X25519 public key (age1…)",
                    i + 1
                )
            })
        })
        .collect()
}

/// age-encrypts what `write` produces to `recipients` (binary format, as `age -r` writes),
/// streaming it into the ciphertext buffer, which starts with `capacity` bytes.
fn seal(
    recipients: &[age::x25519::Recipient],
    capacity: usize,
    write: impl FnOnce(&mut dyn Write) -> std::io::Result<()>,
) -> Result<Vec<u8>, String> {
    let enc = age::Encryptor::with_recipients(recipients.iter().map(|r| r as &dyn age::Recipient))
        .map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(capacity + 512);
    let mut w = enc.wrap_output(&mut out).map_err(|e| e.to_string())?;
    write(&mut w).map_err(|e| e.to_string())?;
    w.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

/// age-encrypts `plain` to `recipients`.
pub fn encrypt(plain: &[u8], recipients: &[age::x25519::Recipient]) -> Result<Vec<u8>, String> {
    seal(recipients, plain.len(), |w| w.write_all(plain))
}

/// age-encrypts `dump`, writing its parts straight into the encryptor (no joined copy).
pub fn encrypt_dump(dump: &Dump, recipients: &[age::x25519::Recipient]) -> Result<Vec<u8>, String> {
    seal(recipients, dump.len(), |mut w| dump.write_to(&mut w))
}

/// The identities in an age identity file (as `age-keygen` writes).
pub fn read_identities(path: &Path) -> Result<Vec<Box<dyn age::Identity + Send + Sync>>, String> {
    let file = age::IdentityFile::from_file(path.display().to_string())
        .map_err(|e| format!("cannot read the identity file {}: {e}", path.display()))?;
    let ids = file
        .into_identities()
        .map_err(|e| format!("the identity file {} is not usable: {e}", path.display()))?;
    if ids.is_empty() {
        return Err(format!(
            "the identity file {} has no identities",
            path.display()
        ));
    }
    Ok(ids)
}

/// Decrypts an age file with `identities`.
pub fn decrypt(
    cipher: &[u8],
    identities: &[Box<dyn age::Identity + Send + Sync>],
) -> Result<Vec<u8>, String> {
    let d = age::Decryptor::new_buffered(cipher).map_err(|e| e.to_string())?;
    let mut r = d
        .decrypt(identities.iter().map(|i| i.as_ref() as &dyn age::Identity))
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    r.read_to_end(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Deletes every nightly backup but the newest `keep` by key, never `just_written` (counted
/// among the newest even when the listing does not show it yet). Returns the deleted keys,
/// oldest first.
pub async fn apply_retention<S: ObjectStore>(
    store: &S,
    just_written: &str,
    keep: usize,
) -> Result<Vec<String>, StoreError> {
    let mut keys: Vec<String> = store
        .list(NIGHTLY_PREFIX)
        .await?
        .into_iter()
        .filter(|k| is_backup_key(k))
        .collect();
    if !keys.iter().any(|k| k == just_written) {
        keys.push(just_written.to_owned());
    }
    keys.sort();
    keys.dedup();
    let excess = keys.len().saturating_sub(keep);
    let doomed: Vec<String> = keys
        .into_iter()
        .take(excess)
        .filter(|k| k != just_written)
        .collect();
    for k in &doomed {
        store.delete(k).await?;
    }
    Ok(doomed)
}

/// What a successful backup did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupOutcome {
    pub key: String,
    /// The encrypted object's size.
    pub bytes: usize,
    /// The dump's manifest counts.
    pub tables: BTreeMap<String, u64>,
    /// Old backups retention removed.
    pub deleted: Vec<String>,
}

/// One backup: dump, encrypt, put, retention.
pub async fn control_backup<S: ObjectStore>(
    db: &ControlDb,
    store: &S,
    recipients: &[age::x25519::Recipient],
    now_ms: i64,
) -> Result<BackupOutcome, JobError> {
    let dump = db
        .dump_parts(now_ms)
        .await
        .map_err(|e| JobError::new(code::DUMP_FAILED, e.to_string()))?;
    let tables = dump.manifest.tables.clone();
    let sealed =
        encrypt_dump(&dump, recipients).map_err(|e| JobError::new(code::ENCRYPT_FAILED, e))?;
    drop(dump);
    let key = object_key(now_ms);
    let bytes = sealed.len();
    store
        .put(&key, sealed)
        .await
        .map_err(|e| JobError::new(code::UPLOAD_FAILED, e.0))?;
    let deleted = apply_retention(store, &key, KEEP)
        .await
        .map_err(|e| JobError::new(code::RETENTION_FAILED, e.0))?;
    Ok(BackupOutcome {
        key,
        bytes,
        tables,
        deleted,
    })
}

/// Where a failure is reported, and where a success is pinged.
#[derive(Clone, PartialEq, Eq)]
pub struct AlertTarget {
    pub webhook: Option<String>,
    pub env_name: String,
    /// `ARENA_BACKUP_HEARTBEAT_URL`: GET after every successful backup, so a dead-man's-switch
    /// monitor alerts when no ping arrives (failures that never reach this binary).
    pub heartbeat: Option<String>,
}

impl std::fmt::Debug for AlertTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlertTarget")
            .field("webhook", &self.webhook.as_ref().map(|_| "[redacted]"))
            .field("env_name", &self.env_name)
            .field("heartbeat", &self.heartbeat.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

impl AlertTarget {
    pub fn from_env(env: &impl Fn(&str) -> Option<OsString>) -> Self {
        Self {
            webhook: alert::webhook_from_env(env),
            env_name: alert::env_name_from_env(env),
            heartbeat: alert::heartbeat_from_env(env),
        }
    }
}

/// Logs `err` and POSTs the alert when a webhook is set. A failed POST is logged too.
pub async fn report_failure(task: Task, err: &JobError, target: &AlertTarget) {
    tracing::error!(
        task = task.as_str(),
        code = err.code,
        detail = %err.detail,
        env = %target.env_name,
        "task failed"
    );
    let Some(url) = &target.webhook else {
        tracing::warn!(
            task = task.as_str(),
            "no {} set; the failure is only in this log",
            alert::WEBHOOK_ENV
        );
        return;
    };
    let text = alert::alert_text(&target.env_name, task.as_str(), err.code);
    match alert::post(url, &text, alert::ALERT_TIMEOUT).await {
        Ok(()) => tracing::info!(task = task.as_str(), code = err.code, "alert sent"),
        Err(e) => tracing::error!(
            task = task.as_str(),
            code = err.code,
            detail = %e,
            "the alert webhook POST failed"
        ),
    }
}

/// Runs one backup within `timeout`, logging the outcome, alerting on failure and pinging the
/// heartbeat on success.
pub async fn run_backup<S: ObjectStore>(
    db: &ControlDb,
    store: &S,
    recipients: &[age::x25519::Recipient],
    now_ms: i64,
    target: &AlertTarget,
    timeout: Duration,
) -> Result<BackupOutcome, JobError> {
    supervise(
        target,
        timeout,
        control_backup(db, store, recipients, now_ms),
    )
    .await
}

/// Runs `run` within `timeout`, then logs and reports the outcome.
async fn supervise(
    target: &AlertTarget,
    timeout: Duration,
    run: impl std::future::Future<Output = Result<BackupOutcome, JobError>>,
) -> Result<BackupOutcome, JobError> {
    let task = Task::ControlBackup;
    let res = match tokio::time::timeout(timeout, run).await {
        Ok(r) => r,
        Err(_) => Err(JobError::new(
            code::TIMEOUT,
            format!("the backup did not finish within {} s", timeout.as_secs()),
        )),
    };
    match &res {
        Ok(o) => {
            tracing::info!(
                task = task.as_str(),
                key = %o.key,
                bytes = o.bytes,
                tables = o.tables.len(),
                rows = o.tables.values().sum::<u64>(),
                deleted = o.deleted.len(),
                "backup written"
            );
            if let Some(url) = &target.heartbeat {
                match alert::ping(url, alert::ALERT_TIMEOUT).await {
                    Ok(()) => tracing::info!(task = task.as_str(), "heartbeat sent"),
                    Err(e) => tracing::error!(
                        task = task.as_str(),
                        detail = %e,
                        "the heartbeat GET failed (the backup itself succeeded)"
                    ),
                }
            }
        }
        Err(e) => report_failure(task, e, target).await,
    }
    res
}

/// Opens `db` without migrating it (a backup or restore never changes the schema).
pub(crate) async fn open(db: &Database) -> Result<ControlDb, ControlError> {
    match db {
        Database::Remote { url, token } => ControlDb::connect(url, token).await,
        Database::Local { data_dir } => {
            std::fs::create_dir_all(data_dir)
                .map_err(|e| ControlError::Db(format!("{}: {e}", data_dir.display())))?;
            ControlDb::open_local(&data_dir.join(crate::config::DEV_DB_FILE)).await
        }
    }
}

/// `--role jobs --task …`: resolves the environment, runs the task and returns whether it
/// succeeded. Setup and the run share one [`RUN_TIMEOUT`]. Every failure, including a bad
/// environment, is logged and alerted.
pub async fn run_job(job: &JobConfig, env: impl Fn(&str) -> Option<OsString>) -> bool {
    let target = AlertTarget::from_env(&env);
    match job.task {
        Task::ControlBackup => {
            let run = async {
                let database = match &job.dev_data_dir {
                    Some(dir) => Database::Local {
                        data_dir: dir.clone(),
                    },
                    None => crate::config::remote_database(&env)
                        .map_err(|e| JobError::new(code::CONFIG_INVALID, e))?,
                };
                let s3 =
                    S3Config::from_env(&env).map_err(|e| JobError::new(code::CONFIG_INVALID, e))?;
                let recipients = parse_recipients(
                    &crate::config::get_env(&env, RECIPIENTS_ENV).unwrap_or_default(),
                )
                .map_err(|e| JobError::new(code::CONFIG_INVALID, e))?;
                let db = open(&database)
                    .await
                    .map_err(|e| JobError::new(code::DUMP_FAILED, e.to_string()))?;
                control_backup(&db, &S3Store::new(&s3), &recipients, crate::now_ms()).await
            };
            supervise(&target, RUN_TIMEOUT, run).await.is_ok()
        }
        Task::CommandSweep => crate::command_sweep::run_job(job, &env, &target).await,
        Task::OfflineWatch => crate::offline_watch::run_job(job, &env, &target).await,
        Task::RetentionSweep => crate::retention::run_job(job, &env, &target).await,
        Task::SubscriptionSweep => crate::subscription::run_job(job, &env, &target).await,
        Task::BillingReconcile => crate::reconcile::run_job(job, &env, &target).await,
        Task::InvoiceRender => crate::invoice::run_job(job, &env, &target).await,
        Task::SubscriptionLifecycle => crate::lifecycle::run_job(job, &env, &target).await,
    }
}

/// `ops restore-control`: reads the dump (bucket key, file or stdin), decrypts it unless
/// `--plaintext`, and restores it into the empty target, verifying every table's row count.
/// A wrong identity or unreadable source fails before the target is touched.
pub async fn restore_control(
    cfg: &RestoreConfig,
    env: impl Fn(&str) -> Option<OsString>,
) -> Result<RestoreReport, JobError> {
    let target = match &cfg.dev_data_dir {
        Some(dir) => Database::Local {
            data_dir: dir.clone(),
        },
        None => crate::config::remote_database(&env)
            .map_err(|e| JobError::new(code::RESTORE_CONFIG_INVALID, e))?,
    };
    let raw = match &cfg.source {
        RestoreSource::Key(key) => {
            let s3 = S3Config::from_env(&env)
                .map_err(|e| JobError::new(code::RESTORE_CONFIG_INVALID, e))?;
            S3Store::new(&s3)
                .get(key)
                .await
                .map_err(|e| JobError::new(code::SOURCE_FAILED, e.0))?
        }
        RestoreSource::File(path) if path.as_os_str() == "-" => {
            let mut buf = Vec::new();
            std::io::stdin()
                .read_to_end(&mut buf)
                .map_err(|e| JobError::new(code::SOURCE_FAILED, format!("stdin: {e}")))?;
            buf
        }
        RestoreSource::File(path) => std::fs::read(path)
            .map_err(|e| JobError::new(code::SOURCE_FAILED, format!("{}: {e}", path.display())))?,
    };
    let plain = match &cfg.decryption {
        Decryption::Plaintext => raw,
        Decryption::Identity(path) => {
            let ids = read_identities(path).map_err(|e| JobError::new(code::DECRYPT_FAILED, e))?;
            decrypt(&raw, &ids).map_err(|e| JobError::new(code::DECRYPT_FAILED, e))?
        }
    };
    let text = String::from_utf8(plain).map_err(|_| {
        JobError::from(ControlError::DumpInvalid(
            "the dump is not UTF-8 text".into(),
        ))
    })?;
    let db = open(&target).await?;
    Ok(db.restore(&text).await?)
}
