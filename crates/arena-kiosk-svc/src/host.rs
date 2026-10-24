//! What the console mode and the Windows service host share: logs, the platform IPC
//! transport, the process enforcement (Windows), and running the [`Runtime`] until asked to stop.

use std::future::Future;
use std::sync::Arc;

use crate::clock::{Clock, SystemClock};
use crate::launcher::AppLauncher;
use crate::paths::DataDir;
use crate::runtime::{Runtime, Timings};

/// Sends `tracing` JSON to the ring under `<data>/logs` (14 days / 50 MB, as the Hub).
pub fn init_logs(data: &DataDir) -> std::io::Result<()> {
    use tracing_subscriber::EnvFilter;
    std::fs::create_dir_all(data.root())?;
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let ring = crate::logs::RingLog::open(data.logs(), clock)?;
    let _ = tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(Arc::new(ring))
        .try_init();
    Ok(())
}

/// Starts the service on the platform transport (the pipe on Windows, `<data>/kiosk.sock`
/// elsewhere) and runs it until `shutdown` resolves.
pub async fn run_until(
    data: DataDir,
    launcher: Arc<dyn AppLauncher>,
    shutdown: impl Future<Output = ()>,
) -> std::io::Result<()> {
    run_until_with(data, launcher, |_| {}, shutdown).await
}

/// [`run_until`], with `setup` called on the started [`Runtime`] before it runs (the dev
/// fixtures add their fake process enforcement there).
pub async fn run_until_with(
    data: DataDir,
    launcher: Arc<dyn AppLauncher>,
    setup: impl FnOnce(&mut Runtime),
    shutdown: impl Future<Output = ()>,
) -> std::io::Result<()> {
    std::fs::create_dir_all(data.root())?;
    // The token, its entropy and the config live here: SYSTEM and Administrators only, or the
    // service does not start.
    #[cfg(windows)]
    crate::winsec::protect_data_dir(data.root())?;
    let secrets = crate::secret::platform_store(data.token());
    #[cfg(windows)]
    let ipc = crate::ipc::PipeIpcServer::bind(
        arena_kiosk_core::ipc::PIPE_NAME,
        &data.config(),
        &crate::launcher::configured_app_path(&data.config()),
    )?;
    #[cfg(unix)]
    let ipc = crate::ipc::UnixIpcServer::bind(&data.socket())?;
    #[cfg(windows)]
    let enforce = crate::procs::EnforceOptions::for_this_install(&data.config());
    let mut runtime = Runtime::start(
        data,
        secrets,
        Arc::new(SystemClock),
        Timings::default(),
        ipc,
        launcher,
    )?;
    // The process lists are enforced on Windows only (there is no player session elsewhere).
    #[cfg(windows)]
    runtime.enforce(
        crate::procs::ProcessApi::of(std::sync::Arc::new(
            crate::procs::windows::WinProcesses::new(),
        )),
        enforce,
    );
    setup(&mut runtime);
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "kiosk service started");
    shutdown.await;
    tracing::info!("kiosk service stopping");
    runtime.shutdown().await;
    Ok(())
}
