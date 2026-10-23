//! `arena-kiosk-svc dev provision` (feature `dev-fixtures`, never in release builds): writes
//! `config.json` and the protected token, standing in for real pairing (Epic 2).

use crate::config::{DEFAULT_PLAYER_USER, KioskConfig};
use crate::paths::DataDir;
use crate::secret::SecretStore;

/// What `dev provision` was given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provision {
    pub hub_addr: String,
    pub fingerprint: String,
    /// The device token `dev seed` printed: `<device_id>.<secret>`.
    pub token: String,
    pub player_user: Option<String>,
}

/// Validates the input, then writes the config and the token (the token only through
/// `secrets`). Returns the device id.
pub fn provision(
    data: &DataDir,
    secrets: &dyn SecretStore,
    p: &Provision,
) -> Result<String, String> {
    let (device_id, _) = p
        .token
        .split_once('.')
        .filter(|(id, s)| !id.is_empty() && !s.is_empty())
        .ok_or("the token must be <device_id>.<secret>, as `arena-hub dev seed` prints it")?;
    let cfg = KioskConfig {
        hub_addr: p.hub_addr.clone(),
        hub_fingerprint: p.fingerprint.to_ascii_lowercase().replace(':', ""),
        device_id: device_id.to_owned(),
        player_user: p
            .player_user
            .clone()
            .unwrap_or_else(|| DEFAULT_PLAYER_USER.to_owned()),
        app_path: None,
    };
    cfg.validate()?;
    std::fs::create_dir_all(data.root()).map_err(|e| format!("create data dir: {e}"))?;
    secrets
        .save(p.token.as_bytes())
        .map_err(|e| format!("save token: {e}"))?;
    cfg.save(&data.config())
        .map_err(|e| format!("save config: {e}"))?;
    Ok(cfg.device_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provision_writes_config_and_a_separate_token() {
        let dir = tempfile::tempdir().unwrap();
        let data = DataDir(dir.path().join("kiosk"));
        let store = crate::secret::platform_store(data.token());
        let token = format!("01JDEVICEAAAAAAAAAAAAAAAAA.{}", "f".repeat(64));
        let p = Provision {
            hub_addr: "127.0.0.1:7400".into(),
            fingerprint: "AB:".repeat(31) + "AB",
            token: token.clone(),
            player_user: None,
        };
        assert_eq!(
            provision(&data, store.as_ref(), &p).unwrap(),
            "01JDEVICEAAAAAAAAAAAAAAAAA"
        );
        let cfg = KioskConfig::load(&data.config()).unwrap().unwrap();
        assert_eq!(cfg.hub_fingerprint, "ab".repeat(32));
        assert_eq!(cfg.player_user, "player");
        let text = std::fs::read_to_string(data.config()).unwrap();
        assert!(
            !text.contains(&token[27..]),
            "the secret is not in config.json"
        );
        assert_eq!(store.load().unwrap().unwrap(), token.as_bytes());

        for bad in [
            Provision {
                token: "nodot".into(),
                ..p.clone()
            },
            Provision {
                hub_addr: "nohost".into(),
                ..p.clone()
            },
            Provision {
                fingerprint: "ab".into(),
                ..p.clone()
            },
        ] {
            assert!(provision(&data, store.as_ref(), &bad).is_err(), "{bad:?}");
        }
    }
}
