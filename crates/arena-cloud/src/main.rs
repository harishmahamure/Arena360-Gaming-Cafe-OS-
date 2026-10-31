//! `arena-cloud`: the cloud binary.
//!
//! Start-up: parse args and env (exit 64 on a usage error), JSON logs to stdout, connect to
//! `gz_control` and migrate it (exit 2 on `SCHEMA_TOO_NEW`, exit 1 after 60 s unreachable), then
//! serve `/healthz` and `/v1` until Ctrl+C or SIGTERM. The database token never appears in a log
//! line. `--dev` uses a local SQLite file instead (no env, loopback only) and says so in one
//! warning line.
//!
//! `--role jobs --task control-backup` runs the nightly backup once: exit 0 on success, 1 on
//! any failure (logged, and alerted when a webhook is set). `ops restore-control` restores a
//! backup into an empty `gz_control` and prints the verified row counts (exit 0) or the reason
//! (exit 1).

use std::process::ExitCode;

use arena_cloud::backup;
use arena_cloud::config::{self, Command};
use arena_cloud::{CHECK_INTERVAL, ServeOptions, StartError, connect_and_migrate, serve};
use tracing_subscriber::EnvFilter;

const EXIT_FAILURE: u8 = 1;
const EXIT_SCHEMA_TOO_NEW: u8 = 2;
const EXIT_USAGE: u8 = 64;

#[tokio::main]
async fn main() -> ExitCode {
    let cfg = match config::parse(std::env::args_os().skip(1), |k| std::env::var_os(k)) {
        Ok(Command::Run(cfg)) => cfg,
        Ok(Command::Job(job)) => {
            init_logging();
            tracing::info!(
                role = "jobs",
                task = job.task.as_str(),
                version = env!("CARGO_PKG_VERSION"),
                "starting"
            );
            if job.dev_data_dir.is_some() {
                tracing::warn!("dev mode is on: local gz_control, not for production");
            }
            let ok = backup::run_job(&job, |k| std::env::var_os(k)).await;
            return ExitCode::from(if ok { 0 } else { EXIT_FAILURE });
        }
        Ok(Command::RestoreControl(restore)) => {
            return match backup::restore_control(&restore, |k| std::env::var_os(k)).await {
                Ok(report) => {
                    println!(
                        "restored gz_control at schema version {}; row counts verified:",
                        report.schema_version
                    );
                    let width = report.tables.keys().map(String::len).max().unwrap_or(0);
                    for (table, rows) in &report.tables {
                        println!("  {table:<width$}  {rows}");
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("arena-cloud: restore-control failed: {e}");
                    ExitCode::from(EXIT_FAILURE)
                }
            };
        }
        Ok(Command::GrantComp(grant)) => {
            return match arena_cloud::ops::grant_comp(
                &grant,
                |k| std::env::var_os(k),
                arena_cloud::now_ms(),
            )
            .await
            {
                Ok(summary) => {
                    for line in summary.lines() {
                        println!("{line}");
                    }
                    ExitCode::SUCCESS
                }
                Err(e @ arena_cloud::ops::OpsError::Config(_)) => {
                    eprintln!("arena-cloud: grant-comp: {e}");
                    ExitCode::from(EXIT_USAGE)
                }
                Err(e) => {
                    eprintln!("arena-cloud: grant-comp failed: {e}");
                    ExitCode::from(EXIT_FAILURE)
                }
            };
        }
        Ok(Command::PriceBook(book)) => {
            return match arena_cloud::ops::price_book(
                &book,
                |k| std::env::var_os(k),
                arena_cloud::now_ms(),
            )
            .await
            {
                Ok(lines) => {
                    for line in lines {
                        println!("{line}");
                    }
                    ExitCode::SUCCESS
                }
                Err(e @ arena_cloud::ops::OpsError::Config(_)) => {
                    eprintln!("arena-cloud: price-book: {e}");
                    ExitCode::from(EXIT_USAGE)
                }
                Err(e) => {
                    eprintln!("arena-cloud: price-book failed: {e}");
                    ExitCode::from(EXIT_FAILURE)
                }
            };
        }
        Ok(Command::CatalogImport(import)) => {
            return match arena_cloud::ops::catalog_import(
                &import,
                |k| std::env::var_os(k),
                arena_cloud::now_ms(),
            )
            .await
            {
                Ok(summary) => {
                    for line in summary.lines() {
                        println!("{line}");
                    }
                    ExitCode::SUCCESS
                }
                Err(e @ arena_cloud::ops::OpsError::Config(_)) => {
                    eprintln!("arena-cloud: catalog import: {e}");
                    ExitCode::from(EXIT_USAGE)
                }
                Err(e) => {
                    eprintln!("arena-cloud: catalog import failed: {e}");
                    ExitCode::from(EXIT_FAILURE)
                }
            };
        }
        Ok(Command::LicenceKeygen { kid }) => {
            return match arena_cloud::licence::generate(kid.as_deref()) {
                Ok(key) => {
                    print!("{}", key.pem);
                    eprintln!("ARENA_LICENCE_KID={}", key.kid);
                    eprintln!(
                        "VerifyKey {{ kid: \"{}\", public_key: \"{}\" }},",
                        key.kid, key.public_key
                    );
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("arena-cloud: licence-keygen failed: {e}");
                    ExitCode::from(EXIT_FAILURE)
                }
            };
        }
        Ok(Command::Help) => {
            println!("{}", config::USAGE);
            return ExitCode::SUCCESS;
        }
        Ok(Command::Version) => {
            println!("arena-cloud {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("arena-cloud: {e}");
            eprintln!("{}", config::USAGE);
            return ExitCode::from(EXIT_USAGE);
        }
    };

    // The builder serves no owner or Hub routes, so it needs no sign-up, SMS or licence settings.
    let auth = if cfg.role == config::Role::Builder {
        None
    } else {
        match config::auth_from_env(cfg.dev(), &|k| std::env::var_os(k)) {
            Ok(a) => Some(a),
            Err(e) => {
                eprintln!("arena-cloud: {e}");
                eprintln!("{}", config::USAGE);
                return ExitCode::from(EXIT_USAGE);
            }
        }
    };

    init_logging();
    // A handler panic is answered with `INTERNAL` by the API layer; the panic itself is logged
    // here as one JSON line instead of the default stderr text.
    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        tracing::error!(location = %location, detail = %msg, "panic");
    }));

    tracing::info!(
        role = cfg.role.as_str(),
        version = env!("CARGO_PKG_VERSION"),
        listen = %cfg.listen,
        sms_provider = match auth.as_ref().map(|a| &a.sms) {
            None => "none",
            Some(config::SmsProviderConfig::Fake) => "fake",
            Some(config::SmsProviderConfig::Gupshup(_)) => "gupshup",
        },
        web_origins = ?auth.as_ref().map(|a| &a.web_origins),
        "starting"
    );
    if let Some(auth) = auth.as_ref()
        && !cfg.dev()
        && !auth
            .licence
            .trusted_by(arena_proto::licence::LICENCE_VERIFY_KEYS)
    {
        tracing::error!(
            licence_kid = auth.licence.kid(),
            "the licence signing key is not in LICENCE_VERIFY_KEYS: Hubs won't trust the licences it signs"
        );
    }
    if let Some(path) = cfg.database.local_path() {
        tracing::warn!(
            db_path = %path.display(),
            "dev mode is on: local gz_control, dev routes mounted, not for production"
        );
    }

    let (db, migration_version) = match connect_and_migrate(&cfg).await {
        Ok(ok) => ok,
        Err(StartError::SchemaTooNew(e)) => {
            tracing::error!(
                code = "SCHEMA_TOO_NEW",
                db_schema_version = e.db,
                binary_schema_version = e.binary,
                "refusing to start: {e}"
            );
            return ExitCode::from(EXIT_SCHEMA_TOO_NEW);
        }
        Err(StartError::Unreachable { attempts, last }) => {
            tracing::error!(
                attempts,
                retry_window_s = cfg.retry_window.as_secs(),
                detail = %last,
                "gz_control unreachable, giving up"
            );
            return ExitCode::from(EXIT_FAILURE);
        }
        Err(StartError::Migration { version, message }) => {
            tracing::error!(
                migration_version = version,
                detail = %message,
                "gz_control migration {version} failed, refusing to start"
            );
            return ExitCode::from(EXIT_FAILURE);
        }
        Err(StartError::Local { path, message }) => {
            tracing::error!(db_path = %path, detail = %message, "cannot open the local gz_control");
            return ExitCode::from(EXIT_FAILURE);
        }
    };
    tracing::info!(migration_version, "gz_control ready");

    let listener = match tokio::net::TcpListener::bind(cfg.listen).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(listen = %cfg.listen, detail = %e, "cannot listen");
            return ExitCode::from(EXIT_FAILURE);
        }
    };
    let opts = ServeOptions {
        role: cfg.role,
        dev: cfg.dev(),
        migration_version,
        check_interval: CHECK_INTERVAL,
        auth,
    };
    match serve(listener, db, opts, shutdown()).await {
        Ok(()) => {
            tracing::info!("stopped");
            ExitCode::SUCCESS
        }
        Err(e) => {
            tracing::error!(detail = %e, "server failed");
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

/// JSON log lines on stdout (`RUST_LOG`, default `info`).
fn init_logging() {
    tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stdout)
        .init();
}

/// Resolves on Ctrl+C or (Unix) SIGTERM, which Kubernetes sends before killing the pod.
async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = term => {},
    }
}
