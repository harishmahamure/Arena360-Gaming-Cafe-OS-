//! `arena-cloud`: the cloud control plane, one binary run per role (`--role control` in Story
//! 2.1).
//!
//! The control role connects to `gz_control` (Turso, Mumbai) through `arena-store`'s `control`
//! module, applies its embedded migrations and serves `GET /healthz`.

pub mod config;
pub mod health;

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use arena_store::control::{ControlDb, ControlError, SchemaTooNew};
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::config::Config;
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
}

/// The longest a single connect-and-migrate attempt may take.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);
const FIRST_BACKOFF: Duration = Duration::from_millis(500);
const MAX_BACKOFF: Duration = Duration::from_secs(8);

/// Connects to `gz_control` and migrates it, retrying with exponential backoff until
/// `cfg.retry_window` has passed. `SCHEMA_TOO_NEW` is never retried.
pub async fn connect_and_migrate(cfg: &Config) -> Result<(ControlDb, u32), StartError> {
    let deadline = Instant::now() + cfg.retry_window;
    let mut backoff = FIRST_BACKOFF;
    let mut attempts = 0;
    loop {
        attempts += 1;
        let left = deadline.saturating_duration_since(Instant::now());
        let attempt = async {
            let db = ControlDb::connect(&cfg.db_url, &cfg.db_token).await?;
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

/// Serves the control role on `listener` until `shutdown` resolves: `/healthz`, with the
/// reachability check running every `check_interval` (production: [`CHECK_INTERVAL`]) in the
/// background, the first at once.
pub async fn serve_control(
    listener: TcpListener,
    db: ControlDb,
    migration_version: u32,
    check_interval: Duration,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
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
    let app = health::router(config::Role::Control, rx, migration_version);
    let served = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await;
    check.abort();
    served
}
