//! Arena 360 Hub UI: Tauri shell around the `@arena/hub` frontend.
//!
//! On start it makes sure the Hub service is running (see [`sidecar`]): it probes
//! `127.0.0.1:7409/healthz` and, when nothing healthy answers, spawns the bundled `arena-hub`
//! sidecar next to this executable (with `--data-dir`, logging failures to
//! `<data dir>/logs/hub-ui.log`). Then it tells the page to re-read the snapshot
//! (`arena:hub-ready`) and shows the window (hidden until then). The service
//! keeps running when the UI closes.
//!
//! On Android there is no sidecar: [`android`] checks the device spec on the first run and then
//! runs `arena-hub` in this process.

#[cfg(target_os = "android")]
mod android;
#[cfg(not(target_os = "android"))]
mod sidecar;
#[cfg(any(all(test, unix), target_os = "android"))]
mod spec;

#[cfg(not(target_os = "android"))]
use tauri::Manager;

fn crate_name() -> &'static str {
    "arena-hub-ui"
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "android")]
    let builder = builder
        .manage(android::Hub::default())
        .invoke_handler(tauri::generate_handler![android::device_spec]);
    builder
        .setup(|app| {
            #[cfg(target_os = "android")]
            android::setup(app.handle());
            #[cfg(not(target_os = "android"))]
            desktop_setup(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .unwrap_or_else(|err| panic!("{}: failed to start: {err}", crate_name()));
}

#[cfg(not(target_os = "android"))]
fn desktop_setup(handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        let data_dir = sidecar::data_dir(|k| std::env::var_os(k), cfg!(windows));
        let log = sidecar::FileLog::in_data_dir(data_dir.as_deref());
        match std::env::current_exe() {
            Ok(exe) => {
                let cmd = sidecar::SidecarCommand::bundled(&exe, data_dir.as_deref());
                sidecar::ensure_service(
                    &cmd,
                    sidecar::HEALTH_PORT,
                    sidecar::START_TIMEOUT,
                    &mut |line| log.line(line),
                );
            }
            Err(e) => log.line(&format!("cannot locate the app executable: {e}")),
        }
        if let Some(window) = handle.get_webview_window("main") {
            // The page may have read the snapshot before the service was up: tell it
            // to read it again (the layout listens for this event).
            let _ = window.eval("window.dispatchEvent(new Event('arena:hub-ready'))");
            let _ = window.show();
            let _ = window.set_focus();
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name_is_set() {
        assert_eq!(super::crate_name(), "arena-hub-ui");
    }
}
