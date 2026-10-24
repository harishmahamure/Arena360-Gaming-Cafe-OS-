//! The kiosk window as a shell: one undecorated, topmost window over the union of every
//! monitor (re-placed on each display or DPI change and re-asserted when another window steals
//! activation), a close request that is always refused and, on Windows, a low-level keyboard
//! hook that swallows the Windows keys, Alt+F4 and the task switchers
//! (`arena_kiosk_core::keys`). Only one instance runs (a machine-wide named mutex); a second
//! exits at once.
//!
//! Debug builds only: `ARENA_KIOSK_DEV_UNLOCKED=1` leaves the window an ordinary one (no hook,
//! not topmost, closable) so a developer's own desktop stays usable.

use arena_kiosk_core::monitors::{Rect, union};
use tauri::{PhysicalPosition, PhysicalSize, Runtime, WebviewWindow};

/// Whether the lockdown is off for this run (never in a release build).
pub fn dev_unlocked() -> bool {
    cfg!(debug_assertions) && std::env::var("ARENA_KIOSK_DEV_UNLOCKED").is_ok_and(|v| v == "1")
}

/// Whether this is the only kiosk instance. On Windows a second instance finds the named mutex
/// already held and returns `false` (the caller exits). Elsewhere always `true`.
pub fn claim_single_instance() -> bool {
    #[cfg(windows)]
    {
        win::claim_single_instance()
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Every monitor's rectangle in physical desktop pixels.
#[cfg(windows)]
fn monitor_rects<R: Runtime>(_window: &WebviewWindow<R>) -> Vec<Rect> {
    win::monitor_rects()
}

#[cfg(not(windows))]
fn monitor_rects<R: Runtime>(window: &WebviewWindow<R>) -> Vec<Rect> {
    window
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let (p, s) = (m.position(), m.size());
            Rect::new(
                p.x,
                p.y,
                i32::try_from(s.width).unwrap_or(i32::MAX),
                i32::try_from(s.height).unwrap_or(i32::MAX),
            )
        })
        .collect()
}

/// Locks the WebView2 down: no default context menus, no browser accelerator keys (Ctrl+P,
/// Ctrl+R, F5, F12, Ctrl+Shift+I…), no devtools and no status bar. The frontend blocks the same
/// keys and `window.print` as defense in depth. A no-op off Windows.
pub fn harden_webview<R: Runtime>(window: &WebviewWindow<R>) {
    #[cfg(windows)]
    {
        let _ = window.with_webview(|webview| {
            use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
            use windows::core::Interface;
            // SAFETY: COM calls on the controller Tauri created for this window. Every failure
            // is ignored (best effort); the keyboard hook and frontend cover the same keys.
            unsafe {
                let controller = webview.controller();
                if let Ok(core) = controller.CoreWebView2()
                    && let Ok(settings) = core.Settings()
                {
                    let _ = settings.SetAreDefaultContextMenusEnabled(false);
                    let _ = settings.SetIsStatusBarEnabled(false);
                    let _ = settings.SetAreDevToolsEnabled(false);
                    if let Ok(s3) = settings.cast::<ICoreWebView2Settings3>() {
                        let _ = s3.SetAreBrowserAcceleratorKeysEnabled(false);
                    }
                }
            }
        });
    }
    #[cfg(not(windows))]
    let _ = window;
}

/// The primary monitor's rectangle translated so its origin is relative to the window's
/// top-left (the union's origin): where the locked placeholder should sit so it is not stretched
/// across every monitor. In physical pixels; `None` when no monitor is known.
pub fn content_box<R: Runtime>(window: &WebviewWindow<R>) -> Option<Rect> {
    let rects = monitor_rects(window);
    let origin = union(&rects).or_else(|| rects.first().copied())?;
    let p = rects
        .iter()
        .find(|r| r.x == 0 && r.y == 0)
        .or_else(|| rects.first())
        .copied()?;
    Some(Rect::new(p.x - origin.x, p.y - origin.y, p.width, p.height))
}

/// Moves and sizes `window` over all monitors, re-asserts topmost, then shows and focuses it.
/// When the union cannot be computed it falls back to the primary monitor's rect, and when no
/// monitor is reported at all to real fullscreen, so the desktop is never left exposed.
pub fn cover_all_monitors<R: Runtime>(window: &WebviewWindow<R>) {
    let rects = monitor_rects(window);
    match union(&rects).or_else(|| rects.first().copied()) {
        Some(r) => {
            let _ = window.set_fullscreen(false);
            let _ = window.set_position(PhysicalPosition::new(r.x, r.y));
            let _ = window.set_size(PhysicalSize::new(
                u32::try_from(r.width).unwrap_or(0),
                u32::try_from(r.height).unwrap_or(0),
            ));
        }
        None => {
            tracing::warn!("no monitor reported; covering the primary with fullscreen");
            let _ = window.set_fullscreen(true);
        }
    }
    let _ = window.set_always_on_top(true);
    let _ = window.show();
    let _ = window.set_focus();
}

/// Set while an admin exit runs (Story 1.20): the keyboard hook passes every key and the window
/// stops re-asserting itself over the desktop.
static LOCKDOWN_SUSPENDED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Suspends (`true`) or resumes the keyboard lockdown and the re-cover on focus loss.
pub fn set_lockdown_suspended(suspended: bool) {
    LOCKDOWN_SUSPENDED.store(suspended, std::sync::atomic::Ordering::SeqCst);
}

/// Whether an admin exit has suspended the lockdown.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn lockdown_suspended() -> bool {
    LOCKDOWN_SUSPENDED.load(std::sync::atomic::Ordering::SeqCst)
}

/// The admin exit's badge size, in logical px.
const BADGE_SIZE: (f64, f64) = (360.0, 88.0);

/// Shrinks `window` to the admin exit's countdown badge: not topmost, in the primary monitor's
/// bottom-right corner, so the desktop behind it is usable.
pub fn place_badge<R: Runtime>(window: &WebviewWindow<R>) {
    let _ = window.set_always_on_top(false);
    let _ = window.set_fullscreen(false);
    let scale = window.scale_factor().unwrap_or(1.0);
    let (w, h) = (BADGE_SIZE.0 * scale, BADGE_SIZE.1 * scale);
    let _ = window.set_size(PhysicalSize::new(w as u32, h as u32));
    if let Ok(Some(m)) = window.primary_monitor() {
        let margin = 16.0 * scale;
        let x = f64::from(m.position().x) + f64::from(m.size().width) - w - margin;
        let y = f64::from(m.position().y) + f64::from(m.size().height) - h - margin * 4.0;
        let _ = window.set_position(PhysicalPosition::new(x as i32, y as i32));
    }
}

#[cfg(windows)]
pub use win::start_input_thread;

#[cfg(windows)]
mod win {
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    use arena_kiosk_core::keys::{Decision, KeyEvent, KeyFilter, Modifiers, vk};
    use arena_kiosk_core::monitors::Rect;
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, HC_ACTION,
        HHOOK, KBDLLHOOKSTRUCT, LLKHF_ALTDOWN, MSG, RegisterClassW, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WINDOW_EX_STYLE, WM_ACTIVATEAPP,
        WM_DISPLAYCHANGE, WM_DPICHANGED, WM_KEYDOWN, WM_SYSKEYDOWN, WM_TIMER, WNDCLASSW,
        WS_EX_TOOLWINDOW, WS_POPUP,
    };
    use windows::core::{BOOL, PCWSTR, w};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    static FILTER: Mutex<KeyFilter> = Mutex::new(KeyFilter::new());
    static ON_REASSERT: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
    static ON_DEGRADED: OnceLock<Box<dyn Fn(bool) + Send + Sync>> = OnceLock::new();
    /// The installed hook handle (as isize), 0 when none is installed.
    static HOOK: AtomicIsize = AtomicIsize::new(0);
    static DEGRADED: AtomicBool = AtomicBool::new(false);

    const RECHECK_TIMER: usize = 1;
    const RECHECK_MS: u32 = 60_000;

    /// Claims the machine-wide single-instance mutex; `false` when another instance holds it.
    pub fn claim_single_instance() -> bool {
        use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
        let name = wide(r"Global\ArenaKioskUi");
        // SAFETY: NUL-terminated name. The handle is intentionally leaked so the mutex lives for
        // the whole process; the OS releases it on exit.
        let created = unsafe { CreateMutexW(None, true, PCWSTR(name.as_ptr())) };
        match created {
            Ok(_) => {
                // SAFETY: reads the last error the CreateMutexW call set.
                let last = unsafe { GetLastError() };
                last != ERROR_ALREADY_EXISTS
            }
            Err(e) => {
                tracing::error!(detail = %e, "could not create the single-instance mutex");
                // Fail open: better one kiosk than none.
                true
            }
        }
    }

    /// The modifiers the OS reports as held right now (`GetAsyncKeyState`), so a missed key-up
    /// or a pre-held modifier never leaves the filter's state stuck.
    fn real_modifiers() -> Modifiers {
        // SAFETY: GetAsyncKeyState is a simple query.
        let down = |k: u32| unsafe { (GetAsyncKeyState(k as i32) as u16 & 0x8000) != 0 };
        Modifiers {
            ctrl: down(vk::CONTROL),
            alt: down(vk::MENU),
            shift: down(vk::SHIFT),
            win: down(vk::LWIN) || down(vk::RWIN),
        }
    }

    unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 && !super::lockdown_suspended() {
            // SAFETY: for WH_KEYBOARD_LL with HC_ACTION, lparam points to a KBDLLHOOKSTRUCT.
            let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
            let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            let alt = kb.flags.0 & LLKHF_ALTDOWN.0 != 0;
            let decision = FILTER
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .on_event_synced(
                    KeyEvent {
                        vk: kb.vkCode,
                        down,
                    },
                    alt,
                    real_modifiers(),
                );
            if decision == Decision::Swallow {
                return LRESULT(1);
            }
        }
        // SAFETY: passes the event on unchanged.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    /// Installs the keyboard hook, retrying a few times with backoff. Reports whether it is now
    /// installed and updates `HOOK`/`DEGRADED`.
    fn install_hook(module: Option<HINSTANCE>) -> bool {
        for attempt in 0..5 {
            // SAFETY: a plain hook install on this thread.
            match unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), module, 0) } {
                Ok(h) => {
                    HOOK.store(h.0 as isize, Ordering::SeqCst);
                    set_degraded(false);
                    // Seed the filter from the live modifier state.
                    FILTER
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .resync(real_modifiers());
                    return true;
                }
                Err(e) => {
                    tracing::error!(attempt, detail = %e, "keyboard hook install failed; retrying");
                    std::thread::sleep(Duration::from_millis(200 * (attempt + 1) as u64));
                }
            }
        }
        set_degraded(true);
        false
    }

    /// Re-installs the hook (every 60 s): a hook silently removed by the OS comes back, and a
    /// persistent failure flips the degraded state.
    fn reinstall_hook(module: Option<HINSTANCE>) {
        let old = HOOK.swap(0, Ordering::SeqCst);
        if old != 0 {
            // SAFETY: a handle this module installed.
            unsafe {
                let _ = UnhookWindowsHookEx(HHOOK(old as *mut _));
            }
        }
        install_hook(module);
    }

    fn set_degraded(now: bool) {
        if DEGRADED.swap(now, Ordering::SeqCst) != now
            && let Some(f) = ON_DEGRADED.get()
        {
            f(now);
        }
    }

    unsafe extern "system" fn watch_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            // A display or DPI change can resize or re-origin the desktop; another window being
            // activated (WM_ACTIVATEAPP with wparam == FALSE) can steal the foreground.
            WM_DISPLAYCHANGE | WM_DPICHANGED => reassert(),
            WM_ACTIVATEAPP if wparam.0 == 0 => reassert(),
            WM_TIMER if wparam.0 == RECHECK_TIMER => {
                // SAFETY: the module handle for this process.
                let module = unsafe { GetModuleHandleW(None) }
                    .ok()
                    .map(|m| HINSTANCE(m.0));
                reinstall_hook(module);
            }
            _ => {}
        }
        // SAFETY: default handling.
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    fn reassert() {
        if super::lockdown_suspended() {
            return;
        }
        if let Some(f) = ON_REASSERT.get() {
            f();
        }
    }

    /// Starts the input thread: the `WH_KEYBOARD_LL` hook (with retry and a 60 s re-install), a
    /// hidden top-level window that catches display/DPI/activation changes, and the thread's
    /// message loop. `on_reassert` re-covers and re-focuses the window; `on_degraded(true)` is
    /// called when the hook is not active (and `false` when it comes back).
    pub fn start_input_thread(
        on_reassert: impl Fn() + Send + Sync + 'static,
        on_degraded: impl Fn(bool) + Send + Sync + 'static,
    ) {
        let _ = ON_REASSERT.set(Box::new(on_reassert));
        let _ = ON_DEGRADED.set(Box::new(on_degraded));
        let spawned = std::thread::Builder::new()
            .name("kiosk-input".into())
            .spawn(|| {
                // SAFETY: plain Win32 calls on this thread; the loop below serves the hook and
                // the window for the life of the process.
                unsafe {
                    let module = GetModuleHandleW(None).ok().map(|m| HINSTANCE(m.0));
                    install_hook(module);
                    let class = w!("ArenaKioskDisplayWatch");
                    let wc = WNDCLASSW {
                        lpfnWndProc: Some(watch_proc),
                        hInstance: module.unwrap_or_default(),
                        lpszClassName: class,
                        ..Default::default()
                    };
                    RegisterClassW(&wc);
                    let hwnd = CreateWindowExW(
                        WINDOW_EX_STYLE(WS_EX_TOOLWINDOW.0),
                        class,
                        w!(""),
                        WS_POPUP,
                        0,
                        0,
                        0,
                        0,
                        None,
                        None,
                        module,
                        None,
                    );
                    match hwnd {
                        Ok(h) => {
                            use windows::Win32::UI::WindowsAndMessaging::SetTimer;
                            let _ = SetTimer(Some(h), RECHECK_TIMER, RECHECK_MS, None);
                        }
                        Err(e) => {
                            tracing::error!(detail = %e, "the display watcher could not be created");
                        }
                    }
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            });
        if let Err(e) = spawned {
            tracing::error!(detail = %e, "the input thread could not be started");
        }
    }

    unsafe extern "system" fn add_monitor(
        _monitor: HMONITOR,
        _hdc: HDC,
        rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        // SAFETY: `data` is the `Vec` passed by `monitor_rects`, `rect` the monitor's rectangle.
        unsafe {
            let out = &mut *(data.0 as *mut Vec<Rect>);
            let r = &*rect;
            out.push(Rect::from_edges(r.left, r.top, r.right, r.bottom));
        }
        true.into()
    }

    /// Every monitor's rectangle (`EnumDisplayMonitors`, virtual-screen coordinates).
    pub fn monitor_rects() -> Vec<Rect> {
        let mut out: Vec<Rect> = Vec::new();
        // SAFETY: the callback only runs during this call, while `out` lives.
        unsafe {
            let _ = EnumDisplayMonitors(
                None,
                None,
                Some(add_monitor),
                LPARAM(&mut out as *mut Vec<Rect> as isize),
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_unlock_is_release_gated() {
        // In a release build the env var must be ignored; in a debug build this is vacuous.
        if !cfg!(debug_assertions) {
            // SAFETY: single-threaded test setup.
            unsafe {
                std::env::set_var("ARENA_KIOSK_DEV_UNLOCKED", "1");
            }
            assert!(!dev_unlocked(), "dev unlock must never apply in release");
        }
    }
}
