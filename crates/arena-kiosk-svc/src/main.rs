//! `arena-kiosk-svc`: see the library docs. Exit codes: 0 success, 1 failure, 64 bad usage.

use std::process::ExitCode;
use std::sync::Arc;

use arena_kiosk_svc::cli::{self, Command};
use arena_kiosk_svc::launcher::{AppLauncher, CommandLauncher, NoopLauncher};
use arena_kiosk_svc::paths::{DataDir, default_data_dir};

const EXIT_FAILURE: u8 = 1;
const EXIT_USAGE: u8 = 64;

fn data_dir(given: Option<std::path::PathBuf>) -> DataDir {
    DataDir(given.unwrap_or_else(|| default_data_dir(|k| std::env::var(k).ok(), cfg!(windows))))
}

fn main() -> ExitCode {
    let cmd = match cli::parse(std::env::args_os().skip(1)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("arena-kiosk-svc: {e}\n{}", cli::USAGE);
            return ExitCode::from(EXIT_USAGE);
        }
    };
    match cmd {
        Command::Help => {
            println!("{}", cli::USAGE);
            ExitCode::SUCCESS
        }
        Command::Version => {
            println!("arena-kiosk-svc {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Command::Console { data_dir: d, app } => console(data_dir(d), app),
        Command::ServiceInstall | Command::ServiceUninstall | Command::ServiceRun => service(cmd),
        Command::DevProvision {
            hub,
            fingerprint,
            token,
            player_user,
            data_dir: d,
        } => dev_provision(data_dir(d), hub, fingerprint, token, player_user),
    }
}

fn console(data: DataDir, app: Option<std::path::PathBuf>) -> ExitCode {
    if let Err(e) = arena_kiosk_svc::host::init_logs(&data) {
        eprintln!("arena-kiosk-svc: cannot open logs: {e}");
        return ExitCode::from(EXIT_FAILURE);
    }
    let launcher: Arc<dyn AppLauncher> = match app {
        Some(p) => Arc::new(CommandLauncher::new(p, Vec::new())),
        None => Arc::new(NoopLauncher),
    };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("arena-kiosk-svc: {e}");
            return ExitCode::from(EXIT_FAILURE);
        }
    };
    println!(
        "arena-kiosk-svc: console mode, data in {}",
        data.root().display()
    );
    let result = runtime.block_on(arena_kiosk_svc::host::run_until(data, launcher, async {
        let _ = tokio::signal::ctrl_c().await;
    }));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("arena-kiosk-svc: {e}");
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

#[cfg(windows)]
fn service(cmd: Command) -> ExitCode {
    use arena_kiosk_svc::service;
    let result = match cmd {
        Command::ServiceInstall => service::install(),
        Command::ServiceUninstall => service::uninstall(),
        _ => service::run_dispatcher(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("arena-kiosk-svc: {e}");
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

#[cfg(not(windows))]
fn service(_cmd: Command) -> ExitCode {
    eprintln!("arena-kiosk-svc: the service commands are Windows only; use --console");
    ExitCode::from(EXIT_USAGE)
}

#[cfg(feature = "dev-fixtures")]
fn dev_provision(
    data: DataDir,
    hub: String,
    fingerprint: String,
    token: String,
    player_user: Option<String>,
) -> ExitCode {
    let secrets = arena_kiosk_svc::secret::platform_store(data.token());
    let p = arena_kiosk_svc::dev::Provision {
        hub_addr: hub,
        fingerprint,
        token,
        player_user,
    };
    match arena_kiosk_svc::dev::provision(&data, secrets.as_ref(), &p) {
        Ok(device_id) => {
            println!(
                "dev provision: device {device_id} provisioned in {}",
                data.root().display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("dev provision: {e}");
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

#[cfg(not(feature = "dev-fixtures"))]
fn dev_provision(
    _data: DataDir,
    _hub: String,
    _fingerprint: String,
    _token: String,
    _player_user: Option<String>,
) -> ExitCode {
    eprintln!("arena-kiosk-svc: dev provision needs a dev-fixtures build");
    ExitCode::from(EXIT_USAGE)
}
