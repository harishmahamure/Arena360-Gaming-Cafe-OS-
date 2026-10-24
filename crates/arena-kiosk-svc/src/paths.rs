//! Where the service keeps its files.
//!
//! - Windows: `%PROGRAMDATA%\Arena360\Kiosk` (protected to SYSTEM and Administrators by the
//!   service and by `lockdown`; see `winsec`).
//! - Elsewhere (dev and tests only): `$ARENA_KIOSK_DATA_DIR`, else `~/.arena360/kiosk`.
//!
//! `--data-dir` overrides both. The directory holds `config.json`, `token.bin`, `queue.db`,
//! `lockdown-backup.json`, `logs/` and, off Windows, the IPC socket `ipc/kiosk.sock` (the
//! `ipc/` directory is 0700).

use std::path::{Path, PathBuf};

pub const CONFIG_FILE: &str = "config.json";
pub const TOKEN_FILE: &str = "token.bin";
/// The cached manager verifier, next to the token.
pub const VERIFIER_FILE: &str = "verifier.bin";
pub const QUEUE_FILE: &str = "queue.db";
pub const LOGS_DIR: &str = "logs";
pub const LOCKDOWN_BACKUP_FILE: &str = "lockdown-backup.json";

/// The data directory when none is given on the command line.
pub fn default_data_dir(env: impl Fn(&str) -> Option<String>, windows: bool) -> PathBuf {
    if windows {
        let base = env("PROGRAMDATA").unwrap_or_else(|| r"C:\ProgramData".into());
        return PathBuf::from(base).join("Arena360").join("Kiosk");
    }
    arena_kiosk_core::ipc::dev_data_dir(env)
}

/// The files under one data directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDir(pub PathBuf);

impl DataDir {
    pub fn root(&self) -> &Path {
        &self.0
    }
    pub fn config(&self) -> PathBuf {
        self.0.join(CONFIG_FILE)
    }
    pub fn token(&self) -> PathBuf {
        self.0.join(TOKEN_FILE)
    }
    pub fn queue(&self) -> PathBuf {
        self.0.join(QUEUE_FILE)
    }
    /// The cached manager verifier (DPAPI-protected, machine scope, on Windows).
    pub fn verifier(&self) -> PathBuf {
        self.0.join(VERIFIER_FILE)
    }
    pub fn logs(&self) -> PathBuf {
        self.0.join(LOGS_DIR)
    }
    /// What `lockdown apply` replaced in the player's hive (`lockdown restore` puts it back).
    pub fn lockdown_backup(&self) -> PathBuf {
        self.0.join(LOCKDOWN_BACKUP_FILE)
    }
    /// The IPC socket (non-Windows builds), inside a 0700 directory.
    pub fn socket(&self) -> PathBuf {
        arena_kiosk_core::ipc::socket_path(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_per_platform() {
        let env = |k: &str| match k {
            "PROGRAMDATA" => Some(r"D:\PD".to_owned()),
            "HOME" => Some("/home/k".to_owned()),
            _ => None,
        };
        assert_eq!(
            default_data_dir(env, true),
            PathBuf::from(r"D:\PD").join("Arena360").join("Kiosk")
        );
        assert_eq!(
            default_data_dir(env, false),
            PathBuf::from("/home/k/.arena360/kiosk")
        );
        let env = |k: &str| (k == "ARENA_KIOSK_DATA_DIR").then(|| "/tmp/k".to_owned());
        assert_eq!(default_data_dir(env, false), PathBuf::from("/tmp/k"));
    }
}
