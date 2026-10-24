//! Command-line arguments and environment for `arena-cloud`.

use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

pub const USAGE: &str =
    "usage: arena-cloud --role control|all [--listen <addr:port>] [--help] [--version]
       arena-cloud --role control|all --dev [--data-dir <dir>] [--listen <loopback addr:port>]

  --role all   every role this build has (control)
  --dev        local dev mode: gz_control is <data-dir>/gz_control.db (SQLite), no env or
               token needed, loopback only (default 127.0.0.1:8080), dev routes under /v1/dev
  --data-dir   dev data directory, created if missing (default ./.arena-cloud-dev)

environment (not with --dev):
  ARENA_CONTROL_DB_URL    gz_control URL (libsql:// or https://)
  ARENA_CONTROL_DB_TOKEN  gz_control auth token";

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
    /// Every role this build has (only `control` today).
    All,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Control => "control",
            Self::All => "all",
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

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Run(Config),
    Help,
    Version,
}

/// Is `url` a plain-HTTP loopback URL (`http://127.0.0.1…` or `http://localhost…`)?
fn is_loopback_http(url: &str) -> bool {
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

/// Parses `args` (without the program name). `env` reads an environment variable.
pub fn parse(
    args: impl IntoIterator<Item = OsString>,
    env: impl Fn(&str) -> Option<OsString>,
) -> Result<Command, String> {
    let mut role: Option<String> = None;
    let mut listen: Option<SocketAddr> = None;
    let mut dev = false;
    let mut data_dir: Option<PathBuf> = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let arg = arg
            .into_string()
            .map_err(|a| format!("invalid argument {a:?}"))?;
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_owned(), Some(OsString::from(v))),
            _ => (arg.clone(), None),
        };
        let mut value = || {
            let v = inline
                .clone()
                .or_else(|| args.next())
                .ok_or_else(|| format!("{flag} needs a value"))?;
            if v.is_empty() || v.to_string_lossy().starts_with("--") {
                return Err(format!("{flag} needs a value"));
            }
            v.into_string().map_err(|v| format!("invalid {flag} {v:?}"))
        };
        match flag.as_str() {
            "--help" | "-h" => return Ok(Command::Help),
            "--version" => return Ok(Command::Version),
            "--role" => role = Some(value()?),
            "--listen" => {
                let v = value()?;
                listen = Some(v.parse().map_err(|_| format!("invalid --listen {v:?}"))?);
            }
            "--dev" if inline.is_none() => dev = true,
            "--data-dir" => data_dir = Some(PathBuf::from(value()?)),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let role = match role.as_deref() {
        None => return Err("--role is required (control or all)".to_owned()),
        Some("control") => Role::Control,
        Some("all") => Role::All,
        Some(other) => return Err(format!("role not available yet: {other}")),
    };

    // Trimmed: YAML block scalars and `echo` often leave a trailing newline or spaces.
    let get = |k: &str| {
        env(k)
            .and_then(|v| v.into_string().ok())
            .map(|v| v.trim().to_owned())
    };
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
    if data_dir.is_some() {
        return Err("--data-dir needs --dev".to_owned());
    }
    let listen = listen.unwrap_or_else(|| {
        DEFAULT_LISTEN
            .parse()
            .expect("valid default listen address")
    });

    let retry_window = match get(DB_RETRY_SECS_ENV).filter(|v| !v.is_empty()) {
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

    let db_url = get(DB_URL_ENV)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("{DB_URL_ENV} is not set"))?;
    let loopback = is_loopback_http(&db_url);
    if !(db_url.starts_with("libsql://") || db_url.starts_with("https://") || loopback) {
        return Err(format!("{DB_URL_ENV} must be a libsql:// or https:// URL"));
    }
    let db_token = get(DB_TOKEN_ENV).unwrap_or_default();
    if db_token.is_empty() && !loopback {
        return Err(format!("{DB_TOKEN_ENV} is not set"));
    }
    Ok(Command::Run(Config {
        role,
        listen,
        database: Database::Remote {
            url: db_url,
            token: db_token,
        },
        retry_window,
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
