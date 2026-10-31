//! `arena-cloud`: the cloud control plane, one binary run per role (`--role control`, or
//! `--role all` for every role this build has).
//!
//! The control role connects to `gz_control` (Turso, Mumbai) through `arena-store`'s `control`
//! module, applies its embedded migrations and serves `GET /healthz` and `/v1` (every failure
//! as the flat `AppError` envelope, see [`api`]). `--dev` runs `gz_control` from a local SQLite
//! file instead and mounts the dev-only routes. `--role jobs --task control-backup` runs the
//! nightly backup once and exits, and `ops restore-control` restores one ([`backup`]).
//!
//! The serving roles also run owner sign-up with a phone one-time code and the owner's cookie
//! session ([`auth`], [`api::signup`], [`api::session`]), sending codes through the
//! [`messaging`] adapter, and account setup: a password ([`passwords`], [`api::account`]), an
//! authenticator app ([`totp`], its secret sealed by [`crypto`]) and backup codes
//! ([`api::mfa`]), with [`api::gate::RequireSetupComplete`] for routes that need all three.

pub mod admin;
pub mod alert;
pub mod api;
pub mod audit_build;
pub mod audit_query;
pub mod auth;
pub mod backup;
pub mod billing;
pub mod branch_code;
pub mod catalog;
pub mod client_ip;
pub mod command_sweep;
pub mod config;
pub mod crypto;
pub mod data_age;
pub mod gaps;
pub mod gstin;
pub mod health;
pub mod key_release;
pub mod licence;
pub mod messaging;
pub mod offline_watch;
pub mod ops;
pub mod owner_alerts;
pub mod owner_read;
pub mod pairing_code;
pub mod passwords;
pub mod presign;
pub mod price_book;
pub mod push;
pub mod reconcile;
pub mod retention;
pub mod rollups;
pub mod store;
pub mod subscription;
pub mod templates;
pub mod totp;
pub mod tunnel;
pub mod useragent;
pub mod webhook;

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use arena_store::control::{ControlDb, ControlError, SchemaTooNew};
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::auth::AuthState;
use crate::billing::{BillingProvider, FakeRazorpay, provider_from_env};
use crate::config::{AuthConfig, Config, Database, Role, SmsProviderConfig};
pub use crate::health::CHECK_INTERVAL;
use crate::health::{CHECK_TIMEOUT, Reachability};
use crate::messaging::{FakeNotifier, FakeSms, GupshupSms, Messenger, Notifier};
use crate::push::{FakePush, WebPush};
use crate::tunnel::{FakeTunnels, TunnelProvider};
use crate::webhook::FAKE_WEBHOOK_SECRET;

/// A source of Unix milliseconds (tests move it by hand).
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// The wall clock as a [`Clock`].
pub fn system_clock() -> Clock {
    Arc::new(now_ms)
}

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
            Ok(Err(other)) => other.to_string(),
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
        Err(other) => Err(local(&path, other.to_string())),
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

/// [`app`] plus the owner sign-up and session routes (and their CORS layer) when `auth` is
/// set.
pub fn app_with_auth(
    role: Role,
    dev: bool,
    reach: watch::Receiver<Reachability>,
    migration_version: u32,
    auth: Option<Arc<AuthState>>,
) -> axum::Router {
    let base = health::router(role, reach, migration_version);
    match role {
        Role::Command => api::with_envelope_commands(base, dev, auth),
        _ => api::with_envelope_and_auth(base, dev, auth),
    }
}

/// The sign-up state for `cfg`: the configured SMS provider over `db`, timed by `clock`.
pub fn auth_state(
    db: Arc<ControlDb>,
    cfg: &AuthConfig,
    clock: Clock,
) -> Result<Arc<AuthState>, String> {
    let sms = match &cfg.sms {
        SmsProviderConfig::Fake => Messenger::Fake(FakeSms::new(Arc::clone(&clock))),
        SmsProviderConfig::Gupshup(g) => {
            Messenger::Gupshup(GupshupSms::new(g.clone()).map_err(|e| e.to_string())?)
        }
    };
    let notifier = match &cfg.sms {
        SmsProviderConfig::Fake => Some(Notifier::Fake(FakeNotifier::new(Arc::clone(&clock)))),
        SmsProviderConfig::Gupshup(g) => match Notifier::from_env(g, &|k| std::env::var_os(k)) {
            Ok(n) => Some(n),
            Err(e) => {
                tracing::warn!(error = %e, "no messaging adapter: /v1/notify answers 503");
                None
            }
        },
    };
    let push = match &cfg.sms {
        SmsProviderConfig::Fake => Some(WebPush::Fake(FakePush::new(Arc::clone(&clock)))),
        SmsProviderConfig::Gupshup(_) => {
            match WebPush::from_env(&|k| std::env::var_os(k), Arc::clone(&clock)) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(error = %e, "no web push: the push routes answer 503");
                    None
                }
            }
        }
    };
    let tunnels = match &cfg.sms {
        SmsProviderConfig::Fake => Some(TunnelProvider::Fake(FakeTunnels::new())),
        SmsProviderConfig::Gupshup(_) => match TunnelProvider::from_env(&|k| std::env::var_os(k)) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "no tunnel provider: branches get no tunnel");
                None
            }
        },
    };
    let billing = match &cfg.sms {
        SmsProviderConfig::Fake => Some(BillingProvider::Fake(FakeRazorpay::new())),
        SmsProviderConfig::Gupshup(_) => match provider_from_env(false, &|k| std::env::var_os(k)) {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::warn!(error = %e, "no billing provider: subscriptions answer 503");
                None
            }
        },
    };
    let webhook_secret = match &cfg.sms {
        SmsProviderConfig::Fake => crate::webhook::WebhookSecret::new(FAKE_WEBHOOK_SECRET).ok(),
        SmsProviderConfig::Gupshup(_) => {
            match crate::webhook::WebhookSecret::from_env(&|k| std::env::var_os(k)) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, "no webhook secret: the Razorpay webhook answers 503");
                    None
                }
            }
        }
    };
    let st = Arc::new(AuthState::new(db, sms, clock, cfg));
    if let Some(b) = billing {
        let _ = st.billing.set(b);
    }
    if let Some(s) = webhook_secret {
        let _ = st.webhook_secret.set(s);
    }
    if let Some(t) = tunnels {
        let _ = st.tunnels.set(t);
    }
    if let Some(n) = notifier {
        let _ = st.notifier.set(n);
    }
    if let Some(p) = push {
        let _ = st.push.set(p);
    }
    Ok(st)
}

/// How [`serve`] runs.
#[derive(Debug, Clone)]
pub struct ServeOptions {
    pub role: Role,
    /// Mount the dev-only routes (`--dev`).
    pub dev: bool,
    pub migration_version: u32,
    /// The reachability check interval (production: [`CHECK_INTERVAL`]).
    pub check_interval: Duration,
    /// Owner sign-up and sessions; `None` serves only `/healthz` and the envelope.
    pub auth: Option<AuthConfig>,
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
        auth: None,
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
        auth,
    } = opts;
    // Not checked yet; the loop's first tick runs immediately.
    let (tx, rx) = watch::channel(Reachability {
        reachable: false,
        checked_at: None,
    });
    let db = Arc::new(db);
    let worker = if role == Role::Builder {
        let data = presign::Presigner::from_env(dev, &|k| std::env::var_os(k))
            .map_err(|e| std::io::Error::other(format!("data bucket: {e}")))?
            .ok_or_else(|| {
                std::io::Error::other("the builder needs the data bucket (ARENA_DATA_S3_*)")
            })?;
        let scratch = std::env::var_os("ARENA_BUILDER_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let builder = Arc::new(audit_build::AuditBuilder::new(
            Arc::clone(&db),
            data,
            system_clock(),
            scratch,
        ));
        Some(tokio::spawn(audit_build::run_worker(builder)))
    } else {
        None
    };
    let auth = match auth {
        Some(cfg) => Some(
            auth_state(Arc::clone(&db), &cfg, system_clock())
                .map_err(|e| std::io::Error::other(format!("sms provider: {e}")))?,
        ),
        None => None,
    };
    if let Some(st) = &auth {
        let store = if dev {
            Some(templates::TemplateStore::memory())
        } else {
            templates::TemplateStore::from_env(&|k| std::env::var_os(k))
                .map_err(|e| std::io::Error::other(format!("setup template store: {e}")))?
        };
        match store {
            Some(store) => {
                let _ = st.templates.set(store);
            }
            None => tracing::warn!("no object store configured: setup templates are not kept"),
        }
        match presign::Presigner::from_env(dev, &|k| std::env::var_os(k))
            .map_err(|e| std::io::Error::other(format!("data bucket: {e}")))?
        {
            Some(presigner) => {
                let _ = st.presigner.set(presigner);
            }
            None => tracing::warn!("no data bucket configured: /v1/presign answers 503"),
        }
        let cache_dir = std::env::var_os("ARENA_AUDIT_CACHE_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("arena-audit-cache"));
        let cache_max = std::env::var("ARENA_AUDIT_CACHE_MAX_BYTES")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(arena_proto::AUDIT_LOCAL_CACHE_BYTES);
        let _ = st
            .audit_cache
            .set(audit_query::AuditCache::new(cache_dir, cache_max));
        let admin = admin::AdminTokens::from_env(&|k| std::env::var_os(k))
            .map_err(|e| std::io::Error::other(format!("admin credentials: {e}")))?;
        if admin.is_empty() {
            tracing::info!("no staff credentials: the admin audit API is off");
        }
        let _ = st.admin_tokens.set(admin);
        match config::catalog_cdn_from_env(dev, &|k| std::env::var_os(k)) {
            Some(base) => {
                let _ = st.catalog_cdn.set(base);
            }
            None => tracing::warn!(
                "{} is not set: the catalogue manifest answers 503",
                config::CATALOG_CDN_ENV
            ),
        }
    }
    let check_db = Arc::clone(&db);
    let check = tokio::spawn(health::reachability_loop(
        tx,
        check_interval,
        CHECK_TIMEOUT,
        now_ms,
        move || {
            let db = Arc::clone(&check_db);
            async move { db.ping().await.map_err(|e| e.to_string()) }
        },
    ));
    let app = app_with_auth(role, dev, rx, migration_version, auth);
    let served = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await;
    check.abort();
    if let Some(worker) = worker {
        worker.abort();
    }
    served
}
