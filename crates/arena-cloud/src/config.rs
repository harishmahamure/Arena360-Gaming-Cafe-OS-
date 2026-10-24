//! Command-line arguments and environment for `arena-cloud`.

use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

pub const USAGE: &str =
    "usage: arena-cloud --role control|all [--listen <addr:port>] [--help] [--version]
       arena-cloud --role control|all --dev [--data-dir <dir>] [--listen <loopback addr:port>]
       arena-cloud --role jobs --task control-backup [--dev [--data-dir <dir>]]
       arena-cloud ops restore-control (--from-key <key> | --from-file <path|->)
                                       (--identity <file> | --plaintext) [--dev [--data-dir <dir>]]

  --role all   every serving role this build has (control)
  --role jobs  run one --task and exit (0 on success, 1 on failure)
  --task       control-backup: dump gz_control, age-encrypt it, put it in the backup bucket
               under gz_control/nightly/ and keep the newest 30
  --dev        local dev mode: gz_control is <data-dir>/gz_control.db (SQLite), no env or
               token needed, loopback only (default 127.0.0.1:8080), dev routes under /v1/dev
  --data-dir   dev data directory, created if missing (default ./.arena-cloud-dev)

  ops restore-control  restore a dump into an EMPTY gz_control and verify every table's row
               count; --from-key reads the backup bucket, --from-file a file (- for stdin);
               --identity decrypts with an age identity file, --plaintext reads a decrypted dump

environment (not with --dev):
  ARENA_CONTROL_DB_URL    gz_control URL (libsql:// or https://)
  ARENA_CONTROL_DB_TOKEN  gz_control auth token
environment (--task control-backup, ops restore-control --from-key):
  ARENA_BACKUP_S3_ENDPOINT, ARENA_BACKUP_S3_BUCKET, ARENA_BACKUP_S3_ACCESS_KEY_ID,
  ARENA_BACKUP_S3_SECRET_ACCESS_KEY  the backup bucket (R2; MinIO in dev)
  ARENA_BACKUP_AGE_RECIPIENTS        comma-separated age X25519 public keys (control-backup)
  ARENA_ALERT_WEBHOOK_URL            optional: Slack-compatible webhook POSTed on failure
  ARENA_ENV                          optional: environment name in the alert";

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
    /// Run one [`Task`] and exit.
    Jobs,
    /// Every serving role this build has (only `control` today).
    All,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Control => "control",
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

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Run(Config),
    Job(JobConfig),
    RestoreControl(RestoreConfig),
    Help,
    Version,
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
        None => return Err("--role is required (control, jobs or all)".to_owned()),
        Some("control") => Role::Control,
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
        Some("--help" | "-h") => return Ok(Command::Help),
        Some(other) => return Err(format!("unknown ops command {other}")),
        None => return Err("ops needs a command (restore-control)".to_owned()),
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
