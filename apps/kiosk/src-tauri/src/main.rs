//! Arena 360 Kiosk UI: the Tauri shell around the `@arena/kiosk` frontend. It runs as the
//! unprivileged player and talks only to the Kiosk service (`arena-kiosk-svc`), through
//! [`bridge`]. The window is the player's shell: topmost over every monitor, never closed, with
//! the Windows keys and task switchers swallowed ([`shell`]).

// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod shell;

use std::sync::Arc;

use arena_proto::KioskHubStatus;
use bridge::{
    ADMIN_EXIT_ENDED_EVENT, ADMIN_EXIT_EVENT, Answer, Bridge, Emit, HUB_STATUS_EVENT,
    RESTORE_STATE_EVENT, TOAST_EVENT,
};
use tauri::{Emitter, Manager, Runtime, WebviewWindow, WindowEvent};

fn crate_name() -> &'static str {
    "arena-kiosk-ui"
}

/// Covers every monitor and tells the webview where the primary monitor is (so the locked
/// placeholder sits on it, not stretched across all of them).
fn place_window<R: Runtime>(window: &WebviewWindow<R>) {
    shell::cover_all_monitors(window);
    if let Some(b) = shell::content_box(window) {
        // Physical px relative to the window; the page divides by devicePixelRatio.
        let js = format!(
            "window.__ARENA_PRIMARY__ = {{ x: {}, y: {}, width: {}, height: {} }};\
             window.dispatchEvent(new CustomEvent('arena-primary'));",
            b.x, b.y, b.width, b.height
        );
        let _ = window.eval(&js);
    }
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

/// The hidden admin-exit panel's PIN (Story 1.20): sent to the service only, as IPC
/// `admin_exit{pin}`, and dropped here. `None` when the service did not answer.
#[tauri::command]
async fn admin_exit(
    pin: String,
    bridge: tauri::State<'_, Arc<Bridge>>,
) -> Result<Option<Answer>, ()> {
    let pin = arena_kiosk_core::ipc::SecretPin::new(pin);
    Ok(bridge
        .request(move |rid| arena_kiosk_core::ipc::IpcMsg::AdminExit { rid, pin })
        .await)
}

/// Whether the admin-exit prompt is paused (`retry_after_s`) and whether an exit runs (`until`).
#[tauri::command]
async fn admin_exit_status(bridge: tauri::State<'_, Arc<Bridge>>) -> Result<Option<Answer>, ()> {
    Ok(bridge
        .request(|rid| arena_kiosk_core::ipc::IpcMsg::Request {
            rid,
            kind: "admin_exit_status".into(),
            body: serde_json::Value::Null,
        })
        .await)
}

/// Admin exit started (`Some`) or ended (`None`): the lockdown is suspended and the window
/// becomes the corner badge, or the window covers every screen again and locks down.
fn on_admin_exit<R: Runtime>(handle: &tauri::AppHandle<R>, running: bool) {
    if shell::dev_unlocked() {
        return;
    }
    shell::set_lockdown_suspended(running);
    let h = handle.clone();
    let _ = handle.run_on_main_thread(move || {
        if let Some(w) = h.get_webview_window("main") {
            if running {
                shell::place_badge(&w);
            } else {
                place_window(&w);
            }
        }
    });
}

fn main() {
    // One kiosk per machine: a second instance (e.g. one the service relaunched while the
    // shell-launched one still runs) exits at once, so there are never two.
    if !shell::dev_unlocked() && !shell::claim_single_instance() {
        eprintln!("{}: another instance is already running", crate_name());
        return;
    }
    tauri::Builder::default()
        .manage(Arc::new(Bridge::default()))
        .invoke_handler(tauri::generate_handler![
            report_app_state,
            hub_status,
            take_restore_state,
            heartbeat,
            admin_exit,
            admin_exit_status
        ])
        .on_window_event(|_window, event| {
            // The kiosk is the shell: nothing closes it (admin exit comes in Story 1.20).
            if let WindowEvent::CloseRequested { api, .. } = event
                && !shell::dev_unlocked()
            {
                api.prevent_close();
            }
        })
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                if shell::dev_unlocked() {
                    let _ = window.set_always_on_top(false);
                    let _ = window.show();
                } else {
                    shell::harden_webview(&window);
                    place_window(&window);
                    #[cfg(windows)]
                    {
                        let handle = app.handle().clone();
                        let bridge = Arc::clone(app.state::<Arc<Bridge>>().inner());
                        let reassert = {
                            let handle = handle.clone();
                            move || {
                                let h = handle.clone();
                                let _ = handle.run_on_main_thread(move || {
                                    if let Some(w) = h.get_webview_window("main") {
                                        place_window(&w);
                                    }
                                });
                            }
                        };
                        let on_degraded = move |degraded: bool| {
                            if degraded {
                                tracing::error!("the kiosk keyboard hook is not active");
                            }
                            bridge.set_lockdown_degraded(degraded);
                        };
                        shell::start_input_thread(reassert, on_degraded);
                    }
                }
            } else {
                // No window is a serious misconfiguration, but the hook must still run so the
                // Windows keys and task switchers are swallowed.
                tracing::error!("no kiosk window; starting the input guard anyway");
                #[cfg(windows)]
                if !shell::dev_unlocked() {
                    let bridge = Arc::clone(app.state::<Arc<Bridge>>().inner());
                    shell::start_input_thread(
                        || {},
                        move |degraded| bridge.set_lockdown_degraded(degraded),
                    );
                }
            }
            let handle = app.handle().clone();
            let bridge = Arc::clone(app.state::<Arc<Bridge>>().inner());
            tauri::async_runtime::spawn(async move {
                let emit = move |e: Emit| {
                    let _ = match e {
                        Emit::HubStatus(s) => handle.emit(HUB_STATUS_EVENT, s),
                        Emit::RestoreState(v) => handle.emit(RESTORE_STATE_EVENT, v),
                        Emit::Toast(key) => handle.emit(TOAST_EVENT, key),
                        Emit::AdminExit(until) => {
                            on_admin_exit(&handle, true);
                            handle.emit(ADMIN_EXIT_EVENT, serde_json::json!({ "until": until }))
                        }
                        Emit::AdminExitEnded => {
                            on_admin_exit(&handle, false);
                            handle.emit(ADMIN_EXIT_ENDED_EVENT, ())
                        }
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
