//! Arena 360 Kiosk UI: Tauri shell around the `@arena/kiosk` frontend.

// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn crate_name() -> &'static str {
    "arena-kiosk-ui"
}

fn main() {
    tauri::Builder::default()
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
