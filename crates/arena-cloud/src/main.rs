//! `arena-cloud`: the cloud binary.
//!
//! Start-up: parse args and env (exit 64 on a usage error), JSON logs to stdout, connect to
//! `gz_control` and migrate it (exit 2 on `SCHEMA_TOO_NEW`, exit 1 after 60 s unreachable), then
//! serve `/healthz` until Ctrl+C or SIGTERM. The database token never appears in a log line.

use std::process::ExitCode;

use arena_cloud::config::{self, Command};
use arena_cloud::{CHECK_INTERVAL, StartError, connect_and_migrate, serve_control};
use tracing_subscriber::EnvFilter;

const EXIT_FAILURE: u8 = 1;
const EXIT_SCHEMA_TOO_NEW: u8 = 2;
const EXIT_USAGE: u8 = 64;

#[tokio::main]
async fn main() -> ExitCode {
    let cfg = match config::parse(std::env::args_os().skip(1), |k| std::env::var_os(k)) {
        Ok(Command::Run(cfg)) => cfg,
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

    tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stdout)
        .init();

    tracing::info!(
        role = cfg.role.as_str(),
        version = env!("CARGO_PKG_VERSION"),
        listen = %cfg.listen,
        "starting"
    );

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
    };
    tracing::info!(migration_version, "gz_control ready");

    let listener = match tokio::net::TcpListener::bind(cfg.listen).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(listen = %cfg.listen, detail = %e, "cannot listen");
            return ExitCode::from(EXIT_FAILURE);
        }
    };
    match serve_control(listener, db, migration_version, CHECK_INTERVAL, shutdown()).await {
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
