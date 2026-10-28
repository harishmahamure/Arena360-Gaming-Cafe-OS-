//! Arena 360 Kiosk UI: the Tauri shell around the `@arena/kiosk` frontend. It runs as the
//! unprivileged player and talks only to the Kiosk service (`arena-kiosk-svc`), through
//! [`bridge`]. The window is the player's shell: topmost over every monitor, never closed, with
//! the Windows keys and task switchers swallowed ([`shell`]).

// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod shell;
mod theme;

use std::sync::Arc;

use arena_proto::KioskHubStatus;
use bridge::{
    ADMIN_EXIT_ENDED_EVENT, ADMIN_EXIT_EVENT, ATTRACT_EVENT, Answer, Attract, Bridge,
    DISCOVERY_DONE_EVENT, DISCOVERY_FOUND_EVENT, Emit, HUB_STATUS_EVENT, PC_SCREEN_EVENT, PcScreen,
    RESTORE_STATE_EVENT, TEST_SESSION_ENDED_EVENT, TEST_SESSION_EVENT, THEME_EVENT, TOAST_EVENT,
};
use tauri::{Emitter, Manager, Runtime, WebviewWindow, WebviewWindowBuilder, WindowEvent};

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

#[tauri::command]
fn pc_screen(bridge: tauri::State<'_, Arc<Bridge>>) -> PcScreen {
    bridge.pc_screen()
}

/// The attract view and logo the service last sent (a cold start asks, then follows the event).
#[tauri::command]
fn attract(bridge: tauri::State<'_, Arc<Bridge>>) -> Attract {
    bridge.attract()
}

#[tauri::command]
async fn pc_dismiss(bridge: tauri::State<'_, Arc<Bridge>>) -> Result<Option<Answer>, ()> {
    Ok(service_request(&bridge, "pc_dismiss", serde_json::Value::Null).await)
}

#[tauri::command]
fn test_session(bridge: tauri::State<'_, Arc<Bridge>>) -> Option<u64> {
    bridge.test_remaining_ms()
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

async fn service_request(bridge: &Bridge, kind: &str, body: serde_json::Value) -> Option<Answer> {
    let kind = kind.to_owned();
    bridge
        .request(move |rid| arena_kiosk_core::ipc::IpcMsg::Request { rid, kind, body })
        .await
}

#[tauri::command]
async fn discover(bridge: tauri::State<'_, Arc<Bridge>>) -> Result<Option<Answer>, ()> {
    Ok(service_request(&bridge, "discover", serde_json::Value::Null).await)
}

#[tauri::command]
async fn probe_ip(ip: String, bridge: tauri::State<'_, Arc<Bridge>>) -> Result<Option<Answer>, ()> {
    Ok(service_request(&bridge, "probe_ip", serde_json::json!({ "ip": ip })).await)
}

#[tauri::command]
async fn select_hub(
    addr: String,
    fingerprint: String,
    bridge: tauri::State<'_, Arc<Bridge>>,
) -> Result<Option<Answer>, ()> {
    let body = serde_json::json!({ "addr": addr, "fingerprint": fingerprint });
    Ok(service_request(&bridge, "select_hub", body).await)
}

#[tauri::command]
async fn pair(
    code: Option<String>,
    pin: Option<String>,
    pc_name: String,
    bridge: tauri::State<'_, Arc<Bridge>>,
) -> Result<Option<Answer>, ()> {
    let mut body = serde_json::Map::new();
    if let Some(code) = code {
        body.insert("code".into(), code.into());
    }
    if let Some(pin) = pin {
        body.insert("pin".into(), pin.into());
    }
    body.insert("pc_name".into(), pc_name.into());
    Ok(service_request(&bridge, "pair", serde_json::Value::Object(body)).await)
}

#[tauri::command]
async fn pair_info(bridge: tauri::State<'_, Arc<Bridge>>) -> Result<Option<Answer>, ()> {
    Ok(service_request(&bridge, "pair_info", serde_json::Value::Null).await)
}

/// Whether this kiosk is high contrast now (the service's last word, or the first-paint
/// decision), so a reloaded page re-applies it.
#[tauri::command]
fn theme(bridge: tauri::State<'_, Arc<Bridge>>) -> bool {
    bridge.theme().unwrap_or(false)
}

/// Where the last theme is saved (`<app local data>/theme.json`), if the directory is known.
fn theme_file<R: Runtime>(handle: &tauri::AppHandle<R>) -> Option<std::path::PathBuf> {
    handle
        .path()
        .app_local_data_dir()
        .ok()
        .map(|d| theme::file_in(&d))
}

/// Builds the hidden `main` window from its configuration with the theme set before the first
/// script runs: the service's theme if it answers within [`theme::SERVICE_WAIT`], else the one
/// saved last time ([`theme::initial`]).
fn build_window<R: Runtime>(app: &tauri::App<R>, bridge: &Arc<Bridge>) -> Option<WebviewWindow<R>> {
    let from_service = tauri::async_runtime::block_on(bridge.wait_theme(theme::SERVICE_WAIT));
    let saved = theme_file(app.handle()).and_then(|p| theme::read(&p));
    let hc = theme::initial(from_service, saved);
    if from_service.is_none() {
        tracing::info!(saved = ?saved, hc, "no theme from the kiosk service in time; using the saved one");
    }
    let Some(config) = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .cloned()
    else {
        tracing::error!("no `main` window in tauri.conf.json");
        return None;
    };
    // The service may have answered just after the wait: the latest word wins.
    let hc = bridge.theme().unwrap_or(hc);
    bridge.settle_theme(hc);
    match WebviewWindowBuilder::from_config(app.handle(), &config)
        .map(|b| b.initialization_script(theme::init_script(hc)).build())
    {
        Ok(Ok(w)) => Some(w),
        Ok(Err(e)) | Err(e) => {
            tracing::error!(error = %e, "the kiosk window could not be built");
            None
        }
    }
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
            admin_exit_status,
            test_session,
            theme,
            discover,
            probe_ip,
            select_hub,
            pair,
            pair_info,
            pc_screen,
            pc_dismiss,
            attract
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
            // The service link first: its theme decides the first paint.
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
                        Emit::TestSession(remaining_ms) => handle.emit(
                            TEST_SESSION_EVENT,
                            serde_json::json!({ "remaining_ms": remaining_ms }),
                        ),
                        Emit::TestSessionEnded => handle.emit(TEST_SESSION_ENDED_EVENT, ()),
                        Emit::PcScreen(screen) => handle.emit(PC_SCREEN_EVENT, screen),
                        Emit::Attract(shown) => handle.emit(ATTRACT_EVENT, shown),
                        Emit::Theme(hc) => {
                            if let Some(path) = theme_file(&handle)
                                && let Err(e) = theme::write(&path, hc)
                            {
                                tracing::warn!(error = %e, "could not save the theme");
                            }
                            handle.emit(THEME_EVENT, serde_json::json!({ "hc": hc }))
                        }
                        Emit::DiscoveryFound(search, hubs) => handle.emit(
                            DISCOVERY_FOUND_EVENT,
                            serde_json::json!({ "search": search, "hubs": hubs }),
                        ),
                        Emit::DiscoveryDone(search, hubs) => handle.emit(
                            DISCOVERY_DONE_EVENT,
                            serde_json::json!({ "search": search, "hubs": hubs }),
                        ),
                    };
                };
                bridge.run(&emit).await;
            });
            let bridge = Arc::clone(app.state::<Arc<Bridge>>().inner());
            if let Some(window) = build_window(app, &bridge) {
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
                // `create: false`: nothing else builds the window. Exit with a failure so the
                // service's watchdog relaunches the app (the screen stays covered meanwhile).
                tracing::error!("no kiosk window; exiting so the service relaunches the app");
                std::process::exit(1);
            }
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
