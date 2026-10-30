use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use arena_kiosk_core::{PAIR_TIMEOUT, PairError};
use arena_proto::{
    AppError, ErrorCode, KioskPairInfo, KioskPaired, PairRequest, SelectedHub, sanitize_pc_name,
};
use serde::Deserialize;
use serde_json::Value;

use crate::config::{DEFAULT_PLAYER_USER, KioskConfig};
use crate::runtime::Ctx;

pub const FALLBACK_PC_NAME: &str = "Kiosk";

#[derive(Deserialize)]
struct PairBody {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    pin: Option<String>,
    pc_name: String,
}

pub struct Refused {
    pub error: AppError,
    pub body: Value,
}

impl From<AppError> for Refused {
    fn from(error: AppError) -> Self {
        Self {
            error,
            body: Value::Null,
        }
    }
}

fn no_hub(message: &str) -> AppError {
    AppError::new(ErrorCode::NoHub, message)
}

fn internal(message: &str) -> AppError {
    AppError::new(ErrorCode::Internal, message)
}

pub fn machine_name() -> Option<String> {
    let from_env = ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .map(|v| v.trim().to_owned())
        .find(|v| !v.is_empty());
    if from_env.is_some() {
        return from_env;
    }
    let out = std::process::Command::new("hostname").output().ok()?;
    let v = String::from_utf8(out.stdout).ok()?;
    let v = v.trim();
    (!v.is_empty()).then(|| v.to_owned())
}

pub fn default_pc_name() -> String {
    machine_name()
        .map(|n| sanitize_pc_name(&n))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| FALLBACK_PC_NAME.to_owned())
}

pub fn info(ctx: &Ctx) -> KioskPairInfo {
    KioskPairInfo {
        pc_name: default_pc_name(),
        hub: crate::discovery::load_selected(&ctx.data),
    }
}

fn host_of(addr: &str) -> Option<&str> {
    let (host, port) = addr.rsplit_once(':')?;
    (!host.is_empty() && port.parse::<u16>().is_ok()).then_some(host)
}

fn running() -> &'static Mutex<HashSet<PathBuf>> {
    static RUNNING: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    RUNNING.get_or_init(Mutex::default)
}

struct Running(PathBuf);

impl Running {
    fn claim(root: &std::path::Path) -> Option<Self> {
        let mut set = running().lock().unwrap_or_else(|p| p.into_inner());
        set.insert(root.to_path_buf())
            .then(|| Self(root.to_path_buf()))
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        running()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.0);
    }
}

pub async fn pair(ctx: &Ctx, body: Value) -> Result<Value, Refused> {
    let b: PairBody = serde_json::from_value(body)
        .map_err(|_| AppError::new(ErrorCode::BadFrame, "pair takes {code | pin, pc_name}"))?;
    let Some(_guard) = Running::claim(ctx.data.root()) else {
        return Err(AppError::new(ErrorCode::HubSlow, "pairing is already running").into());
    };
    if ctx.config().is_some() && ctx.secrets.load().ok().flatten().is_some() {
        return Err(AppError::new(ErrorCode::AlreadySet, "this kiosk is already paired").into());
    }
    let selected = crate::discovery::load_selected(&ctx.data)
        .ok_or_else(|| no_hub("choose a hub before pairing"))?;
    let host = host_of(&selected.addr)
        .ok_or_else(|| no_hub("the chosen hub address is not usable"))?
        .to_owned();
    let request = PairRequest {
        code: b.code,
        pin: b.pin,
        pc_name: b.pc_name,
        version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    request.validate()?;
    // A branch tunnel address pairs over Cloudflare's TLS (Story 8.41); a LAN Hub, over TLS
    // pinned to the fingerprint the kiosk chose.
    let answer = if crate::discovery::is_remote_addr(&selected.addr) {
        arena_kiosk_core::pair_remote(&host, &request, PAIR_TIMEOUT).await
    } else {
        arena_kiosk_core::pair(
            &host,
            selected.web_port,
            &selected.fingerprint,
            &request,
            PAIR_TIMEOUT,
        )
        .await
    };
    drop(request);
    let paired = match answer {
        Ok(p) => p,
        Err(PairError::Hub {
            error,
            retry_after_s,
        }) => {
            tracing::info!(code = error.code.as_str(), "the hub refused pairing");
            let body = match retry_after_s {
                Some(s) if error.code == ErrorCode::PairPinPaused => {
                    serde_json::json!({ "retry_after_s": s })
                }
                _ => Value::Null,
            };
            return Err(Refused { error, body });
        }
        Err(PairError::PinMismatch {
            expected,
            presented,
        }) => {
            tracing::error!(
                hub = %selected.addr,
                expected_fingerprint = %expected,
                presented_fingerprint = %presented,
                "pairing refused: the hub certificate does not match the chosen hub"
            );
            return Err(no_hub("the hub's certificate does not match").into());
        }
        Err(e) => {
            tracing::warn!(hub = %selected.addr, error = %e, "pairing could not reach the hub");
            return Err(no_hub("the hub could not be reached").into());
        }
    };
    store(
        ctx,
        &selected,
        &paired.device_id,
        &paired.token,
        &paired.hub_fingerprint,
    )?;
    tracing::info!(device_id = %paired.device_id, hub = %selected.addr, "paired with the hub");
    serde_json::to_value(KioskPaired {
        device_id: paired.device_id,
        branch_name: selected.branch_name,
    })
    .map_err(|_| internal("the pairing answer could not be built").into())
}

fn store(
    ctx: &Ctx,
    selected: &SelectedHub,
    device_id: &str,
    token: &str,
    fingerprint: &str,
) -> Result<(), AppError> {
    let player_user = ctx
        .config()
        .map(|c| c.player_user)
        .unwrap_or_else(|| DEFAULT_PLAYER_USER.to_owned());
    let config = KioskConfig {
        hub_addr: selected.addr.clone(),
        hub_fingerprint: fingerprint.to_ascii_lowercase(),
        device_id: device_id.to_owned(),
        player_user,
        app_path: None,
    };
    ctx.provision(&config, token.as_bytes()).map_err(|e| {
        tracing::error!(error = %e, "the pairing could not be stored");
        internal("the pairing could not be saved")
    })?;
    crate::discovery::clear_selected(&ctx.data);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_is_taken_from_the_chosen_address() {
        assert_eq!(host_of("192.168.1.10:7400"), Some("192.168.1.10"));
        assert_eq!(host_of("[fe80::1]:7400"), Some("[fe80::1]"));
        assert_eq!(host_of("nohost"), None);
        assert_eq!(host_of(":7400"), None);
    }

    #[test]
    fn the_default_name_is_printable_and_bounded() {
        let name = default_pc_name();
        assert!(arena_proto::is_pc_name(&name), "{name:?}");
    }
}
