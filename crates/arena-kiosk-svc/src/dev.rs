//! Dev fixtures (feature `dev-fixtures`, never in release builds):
//!
//! - `arena-kiosk-svc dev provision` writes `config.json` and the protected token, standing in
//!   for real pairing (Epic 2).
//! - `--console --dev-fake-procs` enforces the process lists against an in-memory process
//!   table ([`FakeSystem`]) with a logged-on `player`, so a console-mode kiosk off Windows can
//!   block a process; without `--app`, its launcher ([`DevLauncher`]) also stands in for the
//!   admin exit's desktop (nothing to start, so a granted exit is not refused).
//! - The IPC requests `dev.*` ([`handle`]), sent on the app channel by a test driver standing
//!   in for the app:
//!   - `dev.spawn_process {path}`: starts a fake player process at `path` (needs
//!     `--dev-fake-procs`); answers `{pid}`.
//!   - `dev.hub_link {online}`: `false` holds the Hub link down (closing it) as if the Hub
//!     were unreachable, `true` releases it.
//!   - `dev.relock`: ends the running admin exit now (as its 15 minutes running out would).
//!   - `dev.status`: `{hub_state, verifier_version, admin_exit_until, queued}`.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use arena_proto::{AppError, ErrorCode};
use serde_json::Value;

use crate::config::{DEFAULT_PLAYER_USER, KioskConfig};
use crate::paths::DataDir;
use crate::procs::fake::{FakeProc, FakeSystem};
use crate::procs::{EnforceOptions, ProcessApi};
use crate::runtime::Ctx;
use crate::secret::SecretStore;

/// The IPC request kinds this module answers start with this.
pub const REQUEST_PREFIX: &str = "dev.";
/// The fake player's SID and session.
pub const FAKE_PLAYER_SID: &str = "S-1-5-21-1000-2000-3000-1001";
const FAKE_SESSION: u32 = 1;
/// The fake player's shell process, the parent of every spawned process.
const FAKE_LAUNCHER: u32 = 2;
/// The fake install directory.
pub const FAKE_INSTALL_DIR: &str = r"C:\Program Files\Arena 360 Kiosk";

static FAKE: OnceLock<Arc<FakeSystem>> = OnceLock::new();
static NEXT_PID: AtomicU32 = AtomicU32::new(1000);

/// The fake process table `--dev-fake-procs` enforces against (created on first use): the
/// player is logged on and its shell runs.
pub fn fake_system() -> Arc<FakeSystem> {
    Arc::clone(FAKE.get_or_init(|| {
        let sys = Arc::new(FakeSystem::default());
        sys.set_player(DEFAULT_PLAYER_USER, FAKE_PLAYER_SID, FAKE_SESSION);
        sys.add_running(
            FAKE_LAUNCHER,
            FakeProc {
                session_id: FAKE_SESSION,
                user_sid: FAKE_PLAYER_SID.into(),
                path: r"C:\Windows\System32\userinit.exe".into(),
                parent: None,
            },
        );
        sys
    }))
}

/// The process API and options for enforcing against [`fake_system`].
pub fn fake_enforcement() -> (ProcessApi, EnforceOptions) {
    let opts = EnforceOptions {
        install_dir: Some(FAKE_INSTALL_DIR.into()),
        app_image: Some(format!(r"{FAKE_INSTALL_DIR}\arena-kiosk-ui.exe").into()),
        app_config: None,
        own_images: vec![format!(r"{FAKE_INSTALL_DIR}\arena-kiosk-svc.exe").into()],
        system_root: r"C:\Windows".into(),
    };
    (ProcessApi::of(fake_system()), opts)
}

/// The console launcher with `--dev-fake-procs` and no `--app`: there is no app to restart
/// and no desktop to start, and both succeed (logged).
pub struct DevLauncher;

impl crate::launcher::AppLauncher for DevLauncher {
    fn restart(&self) -> Result<(), String> {
        tracing::info!("dev: the app would be restarted (no app in this console run)");
        Ok(())
    }

    fn start_desktop(&self) -> Result<(), String> {
        tracing::info!("dev: the admin exit's desktop would start (none in this console run)");
        Ok(())
    }

    fn stop_desktop(&self) {
        tracing::info!("dev: the admin exit's desktop would stop");
    }
}

fn bad(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::BadFrame, message)
}

/// Answers one `dev.*` IPC request.
pub async fn handle(ctx: &Arc<Ctx>, kind: &str, body: Value) -> Result<Value, AppError> {
    match kind {
        "dev.spawn_process" => {
            let path = body
                .get("path")
                .and_then(Value::as_str)
                .filter(|p| !p.is_empty())
                .ok_or_else(|| bad("dev.spawn_process needs {path}"))?;
            let sys = FAKE
                .get()
                .ok_or_else(|| bad("dev.spawn_process needs --dev-fake-procs"))?;
            let pid = NEXT_PID.fetch_add(1, Ordering::SeqCst);
            sys.spawn(
                pid,
                FakeProc {
                    session_id: FAKE_SESSION,
                    user_sid: FAKE_PLAYER_SID.into(),
                    path: path.to_owned(),
                    parent: Some(FAKE_LAUNCHER),
                },
            );
            Ok(serde_json::json!({ "pid": pid }))
        }
        "dev.hub_link" => {
            let online = body
                .get("online")
                .and_then(Value::as_bool)
                .ok_or_else(|| bad("dev.hub_link needs {online}"))?;
            tracing::info!(
                online,
                "dev: hub link {}",
                if online { "released" } else { "held down" }
            );
            ctx.link_hold.send_replace(!online);
            Ok(serde_json::json!({}))
        }
        "dev.relock" => {
            crate::admin_exit::relock(ctx).await;
            Ok(serde_json::json!({}))
        }
        "dev.status" => Ok(serde_json::json!({
            "hub_state": *ctx.hub_state.borrow(),
            "verifier_version": ctx.admin.verifier_version(),
            "admin_exit_until": ctx.admin.active.borrow().as_ref().map(|e| e.until),
            "queued": ctx.queue_len(),
        })),
        other => Err(AppError::new(
            ErrorCode::Unsupported,
            format!("unknown dev request {other}"),
        )),
    }
}

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
