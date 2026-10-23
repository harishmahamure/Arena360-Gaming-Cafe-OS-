//! Arena 360 Hub UI: Tauri shell around the `@arena/hub` frontend.
//!
//! On start it makes sure the Hub service is running (see [`sidecar`]): it probes
//! `127.0.0.1:7409/healthz` and, when nothing healthy answers, spawns the bundled `arena-hub`
//! sidecar next to this executable (with `--data-dir`, logging failures to
//! `<data dir>/logs/hub-ui.log`). Then it tells the page to re-read the snapshot
//! (`arena:hub-ready`) and shows the window (hidden until then). The service
//! keeps running when the UI closes.

// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod sidecar;

use tauri::Manager;

fn crate_name() -> &'static str {
    "arena-hub-ui"
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
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
            Ok(())
        })
        .run(tauri::generate_context!())
        .unwrap_or_else(|err| panic!("{}: failed to start: {err}", crate_name()));
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name_is_set() {
        assert_eq!(super::crate_name(), "arena-hub-ui");
    }
}
