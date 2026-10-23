//! `arena-hub`: the branch Hub service.
//!
//! Start-up: parse args, init logs, open the store (exit 2 on `SCHEMA_TOO_NEW`), load or mint
//! the instance id, load or generate the LAN TLS certificate, start the lease task, `/healthz` + `/ui/snapshot` + the staff auth routes
//! and the LAN listeners (`:7400`, `:7401`), then run until Ctrl+C or SIGTERM.
//!
//! Logs carry ids, codes and counts only — no PII (see `logs.rs`).

mod config;
#[cfg(feature = "dev-fixtures")]
mod dev;
mod logs;

use std::process::ExitCode;
use std::sync::Arc;

use std::net::{IpAddr, Ipv4Addr};

use arena_api::lan::{LanHub, bind_lan, serve_lan, tls};
use arena_api::{bind_health, serve_router};
use arena_core::{
    Clock, DeviceDirectory, HubStatus, Role, SnapshotReader, StaffDirectory, SystemClock,
    load_or_mint_instance_id, record_tls_fingerprint, status_channel,
};
use arena_proto::{AppError, ErrorCode};
use arena_store::{LATEST_SCHEMA_VERSION, Store};
use arena_tasks::{HousekeepingTask, LeaseTask};
use tokio::sync::oneshot;
use tracing_subscriber::EnvFilter;

const EXIT_FAILURE: u8 = 1;
const EXIT_SCHEMA_TOO_NEW: u8 = 2;
const EXIT_USAGE: u8 = 64;

#[tokio::main]
async fn main() -> ExitCode {
    let cfg = match config::parse(
        std::env::args_os().skip(1),
        |k| std::env::var_os(k),
        cfg!(windows),
    ) {
        Ok(config::Command::Run(cfg)) => cfg,
        #[cfg(feature = "dev-fixtures")]
        Ok(config::Command::DevSeed(cfg)) => return dev_seed(cfg).await,
        #[cfg(feature = "dev-fixtures")]
        Ok(config::Command::DevRevokeDevice {
            device_id,
            health_port,
        }) => return dev_revoke(&device_id, health_port),
        Ok(config::Command::Help) => {
            println!("{}", config::USAGE);
            return ExitCode::SUCCESS;
        }
        Ok(config::Command::Version) => {
            println!("arena-hub {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("arena-hub: {e}\n{}", config::USAGE);
            return ExitCode::from(EXIT_USAGE);
        }
    };

    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let ring = match std::fs::create_dir_all(&cfg.data_dir)
        .and_then(|()| logs::RingLog::open(cfg.logs_dir(), Arc::clone(&clock)))
    {
        Ok(ring) => ring,
        Err(e) => {
            eprintln!("arena-hub: cannot open logs: {e}");
            return ExitCode::from(EXIT_FAILURE);
        }
    };
    tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(Arc::new(ring))
        .init();

    match run(cfg, clock).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e.code == ErrorCode::SchemaTooNew => {
            tracing::error!(
                code = e.code.as_str(),
                binary_schema_version = LATEST_SCHEMA_VERSION,
                detail = %e.message,
                "refusing to start"
            );
            eprintln!("arena-hub: {e}");
            ExitCode::from(EXIT_SCHEMA_TOO_NEW)
        }
        Err(e) => {
            tracing::error!(code = e.code.as_str(), detail = %e.message, "hub failed");
            eprintln!("arena-hub: {e}");
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

/// Runs `dev seed` and prints the outcome: exit 0 on success, 2 on `SCHEMA_TOO_NEW` and 1
/// otherwise (including `ALREADY_BOOTSTRAPPED` and `NOT_PRIMARY`).
#[cfg(feature = "dev-fixtures")]
async fn dev_seed(cfg: config::Config) -> ExitCode {
    match dev::seed(&cfg, Arc::new(SystemClock)).await {
        Ok(seeded) => {
            println!(
                "dev seed: bootstrapped branch DEV01 in {} ({} changes)",
                cfg.data_dir.display(),
                seeded.changes
            );
            println!(
                "dev seed: registered paired device \"{}\" {}",
                dev::DEV_KIOSK_LABEL,
                seeded.device_id
            );
            println!("dev seed: device token (shown once): {}", seeded.token);
            // Dev-fixtures builds only: release builds have no dev module.
            println!("dev seed: staff credentials (shown once, dev only):");
            for s in &seeded.staff {
                println!(
                    "dev seed:   {:<8} role {:<8} password {:<20} pin {}  ({})",
                    s.login, s.role, s.password, s.pin, s.staff_id
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("dev seed: {e}");
            if e.code == ErrorCode::SchemaTooNew {
                ExitCode::from(EXIT_SCHEMA_TOO_NEW)
            } else {
                ExitCode::from(EXIT_FAILURE)
            }
        }
    }
}

/// Runs `dev revoke-device` against the running Hub: exit 0 once it applied the revoke.
#[cfg(feature = "dev-fixtures")]
fn dev_revoke(device_id: &str, health_port: u16) -> ExitCode {
    match dev::revoke_device(device_id, health_port) {
        Ok(body) => {
            println!("dev revoke-device: revoked {device_id} {body}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("dev revoke-device: {e}");
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

/// Resolves on Ctrl+C. If the handler cannot be installed, it logs and never resolves, so a
/// handler failure is not mistaken for a shutdown request.
async fn ctrl_c() {
    if let Err(e) = tokio::signal::ctrl_c().await {
        tracing::error!(error = %e, "ctrl-c handler failed");
        std::future::pending::<()>().await;
    }
}

/// Resolves on SIGTERM (Unix only; never resolves elsewhere or if the handler fails).
async fn terminate() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
                return;
            }
            Err(e) => tracing::error!(error = %e, "sigterm handler failed"),
        }
    }
    std::future::pending::<()>().await;
}

fn internal(message: String) -> AppError {
    AppError::new(ErrorCode::Internal, message)
}

async fn run(cfg: config::Config, clock: Arc<dyn Clock>) -> Result<(), AppError> {
    let mut store = Store::open(cfg.db_path(), clock.now_ms())?;
    let schema_version = store.schema_version()?;
    let instance_id = load_or_mint_instance_id(&mut store)?;
    tracing::info!(
        hub_instance_id = %instance_id,
        schema_version,
        writer_lock = store.holds_writer_lock(),
        "store open"
    );

    // The LAN certificate: generated once by the writer, reused afterwards; a follower never
    // writes it. Its fingerprint goes into hub_meta like the instance id.
    let cert = tls::load_or_create(
        &cfg.data_dir,
        tls::hostname().as_deref(),
        store.holds_writer_lock(),
    )?;
    record_tls_fingerprint(&mut store, &cert.fingerprint)?;
    tracing::info!(tls_fingerprint = %cert.fingerprint, "lan certificate ready");
    let server_tls = cert.server_config()?;
    let devices = Arc::new(DeviceDirectory::open(cfg.db_path())?);
    let snapshots = Arc::new(SnapshotReader::open(cfg.db_path())?);
    let staff = Arc::new(StaffDirectory::open(cfg.db_path())?);

    let mut initial = HubStatus::opened(schema_version, instance_id.clone());
    initial.tls_fingerprint = Some(cert.fingerprint.clone());
    let (status_tx, status_rx) = status_channel(initial);
    let listener = bind_health(cfg.health_port)
        .await
        .map_err(|e| internal(format!("bind 127.0.0.1:{}: {e}", cfg.health_port)))?;
    let any = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
    let lan_listeners = bind_lan(any, cfg.lan_port, cfg.web_port)
        .await
        .map_err(|e| {
            internal(format!(
                "bind 0.0.0.0:{} / 0.0.0.0:{}: {e}",
                cfg.lan_port, cfg.web_port
            ))
        })?;

    let lease = LeaseTask::start(store, instance_id, Arc::clone(&clock), status_tx.clone()).await?;
    let writer = lease.writer();
    // Housekeeping (dedupe prune) runs at start and daily while primary; it ends once the
    // writer closes.
    let housekeeping = writer.clone().map(|w| {
        tokio::spawn(HousekeepingTask::new(w, Arc::clone(&clock), status_tx.subscribe()).run())
    });
    let mut lease_job = Some(tokio::spawn(lease.run()));

    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let ui_auth = Arc::new(
        arena_api::UiAuth::new(
            staff,
            Arc::clone(&snapshots),
            writer.clone(),
            Arc::clone(&clock),
        )
        .with_status(status_rx.clone()),
    );
    #[cfg_attr(not(feature = "dev-fixtures"), allow(unused_mut))]
    let mut router = arena_api::router(status_rx.clone(), Arc::clone(&clock))
        .merge(arena_api::ui_router(snapshots, Arc::clone(&clock)))
        .merge(arena_api::ui_auth_router(ui_auth));
    #[cfg(feature = "dev-fixtures")]
    {
        router = router.merge(arena_api::health::dev_router(
            writer.clone(),
            Arc::clone(&clock),
        ));
    }
    let mut server = tokio::spawn(serve_router(listener, router, async {
        let _ = stop_rx.await;
    }));
    tracing::info!(
        port = cfg.health_port,
        "healthz, ui snapshot and ui auth listening on loopback"
    );

    let lan_hub = Arc::new(LanHub::new(
        writer.clone(),
        status_rx,
        Arc::clone(&clock),
        devices,
        server_tls,
        env!("CARGO_PKG_VERSION"),
    ));
    let (lan_stop_tx, lan_stop_rx) = oneshot::channel::<()>();
    let lan = tokio::spawn(serve_lan(lan_listeners, lan_hub, async {
        let _ = lan_stop_rx.await;
    }));
    tracing::info!(
        lan_port = cfg.lan_port,
        web_port = cfg.web_port,
        "lan listening"
    );

    let outcome = loop {
        tokio::select! {
            () = ctrl_c() => {
                tracing::info!(signal = "ctrl_c", "shutting down");
                break Ok(());
            }
            () = terminate() => {
                tracing::info!(signal = "sigterm", "shutting down");
                break Ok(());
            }
            res = &mut server => {
                let msg = match res {
                    Ok(Ok(())) => "healthz server stopped unexpectedly".to_owned(),
                    Ok(Err(e)) => format!("healthz server failed: {e}"),
                    Err(e) => format!("healthz server task failed: {e}"),
                };
                tracing::error!(detail = %msg, "early exit");
                break Err(internal(msg));
            }
            res = async { lease_job.as_mut().expect("guarded").await }, if lease_job.is_some() => {
                lease_job = None;
                match res {
                    // Demotion (or a follower start) ends the lease task; keep serving healthz.
                    Ok(()) => tracing::info!("lease task ended"),
                    Err(e) => {
                        let msg = format!("lease task failed: {e}");
                        tracing::error!(detail = %msg, "early exit");
                        break Err(internal(msg));
                    }
                }
            }
        }
    };

    // Healthz must not claim primary or an open database once the writer goes away.
    status_tx.send_modify(|s| {
        s.role = Role::Follower;
        s.db_open = false;
    });
    if let Some(job) = lease_job {
        job.abort();
        let _ = job.await;
    }
    if let Some(job) = housekeeping {
        job.abort();
        let _ = job.await;
    }
    // Close every LAN session before the writer goes away.
    let _ = lan_stop_tx.send(());
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), lan).await;
    if let Some(writer) = writer {
        writer.close().await;
    }
    let _ = stop_tx.send(());
    if !server.is_finished() {
        let _ = server.await;
    }
    tracing::info!("stopped");
    outcome
}
