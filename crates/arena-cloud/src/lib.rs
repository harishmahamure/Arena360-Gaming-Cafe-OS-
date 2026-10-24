//! `arena-cloud`: the cloud control plane, one binary run per role (`--role control`, or
//! `--role all` for every role this build has).
//!
//! The control role connects to `gz_control` (Turso, Mumbai) through `arena-store`'s `control`
//! module, applies its embedded migrations and serves `GET /healthz` and `/v1` (every failure
//! as the flat `AppError` envelope, see [`api`]). `--dev` runs `gz_control` from a local SQLite
//! file instead and mounts the dev-only routes.

pub mod api;
pub mod config;
pub mod health;

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use arena_store::control::{ControlDb, ControlError, SchemaTooNew};
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::config::{Config, Database, Role};
pub use crate::health::CHECK_INTERVAL;
use crate::health::{CHECK_TIMEOUT, Reachability};

/// Unix time in milliseconds.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Why start-up stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    /// `gz_control` is newer than this binary; nothing was written.
    SchemaTooNew(SchemaTooNew),
    /// `gz_control` stayed unreachable (or failing) for the whole retry window.
    Unreachable { attempts: u32, last: String },
    /// Migration `version` failed with an SQL error; never retried.
    Migration { version: u32, message: String },
    /// `--dev`: the local data directory or database file could not be opened.
    Local { path: String, message: String },
}

/// The longest a single connect-and-migrate attempt may take.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);
const FIRST_BACKOFF: Duration = Duration::from_millis(500);
const MAX_BACKOFF: Duration = Duration::from_secs(8);

/// Connects to `gz_control` and migrates it, retrying with exponential backoff until
/// `cfg.retry_window` has passed. `SCHEMA_TOO_NEW` is never retried.
pub async fn connect_and_migrate(cfg: &Config) -> Result<(ControlDb, u32), StartError> {
    let (url, token) = match &cfg.database {
        Database::Remote { url, token } => (url.as_str(), token.as_str()),
        Database::Local { data_dir } => return open_local_and_migrate(data_dir).await,
    };
    let deadline = Instant::now() + cfg.retry_window;
    let mut backoff = FIRST_BACKOFF;
    let mut attempts = 0;
    loop {
        attempts += 1;
        let left = deadline.saturating_duration_since(Instant::now());
        let attempt = async {
            let db = ControlDb::connect(url, token).await?;
            let version = db.migrate(now_ms()).await?;
            Ok::<_, ControlError>((db, version))
        };
        let last = match tokio::time::timeout(ATTEMPT_TIMEOUT.min(left.max(FIRST_BACKOFF)), attempt)
            .await
        {
            Ok(Ok(ok)) => return Ok(ok),
            Ok(Err(ControlError::SchemaTooNew(e))) => return Err(StartError::SchemaTooNew(e)),
            Ok(Err(ControlError::Migration { version, message })) => {
                return Err(StartError::Migration { version, message });
            }
            Ok(Err(ControlError::Db(m))) => m,
            Err(_) => "timed out".to_owned(),
        };
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(StartError::Unreachable { attempts, last });
        }
        tracing::warn!(
            attempt = attempts,
            retry_in_ms = backoff.min(left).as_millis() as u64,
            detail = %last,
            "gz_control not ready, retrying"
        );
        tokio::time::sleep(backoff.min(left)).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// `--dev`: creates `data_dir` when missing, opens `<data_dir>/gz_control.db` and migrates it
/// with the same runner and migrations. Not retried: a local file has no outage to wait out.
async fn open_local_and_migrate(
    data_dir: &std::path::Path,
) -> Result<(ControlDb, u32), StartError> {
    let local = |path: &std::path::Path, message: String| StartError::Local {
        path: path.display().to_string(),
        message,
    };
    std::fs::create_dir_all(data_dir).map_err(|e| local(data_dir, e.to_string()))?;
    let path = data_dir.join(config::DEV_DB_FILE);
    let db = ControlDb::open_local(&path)
        .await
        .map_err(|e| local(&path, e.to_string()))?;
    match db.migrate(now_ms()).await {
        Ok(version) => Ok((db, version)),
        Err(ControlError::SchemaTooNew(e)) => Err(StartError::SchemaTooNew(e)),
        Err(ControlError::Migration { version, message }) => {
            Err(StartError::Migration { version, message })
        }
        Err(ControlError::Db(m)) => Err(local(&path, m)),
    }
}

/// The whole HTTP app: `/healthz` and `/v1` inside the error envelope. The dev-only routes are
/// mounted only when `dev` is set.
pub fn app(
    role: Role,
    dev: bool,
    reach: watch::Receiver<Reachability>,
    migration_version: u32,
) -> axum::Router {
    api::with_envelope(health::router(role, reach, migration_version), dev)
}

/// How [`serve`] runs.
#[derive(Debug, Clone, Copy)]
pub struct ServeOptions {
    pub role: Role,
    /// Mount the dev-only routes (`--dev`).
    pub dev: bool,
    pub migration_version: u32,
    /// The reachability check interval (production: [`CHECK_INTERVAL`]).
    pub check_interval: Duration,
}

/// Serves the control role on `listener` until `shutdown` resolves: `/healthz`, with the
/// reachability check running every `check_interval` (production: [`CHECK_INTERVAL`]) in the
/// background, the first at once, and `/v1`. Same as [`serve`] with `role: control` and no dev
/// routes.
pub async fn serve_control(
    listener: TcpListener,
    db: ControlDb,
    migration_version: u32,
    check_interval: Duration,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let opts = ServeOptions {
        role: Role::Control,
        dev: false,
        migration_version,
        check_interval,
    };
    serve(listener, db, opts, shutdown).await
}

/// Serves [`app`] on `listener` until `shutdown` resolves, with the background reachability
/// check every `opts.check_interval`, the first at once.
pub async fn serve(
    listener: TcpListener,
    db: ControlDb,
    opts: ServeOptions,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let ServeOptions {
        role,
        dev,
        migration_version,
        check_interval,
    } = opts;
    // Not checked yet; the loop's first tick runs immediately.
    let (tx, rx) = watch::channel(Reachability {
        reachable: false,
        checked_at: None,
    });
    let db = Arc::new(db);
    let check = tokio::spawn(health::reachability_loop(
        tx,
        check_interval,
        CHECK_TIMEOUT,
        now_ms,
        move || {
            let db = Arc::clone(&db);
            async move { db.ping().await.map_err(|e| e.to_string()) }
        },
    ));
    let app = app(role, dev, rx, migration_version);
    let served = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await;
    check.abort();
    served
}
