//! `config.json`: which Hub to talk to and how. The device token is never stored here; it lives
//! in the secret store (`token.bin`).

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The Windows account the Kiosk app runs as when none is configured.
pub const DEFAULT_PLAYER_USER: &str = "player";

/// The service configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KioskConfig {
    /// `host:port` of the Hub's paired-app listener (`:7400`).
    pub hub_addr: String,
    /// The SHA-256 fingerprint of the Hub's certificate (64 hex digits); the only certificate
    /// the service accepts.
    pub hub_fingerprint: String,
    /// This kiosk's device id on the Hub.
    pub device_id: String,
    /// The account the Kiosk app runs as; the only one (with SYSTEM) allowed on the pipe.
    #[serde(default = "default_player_user")]
    pub player_user: String,
    /// The Kiosk app executable the watchdog relaunches (default: `arena-kiosk-ui.exe` next to
    /// the service).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_path: Option<String>,
}

/// A plain local account name: 1–64 of letters, digits, `.`, `_` and `-`.
pub fn valid_account_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

fn default_player_user() -> String {
    DEFAULT_PLAYER_USER.to_owned()
}

impl KioskConfig {
    /// The Hub URL: always `wss://` (pinned TLS, no plain fallback).
    pub fn hub_url(&self) -> String {
        format!("wss://{}/", self.hub_addr)
    }

    /// Checks the fields: a `host:port` address, a 64-hex-digit fingerprint, a device id and a
    /// plain account name.
    pub fn validate(&self) -> Result<(), String> {
        let (host, port) = self
            .hub_addr
            .rsplit_once(':')
            .ok_or_else(|| "hub address must be host:port".to_owned())?;
        if host.is_empty() || port.parse::<u16>().map_or(true, |p| p == 0) {
            return Err("hub address must be host:port".into());
        }
        // An IPv6 literal must be bracketed (`[fe80::1]:7400`), or host and port are ambiguous.
        if host.contains(':') && !(host.starts_with('[') && host.ends_with(']')) {
            return Err("an IPv6 hub address must be bracketed: [addr]:port".into());
        }
        arena_kiosk_core::parse_fingerprint(&self.hub_fingerprint)
            .map_err(|_| "the fingerprint must be 64 hex digits".to_owned())?;
        if self.device_id.is_empty() || !self.device_id.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err("the device id must be letters and digits".into());
        }
        if !valid_account_name(&self.player_user) {
            return Err("the player account name is not valid".into());
        }
        Ok(())
    }

    /// Reads and validates `path`; `Ok(None)` when the file does not exist (not provisioned).
    pub fn load(path: &Path) -> io::Result<Option<Self>> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        check_owner(path)?;
        let bad = |e: String| io::Error::new(io::ErrorKind::InvalidData, e);
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| bad(e.to_string()))?;
        // Unknown keys are ignored (a newer release may add some), but a credential never
        // belongs here: refuse the file rather than keep a token in plain text.
        if ["token", "credential", "device_token"]
            .iter()
            .any(|k| value.get(k).is_some())
        {
            return Err(bad("config.json must not hold the device token".into()));
        }
        let cfg: Self = serde_json::from_value(value).map_err(|e| bad(e.to_string()))?;
        cfg.validate()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(Some(cfg))
    }

    /// Writes `path` atomically (a temp file, then a rename).
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        write_atomic(path, text.as_bytes())
    }
}

pub fn write_provisioning(
    data: &crate::paths::DataDir,
    secrets: &dyn crate::secret::SecretStore,
    cfg: &KioskConfig,
    token: &[u8],
) -> Result<(), String> {
    cfg.validate()?;
    std::fs::create_dir_all(data.root()).map_err(|e| format!("create data dir: {e}"))?;
    secrets
        .save(token)
        .map_err(|e| format!("save token: {e}"))?;
    cfg.save(&data.config())
        .map_err(|e| format!("save config: {e}"))
}

/// On Windows, refuses a file not owned by SYSTEM or Administrators (one another account could
/// have planted). A no-op elsewhere.
pub(crate) fn check_owner(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    if !crate::winsec::owner_is_trusted(path)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} is not owned by SYSTEM or Administrators; refusing it",
                path.display()
            ),
        ));
    }
    let _ = path;
    Ok(())
}

/// Writes `bytes` to `path` through a fresh sibling temp file (created with `create_new`, so a
/// file planted under the temp name is never written through) and a rename.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", ulid::Ulid::generate()));
    let written = (|| {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> KioskConfig {
        KioskConfig {
            hub_addr: "192.168.1.10:7400".into(),
            hub_fingerprint: "ab".repeat(32),
            device_id: "01JDEVICEAAAAAAAAAAAAAAAAA".into(),
            player_user: DEFAULT_PLAYER_USER.into(),
            app_path: None,
        }
    }

    #[test]
    fn round_trips_and_never_carries_a_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        assert_eq!(KioskConfig::load(&path).unwrap(), None);
        cfg().save(&path).unwrap();
        assert_eq!(KioskConfig::load(&path).unwrap(), Some(cfg()));
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            ["device_id", "hub_addr", "hub_fingerprint", "player_user"]
        );
        // A token smuggled in is refused, not silently kept.
        std::fs::write(
            &path,
            r#"{"hub_addr":"h:1","hub_fingerprint":"","device_id":"D","token":"x"}"#,
        )
        .unwrap();
        assert!(KioskConfig::load(&path).is_err());
        assert_eq!(cfg().hub_url(), "wss://192.168.1.10:7400/");
    }

    #[test]
    fn validation() {
        assert!(cfg().validate().is_ok());
        assert!(
            KioskConfig {
                hub_addr: "[fe80::1]:7400".into(),
                ..cfg()
            }
            .validate()
            .is_ok()
        );
        for bad in [
            KioskConfig {
                hub_addr: "nohost".into(),
                ..cfg()
            },
            KioskConfig {
                hub_addr: "h:0".into(),
                ..cfg()
            },
            KioskConfig {
                hub_fingerprint: "ab".into(),
                ..cfg()
            },
            KioskConfig {
                device_id: "a/b".into(),
                ..cfg()
            },
            KioskConfig {
                player_user: "evil user".into(),
                ..cfg()
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
    }
}
