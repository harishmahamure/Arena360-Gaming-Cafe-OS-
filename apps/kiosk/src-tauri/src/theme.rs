//! The kiosk's high-contrast theme before its first paint (Story 1.21).
//!
//! The window is configured hidden and is not created by Tauri (`create: false`). At start the
//! shell waits up to [`SERVICE_WAIT`] for the Kiosk service's `theme{hc}` (sent on every IPC
//! connection), else uses the value this app last saved in `<app local data>/theme.json`
//! ([`initial`], pure). It then builds the window with an initialization script
//! ([`init_script`]) that sets `document.documentElement.dataset.theme` before any page script
//! runs, and only then shows it. Each later value is saved to `theme.json` and sent to the
//! webview as the `theme` event; nothing restarts.
//!
//! There is no player-facing theme control and the OS high-contrast setting is not read: a
//! manager sets it per device on the Hub.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long the shell waits for the service's theme before it falls back to the saved one.
pub const SERVICE_WAIT: Duration = Duration::from_secs(1);
/// The file the last theme is saved in, under the app's local data directory.
pub const FILE_NAME: &str = "theme.json";

/// The theme to paint first: the service's answer when it came in time, else the value saved
/// last time, else the default dark theme.
pub fn initial(from_service: Option<bool>, saved: Option<bool>) -> bool {
    from_service.or(saved).unwrap_or(false)
}

/// The `data-theme` value of `hc`.
pub fn theme_name(hc: bool) -> &'static str {
    if hc { "hc" } else { "dark" }
}

/// A script, run at document creation (before any page script), that sets `data-theme` on
/// `<html>` as soon as it exists.
pub fn init_script(hc: bool) -> String {
    format!(
        "(function () {{\
           var t = {name:?};\
           function apply() {{\
             var e = document.documentElement;\
             if (!e) return false;\
             e.dataset.theme = t;\
             return true;\
           }}\
           if (!apply()) {{\
             var o = new MutationObserver(function () {{ if (apply()) o.disconnect(); }});\
             o.observe(document, {{ childList: true }});\
           }}\
         }})();",
        name = theme_name(hc)
    )
}

/// `theme.json` in `dir`.
pub fn file_in(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

/// The saved theme, if the file holds one (`{"hc": true|false}`); anything else is none.
pub fn read(path: &Path) -> Option<bool> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("hc")?.as_bool()
}

/// Saves `hc` (through a temporary file, so a crash never leaves half a file).
pub fn write(path: &Path, hc: bool) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::json!({ "hc": hc }).to_string())?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_service_wins_then_the_saved_value_then_dark() {
        assert!(initial(Some(true), Some(false)));
        assert!(!initial(Some(false), Some(true)));
        assert!(initial(None, Some(true)), "the service was slow or down");
        assert!(!initial(None, Some(false)));
        assert!(!initial(None, None), "never saved: dark");
    }

    #[test]
    fn the_saved_value_round_trips_and_garbage_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = file_in(&dir.path().join("nested"));
        assert_eq!(read(&path), None);
        write(&path, true).unwrap();
        assert_eq!(read(&path), Some(true));
        write(&path, false).unwrap();
        assert_eq!(read(&path), Some(false));
        for bad in ["", "{", "{\"hc\":\"yes\"}", "[]", "true"] {
            std::fs::write(&path, bad).unwrap();
            assert_eq!(read(&path), None, "{bad:?}");
        }
    }

    /// The fixtures the kiosk Playwright suite injects as the shell's init script
    /// (`ARENA_WRITE_FIXTURES=1` rewrites them).
    const FIXTURES: [(bool, &str, &str); 2] = [
        (
            true,
            "../tests/fixtures/theme-init-hc.js",
            include_str!("../../tests/fixtures/theme-init-hc.js"),
        ),
        (
            false,
            "../tests/fixtures/theme-init-dark.js",
            include_str!("../../tests/fixtures/theme-init-dark.js"),
        ),
    ];

    #[test]
    fn the_playwright_fixtures_are_the_real_init_scripts() {
        for (hc, path, committed) in FIXTURES {
            if std::env::var_os("ARENA_WRITE_FIXTURES").is_some() {
                std::fs::write(path, init_script(hc)).unwrap();
            } else {
                assert_eq!(committed, init_script(hc), "{path} is stale");
            }
        }
    }

    #[test]
    fn the_main_window_is_hidden_and_built_by_the_shell() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let windows = conf["app"]["windows"].as_array().unwrap();
        let main = windows
            .iter()
            .find(|w| w["label"] == "main")
            .expect("a window labelled main");
        assert_eq!(main["create"], false);
        assert_eq!(main["visible"], false);
    }

    #[test]
    fn the_scripts_name_the_theme() {
        assert!(init_script(true).contains("\"hc\""));
        assert!(init_script(false).contains("\"dark\""));
        assert!(init_script(true).contains("dataset.theme = t"));
        assert!(!init_script(true).contains("light"));
    }
}
