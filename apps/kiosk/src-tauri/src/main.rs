//! Arena 360 Kiosk UI: the Tauri shell around the `@arena/kiosk` frontend. It runs as the
//! unprivileged player and talks only to the Kiosk service (`arena-kiosk-svc`), through
//! [`bridge`]. The window is the player's shell: topmost over every monitor, never closed, with
//! the Windows keys and task switchers swallowed ([`shell`]).

// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod games;
mod hud;
mod shell;
mod theme;

use std::sync::Arc;

use arena_proto::KioskHubStatus;
use bridge::{
    ADMIN_EXIT_ENDED_EVENT, ADMIN_EXIT_EVENT, ATTRACT_EVENT, Answer, Attract, Bridge,
    DISCOVERY_DONE_EVENT, DISCOVERY_FOUND_EVENT, Emit, GAME_LAUNCH_FAILED_EVENT, HUB_STATUS_EVENT,
    LaunchOutcome, PC_SCREEN_EVENT, PcScreen, RESTORE_STATE_EVENT, STAFF_MESSAGE_EVENT,
    STAFF_MESSAGES_CLEARED_EVENT, StaffMessage, TEST_SESSION_ENDED_EVENT, TEST_SESSION_EVENT,
    THEME_EVENT, TOAST_EVENT,
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

/// The session's staff messages, newest first (a cold start asks, then follows the events).
#[tauri::command]
fn staff_messages(bridge: tauri::State<'_, Arc<Bridge>>) -> Vec<StaffMessage> {
    bridge.staff_messages()
}

/// The player opened the messages bell: the service marks every message read.
#[tauri::command]
async fn messages_read(bridge: tauri::State<'_, Arc<Bridge>>) -> Result<Option<Answer>, ()> {
    let answer = service_request(&bridge, "messages_read", serde_json::Value::Null).await;
    if answer.as_ref().is_some_and(|a| a.ok) {
        bridge.mark_messages_read();
    }
    Ok(answer)
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

/// A player's login (Story 6.8): the username and password go to the service only, as IPC
/// `player_login`, and are dropped here. The answer is the Hub's outcome (`{outcome, ...}`);
/// `None` when the service did not answer.
#[tauri::command]
async fn player_login(
    username: String,
    password: String,
    bridge: tauri::State<'_, Arc<Bridge>>,
) -> Result<Option<Answer>, ()> {
    let password = arena_kiosk_core::ipc::SecretPin::new(password);
    Ok(bridge
        .request(move |rid| arena_kiosk_core::ipc::IpcMsg::PlayerLogin {
            rid,
            username,
            password,
        })
        .await)
}

/// The plan picker's options for the logged-in player (Story 6.9): IPC `player_plans`, answered with
/// the Hub's `player_plans_result` as the body. `None` when the service did not answer.
#[tauri::command]
async fn player_plans(bridge: tauri::State<'_, Arc<Bridge>>) -> Result<Option<Answer>, ()> {
    Ok(bridge
        .request(|rid| arena_kiosk_core::ipc::IpcMsg::PlayerPlans { rid })
        .await)
}

/// Starts the player's choice (Story 6.9): IPC `player_start`. The choice is `{mode, pack_id | plan_id}`
/// and never names a player; an unknown mode (postpaid) is refused here without asking the service.
#[tauri::command]
async fn player_start(
    choice: arena_proto::PlayerChoice,
    bridge: tauri::State<'_, Arc<Bridge>>,
) -> Result<Option<Answer>, ()> {
    Ok(bridge
        .request(move |rid| arena_kiosk_core::ipc::IpcMsg::PlayerStart { rid, choice })
        .await)
}

/// Forgets the player at the Hub (Story 6.9): IPC `player_logout`.
#[tauri::command]
async fn player_logout(bridge: tauri::State<'_, Arc<Bridge>>) -> Result<Option<Answer>, ()> {
    Ok(bridge
        .request(|rid| arena_kiosk_core::ipc::IpcMsg::PlayerLogout { rid })
        .await)
}

/// A new player's sign-up (Story 6.10): the fields go to the service only, as IPC `player_signup`,
/// and are dropped here. The answer is the Hub's outcome (`{outcome, ...}`); `None` when the
/// service did not answer.
#[tauri::command]
async fn player_signup(
    username: String,
    password: String,
    phone: String,
    name: String,
    consent: bool,
    bridge: tauri::State<'_, Arc<Bridge>>,
) -> Result<Option<Answer>, ()> {
    let form = arena_kiosk_core::ipc::SignupFields {
        username,
        password: arena_kiosk_core::ipc::SecretPin::new(password),
        phone,
        name,
        consent,
    };
    Ok(bridge
        .request(move |rid| arena_kiosk_core::ipc::IpcMsg::PlayerSignup { rid, form })
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

/// The volume slider's level (Story 6.15): the service's `volume` request answers `{percent}`.
#[tauri::command]
async fn volume_get(bridge: tauri::State<'_, Arc<Bridge>>) -> Result<Option<Answer>, ()> {
    Ok(service_request(&bridge, "volume", serde_json::Value::Null).await)
}

/// Sets the system volume (0 to 100) through the service; the answer is `{percent}`.
#[tauri::command]
async fn volume_set(
    percent: u8,
    bridge: tauri::State<'_, Arc<Bridge>>,
) -> Result<Option<Answer>, ()> {
    Ok(service_request(&bridge, "volume", serde_json::json!({ "percent": percent })).await)
}

/// Call staff (Story 6.15): the service sends `device.call_staff` to the Hub. The HUD toast says
/// "Staff notified." when the Hub took it and that the counter is reconnecting when it did not.
#[tauri::command]
async fn call_staff<R: Runtime>(
    app: tauri::AppHandle<R>,
    bridge: tauri::State<'_, Arc<Bridge>>,
) -> Result<Option<Answer>, ()> {
    let answer = service_request(&bridge, "call_staff", serde_json::Value::Null).await;
    let _ = app.emit(TOAST_EVENT, bridge::call_staff_toast(answer.as_ref()));
    Ok(answer)
}

/// What the account menu's Balance row shows (Story 6.17): IPC `player_account`, answered with the
/// Hub's `player.account_result` as the body. `None` when the service did not answer.
#[tauri::command]
async fn player_account(bridge: tauri::State<'_, Arc<Bridge>>) -> Result<Option<Answer>, ()> {
    Ok(service_request(&bridge, "player_account", serde_json::Value::Null).await)
}

/// Log out from the account menu (Story 6.17): IPC `player_end`, answered with the Hub's
/// `player.end_result`. The Hub then locks the PC: the cleanup runs and the screen follows.
#[tauri::command]
async fn player_end(bridge: tauri::State<'_, Arc<Bridge>>) -> Result<Option<Answer>, ()> {
    Ok(service_request(&bridge, "player_end", serde_json::Value::Null).await)
}

/// Lock from the account menu (Story 6.17): the service covers the screen; time keeps running.
#[tauri::command]
async fn player_lock(bridge: tauri::State<'_, Arc<Bridge>>) -> Result<Option<Answer>, ()> {
    Ok(service_request(&bridge, "player_lock", serde_json::Value::Null).await)
}

/// What the extend panel lists (Story 6.21): IPC `player_extend_options`, answered with the Hub's
/// `player.extend_options_result` as the body (quotes at the live rate, wallet, packs, credit).
#[tauri::command]
async fn player_extend_options(
    bridge: tauri::State<'_, Arc<Bridge>>,
) -> Result<Option<Answer>, ()> {
    Ok(service_request(&bridge, "player_extend_options", serde_json::Value::Null).await)
}

/// Adds time to the player's session (Story 6.21): IPC `player_extend`, answered with the Hub's
/// `player.extend_result` (`{outcome, ...}`). The request is `{minutes, source, pack_id?}` and
/// never names a session or a player; a step the Hub does not offer is refused by the service.
#[tauri::command]
async fn player_extend(
    extend: arena_proto::PlayerExtend,
    bridge: tauri::State<'_, Arc<Bridge>>,
) -> Result<Option<Answer>, ()> {
    let body = serde_json::to_value(&extend).unwrap_or(serde_json::Value::Null);
    Ok(service_request(&bridge, "player_extend", body).await)
}

/// The Games tab (Story 6.23): IPC `games`, answered with `{games: [{id, name, running}],
/// running}`. `None` when the service did not answer.
#[tauri::command]
async fn games(bridge: tauri::State<'_, Arc<Bridge>>) -> Result<Option<Answer>, ()> {
    Ok(service_request(&bridge, "games", serde_json::Value::Null).await)
}

/// Starts a game from its tile (Story 6.23): IPC `game_launch{id}`, answered with
/// `{outcome: started | already_running | failed}`. A game that is already running has its window
/// brought to the front here (this runs in the player's session); a start that failed, or that
/// the service did not answer, raises the HUD's "Couldn't start <name>" toast. `name` only words
/// that toast: the service takes the game from `id`.
#[tauri::command]
async fn game_launch<R: Runtime>(
    id: String,
    name: String,
    app: tauri::AppHandle<R>,
    bridge: tauri::State<'_, Arc<Bridge>>,
) -> Result<Option<Answer>, ()> {
    let answer = service_request(&bridge, "game_launch", serde_json::json!({ "id": id })).await;
    match bridge::launch_outcome(answer.as_ref()) {
        LaunchOutcome::Started => {}
        LaunchOutcome::AlreadyRunning(pid) => {
            if !games::bring_forward(pid) {
                tracing::warn!(pid, "no window to bring forward for the running game");
            }
        }
        LaunchOutcome::Failed => {
            let _ = app.emit(GAME_LAUNCH_FAILED_EVENT, name);
        }
    }
    Ok(answer)
}

/// The Apps tab (Story 6.28): IPC `apps`, answered with `{apps: [{id, name, running, category,
/// tags}]}` (only the library's apps this PC has). `None` when the service did not answer.
#[tauri::command]
async fn apps(bridge: tauri::State<'_, Arc<Bridge>>) -> Result<Option<Answer>, ()> {
    Ok(service_request(&bridge, "apps", serde_json::Value::Null).await)
}

/// Opens an app from its tile (Story 6.28): IPC `app_launch{id}`, answered like `game_launch`
/// (`{outcome: started | already_running | failed}`). An app that is already open has its window
/// brought to the front here (this runs in the player's session); one that did not start raises
/// the HUD's "Couldn't start <name>" toast. `name` only words that toast: the service takes the
/// app from `id`.
#[tauri::command]
async fn app_launch<R: Runtime>(
    id: String,
    name: String,
    app: tauri::AppHandle<R>,
    bridge: tauri::State<'_, Arc<Bridge>>,
) -> Result<Option<Answer>, ()> {
    let answer = service_request(&bridge, "app_launch", serde_json::json!({ "id": id })).await;
    match bridge::launch_outcome(answer.as_ref()) {
        LaunchOutcome::Started => {}
        LaunchOutcome::AlreadyRunning(pid) => {
            if !games::bring_forward(pid) {
                tracing::warn!(pid, "no window to bring forward for the open app");
            }
        }
        LaunchOutcome::Failed => {
            let _ = app.emit(GAME_LAUNCH_FAILED_EVENT, name);
        }
    }
    Ok(answer)
}

/// The launcher cards (Story 6.26): IPC `launchers`, answered with `{launchers: [{id, name,
/// running}]}` (only the launchers installed on this PC). `None` when the service did not
/// answer.
#[tauri::command]
async fn launchers(bridge: tauri::State<'_, Arc<Bridge>>) -> Result<Option<Answer>, ()> {
    Ok(service_request(&bridge, "launchers", serde_json::Value::Null).await)
}

/// The Games tab's "Most played here" carousel (Story 6.27): IPC `most_played`, answered with the
/// Hub's `games.most_played_result` (`{top: [{game_id, minutes}], last_played: [{game_id, at}]}`).
/// `None` when the service did not answer; an error answer (the Hub is unreachable) hides the
/// carousel.
#[tauri::command]
async fn most_played(bridge: tauri::State<'_, Arc<Bridge>>) -> Result<Option<Answer>, ()> {
    Ok(service_request(&bridge, "most_played", serde_json::Value::Null).await)
}

/// The Shop tab (Story 6.30): IPC `shop_menu`, answered with the Hub's `shop.menu_result`
/// (`{enabled, items}`). `None` when the service did not answer; an error answer (the Hub is
/// unreachable) leaves the app's last menu in place.
#[tauri::command]
async fn shop_menu(bridge: tauri::State<'_, Arc<Bridge>>) -> Result<Option<Answer>, ()> {
    Ok(service_request(&bridge, "shop_menu", serde_json::Value::Null).await)
}

/// One product's photo for a Shop card (Story 6.30): IPC `shop_image{id}`, answered with the
/// Hub's `shop.image_result` (`{id, image}`, `image` a JPEG data URL or `null`).
#[tauri::command]
async fn shop_image(
    id: String,
    bridge: tauri::State<'_, Arc<Bridge>>,
) -> Result<Option<Answer>, ()> {
    Ok(service_request(&bridge, "shop_image", serde_json::json!({ "id": id })).await)
}

/// The player's cart (Story 6.31): IPC `shop_place{lines}`, answered with the Hub's
/// `shop.place_result` (`{outcome: "ok"}` or `{outcome: "refused", code, message, product_id?}`). An
/// error answer (the Hub is unreachable, `HUB_SLOW`) leaves the cart where it is.
#[tauri::command]
async fn shop_place(
    lines: serde_json::Value,
    bridge: tauri::State<'_, Arc<Bridge>>,
) -> Result<Option<Answer>, ()> {
    Ok(service_request(&bridge, "shop_place", serde_json::json!({ "lines": lines })).await)
}

/// Opens Steam, Epic or Riot from its card (Story 6.26): IPC `launcher_open{id}`, answered like
/// `game_launch`. A launcher that is already open has its window brought to the front here (this
/// runs in the player's session); one that did not open raises the HUD's "Couldn't start <name>"
/// toast. `name` only words that toast: the service takes the launcher from `id`.
#[tauri::command]
async fn launcher_open<R: Runtime>(
    id: String,
    name: String,
    app: tauri::AppHandle<R>,
    bridge: tauri::State<'_, Arc<Bridge>>,
) -> Result<Option<Answer>, ()> {
    let answer = service_request(&bridge, "launcher_open", serde_json::json!({ "id": id })).await;
    match bridge::launch_outcome(answer.as_ref()) {
        LaunchOutcome::Started => {}
        LaunchOutcome::AlreadyRunning(pid) => {
            if !games::bring_forward(pid) {
                tracing::warn!(pid, "no window to bring forward for the open launcher");
            }
        }
        LaunchOutcome::Failed => {
            let _ = app.emit(GAME_LAUNCH_FAILED_EVENT, name);
        }
    }
    Ok(answer)
}

/// The password typed on the player's own lock screen (Story 6.17): sent to the service only, as
/// IPC `player_unlock`, and dropped here. The answer is the Hub's outcome (`{outcome, ...}`).
#[tauri::command]
async fn player_unlock(
    password: String,
    bridge: tauri::State<'_, Arc<Bridge>>,
) -> Result<Option<Answer>, ()> {
    let password = arena_kiosk_core::ipc::SecretPin::new(password);
    Ok(bridge
        .request(move |rid| arena_kiosk_core::ipc::IpcMsg::PlayerUnlock { rid, password })
        .await)
}

/// The HUD page's content size in logical px (`0 × 0`: nothing to draw): the shell places,
/// shows or hides the HUD window for it (Story 6.14).
#[tauri::command]
fn hud_layout<R: Runtime>(
    width: f64,
    height: f64,
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, Arc<hud::Hud>>,
) {
    state.set_content(width, height);
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || hud::refresh(&handle));
}

/// Extend on a warning toast or in the account menu (Story 6.21): the same as Ctrl+Shift+E. The
/// page asks, the shell opens the panel for a running session and emits `hud-expand`.
#[tauri::command]
fn hud_expand<R: Runtime>(app: tauri::AppHandle<R>) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || hud::expand(&handle));
}

/// Esc in the expanded HUD panel: the pill again, focus back to the game.
#[tauri::command]
fn hud_collapse<R: Runtime>(app: tauri::AppHandle<R>) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || hud::collapse(&handle));
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
        .manage(Arc::new(hud::Hud::default()))
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
            staff_messages,
            messages_read,
            attract,
            player_login,
            player_plans,
            player_start,
            player_logout,
            player_signup,
            hud_layout,
            hud_collapse,
            hud_expand,
            volume_get,
            volume_set,
            call_staff,
            player_account,
            player_end,
            player_lock,
            player_unlock,
            player_extend_options,
            player_extend,
            games,
            game_launch,
            apps,
            app_launch,
            launchers,
            launcher_open,
            most_played,
            shop_menu,
            shop_image,
            shop_place
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
                        Emit::PcScreen(screen) => {
                            let hud = Arc::clone(handle.state::<Arc<hud::Hud>>().inner());
                            let running = screen.session.is_some() && !screen.locked;
                            hud.set_session(running);
                            let mark = screen
                                .session
                                .as_ref()
                                .filter(|_| running)
                                .and_then(|s| s.warning_min);
                            let in_grace =
                                screen
                                    .session
                                    .as_ref()
                                    .filter(|_| running)
                                    .is_some_and(|s| {
                                        s.grace_ms > 0
                                            && s.grace_in_ms == Some(0)
                                            && s.remaining_ms == Some(0)
                                    });
                            // A mark and a grace start are different moments; either one over a
                            // fullscreen-exclusive game is one taskbar flash and one sound.
                            let flash = hud.note_warning(mark);
                            if hud.note_grace(in_grace) || flash {
                                hud::flash_taskbar(&handle);
                            }
                            handle.emit(PC_SCREEN_EVENT, screen)
                        }
                        Emit::StaffMessage(message) => handle.emit(STAFF_MESSAGE_EVENT, message),
                        Emit::StaffMessagesCleared => handle.emit(STAFF_MESSAGES_CLEARED_EVENT, ()),
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
                        let on_hotkey = {
                            let handle = handle.clone();
                            move || {
                                let hud = handle.state::<Arc<hud::Hud>>();
                                if !hud.session() {
                                    return false;
                                }
                                let h = handle.clone();
                                let _ = handle.run_on_main_thread(move || hud::expand(&h));
                                true
                            }
                        };
                        shell::start_input_thread(reassert, on_degraded, on_hotkey);
                    }
                }
            } else {
                // `create: false`: nothing else builds the window. Exit with a failure so the
                // service's watchdog relaunches the app (the screen stays covered meanwhile).
                tracing::error!("no kiosk window; exiting so the service relaunches the app");
                std::process::exit(1);
            }
            // The overlay HUD (Story 6.14): hidden until its page reports something to draw.
            if hud::build(app).is_none() {
                tracing::warn!("no HUD window; the player gets no overlay");
            }
            hud::watch_fullscreen(app.handle().clone());
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
