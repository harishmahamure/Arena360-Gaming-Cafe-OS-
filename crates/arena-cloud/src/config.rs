//! Command-line arguments and environment for `arena-cloud`.

use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const USAGE: &str =
    "usage: arena-cloud --role control|command|all [--listen <addr:port>] [--help] [--version]
       arena-cloud --role control|command|all --dev [--data-dir <dir>] [--listen <loopback addr:port>]
       arena-cloud --role jobs --task control-backup [--dev [--data-dir <dir>]]
       arena-cloud ops restore-control (--from-key <key> | --from-file <path|->)
                                       (--identity <file> | --plaintext) [--dev [--data-dir <dir>]]
       arena-cloud ops grant-comp --branch <id> --tier basic|premium --until <YYYY-MM-DD>
                                  --operator <name> --reason <text> [--dev [--data-dir <dir>]]
       arena-cloud ops licence-keygen [--kid <kid>]
       arena-cloud catalog import <file.json|file.csv> --operator <name> [--media-dir <dir>]
                                  [--dev [--data-dir <dir>]]

  --role all   every serving role this build has (control, command)
  --role jobs  run one --task and exit (0 on success, 1 on failure)
  --task       control-backup: dump gz_control, age-encrypt it, put it in the backup bucket
               under gz_control/nightly/ and keep the newest 30
  --dev        local dev mode: gz_control is <data-dir>/gz_control.db (SQLite), no env or
               token needed, loopback only (default 127.0.0.1:8080), dev routes under /v1/dev
  --data-dir   dev data directory, created if missing (default ./.arena-cloud-dev)

  ops restore-control  restore a dump into an EMPTY gz_control and verify every table's row
               count; --from-key reads the backup bucket, --from-file a file (- for stdin);
               --identity decrypts with an age identity file, --plaintext reads a decrypted dump
  ops grant-comp  give a branch a comp Basic or Premium entitlement until the end of that date
               (Asia/Kolkata), write an ops_audit row and mark the wizard's plan step done;
               exit 1 for an unknown branch, a past date or a database failure
  ops licence-keygen  print a new Ed25519 PKCS#8 private key PEM on stdout (for SOPS) and, on
               stderr, its kid and the VerifyKey line for arena_proto::licence::LICENCE_VERIFY_KEYS
  catalog import  upsert the game catalogue from a JSON or CSV list by name + launcher, upload
               its media (relative to --media-dir, default the file's folder) once by hash,
               move the catalogue version and write one admin_audit row; needs the object store
               (ARENA_BACKUP_S3_*) only when a changed game has media

environment (not with --dev):
  ARENA_CONTROL_DB_URL    gz_control URL (libsql:// or https://)
  ARENA_CONTROL_DB_TOKEN  gz_control auth token
  ARENA_OTP_PEPPER        sign-up code pepper, 32+ characters (--dev: a fixed dev pepper)
  ARENA_WEB_ORIGINS       comma-separated browser origins allowed with credentials, exact
                          match (--dev default: http://localhost:5179)
  ARENA_SMS_PROVIDER      gupshup (--dev: always the fake, read at GET /v1/dev/sms)
  ARENA_SMS_GLOBAL_PER_MIN  SMS sends per rolling minute across all phones (default 60;
                          --dev 10000)
  ARENA_MFA_KEY           base64 of 32 random bytes: the AES-256-GCM key of owners' TOTP
                          secrets (--dev: a fixed dev key)
  ARENA_LICENCE_SIGNING_KEY  the Ed25519 PKCS#8 PEM that signs branch licences (from
                          ops licence-keygen; --dev: the committed dev key)
  ARENA_LICENCE_KID       the kid of that key, as listed in LICENCE_VERIFY_KEYS
  ARENA_CATALOG_CDN_URL   the CDN base URL game media are served from, `<base>/catalog/<sha256>.<ext>`
                          (--dev default: http://127.0.0.1:9000); unset: the manifest answers 503
  ARENA_BILLING_ENABLED   unset or false: billing is not available in this build (true exits 64)
  ARENA_GUPSHUP_ENDPOINT, ARENA_GUPSHUP_USERID, ARENA_GUPSHUP_PASSWORD, ARENA_GUPSHUP_SENDER,
  ARENA_GUPSHUP_DLT_TEMPLATE_ID  the Gupshup Enterprise SMS account (gupshup only)
environment (--task control-backup, ops restore-control --from-key):
  ARENA_BACKUP_S3_ENDPOINT, ARENA_BACKUP_S3_BUCKET, ARENA_BACKUP_S3_ACCESS_KEY_ID,
  ARENA_BACKUP_S3_SECRET_ACCESS_KEY  the backup bucket (R2; MinIO in dev)
  ARENA_BACKUP_AGE_RECIPIENTS        comma-separated age X25519 public keys (control-backup)
  ARENA_ALERT_WEBHOOK_URL            optional: Slack-compatible webhook POSTed on failure
  ARENA_ENV                          optional: environment name in the alert";

/// The CDN base URL catalogue media are served from (Story 6.1).
pub const CATALOG_CDN_ENV: &str = "ARENA_CATALOG_CDN_URL";

/// The CDN base from the environment (`--dev`: a local default), without a trailing slash.
pub fn catalog_cdn_from_env(dev: bool, env: &impl Fn(&str) -> Option<OsString>) -> Option<String> {
    get_env(env, CATALOG_CDN_ENV)
        .map(|v| v.trim().trim_end_matches('/').to_owned())
        .filter(|v| v.starts_with("https://") || v.starts_with("http://"))
        .or_else(|| dev.then(|| "http://127.0.0.1:9000".to_owned()))
}

/// The `gz_control` URL.
pub const DB_URL_ENV: &str = "ARENA_CONTROL_DB_URL";
/// The `gz_control` auth token (never logged).
pub const DB_TOKEN_ENV: &str = "ARENA_CONTROL_DB_TOKEN";
/// Test-only: shortens the start-up retry window (seconds, at most the 60 s default).
pub const DB_RETRY_SECS_ENV: &str = "ARENA_CONTROL_DB_RETRY_SECS";

/// The default listen address.
pub const DEFAULT_LISTEN: &str = "0.0.0.0:8080";
/// The default listen address with `--dev`.
pub const DEFAULT_DEV_LISTEN: &str = "127.0.0.1:8080";
/// The default `--data-dir`.
pub const DEFAULT_DATA_DIR: &str = "./.arena-cloud-dev";
/// The local `gz_control` file inside the dev data directory.
pub const DEV_DB_FILE: &str = "gz_control.db";
/// How long start-up keeps retrying an unreachable `gz_control`.
pub const DEFAULT_RETRY_WINDOW: Duration = Duration::from_secs(60);

/// The workload this process runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Control,
    Command,
    /// Run one [`Task`] and exit.
    Jobs,
    All,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Control => "control",
            Self::Command => "command",
            Self::Jobs => "jobs",
            Self::All => "all",
        }
    }
}

/// A run-once task of `--role jobs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    /// The nightly `gz_control` backup.
    ControlBackup,
}

impl Task {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ControlBackup => "control-backup",
        }
    }
}

/// Where `gz_control` lives.
#[derive(Clone, PartialEq, Eq)]
pub enum Database {
    /// Turso (or a loopback sqld) over libsql remote.
    Remote { url: String, token: String },
    /// `--dev`: a local SQLite file in `data_dir` ([`DEV_DB_FILE`]).
    Local { data_dir: PathBuf },
}

impl Database {
    /// The local file path, for [`Database::Local`].
    pub fn local_path(&self) -> Option<PathBuf> {
        match self {
            Self::Local { data_dir } => Some(data_dir.join(DEV_DB_FILE)),
            Self::Remote { .. } => None,
        }
    }
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Remote { url, .. } => f
                .debug_struct("Remote")
                .field("url", url)
                .field("token", &"[redacted]")
                .finish(),
            Self::Local { data_dir } => {
                f.debug_struct("Local").field("data_dir", data_dir).finish()
            }
        }
    }
}

/// Resolved start-up configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub role: Role,
    pub listen: SocketAddr,
    pub database: Database,
    pub retry_window: Duration,
}

impl Config {
    /// Local dev mode (`--dev`): the local database and the dev routes.
    pub fn dev(&self) -> bool {
        matches!(self.database, Database::Local { .. })
    }
}

/// `--role jobs --task <task>`. The environment is read when the task runs, so a missing or
/// bad variable is a task failure (exit 1, alerted), not a usage error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobConfig {
    pub task: Task,
    /// `--dev`: the local `gz_control` in this directory instead of `ARENA_CONTROL_DB_*`.
    pub dev_data_dir: Option<PathBuf>,
}

/// Where `ops restore-control` reads the dump.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreSource {
    /// An object key in the backup bucket (`ARENA_BACKUP_S3_*`).
    Key(String),
    /// A local file; `-` is stdin.
    File(PathBuf),
}

/// How `ops restore-control` reads the dump's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decryption {
    /// age-decrypt with the identities in this file.
    Identity(PathBuf),
    /// The input is an already decrypted dump.
    Plaintext,
}

/// `ops restore-control`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreConfig {
    pub source: RestoreSource,
    pub decryption: Decryption,
    /// `--dev`: restore into the local `gz_control` in this directory. Otherwise the target
    /// is `ARENA_CONTROL_DB_*`, read when the restore runs (a missing variable exits 1).
    pub dev_data_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantCompConfig {
    pub branch_id: String,
    pub tier: arena_proto::Tier,
    pub until_date: String,
    pub until_ms: i64,
    pub operator: String,
    pub reason: String,
    pub dev_data_dir: Option<PathBuf>,
}

/// `catalog import` (Story 6.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogImportConfig {
    pub file: PathBuf,
    /// Media paths in the file are relative to this folder.
    pub media_dir: PathBuf,
    pub operator: String,
    pub dev_data_dir: Option<PathBuf>,
}

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Run(Config),
    Job(JobConfig),
    RestoreControl(RestoreConfig),
    GrantComp(GrantCompConfig),
    CatalogImport(CatalogImportConfig),
    LicenceKeygen { kid: Option<String> },
    Help,
    Version,
}

pub const IST_OFFSET_MS: i64 = (5 * 60 + 30) * 60 * 1000;
const OPS_TEXT_MAX: usize = 200;

pub fn end_of_ist_day(date: &str) -> Option<i64> {
    let b = date.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        let s = date.get(r)?;
        if !s.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        s.parse().ok()
    };
    let day = chrono::NaiveDate::from_ymd_opt(num(0..4)? as i32, num(5..7)?, num(8..10)?)?;
    let next = day.succ_opt()?.and_hms_opt(0, 0, 0)?;
    Some(next.and_utc().timestamp_millis() - IST_OFFSET_MS - 1)
}

fn ops_text(flag: &str, v: Option<String>) -> Result<String, String> {
    let v = v.ok_or_else(|| format!("grant-comp needs {flag}"))?;
    let v = v.trim().to_owned();
    if v.is_empty() || v.chars().count() > OPS_TEXT_MAX || v.chars().any(char::is_control) {
        return Err(format!(
            "{flag} must be 1 to {OPS_TEXT_MAX} characters of plain text"
        ));
    }
    Ok(v)
}

fn parse_grant_comp(args: impl Iterator<Item = OsString>) -> Result<Command, String> {
    let mut branch: Option<String> = None;
    let mut tier: Option<String> = None;
    let mut until: Option<String> = None;
    let mut operator: Option<String> = None;
    let mut reason: Option<String> = None;
    let mut dev = false;
    let mut data_dir: Option<PathBuf> = None;
    let early = walk_flags(args, |flag, inline, value| {
        match flag {
            "--help" | "-h" => return Ok(Some(Command::Help)),
            "--branch" => branch = Some(value()?),
            "--tier" => tier = Some(value()?),
            "--until" => until = Some(value()?),
            "--operator" => operator = Some(value()?),
            "--reason" => reason = Some(value()?),
            "--dev" if !inline => dev = true,
            "--data-dir" => data_dir = Some(PathBuf::from(value()?)),
            other => return Err(format!("unknown argument {other}")),
        }
        Ok(None)
    })?;
    if let Some(cmd) = early {
        return Ok(cmd);
    }
    let branch_id = ops_text("--branch", branch)?;
    let tier = match tier.as_deref().map(str::trim) {
        Some("basic") => arena_proto::Tier::Basic,
        Some("premium") => arena_proto::Tier::Premium,
        None => return Err("grant-comp needs --tier (basic or premium)".to_owned()),
        Some(other) => return Err(format!("--tier must be basic or premium, not {other:?}")),
    };
    let until_date = until
        .map(|u| u.trim().to_owned())
        .ok_or("grant-comp needs --until <YYYY-MM-DD>")?;
    let until_ms = end_of_ist_day(&until_date)
        .ok_or_else(|| format!("--until must be a date as YYYY-MM-DD, not {until_date:?}"))?;
    let operator = ops_text("--operator", operator)?;
    let reason = ops_text("--reason", reason)?;
    if data_dir.is_some() && !dev {
        return Err("--data-dir needs --dev".to_owned());
    }
    Ok(Command::GrantComp(GrantCompConfig {
        branch_id,
        tier,
        until_date,
        until_ms,
        operator,
        reason,
        dev_data_dir: dev.then(|| data_dir.unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR))),
    }))
}

fn parse_catalog(mut args: impl Iterator<Item = OsString>) -> Result<Command, String> {
    match args.next().and_then(|a| a.into_string().ok()).as_deref() {
        Some("import") => {}
        Some("--help" | "-h") => return Ok(Command::Help),
        Some(other) => return Err(format!("unknown catalog command {other}")),
        None => return Err("catalog needs a command (import)".to_owned()),
    }
    let mut file: Option<String> = None;
    let mut media_dir: Option<PathBuf> = None;
    let mut operator: Option<String> = None;
    let mut dev = false;
    let mut data_dir: Option<PathBuf> = None;
    let early = walk_flags(args, |flag, inline, value| {
        match flag {
            "--help" | "-h" => return Ok(Some(Command::Help)),
            "--media-dir" => media_dir = Some(PathBuf::from(value()?)),
            "--operator" => operator = Some(value()?),
            "--dev" if !inline => dev = true,
            "--data-dir" => data_dir = Some(PathBuf::from(value()?)),
            other if !other.starts_with('-') && file.is_none() => file = Some(other.to_owned()),
            other => return Err(format!("unknown argument {other}")),
        }
        Ok(None)
    })?;
    if let Some(cmd) = early {
        return Ok(cmd);
    }
    let file = PathBuf::from(file.ok_or("catalog import needs a file (.json or .csv)")?);
    let operator = operator
        .map(|v| v.trim().to_owned())
        .filter(|v| {
            !v.is_empty() && v.chars().count() <= OPS_TEXT_MAX && !v.chars().any(char::is_control)
        })
        .ok_or("catalog import needs --operator")?;
    if data_dir.is_some() && !dev {
        return Err("--data-dir needs --dev".to_owned());
    }
    let media_dir = media_dir.unwrap_or_else(|| {
        file.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    });
    Ok(Command::CatalogImport(CatalogImportConfig {
        file,
        media_dir,
        operator,
        dev_data_dir: dev.then(|| data_dir.unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR))),
    }))
}

fn parse_keygen(args: impl Iterator<Item = OsString>) -> Result<Command, String> {
    let mut kid: Option<String> = None;
    let early = walk_flags(args, |flag, _, value| {
        match flag {
            "--help" | "-h" => return Ok(Some(Command::Help)),
            "--kid" => kid = Some(value()?),
            other => return Err(format!("unknown argument {other}")),
        }
        Ok(None)
    })?;
    if let Some(cmd) = early {
        return Ok(cmd);
    }
    if let Some(k) = &kid
        && !crate::licence::valid_kid(k)
    {
        return Err("--kid must be 1 to 64 letters, digits, '-', '_' or '.'".to_owned());
    }
    Ok(Command::LicenceKeygen { kid })
}

/// Is `url` a plain-HTTP loopback URL (`http://127.0.0.1…` or `http://localhost…`)?
pub(crate) fn is_loopback_http(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let host = rest.split(['/', '?']).next().unwrap_or("");
    let name = match host.rsplit_once(':') {
        Some((name, port)) if port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => host,
    };
    name == "127.0.0.1" || name.eq_ignore_ascii_case("localhost")
}

/// Reads `k` from `env`, trimmed: YAML block scalars and `echo` often leave a trailing newline
/// or spaces.
pub(crate) fn get_env(env: &impl Fn(&str) -> Option<OsString>, k: &str) -> Option<String> {
    env(k)
        .and_then(|v| v.into_string().ok())
        .map(|v| v.trim().to_owned())
}

/// The remote `gz_control` from `ARENA_CONTROL_DB_URL` and `ARENA_CONTROL_DB_TOKEN`. Errors
/// name the variable, never its value.
pub fn remote_database(env: &impl Fn(&str) -> Option<OsString>) -> Result<Database, String> {
    let db_url = get_env(env, DB_URL_ENV)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("{DB_URL_ENV} is not set"))?;
    let loopback = is_loopback_http(&db_url);
    if !(db_url.starts_with("libsql://") || db_url.starts_with("https://") || loopback) {
        return Err(format!("{DB_URL_ENV} must be a libsql:// or https:// URL"));
    }
    let db_token = get_env(env, DB_TOKEN_ENV).unwrap_or_default();
    if db_token.is_empty() && !loopback {
        return Err(format!("{DB_TOKEN_ENV} is not set"));
    }
    Ok(Database::Remote {
        url: db_url,
        token: db_token,
    })
}

/// Splits `args` into `(flag, inline value)` pairs and hands each to `on_flag` with a closure
/// that takes the flag's value (inline `--f=v` or the next argument).
fn walk_flags(
    args: impl IntoIterator<Item = OsString>,
    mut on_flag: impl FnMut(
        &str,
        bool,
        &mut dyn FnMut() -> Result<String, String>,
    ) -> Result<Option<Command>, String>,
) -> Result<Option<Command>, String> {
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let arg = arg
            .into_string()
            .map_err(|a| format!("invalid argument {a:?}"))?;
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_owned(), Some(OsString::from(v))),
            _ => (arg.clone(), None),
        };
        let has_inline = inline.is_some();
        let mut value = || {
            let v = inline
                .clone()
                .or_else(|| args.next())
                .ok_or_else(|| format!("{flag} needs a value"))?;
            if v.is_empty() || (v.to_string_lossy().starts_with("--")) {
                return Err(format!("{flag} needs a value"));
            }
            v.into_string().map_err(|v| format!("invalid {flag} {v:?}"))
        };
        if let Some(cmd) = on_flag(&flag, has_inline, &mut value)? {
            return Ok(Some(cmd));
        }
    }
    Ok(None)
}

/// Parses `args` (without the program name). `env` reads an environment variable.
pub fn parse(
    args: impl IntoIterator<Item = OsString>,
    env: impl Fn(&str) -> Option<OsString>,
) -> Result<Command, String> {
    let mut args = args.into_iter().peekable();
    if args.peek().is_some_and(|a| a == "ops") {
        args.next();
        return parse_ops(args);
    }
    if args.peek().is_some_and(|a| a == "catalog") {
        args.next();
        return parse_catalog(args);
    }
    let mut role: Option<String> = None;
    let mut task: Option<String> = None;
    let mut listen: Option<SocketAddr> = None;
    let mut dev = false;
    let mut data_dir: Option<PathBuf> = None;
    let early = walk_flags(args, |flag, inline, value| {
        match flag {
            "--help" | "-h" => return Ok(Some(Command::Help)),
            "--version" => return Ok(Some(Command::Version)),
            "--role" => role = Some(value()?),
            "--task" => task = Some(value()?),
            "--listen" => {
                let v = value()?;
                listen = Some(v.parse().map_err(|_| format!("invalid --listen {v:?}"))?);
            }
            "--dev" if !inline => dev = true,
            "--data-dir" => data_dir = Some(PathBuf::from(value()?)),
            other => return Err(format!("unknown argument {other}")),
        }
        Ok(None)
    })?;
    if let Some(cmd) = early {
        return Ok(cmd);
    }
    let role = match role.as_deref() {
        None => return Err("--role is required (control, command, jobs or all)".to_owned()),
        Some("control") => Role::Control,
        Some("command") => Role::Command,
        Some("jobs") => Role::Jobs,
        Some("all") => Role::All,
        Some(other) => return Err(format!("role not available yet: {other}")),
    };
    if data_dir.is_some() && !dev {
        return Err("--data-dir needs --dev".to_owned());
    }
    if role == Role::Jobs {
        let task = match task.as_deref() {
            None => return Err("--role jobs needs --task (control-backup)".to_owned()),
            Some("control-backup") => Task::ControlBackup,
            Some(other) => return Err(format!("unknown --task {other}")),
        };
        if listen.is_some() {
            return Err("--listen is not used by --role jobs".to_owned());
        }
        return Ok(Command::Job(JobConfig {
            task,
            dev_data_dir: dev.then(|| data_dir.unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR))),
        }));
    }
    if task.is_some() {
        return Err("--task needs --role jobs".to_owned());
    }

    if dev {
        let listen = match listen {
            None => DEFAULT_DEV_LISTEN
                .parse()
                .expect("valid default dev listen address"),
            Some(l) if l.ip().is_loopback() => l,
            Some(l) => {
                return Err(format!(
                    "--dev listens on loopback only; refusing --listen {l}"
                ));
            }
        };
        return Ok(Command::Run(Config {
            role,
            listen,
            database: Database::Local {
                data_dir: data_dir.unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR)),
            },
            // Unused: a local file is opened once, never retried.
            retry_window: DEFAULT_RETRY_WINDOW,
        }));
    }
    let listen = listen.unwrap_or_else(|| {
        DEFAULT_LISTEN
            .parse()
            .expect("valid default listen address")
    });

    let retry_window = match get_env(&env, DB_RETRY_SECS_ENV).filter(|v| !v.is_empty()) {
        None => DEFAULT_RETRY_WINDOW,
        Some(v) => {
            let secs: u64 = v
                .parse()
                .ok()
                .filter(|s| Duration::from_secs(*s) <= DEFAULT_RETRY_WINDOW)
                .ok_or_else(|| format!("{DB_RETRY_SECS_ENV} must be 0–60"))?;
            Duration::from_secs(secs)
        }
    };
    Ok(Command::Run(Config {
        role,
        listen,
        database: remote_database(&env)?,
        retry_window,
    }))
}

/// `ops <subcommand> …`.
fn parse_ops(mut args: impl Iterator<Item = OsString>) -> Result<Command, String> {
    match args.next().and_then(|a| a.into_string().ok()).as_deref() {
        Some("restore-control") => {}
        Some("grant-comp") => return parse_grant_comp(args),
        Some("licence-keygen") => return parse_keygen(args),
        Some("--help" | "-h") => return Ok(Command::Help),
        Some(other) => return Err(format!("unknown ops command {other}")),
        None => {
            return Err(
                "ops needs a command (restore-control, grant-comp or licence-keygen)".to_owned(),
            );
        }
    }
    let mut from_key: Option<String> = None;
    let mut from_file: Option<PathBuf> = None;
    let mut identity: Option<PathBuf> = None;
    let mut plaintext = false;
    let mut dev = false;
    let mut data_dir: Option<PathBuf> = None;
    let early = walk_flags(args, |flag, inline, value| {
        match flag {
            "--help" | "-h" => return Ok(Some(Command::Help)),
            "--from-key" => from_key = Some(value()?),
            "--from-file" => from_file = Some(PathBuf::from(value()?)),
            "--identity" => identity = Some(PathBuf::from(value()?)),
            "--plaintext" if !inline => plaintext = true,
            "--dev" if !inline => dev = true,
            "--data-dir" => data_dir = Some(PathBuf::from(value()?)),
            other => return Err(format!("unknown argument {other}")),
        }
        Ok(None)
    });
    // `--from-file -` (stdin) is a value, not a flag.
    if let Some(cmd) = early? {
        return Ok(cmd);
    }
    let source = match (from_key, from_file) {
        (Some(k), None) => RestoreSource::Key(k),
        (None, Some(f)) => RestoreSource::File(f),
        _ => return Err("restore-control needs exactly one of --from-key, --from-file".to_owned()),
    };
    let decryption = match (identity, plaintext) {
        (Some(i), false) => Decryption::Identity(i),
        (None, true) => Decryption::Plaintext,
        _ => return Err("restore-control needs exactly one of --identity, --plaintext".to_owned()),
    };
    if data_dir.is_some() && !dev {
        return Err("--data-dir needs --dev".to_owned());
    }
    Ok(Command::RestoreControl(RestoreConfig {
        source,
        decryption,
        dev_data_dir: dev.then(|| data_dir.unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR))),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_http_is_only_127_0_0_1_or_localhost() {
        for (url, ok) in [
            ("http://127.0.0.1:8080", true),
            ("http://localhost:1/x", true),
            ("http://127.0.0.1", true),
            ("http://127.0.0.1.evil.example", false),
            ("http://localhost.evil:80", false),
            ("http://10.0.0.1:8080", false),
            ("https://127.0.0.1", false),
        ] {
            assert_eq!(is_loopback_http(url), ok, "{url}");
        }
    }

    #[test]
    fn debug_never_prints_the_token() {
        let cfg = Config {
            role: Role::Control,
            listen: DEFAULT_LISTEN.parse().unwrap(),
            database: Database::Remote {
                url: "libsql://x".into(),
                token: "tok-SECRET".into(),
            },
            retry_window: DEFAULT_RETRY_WINDOW,
        };
        assert!(!format!("{cfg:?}").contains("tok-SECRET"));
    }
}

/// The OTP pepper (a secret: never logged), at least [`MIN_PEPPER_LEN`] bytes.
pub const OTP_PEPPER_ENV: &str = "ARENA_OTP_PEPPER";
/// Comma-separated browser origins allowed to call `/v1` with credentials (exact match).
pub const WEB_ORIGINS_ENV: &str = "ARENA_WEB_ORIGINS";
/// `gupshup` (`fake` only with `--dev`).
pub const SMS_PROVIDER_ENV: &str = "ARENA_SMS_PROVIDER";
/// SMS sends allowed per rolling minute across every phone (default
/// [`DEFAULT_SMS_GLOBAL_PER_MIN`]).
pub const SMS_GLOBAL_PER_MIN_ENV: &str = "ARENA_SMS_GLOBAL_PER_MIN";
pub const DEFAULT_SMS_GLOBAL_PER_MIN: u64 = 60;
/// The `--dev` default of [`SMS_GLOBAL_PER_MIN_ENV`].
pub const DEV_SMS_GLOBAL_PER_MIN: u64 = 10_000;
pub const GUPSHUP_ENDPOINT_ENV: &str = "ARENA_GUPSHUP_ENDPOINT";
pub const GUPSHUP_USERID_ENV: &str = "ARENA_GUPSHUP_USERID";
/// A secret: never logged.
pub const GUPSHUP_PASSWORD_ENV: &str = "ARENA_GUPSHUP_PASSWORD";
pub const GUPSHUP_SENDER_ENV: &str = "ARENA_GUPSHUP_SENDER";
pub const GUPSHUP_DLT_TEMPLATE_ID_ENV: &str = "ARENA_GUPSHUP_DLT_TEMPLATE_ID";
/// The AES-256-GCM key of owners' TOTP secrets: base64 of 32 bytes (a secret: never logged).
pub const MFA_KEY_ENV: &str = "ARENA_MFA_KEY";
pub const LICENCE_SIGNING_KEY_ENV: &str = "ARENA_LICENCE_SIGNING_KEY";
pub const LICENCE_KID_ENV: &str = "ARENA_LICENCE_KID";
pub const BILLING_ENABLED_ENV: &str = "ARENA_BILLING_ENABLED";
/// The fixed key `--dev` uses (32 bytes).
pub const DEV_MFA_KEY: &[u8; 32] = b"arena-dev-mfa-key-not-for-prod!!";
/// The shortest accepted `ARENA_OTP_PEPPER`.
pub const MIN_PEPPER_LEN: usize = 32;
/// The fixed pepper `--dev` uses.
pub const DEV_OTP_PEPPER: &str = "arena-dev-otp-pepper-not-for-production-use";
/// The browser origin `--dev` allows when `ARENA_WEB_ORIGINS` is unset (the onboarding dev
/// server).
pub const DEV_WEB_ORIGIN: &str = "http://localhost:5179";

/// Which SMS provider sends codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmsProviderConfig {
    /// The recorded fake.
    Fake,
    Gupshup(crate::messaging::GupshupConfig),
}

/// Owner sign-up and session settings for the serving roles.
#[derive(Clone, PartialEq, Eq)]
pub struct AuthConfig {
    pub pepper: Vec<u8>,
    /// Exact origins (`https://start.arena360.cloud`).
    pub web_origins: Vec<String>,
    pub sms: SmsProviderConfig,
    /// SMS sends allowed per rolling minute across every phone.
    pub sms_global_per_min: u64,
    /// Seals owners' TOTP secrets.
    pub mfa_key: crate::crypto::MfaKey,
    pub licence: crate::licence::LicenceSigner,
    pub billing_enabled: bool,
}

impl std::fmt::Debug for AuthConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthConfig")
            .field("pepper", &"[redacted]")
            .field("web_origins", &self.web_origins)
            .field("sms", &self.sms)
            .field("sms_global_per_min", &self.sms_global_per_min)
            .field("mfa_key", &self.mfa_key)
            .field("licence", &self.licence)
            .field("billing_enabled", &self.billing_enabled)
            .finish()
    }
}

impl AuthConfig {
    /// The `--dev` settings: the fixed pepper and MFA key, the fake SMS and
    /// [`DEV_WEB_ORIGIN`].
    pub fn dev() -> Self {
        Self {
            pepper: DEV_OTP_PEPPER.as_bytes().to_vec(),
            web_origins: vec![DEV_WEB_ORIGIN.to_owned()],
            sms: SmsProviderConfig::Fake,
            sms_global_per_min: DEV_SMS_GLOBAL_PER_MIN,
            mfa_key: crate::crypto::MfaKey::from_bytes(*DEV_MFA_KEY),
            licence: crate::licence::LicenceSigner::dev(),
            billing_enabled: false,
        }
    }
}

fn billing_enabled(env: &impl Fn(&str) -> Option<OsString>) -> Result<bool, String> {
    match get_env(env, BILLING_ENABLED_ENV).as_deref() {
        None | Some("") | Some("false") => Ok(false),
        Some("true") => Err(format!(
            "{BILLING_ENABLED_ENV}=true: billing is not available in this build"
        )),
        Some(_) => Err(format!("{BILLING_ENABLED_ENV} must be true or false")),
    }
}

pub fn licence_from_env(
    env: &impl Fn(&str) -> Option<OsString>,
) -> Result<crate::licence::LicenceSigner, String> {
    let pem = get_env(env, LICENCE_SIGNING_KEY_ENV)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| {
            format!("{LICENCE_SIGNING_KEY_ENV} is not set (a PEM from ops licence-keygen)")
        })?;
    let kid = get_env(env, LICENCE_KID_ENV)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("{LICENCE_KID_ENV} is not set"))?;
    let signer = crate::licence::LicenceSigner::from_pem(&kid, &pem).map_err(|e| {
        if e.starts_with("kid") {
            format!("{LICENCE_KID_ENV}: {e}")
        } else {
            format!("{LICENCE_SIGNING_KEY_ENV} {e}")
        }
    })?;
    if signer.is_dev_key() {
        return Err(format!(
            "{LICENCE_SIGNING_KEY_ENV} is the committed dev key; generate one with ops licence-keygen"
        ));
    }
    Ok(signer)
}

/// Parses one origin: `http(s)://host[:port]`, no path, query or trailing slash.
fn parse_origin(raw: &str) -> Result<String, String> {
    let o = raw.trim();
    let rest = o
        .strip_prefix("https://")
        .or_else(|| o.strip_prefix("http://"))
        .ok_or_else(|| format!("{WEB_ORIGINS_ENV}: {o:?} must start with https:// or http://"))?;
    let ok = !rest.is_empty()
        && rest
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':'));
    if !ok {
        return Err(format!(
            "{WEB_ORIGINS_ENV}: {o:?} must be an origin (scheme://host[:port]) with no path"
        ));
    }
    Ok(o.to_ascii_lowercase())
}

/// Parses `ARENA_WEB_ORIGINS`.
pub fn parse_origins(v: &str) -> Result<Vec<String>, String> {
    v.split(',')
        .filter(|p| !p.trim().is_empty())
        .map(parse_origin)
        .collect()
}

/// The sign-up settings from the environment. `--dev` needs none of it: the pepper is fixed,
/// the SMS provider is the fake, and the origins default to [`DEV_WEB_ORIGIN`] (or
/// `ARENA_WEB_ORIGINS` when set). Otherwise `ARENA_OTP_PEPPER`, `ARENA_WEB_ORIGINS` and
/// `ARENA_SMS_PROVIDER` (plus `ARENA_GUPSHUP_*` for `gupshup`) and `ARENA_MFA_KEY` are
/// required. Errors name the variable, never a secret's value.
pub fn auth_from_env(
    dev: bool,
    env: &impl Fn(&str) -> Option<OsString>,
) -> Result<AuthConfig, String> {
    let origins = get_env(env, WEB_ORIGINS_ENV).filter(|v| !v.is_empty());
    let global = match get_env(env, SMS_GLOBAL_PER_MIN_ENV).filter(|v| !v.is_empty()) {
        None => None,
        Some(v) => Some(
            v.parse::<u64>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("{SMS_GLOBAL_PER_MIN_ENV} must be a positive number"))?,
        ),
    };
    let billing = billing_enabled(env)?;
    if dev {
        let mut cfg = AuthConfig::dev();
        cfg.billing_enabled = billing;
        if let Some(v) = origins {
            cfg.web_origins = parse_origins(&v)?;
        }
        if let Some(n) = global {
            cfg.sms_global_per_min = n;
        }
        return Ok(cfg);
    }
    let pepper = get_env(env, OTP_PEPPER_ENV)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("{OTP_PEPPER_ENV} is not set"))?;
    if pepper.len() < MIN_PEPPER_LEN {
        return Err(format!(
            "{OTP_PEPPER_ENV} must be at least {MIN_PEPPER_LEN} characters"
        ));
    }
    let web_origins =
        parse_origins(&origins.ok_or_else(|| format!("{WEB_ORIGINS_ENV} is not set"))?)?;
    if web_origins.is_empty() {
        return Err(format!("{WEB_ORIGINS_ENV} lists no origin"));
    }
    let sms = match get_env(env, SMS_PROVIDER_ENV).as_deref() {
        Some("fake") => {
            return Err(format!(
                "{SMS_PROVIDER_ENV}=fake is only for --dev; use gupshup"
            ));
        }
        Some("gupshup") => {
            let req = |k: &str| {
                get_env(env, k)
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| format!("{k} is not set (ARENA_SMS_PROVIDER=gupshup)"))
            };
            let endpoint = req(GUPSHUP_ENDPOINT_ENV)?;
            if !(endpoint.starts_with("https://") || is_loopback_http(&endpoint)) {
                return Err(format!("{GUPSHUP_ENDPOINT_ENV} must be an https:// URL"));
            }
            SmsProviderConfig::Gupshup(crate::messaging::GupshupConfig {
                endpoint,
                userid: req(GUPSHUP_USERID_ENV)?,
                password: req(GUPSHUP_PASSWORD_ENV)?,
                sender: req(GUPSHUP_SENDER_ENV)?,
                dlt_template_id: req(GUPSHUP_DLT_TEMPLATE_ID_ENV)?,
            })
        }
        None | Some("") => return Err(format!("{SMS_PROVIDER_ENV} is not set (gupshup)")),
        Some(_) => return Err(format!("{SMS_PROVIDER_ENV} must be gupshup")),
    };
    let mfa_key = get_env(env, MFA_KEY_ENV)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("{MFA_KEY_ENV} is not set (base64 of 32 random bytes)"))?;
    let mfa_key =
        crate::crypto::MfaKey::from_base64(&mfa_key).map_err(|e| format!("{MFA_KEY_ENV} {e}"))?;
    let licence = licence_from_env(env)?;
    Ok(AuthConfig {
        pepper: pepper.into_bytes(),
        web_origins,
        sms,
        sms_global_per_min: global.unwrap_or(DEFAULT_SMS_GLOBAL_PER_MIN),
        mfa_key,
        licence,
        billing_enabled: billing,
    })
}

#[cfg(test)]
mod auth_tests {
    use super::*;

    fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        move |k| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| OsString::from(v))
        }
    }

    const PEPPER: &str = "pepper-SECRET-0123456789abcdef0123456789";
    /// base64 of 32 bytes.
    const MFA_KEY: &str = "c2VjcmV0LW1mYS1rZXktMDEyMzQ1Njc4OWFiY2RlZmc=";

    #[test]
    fn the_fake_provider_is_refused_outside_dev_and_the_global_cap_parses() {
        let base = [
            ("ARENA_OTP_PEPPER", PEPPER),
            ("ARENA_WEB_ORIGINS", "https://start.arena360.cloud"),
        ];
        let with = |extra: &'static [(&'static str, &'static str)]| {
            let all: &'static [(&'static str, &'static str)] =
                Box::leak([&base[..], extra].concat().into());
            auth_from_env(false, &env_of(all))
        };
        let e = with(&[("ARENA_SMS_PROVIDER", "fake")]).unwrap_err();
        assert!(e.contains("only for --dev"), "{e}");
        assert_eq!(
            auth_from_env(true, &env_of(&[]))
                .unwrap()
                .sms_global_per_min,
            DEV_SMS_GLOBAL_PER_MIN
        );
        let dev = auth_from_env(true, &env_of(&[("ARENA_SMS_GLOBAL_PER_MIN", "7")])).unwrap();
        assert_eq!(dev.sms_global_per_min, 7);
        let e = with(&[
            ("ARENA_SMS_PROVIDER", "gupshup"),
            ("ARENA_SMS_GLOBAL_PER_MIN", "0"),
        ])
        .unwrap_err();
        assert!(e.contains("ARENA_SMS_GLOBAL_PER_MIN"), "{e}");
    }

    #[test]
    fn dev_needs_nothing_and_forces_the_fake() {
        let cfg = auth_from_env(true, &env_of(&[("ARENA_SMS_PROVIDER", "gupshup")])).unwrap();
        assert_eq!(cfg, AuthConfig::dev());
        assert_eq!(cfg.web_origins, vec!["http://localhost:5179"]);
        let cfg = auth_from_env(
            true,
            &env_of(&[(
                "ARENA_WEB_ORIGINS",
                "http://localhost:5179, http://127.0.0.1:5179",
            )]),
        )
        .unwrap();
        assert_eq!(cfg.web_origins.len(), 2);
    }

    #[test]
    fn production_requires_each_variable_naming_it() {
        for (pairs, needle) in [
            (&[][..], "ARENA_OTP_PEPPER"),
            (&[("ARENA_OTP_PEPPER", "short")][..], "at least 32"),
            (&[("ARENA_OTP_PEPPER", PEPPER)][..], "ARENA_WEB_ORIGINS"),
            (
                &[
                    ("ARENA_OTP_PEPPER", PEPPER),
                    ("ARENA_WEB_ORIGINS", "https://start.arena360.cloud/path"),
                ][..],
                "no path",
            ),
            (
                &[
                    ("ARENA_OTP_PEPPER", PEPPER),
                    ("ARENA_WEB_ORIGINS", "https://start.arena360.cloud"),
                ][..],
                "ARENA_SMS_PROVIDER",
            ),
            (
                &[
                    ("ARENA_OTP_PEPPER", PEPPER),
                    ("ARENA_WEB_ORIGINS", "https://start.arena360.cloud"),
                    ("ARENA_SMS_PROVIDER", "gupshup"),
                ][..],
                "ARENA_GUPSHUP_ENDPOINT",
            ),
        ] {
            let pairs: &'static [(&'static str, &'static str)] = Box::leak(pairs.to_vec().into());
            let e = auth_from_env(false, &env_of(pairs)).unwrap_err();
            assert!(e.contains(needle), "{e}");
            assert!(!e.contains(PEPPER), "{e}");
        }
    }

    fn licence_pairs() -> Vec<(&'static str, &'static str)> {
        let g = crate::licence::generate(Some("test-1")).unwrap();
        let pem: &'static str = Box::leak(g.pem.into_boxed_str());
        vec![
            ("ARENA_LICENCE_SIGNING_KEY", pem),
            ("ARENA_LICENCE_KID", "test-1"),
        ]
    }

    fn with_licence(
        pairs: &[(&'static str, &'static str)],
    ) -> &'static [(&'static str, &'static str)] {
        let mut v = pairs.to_vec();
        v.extend(licence_pairs());
        Box::leak(v.into())
    }

    #[test]
    fn a_full_gupshup_config_parses_and_debug_hides_secrets() {
        let cfg = auth_from_env(
            false,
            &env_of(with_licence(&[
                ("ARENA_OTP_PEPPER", PEPPER),
                (
                    "ARENA_WEB_ORIGINS",
                    "https://start.arena360.cloud,https://start.staging.arena360.cloud",
                ),
                ("ARENA_SMS_PROVIDER", "gupshup"),
                (
                    "ARENA_GUPSHUP_ENDPOINT",
                    "https://enterprise.smsgupshup.com/GatewayAPI/rest",
                ),
                ("ARENA_GUPSHUP_USERID", "2000000000"),
                ("ARENA_GUPSHUP_PASSWORD", "gs-SECRET-pass"),
                ("ARENA_GUPSHUP_SENDER", "ARENA"),
                ("ARENA_GUPSHUP_DLT_TEMPLATE_ID", "1007000000000000000"),
                ("ARENA_MFA_KEY", MFA_KEY),
            ])),
        )
        .unwrap();
        assert_eq!(cfg.licence.kid(), "test-1");
        assert!(!cfg.billing_enabled);
        assert_eq!(cfg.web_origins.len(), 2);
        assert!(matches!(cfg.sms, SmsProviderConfig::Gupshup(_)));
        assert_eq!(cfg.sms_global_per_min, DEFAULT_SMS_GLOBAL_PER_MIN);
        let d = format!("{cfg:?}");
        assert!(!d.contains("PRIVATE"), "{d}");
        assert!(!d.contains(PEPPER) && !d.contains("gs-SECRET-pass"), "{d}");
        assert!(!d.contains("secret-mfa-key") && !d.contains(MFA_KEY), "{d}");
    }

    #[test]
    fn production_requires_a_32_byte_base64_mfa_key() {
        let base: &[(&str, &str)] = &[
            ("ARENA_OTP_PEPPER", PEPPER),
            ("ARENA_WEB_ORIGINS", "https://start.arena360.cloud"),
            ("ARENA_SMS_PROVIDER", "gupshup"),
            (
                "ARENA_GUPSHUP_ENDPOINT",
                "https://enterprise.smsgupshup.com/GatewayAPI/rest",
            ),
            ("ARENA_GUPSHUP_USERID", "2000000000"),
            ("ARENA_GUPSHUP_PASSWORD", "gs-pass"),
            ("ARENA_GUPSHUP_SENDER", "ARENA"),
            ("ARENA_GUPSHUP_DLT_TEMPLATE_ID", "1007000000000000000"),
        ];
        let with = |key: Option<&'static str>| {
            let mut v = with_licence(base).to_vec();
            if let Some(k) = key {
                v.push(("ARENA_MFA_KEY", k));
            }
            let pairs: &'static [(&'static str, &'static str)] = Box::leak(v.into());
            auth_from_env(false, &env_of(pairs))
        };
        assert!(with(None).unwrap_err().contains("ARENA_MFA_KEY is not set"));
        let short = with(Some("c2hvcnQ=")).unwrap_err();
        assert!(
            short.contains("ARENA_MFA_KEY") && short.contains("32 bytes"),
            "{short}"
        );
        assert!(with(Some("%%%")).unwrap_err().contains("base64"));
        assert!(with(Some(MFA_KEY)).is_ok());
        // `--dev` uses the fixed key and ignores the variable.
        assert_eq!(
            auth_from_env(true, &env_of(&[("ARENA_MFA_KEY", "c2hvcnQ=")]))
                .unwrap()
                .mfa_key,
            AuthConfig::dev().mfa_key
        );
    }

    #[test]
    fn production_requires_a_licence_key_that_is_not_the_dev_key() {
        let base: Vec<(&'static str, &'static str)> = vec![
            ("ARENA_OTP_PEPPER", PEPPER),
            ("ARENA_WEB_ORIGINS", "https://start.arena360.cloud"),
            ("ARENA_SMS_PROVIDER", "gupshup"),
            (
                "ARENA_GUPSHUP_ENDPOINT",
                "https://enterprise.smsgupshup.com/GatewayAPI/rest",
            ),
            ("ARENA_GUPSHUP_USERID", "2000000000"),
            ("ARENA_GUPSHUP_PASSWORD", "gs-pass"),
            ("ARENA_GUPSHUP_SENDER", "ARENA"),
            ("ARENA_GUPSHUP_DLT_TEMPLATE_ID", "1007000000000000000"),
            ("ARENA_MFA_KEY", MFA_KEY),
        ];
        let with = |extra: Vec<(&'static str, &'static str)>| {
            let mut v = base.clone();
            v.extend(extra);
            let pairs: &'static [(&'static str, &'static str)] = Box::leak(v.into());
            auth_from_env(false, &env_of(pairs))
        };
        let e = with(vec![]).unwrap_err();
        assert!(e.contains("ARENA_LICENCE_SIGNING_KEY is not set"), "{e}");
        let pem = licence_pairs()[0].1;
        let e = with(vec![("ARENA_LICENCE_SIGNING_KEY", pem)]).unwrap_err();
        assert!(e.contains("ARENA_LICENCE_KID is not set"), "{e}");
        let e = with(vec![
            ("ARENA_LICENCE_SIGNING_KEY", "garbage"),
            ("ARENA_LICENCE_KID", "k1"),
        ])
        .unwrap_err();
        assert!(e.starts_with("ARENA_LICENCE_SIGNING_KEY"), "{e}");
        let e = with(vec![
            ("ARENA_LICENCE_SIGNING_KEY", pem),
            ("ARENA_LICENCE_KID", "k 1"),
        ])
        .unwrap_err();
        assert!(e.starts_with("ARENA_LICENCE_KID"), "{e}");
        let e = with(vec![
            (
                "ARENA_LICENCE_SIGNING_KEY",
                crate::licence::DEV_SIGNING_KEY_PEM,
            ),
            ("ARENA_LICENCE_KID", "k1"),
        ])
        .unwrap_err();
        assert!(e.contains("dev key"), "{e}");
        assert!(!e.contains("MC4C"), "{e}");
        let mut on = licence_pairs();
        on.push(("ARENA_BILLING_ENABLED", "true"));
        let e = with(on).unwrap_err();
        assert!(e.contains("billing is not available in this build"), "{e}");
        let e = auth_from_env(true, &env_of(&[("ARENA_BILLING_ENABLED", "true")])).unwrap_err();
        assert!(e.contains("billing is not available in this build"), "{e}");
        let mut off = licence_pairs();
        off.push(("ARENA_BILLING_ENABLED", "false"));
        assert!(!with(off).unwrap().billing_enabled);
        let mut bad = licence_pairs();
        bad.push(("ARENA_BILLING_ENABLED", "yes"));
        assert!(with(bad).unwrap_err().contains("ARENA_BILLING_ENABLED"));
        let dev = auth_from_env(true, &env_of(&[])).unwrap();
        assert!(dev.licence.is_dev_key() && !dev.billing_enabled);
    }
}

#[cfg(test)]
mod ops_tests {
    use super::*;

    fn ops(args: &[&str]) -> Result<Command, String> {
        parse(
            std::iter::once("ops")
                .chain(args.iter().copied())
                .map(OsString::from),
            |_| None,
        )
    }

    const GOOD: &[&str] = &[
        "grant-comp",
        "--branch",
        "01JB",
        "--tier",
        "premium",
        "--until",
        "2026-12-31",
        "--operator",
        "harish",
        "--reason",
        "beta café",
    ];

    #[test]
    fn the_end_of_an_ist_day_is_one_ms_before_the_next_ist_midnight() {
        let end = end_of_ist_day("2026-12-31").unwrap();
        let next_utc = chrono::NaiveDate::from_ymd_opt(2027, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis();
        assert_eq!(end, next_utc - IST_OFFSET_MS - 1);
        for bad in [
            "2026-02-30",
            "2026-1-01",
            "26-01-01",
            "2026-01-01x",
            "2026/01/01",
            "+026-01-01",
        ] {
            assert_eq!(end_of_ist_day(bad), None, "{bad}");
        }
    }

    #[test]
    fn grant_comp_parses_and_refuses_usage_errors() {
        let Command::GrantComp(g) = ops(GOOD).unwrap() else {
            panic!("not grant-comp")
        };
        assert_eq!(g.branch_id, "01JB");
        assert_eq!(g.tier, arena_proto::Tier::Premium);
        assert_eq!(g.until_ms, end_of_ist_day("2026-12-31").unwrap());
        assert_eq!(
            (g.operator.as_str(), g.reason.as_str()),
            ("harish", "beta café")
        );
        assert_eq!(g.dev_data_dir, None);
        let mut dev = GOOD.to_vec();
        dev.extend(["--dev", "--data-dir", "/tmp/x"]);
        let Command::GrantComp(g) = ops(&dev).unwrap() else {
            panic!("not grant-comp")
        };
        assert_eq!(g.dev_data_dir, Some(PathBuf::from("/tmp/x")));
        let replace = |flag: &str, value: &'static str| {
            let mut v = GOOD.to_vec();
            let i = v.iter().position(|a| *a == flag).unwrap();
            v[i + 1] = value;
            ops(&v)
        };
        assert!(
            replace("--tier", "free")
                .unwrap_err()
                .contains("basic or premium")
        );
        assert!(
            replace("--until", "31-12-2026")
                .unwrap_err()
                .contains("YYYY-MM-DD")
        );
        assert!(replace("--operator", " ").is_err());
        let without = |flag: &str| {
            let mut all = vec!["grant-comp"];
            all.extend(
                GOOD[1..]
                    .chunks(2)
                    .filter(|c| c[0] != flag)
                    .flatten()
                    .copied(),
            );
            ops(&all)
        };
        for flag in ["--branch", "--tier", "--until", "--operator", "--reason"] {
            assert!(without(flag).unwrap_err().contains(flag), "{flag}");
        }
        let mut stray = GOOD.to_vec();
        stray.push("--force");
        assert!(ops(&stray).unwrap_err().contains("unknown argument"));
        let mut dir = GOOD.to_vec();
        dir.extend(["--data-dir", "/tmp/x"]);
        assert!(ops(&dir).unwrap_err().contains("--data-dir needs --dev"));
    }

    #[test]
    fn catalog_import_parses_a_file_an_operator_and_a_media_folder() {
        let cat = |args: &[&str]| {
            parse(
                std::iter::once("catalog")
                    .chain(args.iter().copied())
                    .map(OsString::from),
                |_| None,
            )
        };
        let Command::CatalogImport(c) =
            cat(&["import", "lists/games.csv", "--operator", "harish"]).unwrap()
        else {
            panic!("not catalog import")
        };
        assert_eq!(c.file, PathBuf::from("lists/games.csv"));
        assert_eq!(c.media_dir, PathBuf::from("lists"));
        assert_eq!(
            (c.operator.as_str(), c.dev_data_dir.clone()),
            ("harish", None)
        );
        let Command::CatalogImport(c) = cat(&[
            "import",
            "games.json",
            "--operator",
            "h",
            "--media-dir",
            "art",
            "--dev",
        ])
        .unwrap() else {
            panic!("not catalog import")
        };
        assert_eq!(c.media_dir, PathBuf::from("art"));
        assert_eq!(c.dev_data_dir, Some(PathBuf::from(DEFAULT_DATA_DIR)));
        assert_eq!(c.file, PathBuf::from("games.json"));
        assert!(
            cat(&["import", "--operator", "h"])
                .unwrap_err()
                .contains("needs a file")
        );
        assert!(
            cat(&["import", "g.json"])
                .unwrap_err()
                .contains("--operator")
        );
        assert!(cat(&["import", "a.json", "b.json", "--operator", "h"]).is_err());
        assert!(
            cat(&["export"])
                .unwrap_err()
                .contains("unknown catalog command")
        );
        assert!(cat(&["import", "g.json", "--operator", "h", "--data-dir", "x"]).is_err());
    }

    #[test]
    fn licence_keygen_parses_an_optional_kid() {
        assert_eq!(
            ops(&["licence-keygen"]).unwrap(),
            Command::LicenceKeygen { kid: None }
        );
        assert_eq!(
            ops(&["licence-keygen", "--kid", "prod-2026"]).unwrap(),
            Command::LicenceKeygen {
                kid: Some("prod-2026".into())
            }
        );
        assert!(ops(&["licence-keygen", "--kid", "a b"]).is_err());
        assert!(ops(&["licence-keygen", "--out", "x"]).is_err());
        assert!(ops(&["nope"]).unwrap_err().contains("unknown ops command"));
    }
}
