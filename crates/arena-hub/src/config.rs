//! Command-line arguments and the data directory.

use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::PathBuf;

use arena_api::DEFAULT_HEALTH_PORT;
use arena_api::lan::{DEFAULT_LAN_PORT, DEFAULT_WEB_PORT};
use arena_tasks::{CloudHost, DEFAULT_CLOUD_HOST, default_internet_endpoints};

#[cfg(not(feature = "dev-fixtures"))]
pub const USAGE: &str = "usage: arena-hub [--data-dir <path>] [--health-port <port>] [--lan-port <port>] [--web-port <port>] [--cloud-host <host:port>] [--probe-internet <ip:port,ip:port>] [--printer system] [--help] [--version]";
#[cfg(feature = "dev-fixtures")]
pub const USAGE: &str = "usage: arena-hub [--data-dir <path>] [--health-port <port>] [--lan-port <port>] [--web-port <port>] [--cloud-host <host:port>] [--probe-internet <ip:port,ip:port>] [--printer <system|fake>] [--help] [--version]
       arena-hub dev seed [--data-dir <path>] [--kiosk-allow <path_glob>]... [--kiosk-process-mode <enforce|report>]
       arena-hub dev revoke-device <device_id> [--health-port <port>]";
pub const DATA_DIR_ENV: &str = "ARENA_HUB_DATA_DIR";

/// Resolved start-up configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub data_dir: PathBuf,
    pub health_port: u16,
    /// The TLS WebSocket port for paired apps (default 7400).
    pub lan_port: u16,
    /// The sniffed TLS / plain HTTP port (default 7401).
    pub web_port: u16,
    /// The cloud host the connectivity probe checks (`--cloud-host`, default
    /// `api.arena360.cloud:443`, the production `arena-cloud` behind Cloudflare).
    pub cloud_host: CloudHost,
    /// The internet endpoints the probe connects to (`--probe-internet`, default `1.1.1.1:443`
    /// and `8.8.8.8:443`).
    pub probe_internet: Vec<SocketAddr>,
    pub printer: PrinterChoice,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PrinterChoice {
    #[default]
    System,
    #[cfg(feature = "dev-fixtures")]
    Fake,
}

fn printer_choice(v: &OsString) -> Result<PrinterChoice, String> {
    match v.to_str() {
        Some("system") => Ok(PrinterChoice::System),
        #[cfg(feature = "dev-fixtures")]
        Some("fake") => Ok(PrinterChoice::Fake),
        #[cfg(not(feature = "dev-fixtures"))]
        Some("fake") => Err("--printer fake needs a dev-fixtures build".to_owned()),
        _ => Err(format!("invalid --printer {v:?} (system or fake)")),
    }
}

impl Config {
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("branch.db")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }
}

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Run(Config),
    /// `dev seed`: bootstrap a fixture branch (feature `dev-fixtures` only).
    #[cfg(feature = "dev-fixtures")]
    DevSeed(Config, SeedOptions),
    /// `dev revoke-device <id>`: ask the running Hub on `health_port` to revoke a device
    /// (feature `dev-fixtures` only).
    #[cfg(feature = "dev-fixtures")]
    DevRevokeDevice {
        device_id: String,
        health_port: u16,
    },
    Help,
    Version,
}

/// `dev seed`'s own flags (feature `dev-fixtures` only): extra unsigned kiosk allow rules
/// appended to the default allow-list, and an explicit `kiosk.process_mode`. The escape-suite
/// VMs (Story 1.19) use them to allow the harness and pin `enforce`.
#[cfg(feature = "dev-fixtures")]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeedOptions {
    /// `--kiosk-allow <path_glob>` (repeatable): full-path globs, no signer.
    pub kiosk_allow: Vec<String>,
    /// `--kiosk-process-mode <enforce|report>`.
    pub kiosk_process_mode: Option<String>,
}

/// Splits `dev seed`'s own flags off `args`; the rest go to the shared flag parser.
#[cfg(feature = "dev-fixtures")]
fn split_seed_flags(
    args: impl Iterator<Item = OsString>,
) -> Result<(Vec<OsString>, SeedOptions), String> {
    let mut rest = Vec::new();
    let mut opts = SeedOptions::default();
    let mut args = args;
    while let Some(arg) = args.next() {
        let Some(s) = arg.to_str() else {
            rest.push(arg);
            continue;
        };
        let (flag, inline) = match s.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_owned(), Some(v.to_owned())),
            _ => (s.to_owned(), None),
        };
        if flag != "--kiosk-allow" && flag != "--kiosk-process-mode" {
            rest.push(arg);
            continue;
        }
        let v = match inline {
            Some(v) => v,
            None => args
                .next()
                .and_then(|v| v.into_string().ok())
                .ok_or_else(|| format!("{flag} needs a value"))?,
        };
        if v.is_empty() || v.starts_with("--") {
            return Err(format!("{flag} needs a value"));
        }
        if flag == "--kiosk-allow" {
            let rule = arena_proto::ProcessRule {
                path_glob: v.clone(),
                signer: None,
            };
            let json = serde_json::to_string(&[rule]).map_err(|e| e.to_string())?;
            arena_proto::parse_process_rules(arena_proto::ProcessList::Allow, &json)
                .map_err(|e| format!("invalid --kiosk-allow {v:?}: {}", e.message))?;
            opts.kiosk_allow.push(v);
        } else {
            arena_proto::parse_process_mode(&v)
                .map_err(|_| format!("invalid --kiosk-process-mode {v:?} (enforce or report)"))?;
            opts.kiosk_process_mode = Some(v);
        }
    }
    Ok((rest, opts))
}

/// Parses `args` (without the program name). `env` reads an environment variable.
pub fn parse(
    args: impl IntoIterator<Item = OsString>,
    env: impl Fn(&str) -> Option<OsString>,
    windows: bool,
) -> Result<Command, String> {
    #[cfg_attr(not(feature = "dev-fixtures"), allow(unused_mut))]
    let mut args = args.into_iter().peekable();
    #[cfg(feature = "dev-fixtures")]
    if args.peek().is_some_and(|a| a == "dev") {
        args.next();
        return match args.next() {
            Some(sub) if sub == "seed" => {
                let (rest, opts) = split_seed_flags(args)?;
                match parse_flags(rest.into_iter(), &env, windows, false)? {
                    Command::Run(cfg) => Ok(Command::DevSeed(cfg, opts)),
                    other => Ok(other),
                }
            }
            Some(sub) if sub == "revoke-device" => parse_revoke(args),
            Some(sub) => Err(format!("unknown dev command {}", sub.to_string_lossy())),
            None => Err("dev needs a command (seed, revoke-device)".to_owned()),
        };
    }
    parse_flags(args, &env, windows, true)
}

/// A port flag's value: 1–65535.
fn port(flag: &str, v: &OsString) -> Result<u16, String> {
    v.to_str()
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|p| *p != 0)
        .ok_or_else(|| format!("invalid {flag} {v:?}"))
}

/// `dev revoke-device <device_id> [--health-port <port>]`.
#[cfg(feature = "dev-fixtures")]
fn parse_revoke(mut args: impl Iterator<Item = OsString>) -> Result<Command, String> {
    let device_id = args
        .next()
        .and_then(|a| a.into_string().ok())
        .filter(|a| !a.is_empty() && !a.starts_with("--"))
        .ok_or("dev revoke-device needs a device id")?;
    let mut health_port = DEFAULT_HEALTH_PORT;
    while let Some(arg) = args.next() {
        let arg = arg
            .into_string()
            .map_err(|a| format!("invalid argument {a:?}"))?;
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) => (f.to_owned(), Some(OsString::from(v))),
            None => (arg, None),
        };
        if flag != "--health-port" {
            return Err(format!("unknown argument {flag}"));
        }
        let v = inline
            .or_else(|| args.next())
            .ok_or("--health-port needs a value")?;
        health_port = port("--health-port", &v)?;
    }
    Ok(Command::DevRevokeDevice {
        device_id,
        health_port,
    })
}

/// Parses the flags; the port flags are accepted only when `ports` is set.
fn parse_flags(
    args: impl Iterator<Item = OsString>,
    env: &impl Fn(&str) -> Option<OsString>,
    windows: bool,
    ports: bool,
) -> Result<Command, String> {
    let mut data_dir: Option<PathBuf> = None;
    let mut health_port = DEFAULT_HEALTH_PORT;
    let mut lan_port = DEFAULT_LAN_PORT;
    let mut web_port = DEFAULT_WEB_PORT;
    let mut cloud_host = CloudHost::parse(DEFAULT_CLOUD_HOST).expect("valid default cloud host");
    let mut probe_internet = default_internet_endpoints();
    let mut printer = PrinterChoice::default();
    let mut args = args;
    while let Some(arg) = args.next() {
        let arg = arg
            .into_string()
            .map_err(|a| format!("invalid argument {a:?}"))?;
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_owned(), Some(OsString::from(v))),
            _ => (arg.clone(), None),
        };
        let mut value = || {
            let v = inline
                .clone()
                .or_else(|| args.next())
                .ok_or_else(|| format!("{flag} needs a value"))?;
            if v.is_empty() || v.to_string_lossy().starts_with("--") {
                return Err(format!("{flag} needs a value"));
            }
            Ok(v)
        };
        match flag.as_str() {
            "--help" | "-h" => return Ok(Command::Help),
            "--version" => return Ok(Command::Version),
            "--data-dir" => data_dir = Some(PathBuf::from(value()?)),
            "--health-port" if ports => health_port = port(&flag, &value()?)?,
            "--lan-port" if ports => lan_port = port(&flag, &value()?)?,
            "--web-port" if ports => web_port = port(&flag, &value()?)?,
            "--cloud-host" if ports => {
                cloud_host = CloudHost::parse(&value()?.to_string_lossy())?;
            }
            "--probe-internet" if ports => {
                probe_internet =
                    arena_tasks::parse_internet_endpoints(&value()?.to_string_lossy())?;
            }
            "--printer" if ports => printer = printer_choice(&value()?)?,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let data_dir = match data_dir {
        Some(d) => d,
        None => match env(DATA_DIR_ENV).filter(|v| !v.is_empty()) {
            Some(d) => PathBuf::from(d),
            None => default_data_dir(env, windows)?,
        },
    };
    if ports {
        let mut seen = [health_port, lan_port, web_port];
        seen.sort_unstable();
        if seen.windows(2).any(|w| w[0] == w[1]) {
            return Err("--health-port, --lan-port and --web-port must differ".to_owned());
        }
    }
    Ok(Command::Run(Config {
        data_dir,
        health_port,
        lan_port,
        web_port,
        cloud_host,
        probe_internet,
        printer,
    }))
}

/// `%PROGRAMDATA%\Arena360\Hub` on Windows; `$XDG_DATA_HOME/arena360/hub` (falling back to
/// `~/.local/share/arena360/hub`) elsewhere.
fn default_data_dir(
    env: &impl Fn(&str) -> Option<OsString>,
    windows: bool,
) -> Result<PathBuf, String> {
    let get = |k: &str| env(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if windows {
        let base = get("PROGRAMDATA").unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
        return Ok(base.join("Arena360").join("Hub"));
    }
    if let Some(xdg) = get("XDG_DATA_HOME") {
        return Ok(xdg.join("arena360").join("hub"));
    }
    let home = get("HOME").ok_or("neither --data-dir, ARENA_HUB_DATA_DIR nor HOME is set")?;
    Ok(home
        .join(".local")
        .join("share")
        .join("arena360")
        .join("hub"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), OsString::from(v)))
            .collect();
        move |k| map.get(k).cloned()
    }

    fn args(a: &[&str]) -> Vec<OsString> {
        a.iter().map(OsString::from).collect()
    }

    fn run(
        a: &[&str],
        env: impl Fn(&str) -> Option<OsString>,
        windows: bool,
    ) -> Result<Config, String> {
        match parse(args(a), env, windows)? {
            Command::Run(c) => Ok(c),
            other => Err(format!("{other:?}")),
        }
    }

    #[test]
    fn help_and_version() {
        let env = env_of(&[("HOME", "/h")]);
        assert_eq!(parse(args(&["--help"]), &env, false), Ok(Command::Help));
        assert_eq!(parse(args(&["-h"]), &env, false), Ok(Command::Help));
        assert_eq!(
            parse(args(&["--version"]), &env, false),
            Ok(Command::Version)
        );
    }

    #[test]
    fn rejects_empty_and_flag_like_values_and_port_zero() {
        let env = env_of(&[("HOME", "/h")]);
        assert!(run(&["--data-dir="], &env, false).is_err());
        assert!(run(&["--data-dir", ""], &env, false).is_err());
        assert!(run(&["--data-dir", "--health-port", "7410"], &env, false).is_err());
        assert!(run(&["--health-port", "0"], &env, false).is_err());
        assert!(run(&["--health-port=0"], &env, false).is_err());
    }

    #[test]
    fn flag_beats_env_and_port_defaults() {
        let env = env_of(&[(DATA_DIR_ENV, "/env"), ("HOME", "/home/u")]);
        let c = run(&["--data-dir", "/flag"], &env, false).unwrap();
        assert_eq!(c.data_dir, PathBuf::from("/flag"));
        assert_eq!(c.health_port, 7409);
        assert_eq!(c.db_path(), PathBuf::from("/flag").join("branch.db"));
        assert_eq!(c.logs_dir(), PathBuf::from("/flag").join("logs"));
    }

    #[test]
    fn env_then_xdg_then_home() {
        let c = run(&[], env_of(&[(DATA_DIR_ENV, "/env")]), false).unwrap();
        assert_eq!(c.data_dir, PathBuf::from("/env"));
        let c = run(
            &[],
            env_of(&[("XDG_DATA_HOME", "/xdg"), ("HOME", "/h")]),
            false,
        )
        .unwrap();
        assert_eq!(
            c.data_dir,
            PathBuf::from("/xdg").join("arena360").join("hub")
        );
        let c = run(&[], env_of(&[("HOME", "/h")]), false).unwrap();
        assert_eq!(
            c.data_dir,
            PathBuf::from("/h")
                .join(".local")
                .join("share")
                .join("arena360")
                .join("hub")
        );
    }

    #[test]
    fn windows_uses_programdata() {
        let c = run(&[], env_of(&[("PROGRAMDATA", r"D:\PD")]), true).unwrap();
        assert_eq!(
            c.data_dir,
            PathBuf::from(r"D:\PD").join("Arena360").join("Hub")
        );
    }

    #[test]
    #[cfg(not(feature = "dev-fixtures"))]
    fn dev_is_unknown_without_the_feature() {
        let env = env_of(&[("HOME", "/h")]);
        let err = parse(args(&["dev", "seed"]), &env, false).unwrap_err();
        assert!(err.contains("unknown argument dev"), "{err}");
        assert!(!USAGE.contains("dev"));
    }

    #[test]
    #[cfg(feature = "dev-fixtures")]
    fn dev_seed_parses_with_the_feature() {
        let env = env_of(&[("HOME", "/h")]);
        let cmd = parse(args(&["dev", "seed", "--data-dir", "/d"]), &env, false).unwrap();
        let Command::DevSeed(cfg, opts) = cmd else {
            panic!("{cmd:?}")
        };
        assert_eq!(opts, SeedOptions::default());
        assert_eq!(cfg.data_dir, PathBuf::from("/d"));
        let cmd = parse(
            args(&["dev", "seed"]),
            env_of(&[(DATA_DIR_ENV, "/env")]),
            false,
        )
        .unwrap();
        assert_eq!(
            cmd,
            Command::DevSeed(
                Config {
                    data_dir: PathBuf::from("/env"),
                    health_port: DEFAULT_HEALTH_PORT,
                    lan_port: DEFAULT_LAN_PORT,
                    web_port: DEFAULT_WEB_PORT,
                    cloud_host: CloudHost::parse(DEFAULT_CLOUD_HOST).unwrap(),
                    probe_internet: default_internet_endpoints(),
                    printer: PrinterChoice::System,
                },
                SeedOptions::default()
            )
        );
        let cmd = parse(
            args(&[
                "dev",
                "seed",
                "--kiosk-allow",
                r"C:\ArenaEscape\bin\arena-escape.exe",
                "--data-dir=/d",
                "--kiosk-process-mode=enforce",
                r"--kiosk-allow=C:\Games\*.exe",
            ]),
            &env,
            false,
        )
        .unwrap();
        let Command::DevSeed(cfg, opts) = cmd else {
            panic!("{cmd:?}")
        };
        assert_eq!(cfg.data_dir, PathBuf::from("/d"));
        assert_eq!(
            opts,
            SeedOptions {
                kiosk_allow: vec![
                    r"C:\ArenaEscape\bin\arena-escape.exe".into(),
                    r"C:\Games\*.exe".into()
                ],
                kiosk_process_mode: Some("enforce".into()),
            }
        );
        // A bare name, an unsigned user-writable path and an unknown mode are refused.
        for bad in [
            &["dev", "seed", "--kiosk-allow", "cmd.exe"][..],
            &["dev", "seed", "--kiosk-allow", r"C:\Users\x\a.exe"],
            &["dev", "seed", "--kiosk-allow"],
            &["dev", "seed", "--kiosk-process-mode", "off"],
        ] {
            assert!(parse(args(bad), &env, false).is_err(), "{bad:?}");
        }
        assert!(parse(args(&["dev", "seed", "--lan-port", "1"]), &env, false).is_err());
        assert_eq!(
            parse(
                args(&["dev", "revoke-device", "01JD", "--health-port", "7500"]),
                &env,
                false
            ),
            Ok(Command::DevRevokeDevice {
                device_id: "01JD".into(),
                health_port: 7500
            })
        );
        assert_eq!(
            parse(args(&["dev", "revoke-device", "01JD"]), &env, false),
            Ok(Command::DevRevokeDevice {
                device_id: "01JD".into(),
                health_port: DEFAULT_HEALTH_PORT
            })
        );
        assert!(parse(args(&["dev", "revoke-device"]), &env, false).is_err());
        assert!(
            parse(
                args(&["dev", "revoke-device", "--health-port", "1"]),
                &env,
                false
            )
            .is_err()
        );
        assert!(
            parse(
                args(&["dev", "revoke-device", "D", "--data-dir", "/d"]),
                &env,
                false
            )
            .is_err()
        );
        assert!(parse(args(&["dev"]), &env, false).is_err());
        assert!(parse(args(&["dev", "wipe"]), &env, false).is_err());
        assert!(parse(args(&["dev", "seed", "--health-port", "1"]), &env, false).is_err());
        assert!(parse(args(&["--data-dir", "/d", "dev", "seed"]), &env, false).is_err());
    }

    #[test]
    fn printer_flag_defaults_to_the_system_adapter() {
        let env = env_of(&[("HOME", "/h")]);
        assert_eq!(
            run(&[], &env, false).unwrap().printer,
            PrinterChoice::System
        );
        assert_eq!(
            run(&["--printer", "system"], &env, false).unwrap().printer,
            PrinterChoice::System
        );
        assert!(run(&["--printer", "laser"], &env, false).is_err());
        assert!(run(&["--printer"], &env, false).is_err());
        #[cfg(feature = "dev-fixtures")]
        {
            assert_eq!(
                run(&["--printer=fake"], &env, false).unwrap().printer,
                PrinterChoice::Fake
            );
            assert!(parse(args(&["dev", "seed", "--printer", "fake"]), &env, false).is_err());
        }
        #[cfg(not(feature = "dev-fixtures"))]
        assert!(run(&["--printer", "fake"], &env, false).is_err());
    }

    #[test]
    fn probe_flags_default_and_override() {
        let env = env_of(&[("HOME", "/h")]);
        let c = run(&[], &env, false).unwrap();
        assert_eq!(c.cloud_host.host, "api.arena360.cloud");
        assert_eq!(c.cloud_host.port, 443);
        assert_eq!(
            c.probe_internet,
            vec![
                "1.1.1.1:443".parse::<SocketAddr>().unwrap(),
                "8.8.8.8:443".parse().unwrap()
            ]
        );
        let c = run(
            &[
                "--cloud-host",
                "cloud.example:8443",
                "--probe-internet=9.9.9.9:443,[2606:4700::1111]:443",
            ],
            &env,
            false,
        )
        .unwrap();
        assert_eq!(
            (c.cloud_host.host.as_str(), c.cloud_host.port),
            ("cloud.example", 8443)
        );
        assert_eq!(
            c.probe_internet,
            vec![
                "9.9.9.9:443".parse::<SocketAddr>().unwrap(),
                "[2606:4700::1111]:443".parse().unwrap()
            ]
        );
        for bad in [
            &["--cloud-host", "cloud.example"][..],
            &["--cloud-host", "cloud.example:0"],
            &["--cloud-host"],
            &["--probe-internet", "1.1.1.1"],
            &["--probe-internet", ","],
            &["--probe-internet"],
        ] {
            assert!(run(bad, &env, false).is_err(), "{bad:?}");
        }
        assert!(USAGE.contains("--cloud-host <host:port>"));
        assert!(USAGE.contains("--probe-internet"));
    }

    #[test]
    fn health_port_override_and_errors() {
        let env = env_of(&[("HOME", "/h")]);
        let c = run(&["--health-port", "7410"], &env, false).unwrap();
        assert_eq!(c.health_port, 7410);
        let c = run(&["--health-port=7411", "--data-dir=/d"], &env, false).unwrap();
        assert_eq!((c.health_port, c.data_dir), (7411, PathBuf::from("/d")));
        assert!(run(&["--health-port", "x"], &env, false).is_err());
        let c = run(&["--lan-port", "8400", "--web-port=8401"], &env, false).unwrap();
        assert_eq!((c.lan_port, c.web_port, c.health_port), (8400, 8401, 7409));
        let c = run(&[], &env, false).unwrap();
        assert_eq!((c.lan_port, c.web_port), (7400, 7401));
        assert!(run(&["--lan-port", "0"], &env, false).is_err());
        assert!(run(&["--lan-port", "7401"], &env, false).is_err());
        assert!(run(&["--web-port", "7409"], &env, false).is_err());
        assert!(run(&["--health-port"], &env, false).is_err());
        assert!(run(&["--bogus"], &env, false).is_err());
    }
}
