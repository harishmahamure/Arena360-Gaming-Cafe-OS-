//! The overlay HUD window (Story 6.14): a second always-on-top window in the top-right of the
//! primary monitor that shows the player's time and balance over a running game.
//!
//! - **Never takes focus.** It is built non-focusable (`WS_EX_NOACTIVATE` on Windows) and hidden
//!   until its page reports content, so a click or a toast never pulls keyboard input from the
//!   game. Only the expanded panel (Ctrl+Shift+E, from the keyboard hook) is focusable; Esc
//!   collapses it and gives focus back to the window that had it.
//! - **The page owns the content, the shell owns the window.** The HUD page reports the size of
//!   what it draws (`hud_layout`, logical px; `0 × 0` for nothing) and the shell places, shows
//!   or hides the window ([`arena_kiosk_core::hud`]).
//! - **Fullscreen-exclusive games.** Once a second the shell asks Windows for the user
//!   notification state; while a Direct3D exclusive game is in front the window is hidden and
//!   nothing is drawn over it.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use arena_kiosk_core::hud::{grace_flash, place, warning_flash, window_visible};
use arena_kiosk_core::monitors::Rect;
use tauri::{
    AppHandle, Emitter, EventTarget, Manager, PhysicalPosition, PhysicalSize, Runtime,
    WebviewWindow, WebviewWindowBuilder,
};

use crate::bridge::HUD_EXPAND_EVENT;
use crate::shell;

/// The HUD window's label in `tauri.conf.json`.
pub const LABEL: &str = "hud";

/// What the shell knows about the HUD.
#[derive(Default)]
pub struct Hud {
    /// The size of what the page draws, in logical px (`0 × 0`: nothing).
    content: Mutex<(f64, f64)>,
    /// A Direct3D fullscreen-exclusive game is in the foreground.
    exclusive: AtomicBool,
    /// The panel is expanded and holds the keyboard focus.
    expanded: AtomicBool,
    /// The window that had the focus when the panel opened (an `HWND`; 0: none).
    previous: AtomicIsize,
    /// A session runs: only then does the hotkey open the panel.
    session: AtomicBool,
    /// The time-warning mark last seen on the session (minutes; 0: none), so a mark flashes once.
    warned: AtomicU8,
    /// The session is in its grace (Story 6.22), so the start of one flashes once.
    in_grace: AtomicBool,
}

impl Hud {
    /// The page reported what it draws.
    pub fn set_content(&self, width: f64, height: f64) {
        let ok = |v: f64| if v.is_finite() { v.max(0.0) } else { 0.0 };
        *self.content.lock().unwrap_or_else(|p| p.into_inner()) = (ok(width), ok(height));
    }

    fn content(&self) -> (f64, f64) {
        *self.content.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// A session started or ended.
    pub fn set_session(&self, running: bool) {
        self.session.store(running, Ordering::SeqCst);
    }

    pub fn session(&self) -> bool {
        self.session.load(Ordering::SeqCst)
    }

    /// The session's warning mark is now `mark` (Story 6.19). Whether the taskbar must flash:
    /// a newly reached 5- or 1-minute mark while a fullscreen-exclusive game hides the HUD.
    pub fn note_warning(&self, mark: Option<u8>) -> bool {
        let before = self.warned.swap(mark.unwrap_or(0), Ordering::SeqCst);
        warning_flash(
            (before != 0).then_some(before),
            mark,
            self.exclusive.load(Ordering::SeqCst),
        )
    }

    /// The session's grace is running (`true`) or not (Story 6.22). Whether the taskbar must
    /// flash: the grace has just begun while a fullscreen-exclusive game hides the HUD.
    pub fn note_grace(&self, grace: bool) -> bool {
        let was = self.in_grace.swap(grace, Ordering::SeqCst);
        grace_flash(was, grace, self.exclusive.load(Ordering::SeqCst))
    }

    fn set_exclusive(&self, now: bool) -> bool {
        self.exclusive.swap(now, Ordering::SeqCst) != now
    }

    fn visible(&self) -> bool {
        window_visible(self.content(), self.exclusive.load(Ordering::SeqCst))
    }
}

/// Builds the hidden, non-focusable HUD window from its configuration.
pub fn build<R: Runtime>(app: &tauri::App<R>) -> Option<WebviewWindow<R>> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == LABEL)
        .cloned()?;
    match WebviewWindowBuilder::from_config(app.handle(), &config)
        .map(|b| b.focusable(false).focused(false).build())
    {
        Ok(Ok(w)) => {
            if !shell::dev_unlocked() {
                shell::harden_webview(&w);
            }
            Some(w)
        }
        Ok(Err(e)) | Err(e) => {
            tracing::error!(error = %e, "the HUD window could not be built");
            None
        }
    }
}

/// The primary monitor's rectangle in physical px, with the window's scale.
fn primary<R: Runtime>(window: &WebviewWindow<R>) -> Option<(Rect, f64)> {
    let m = window
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| window.available_monitors().ok()?.into_iter().next())?;
    let (p, s) = (m.position(), m.size());
    Some((
        Rect::new(
            p.x,
            p.y,
            i32::try_from(s.width).unwrap_or(i32::MAX),
            i32::try_from(s.height).unwrap_or(i32::MAX),
        ),
        m.scale_factor(),
    ))
}

/// Places, shows or hides the window for the content and fullscreen state. Runs on the main
/// thread. A shown window is never activated.
pub fn refresh<R: Runtime>(handle: &AppHandle<R>) {
    let hud = Arc::clone(handle.state::<Arc<Hud>>().inner());
    let Some(window) = handle.get_webview_window(LABEL) else {
        return;
    };
    if !hud.visible() {
        let _ = window.hide();
        return;
    }
    if let Some((monitor, scale)) = primary(&window) {
        let r = place(monitor, scale, hud.content());
        let _ = window.set_size(PhysicalSize::new(
            u32::try_from(r.width).unwrap_or(1),
            u32::try_from(r.height).unwrap_or(1),
        ));
        let _ = window.set_position(PhysicalPosition::new(r.x, r.y));
    }
    let _ = window.set_always_on_top(true);
    let _ = window.show();
}

/// Ctrl+Shift+E: expands the pill into its panel and moves focus into it. Does nothing without
/// a session or while a fullscreen-exclusive game hides the HUD.
pub fn expand<R: Runtime>(handle: &AppHandle<R>) {
    let hud = Arc::clone(handle.state::<Arc<Hud>>().inner());
    if !hud.session() || hud.exclusive.load(Ordering::SeqCst) {
        return;
    }
    let Some(window) = handle.get_webview_window(LABEL) else {
        return;
    };
    if !hud.expanded.swap(true, Ordering::SeqCst) {
        hud.previous.store(sys::foreground(), Ordering::SeqCst);
    }
    let _ = window.set_focusable(true);
    let _ = window.show();
    let _ = window.set_focus();
    let _ = handle.emit_to(EventTarget::webview_window(LABEL), HUD_EXPAND_EVENT, ());
}

/// Esc in the panel: back to the non-focusable pill, focus to the window that had it.
pub fn collapse<R: Runtime>(handle: &AppHandle<R>) {
    let hud = Arc::clone(handle.state::<Arc<Hud>>().inner());
    if !hud.expanded.swap(false, Ordering::SeqCst) {
        return;
    }
    if let Some(window) = handle.get_webview_window(LABEL) {
        let _ = window.set_focusable(false);
    }
    sys::restore_foreground(hud.previous.swap(0, Ordering::SeqCst));
}

/// Flashes the kiosk's taskbar button and plays the warning sound: a fullscreen-exclusive game
/// hides the overlay, so this is all a 5- or 1-minute warning can draw (Story 6.19).
pub fn flash_taskbar<R: Runtime>(handle: &AppHandle<R>) {
    let hwnd = handle
        .get_webview_window("main")
        .and_then(|w| w.hwnd().ok())
        .map_or(0, |h| h.0 as isize);
    sys::flash(hwnd);
}

/// Asks Windows once a second whether a fullscreen-exclusive game is in front and refreshes the
/// window when the answer changes.
pub fn watch_fullscreen<R: Runtime>(handle: AppHandle<R>) {
    let spawned = std::thread::Builder::new()
        .name("kiosk-hud-watch".into())
        .spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let hud = Arc::clone(handle.state::<Arc<Hud>>().inner());
                if hud.set_exclusive(sys::exclusive_fullscreen()) {
                    let h = handle.clone();
                    // The panel's state is left alone: the page and the shell must agree on
                    // whether it is open, and `collapse` is the only way that changes.
                    let _ = handle.run_on_main_thread(move || refresh(&h));
                }
            }
        });
    if let Err(e) = spawned {
        tracing::error!(detail = %e, "the HUD fullscreen watcher could not be started");
    }
}

#[cfg(windows)]
mod sys {
    use arena_kiosk_core::hud::Notification;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::SHQueryUserNotificationState;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};

    /// The window with the keyboard focus (an `HWND`; 0: none).
    pub fn foreground() -> isize {
        // SAFETY: a plain query.
        unsafe { GetForegroundWindow() }.0 as isize
    }

    /// Gives the focus back to `hwnd` (a no-op for 0 or a window that has gone).
    pub fn restore_foreground(hwnd: isize) {
        if hwnd != 0 {
            // SAFETY: a stale handle makes the call fail harmlessly.
            let _ = unsafe { SetForegroundWindow(HWND(hwnd as *mut _)) };
        }
    }

    /// Flashes `hwnd`'s taskbar button until it is brought forward, and sounds the system alert.
    pub fn flash(hwnd: isize) {
        use windows::Win32::System::Diagnostics::Debug::MessageBeep;
        use windows::Win32::UI::WindowsAndMessaging::{
            FLASHW_ALL, FLASHW_TIMERNOFG, FLASHWINFO, FlashWindowEx, MB_ICONEXCLAMATION,
        };
        if hwnd != 0 {
            let info = FLASHWINFO {
                cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
                hwnd: HWND(hwnd as *mut _),
                dwFlags: FLASHW_ALL | FLASHW_TIMERNOFG,
                uCount: 0,
                dwTimeout: 0,
            };
            // SAFETY: a plain call with a fully set structure; a stale handle fails harmlessly.
            let _ = unsafe { FlashWindowEx(&info) };
        }
        // SAFETY: a plain call.
        let _ = unsafe { MessageBeep(MB_ICONEXCLAMATION) };
    }

    /// Whether a Direct3D fullscreen-exclusive game is in the foreground.
    pub fn exclusive_fullscreen() -> bool {
        // SAFETY: a plain query.
        unsafe { SHQueryUserNotificationState() }
            .ok()
            .and_then(|s| Notification::from_raw(s.0))
            .is_some_and(Notification::hides_overlay)
    }
}

#[cfg(not(windows))]
mod sys {
    pub fn foreground() -> isize {
        0
    }
    pub fn restore_foreground(_hwnd: isize) {}
    pub fn flash(_hwnd: isize) {}
    pub fn exclusive_fullscreen() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_needs_content_and_no_exclusive_game() {
        let hud = Hud::default();
        assert!(!hud.visible(), "nothing to draw: hidden");
        hud.set_content(260.0, 56.0);
        assert!(hud.visible());
        assert!(hud.set_exclusive(true), "the state changed");
        assert!(!hud.visible(), "a fullscreen-exclusive game hides it");
        assert!(!hud.set_exclusive(true), "no change");
        hud.set_exclusive(false);
        hud.set_content(0.0, 0.0);
        assert!(!hud.visible());
    }

    #[test]
    fn a_bad_size_from_the_page_is_nothing() {
        let hud = Hud::default();
        hud.set_content(f64::NAN, -5.0);
        assert_eq!(hud.content(), (0.0, 0.0));
    }

    #[test]
    fn a_warning_mark_flashes_once_and_only_over_an_exclusive_game() {
        let hud = Hud::default();
        assert!(!hud.note_warning(Some(5)), "the HUD is drawing: no flash");
        hud.set_exclusive(true);
        assert!(
            hud.note_warning(Some(1)),
            "a new mark over the game flashes"
        );
        assert!(!hud.note_warning(Some(1)), "once");
        assert!(!hud.note_warning(None));
        assert!(
            hud.note_warning(Some(5)),
            "after an extension the mark can come again"
        );
    }

    #[test]
    fn a_grace_flashes_once_and_only_over_an_exclusive_game() {
        let hud = Hud::default();
        assert!(!hud.note_grace(true), "the HUD draws the card: no flash");
        assert!(!hud.note_grace(false));
        hud.set_exclusive(true);
        assert!(hud.note_grace(true), "a new grace over the game flashes");
        assert!(!hud.note_grace(true), "once");
        assert!(!hud.note_grace(false));
        assert!(
            hud.note_grace(true),
            "after an extension a grace can come again"
        );
    }

    #[test]
    fn the_hotkey_works_only_while_a_session_runs() {
        let hud = Hud::default();
        assert!(!hud.session());
        hud.set_session(true);
        assert!(hud.session());
        hud.set_session(false);
        assert!(!hud.session());
    }
}
