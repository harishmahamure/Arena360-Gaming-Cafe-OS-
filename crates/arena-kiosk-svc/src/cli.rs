//! Command-line parsing.

use std::ffi::OsString;
use std::path::PathBuf;

#[cfg(feature = "dev-fixtures")]
pub const USAGE: &str = "usage:
  arena-kiosk-svc --console [--data-dir <path>] [--app <path>] [--dev-fake-procs]
  arena-kiosk-svc service install|uninstall|run          (Windows)
  arena-kiosk-svc lockdown apply|restore|reset-shell [--player-user <name>] [--data-dir <path>]   (Windows, elevated)
  arena-kiosk-svc cleanup-helper --clipboard | --volume <0-100>   (Windows; started by the service in the player's session)
  arena-kiosk-svc dev provision --hub <host:7400> --fingerprint <hex> --token <token>
                  [--player-user <name>] [--data-dir <path>]   (dev-fixtures builds)
  arena-kiosk-svc --help | --version";

#[cfg(not(feature = "dev-fixtures"))]
pub const USAGE: &str = "usage:
  arena-kiosk-svc --console [--data-dir <path>] [--app <path>]
  arena-kiosk-svc service install|uninstall|run          (Windows)
  arena-kiosk-svc lockdown apply|restore|reset-shell [--player-user <name>] [--data-dir <path>]   (Windows, elevated)
  arena-kiosk-svc cleanup-helper --clipboard | --volume <0-100>   (Windows; started by the service in the player's session)
  arena-kiosk-svc --help | --version";

/// A parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Run in the foreground (every platform).
    Console {
        data_dir: Option<PathBuf>,
        app: Option<PathBuf>,
        /// `--dev-fake-procs` (dev-fixtures builds): enforce the process lists against an
        /// in-memory process table, so a console kiosk off Windows can block a process.
        dev_fake_procs: bool,
    },
    /// Windows service management.
    ServiceInstall,
    ServiceUninstall,
    ServiceRun,
    /// `lockdown apply|restore`: lock the player account down, or put it back.
    Lockdown {
        action: LockdownAction,
        player_user: Option<String>,
        data_dir: Option<PathBuf>,
    },
    /// `cleanup-helper`: the part of the session-end cleanup that must run in the player's
    /// session (Story 6.11). The service starts it there; it exits 0 on success.
    CleanupHelper(HelperTask),
    /// `dev provision`.
    #[cfg(feature = "dev-fixtures")]
    DevProvision {
        hub: String,
        fingerprint: String,
        token: String,
        player_user: Option<String>,
        data_dir: Option<PathBuf>,
    },
    Help,
    Version,
}

/// What `cleanup-helper` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperTask {
    /// Empty the clipboard and its history.
    Clipboard,
    /// Set the system volume to this percent and unmute.
    Volume(u8),
    /// Forget the remembered Steam account in the player's registry (Story 6.12).
    SteamLogin,
    /// Remove the player's Credential Manager entries (Story 6.12).
    Credentials,
}

/// What `lockdown` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockdownAction {
    Apply,
    Restore,
    /// Last-resort: point the player's `Winlogon\Shell` back at Explorer (used by the
    /// uninstaller when a restore failed, so the PC is still usable).
    ResetShell,
}

fn text(v: OsString, flag: &str) -> Result<String, String> {
    v.into_string()
        .map_err(|_| format!("{flag} must be valid UTF-8"))
}

/// Parses the arguments after the program name.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command, String> {
    let args: Vec<OsString> = args.into_iter().collect();
    let first = args.first().and_then(|a| a.to_str()).unwrap_or("");
    match first {
        "--help" | "-h" if args.len() == 1 => Ok(Command::Help),
        "--version" if args.len() == 1 => Ok(Command::Version),
        "service" => match args.get(1).and_then(|a| a.to_str()) {
            Some("install") if args.len() == 2 => Ok(Command::ServiceInstall),
            Some("uninstall") if args.len() == 2 => Ok(Command::ServiceUninstall),
            Some("run") if args.len() == 2 => Ok(Command::ServiceRun),
            _ => Err("service takes install, uninstall or run".into()),
        },
        "lockdown" => {
            let action = match args.get(1).and_then(|a| a.to_str()) {
                Some("apply") => LockdownAction::Apply,
                Some("restore") => LockdownAction::Restore,
                Some("reset-shell") => LockdownAction::ResetShell,
                _ => return Err("lockdown takes apply, restore or reset-shell".into()),
            };
            let (mut player_user, mut data_dir) = (None, None);
            let mut it = args.into_iter().skip(2);
            while let Some(flag) = it.next() {
                let flag = text(flag, "a flag")?;
                let value = it.next().ok_or_else(|| format!("{flag} needs a value"))?;
                match flag.as_str() {
                    "--player-user" => player_user = Some(text(value, "--player-user")?),
                    "--data-dir" => data_dir = Some(PathBuf::from(value)),
                    other => return Err(format!("unknown flag {other}")),
                }
            }
            Ok(Command::Lockdown {
                action,
                player_user,
                data_dir,
            })
        }
        "cleanup-helper" => match (args.get(1).and_then(|a| a.to_str()), args.get(2)) {
            (Some("--clipboard"), None) => Ok(Command::CleanupHelper(HelperTask::Clipboard)),
            (Some("--steam-login"), None) => Ok(Command::CleanupHelper(HelperTask::SteamLogin)),
            (Some("--credentials"), None) => Ok(Command::CleanupHelper(HelperTask::Credentials)),
            (Some("--volume"), Some(v)) if args.len() == 3 => v
                .to_str()
                .filter(|s| s.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|s| s.parse::<u8>().ok())
                .filter(|p| *p <= 100)
                .map(|p| Command::CleanupHelper(HelperTask::Volume(p)))
                .ok_or_else(|| "--volume takes a whole number from 0 to 100".into()),
            _ => Err("cleanup-helper takes --clipboard, --volume <0-100>, --steam-login or --credentials".into()),
        },
        #[cfg(feature = "dev-fixtures")]
        "dev" => {
            if args.get(1).and_then(|a| a.to_str()) != Some("provision") {
                return Err("dev takes provision".into());
            }
            let (mut hub, mut fingerprint, mut token, mut player_user, mut data_dir) =
                (None, None, None, None, None);
            let mut it = args.into_iter().skip(2);
            while let Some(flag) = it.next() {
                let flag = text(flag, "a flag")?;
                let value = it.next().ok_or_else(|| format!("{flag} needs a value"))?;
                match flag.as_str() {
                    "--hub" => hub = Some(text(value, "--hub")?),
                    "--fingerprint" => fingerprint = Some(text(value, "--fingerprint")?),
                    "--token" => token = Some(text(value, "--token")?),
                    "--player-user" => player_user = Some(text(value, "--player-user")?),
                    "--data-dir" => data_dir = Some(PathBuf::from(value)),
                    other => return Err(format!("unknown flag {other}")),
                }
            }
            Ok(Command::DevProvision {
                hub: hub.ok_or("--hub is required")?,
                fingerprint: fingerprint.ok_or("--fingerprint is required")?,
                token: token.ok_or("--token is required")?,
                player_user,
                data_dir,
            })
        }
        "--console" => {
            let (mut data_dir, mut app, mut dev_fake_procs) = (None, None, false);
            let mut it = args.into_iter().skip(1);
            while let Some(flag) = it.next() {
                let flag = text(flag, "a flag")?;
                if flag == "--dev-fake-procs" {
                    dev_fake_procs = true;
                    continue;
                }
                let value = it.next().ok_or_else(|| format!("{flag} needs a value"))?;
                match flag.as_str() {
                    "--data-dir" => data_dir = Some(PathBuf::from(value)),
                    "--app" => app = Some(PathBuf::from(value)),
                    other => return Err(format!("unknown flag {other}")),
                }
            }
            Ok(Command::Console {
                data_dir,
                app,
                dev_fake_procs,
            })
        }
        "" => Err("no command given".into()),
        other => Err(format!("unknown command {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Command, String> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn commands() {
        assert_eq!(
            p(&["--console"]).unwrap(),
            Command::Console {
                data_dir: None,
                app: None,
                dev_fake_procs: false,
            }
        );
        assert_eq!(
            p(&["--console", "--data-dir", "/d", "--app", "/a"]).unwrap(),
            Command::Console {
                data_dir: Some("/d".into()),
                app: Some("/a".into()),
                dev_fake_procs: false,
            }
        );
        assert_eq!(
            p(&["--console", "--dev-fake-procs", "--data-dir", "/d"]).unwrap(),
            Command::Console {
                data_dir: Some("/d".into()),
                app: None,
                dev_fake_procs: true,
            }
        );
        assert_eq!(p(&["service", "install"]).unwrap(), Command::ServiceInstall);
        assert_eq!(
            p(&["service", "uninstall"]).unwrap(),
            Command::ServiceUninstall
        );
        assert_eq!(p(&["service", "run"]).unwrap(), Command::ServiceRun);
        assert_eq!(
            p(&["lockdown", "apply", "--player-user", "player"]).unwrap(),
            Command::Lockdown {
                action: LockdownAction::Apply,
                player_user: Some("player".into()),
                data_dir: None,
            }
        );
        assert_eq!(
            p(&["lockdown", "restore"]).unwrap(),
            Command::Lockdown {
                action: LockdownAction::Restore,
                player_user: None,
                data_dir: None,
            }
        );
        assert_eq!(
            p(&["lockdown", "reset-shell"]).unwrap(),
            Command::Lockdown {
                action: LockdownAction::ResetShell,
                player_user: None,
                data_dir: None,
            }
        );
        assert_eq!(
            p(&["cleanup-helper", "--clipboard"]).unwrap(),
            Command::CleanupHelper(HelperTask::Clipboard)
        );
        assert_eq!(
            p(&["cleanup-helper", "--volume", "40"]).unwrap(),
            Command::CleanupHelper(HelperTask::Volume(40))
        );
        assert_eq!(
            p(&["cleanup-helper", "--steam-login"]).unwrap(),
            Command::CleanupHelper(HelperTask::SteamLogin)
        );
        assert_eq!(
            p(&["cleanup-helper", "--credentials"]).unwrap(),
            Command::CleanupHelper(HelperTask::Credentials)
        );
        assert_eq!(p(&["--help"]).unwrap(), Command::Help);
        assert_eq!(p(&["--version"]).unwrap(), Command::Version);
        #[cfg(feature = "dev-fixtures")]
        {
            assert_eq!(
                p(&[
                    "dev",
                    "provision",
                    "--hub",
                    "h:7400",
                    "--fingerprint",
                    "f",
                    "--token",
                    "t"
                ])
                .unwrap(),
                Command::DevProvision {
                    hub: "h:7400".into(),
                    fingerprint: "f".into(),
                    token: "t".into(),
                    player_user: None,
                    data_dir: None,
                }
            );
        }
        for bad in [
            &[][..],
            &["service"],
            &["service", "start"],
            &["--console", "--data-dir"],
            &["--console", "--nope", "x"],
            &["dev", "provision", "--hub", "h"],
            &["dev", "seed"],
            &["lockdown"],
            &["lockdown", "undo"],
            &["lockdown", "apply", "--player-user"],
            &["lockdown", "apply", "--force", "x"],
            &["cleanup-helper"],
            &["cleanup-helper", "--volume"],
            &["cleanup-helper", "--volume", "101"],
            &["cleanup-helper", "--volume", "-1"],
            &["cleanup-helper", "--volume", "5", "x"],
            &["cleanup-helper", "--clipboard", "x"],
            &["bogus"],
        ] {
            assert!(p(bad).is_err(), "{bad:?}");
        }
    }

    #[cfg(not(feature = "dev-fixtures"))]
    #[test]
    fn a_release_build_has_no_dev_provision_command() {
        let full = [
            "dev",
            "provision",
            "--hub",
            "h:7400",
            "--fingerprint",
            "f",
            "--token",
            "t",
        ];
        assert_eq!(p(&full).unwrap_err(), "unknown command dev");
        assert!(!USAGE.contains("dev provision"));
        assert!(!USAGE.contains("--dev-fake-procs"));
    }
}
