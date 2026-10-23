//! Command-line arguments and the data directory.

use std::ffi::OsString;
use std::path::PathBuf;

use arena_api::DEFAULT_HEALTH_PORT;

pub const USAGE: &str =
    "usage: arena-hub [--data-dir <path>] [--health-port <port>] [--help] [--version]";
pub const DATA_DIR_ENV: &str = "ARENA_HUB_DATA_DIR";

/// Resolved start-up configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub data_dir: PathBuf,
    pub health_port: u16,
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
    Help,
    Version,
}

/// Parses `args` (without the program name). `env` reads an environment variable.
pub fn parse(
    args: impl IntoIterator<Item = OsString>,
    env: impl Fn(&str) -> Option<OsString>,
    windows: bool,
) -> Result<Command, String> {
    let mut data_dir: Option<PathBuf> = None;
    let mut health_port = DEFAULT_HEALTH_PORT;
    let mut args = args.into_iter();
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
            "--health-port" => {
                let v = value()?;
                health_port = v
                    .to_str()
                    .and_then(|s| s.parse::<u16>().ok())
                    .filter(|p| *p != 0)
                    .ok_or_else(|| format!("invalid --health-port {v:?}"))?;
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let data_dir = match data_dir {
        Some(d) => d,
        None => match env(DATA_DIR_ENV).filter(|v| !v.is_empty()) {
            Some(d) => PathBuf::from(d),
            None => default_data_dir(&env, windows)?,
        },
    };
    Ok(Command::Run(Config {
        data_dir,
        health_port,
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
    fn health_port_override_and_errors() {
        let env = env_of(&[("HOME", "/h")]);
        let c = run(&["--health-port", "7410"], &env, false).unwrap();
        assert_eq!(c.health_port, 7410);
        let c = run(&["--health-port=7411", "--data-dir=/d"], &env, false).unwrap();
        assert_eq!((c.health_port, c.data_dir), (7411, PathBuf::from("/d")));
        assert!(run(&["--health-port", "x"], &env, false).is_err());
        assert!(run(&["--health-port"], &env, false).is_err());
        assert!(run(&["--bogus"], &env, false).is_err());
    }
}
