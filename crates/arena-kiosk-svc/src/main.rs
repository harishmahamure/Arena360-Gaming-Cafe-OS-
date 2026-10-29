//! `arena-kiosk-svc`: see the library docs. Exit codes: 0 success, 1 failure, 2 `lockdown` off
//! Windows, 64 bad usage.

use std::process::ExitCode;
use std::sync::Arc;

use arena_kiosk_svc::cli::{self, Command, HelperTask, LockdownAction};
use arena_kiosk_svc::launcher::{AppLauncher, CommandLauncher, NoopLauncher};
use arena_kiosk_svc::paths::{DataDir, default_data_dir};

const EXIT_FAILURE: u8 = 1;
#[cfg(not(windows))]
const EXIT_NOT_WINDOWS: u8 = 2;
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
        Command::Console {
            data_dir: d,
            app,
            dev_fake_procs,
        } => console(data_dir(d), app, dev_fake_procs),
        Command::ServiceInstall | Command::ServiceUninstall | Command::ServiceRun => service(cmd),
        Command::Lockdown {
            action,
            player_user,
            data_dir: d,
        } => lockdown(data_dir(d), action, player_user),
        Command::CleanupHelper(task) => cleanup_helper(task),
        Command::WindowWatch(pid) => window_watch(pid),
        #[cfg(feature = "dev-fixtures")]
        Command::DevProvision {
            hub,
            fingerprint,
            token,
            player_user,
            data_dir: d,
        } => dev_provision(data_dir(d), hub, fingerprint, token, player_user),
    }
}

fn console(data: DataDir, app: Option<std::path::PathBuf>, dev_fake_procs: bool) -> ExitCode {
    #[cfg(not(feature = "dev-fixtures"))]
    if dev_fake_procs {
        eprintln!("arena-kiosk-svc: --dev-fake-procs needs a dev-fixtures build");
        return ExitCode::from(EXIT_USAGE);
    }
    if let Err(e) = arena_kiosk_svc::host::init_logs(&data) {
        eprintln!("arena-kiosk-svc: cannot open logs: {e}");
        return ExitCode::from(EXIT_FAILURE);
    }
    let launcher: Arc<dyn AppLauncher> = match app {
        Some(p) => Arc::new(CommandLauncher::new(p, Vec::new())),
        #[cfg(feature = "dev-fixtures")]
        None if dev_fake_procs => Arc::new(arena_kiosk_svc::dev::DevLauncher),
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
    let setup = move |_rt: &mut arena_kiosk_svc::Runtime| {
        #[cfg(feature = "dev-fixtures")]
        if dev_fake_procs {
            let (api, opts) = arena_kiosk_svc::dev::fake_enforcement();
            _rt.enforce(api, opts);
            println!("arena-kiosk-svc: enforcing against the dev fake process table");
        }
    };
    let result = runtime.block_on(arena_kiosk_svc::host::run_until_with(
        data,
        launcher,
        setup,
        async {
            let _ = tokio::signal::ctrl_c().await;
        },
    ));
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

#[cfg(windows)]
fn lockdown(data: DataDir, action: LockdownAction, player_user: Option<String>) -> ExitCode {
    use arena_kiosk_svc::lockdown::{self, RestoreOutcome, windows::WinAccounts};
    if let Some(u) = &player_user
        && !arena_kiosk_svc::config::valid_account_name(u)
    {
        eprintln!("arena-kiosk-svc: the player account name is not valid");
        return ExitCode::from(EXIT_USAGE);
    }
    if let Err(e) = arena_kiosk_svc::winsec::protect_data_dir(data.root()) {
        eprintln!("lockdown: cannot protect {}: {e}", data.root().display());
        return ExitCode::from(EXIT_FAILURE);
    }
    let mut accounts = WinAccounts;
    let backup = data.lockdown_backup();
    let result = match action {
        LockdownAction::Apply => {
            // The installer passes no `--player-user`: take it from the provisioned config, so
            // the account the app runs as and the account locked down are always the same.
            let user = player_user.unwrap_or_else(|| {
                arena_kiosk_svc::config::KioskConfig::load(&data.config())
                    .ok()
                    .flatten()
                    .map(|c| c.player_user)
                    .unwrap_or_else(|| arena_kiosk_svc::config::DEFAULT_PLAYER_USER.to_owned())
            });
            let shell = arena_kiosk_svc::launcher::configured_app_path(&data.config());
            lockdown::apply(&mut accounts, &backup, &user, &shell.display().to_string()).map(|b| {
                format!(
                    "lockdown applied to {} (shell {}); previous values in {}",
                    b.player_user,
                    shell.display(),
                    backup.display()
                )
            })
        }
        LockdownAction::Restore => {
            lockdown::restore(&mut accounts, &backup, player_user.as_deref()).map(|o| match o {
                RestoreOutcome::Restored { player_user } => {
                    format!("lockdown restored for {player_user}")
                }
                RestoreOutcome::NothingToRestore => "lockdown: nothing to restore".to_owned(),
                RestoreOutcome::AccountGone { player_user } => {
                    format!("lockdown: {player_user} no longer exists; backup removed")
                }
            })
        }
        LockdownAction::ResetShell => {
            lockdown::reset_shell(&mut accounts, &backup, player_user.as_deref()).map(|o| match o {
                RestoreOutcome::Restored { player_user } => {
                    format!("lockdown: {player_user} shell reset to explorer.exe")
                }
                RestoreOutcome::NothingToRestore => "lockdown: nothing to reset".to_owned(),
                RestoreOutcome::AccountGone { player_user } => {
                    format!("lockdown: {player_user} no longer exists")
                }
            })
        }
    };
    match result {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("lockdown: {e}");
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

#[cfg(not(windows))]
fn lockdown(_data: DataDir, _action: LockdownAction, _player_user: Option<String>) -> ExitCode {
    eprintln!("{}", arena_kiosk_svc::lockdown::ONLY_ON_WINDOWS);
    ExitCode::from(EXIT_NOT_WINDOWS)
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

/// The session-end cleanup's player-session part (Story 6.11): the service starts this in the
/// player's session, where the clipboard and the volume are.
#[cfg(windows)]
fn cleanup_helper(task: HelperTask) -> ExitCode {
    match arena_kiosk_svc::cleanup_windows::run_helper(task) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cleanup-helper: {e}");
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

#[cfg(not(windows))]
fn cleanup_helper(_task: HelperTask) -> ExitCode {
    eprintln!("arena-kiosk-svc: cleanup-helper is Windows only");
    ExitCode::from(EXIT_NOT_WINDOWS)
}

/// A game's window watcher (Story 6.25): the service starts this in the player's session, where
/// the game's windows are, and reads how it exited (0: a window appeared).
#[cfg(windows)]
fn window_watch(pid: u32) -> ExitCode {
    ExitCode::from(arena_kiosk_svc::games_windows::run_window_watch(pid) as u8)
}

#[cfg(not(windows))]
fn window_watch(_pid: u32) -> ExitCode {
    eprintln!("arena-kiosk-svc: window-watch is Windows only");
    ExitCode::from(EXIT_NOT_WINDOWS)
}
