//! Arena 360 Kiosk UI: the Tauri shell around the `@arena/kiosk` frontend. It runs as the
//! unprivileged player and talks only to the Kiosk service (`arena-kiosk-svc`), through
//! [`bridge`]. The window covers the whole screen.

// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;

use std::sync::Arc;

use arena_proto::KioskHubStatus;
use bridge::{Bridge, Emit, HUB_STATUS_EVENT, RESTORE_STATE_EVENT};
use tauri::{Emitter, Manager};

fn crate_name() -> &'static str {
    "arena-kiosk-ui"
}

/// The webview's restorable state changed (its route, for now).
#[tauri::command]
fn report_app_state(state: serde_json::Value, bridge: tauri::State<'_, Arc<Bridge>>) {
    bridge.report(state);
}

/// The Hub link state, if the service has said.
#[tauri::command]
fn hub_status(bridge: tauri::State<'_, Arc<Bridge>>) -> Option<KioskHubStatus> {
    bridge.hub_status()
}

/// The webview is alive (every second); the shell answers the service's pings only while it is.
#[tauri::command]
fn heartbeat(bridge: tauri::State<'_, Arc<Bridge>>) {
    bridge.heartbeat();
}

/// The state to restore after a restart, once.
#[tauri::command]
fn take_restore_state(bridge: tauri::State<'_, Arc<Bridge>>) -> Option<serde_json::Value> {
    bridge.take_restore()
}

fn main() {
    tauri::Builder::default()
        .manage(Arc::new(Bridge::default()))
        .invoke_handler(tauri::generate_handler![
            report_app_state,
            hub_status,
            take_restore_state,
            heartbeat
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let bridge = Arc::clone(app.state::<Arc<Bridge>>().inner());
            tauri::async_runtime::spawn(async move {
                let emit = move |e: Emit| {
                    let _ = match e {
                        Emit::HubStatus(s) => handle.emit(HUB_STATUS_EVENT, s),
                        Emit::RestoreState(v) => handle.emit(RESTORE_STATE_EVENT, v),
                    };
                };
                bridge.run(&emit).await;
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .unwrap_or_else(|err| panic!("{}: failed to start: {err}", crate_name()));
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name_is_set() {
        assert_eq!(super::crate_name(), "arena-kiosk-ui");
    }
}
