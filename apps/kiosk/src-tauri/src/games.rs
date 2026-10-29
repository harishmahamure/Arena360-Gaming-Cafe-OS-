//! Bringing a running game's window forward (Story 6.23). The service starts games in the
//! player's session and says when one is already running; this runs in that session, where the
//! app has just received the player's click and so may take the foreground.

/// Brings the first visible top-level window of process `pid` to the front, restoring it if it is
/// minimised. Answers whether a window was found. Off Windows there is none.
pub fn bring_forward(pid: u32) -> bool {
    sys::bring_forward(pid)
}

#[cfg(windows)]
mod sys {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GetWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
        SW_RESTORE, SetForegroundWindow, ShowWindow,
    };
    use windows::core::BOOL;

    struct Search {
        pid: u32,
        found: Option<HWND>,
    }

    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: `lparam` is the `Search` `bring_forward` passes and outlives the enumeration.
        let search = unsafe { &mut *(lparam.0 as *mut Search) };
        let mut owner = 0u32;
        // SAFETY: plain queries on a handle Windows just gave us.
        let (visible, unowned) = unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut owner));
            (
                IsWindowVisible(hwnd).as_bool(),
                GetWindow(hwnd, GW_OWNER).is_err(),
            )
        };
        if owner == search.pid && visible && unowned {
            search.found = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }

    pub fn bring_forward(pid: u32) -> bool {
        let mut search = Search { pid, found: None };
        // SAFETY: the callback only touches `search`, which lives across the call. The call
        // reports an error when the callback stops it early, which is how a window is found.
        let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize)) };
        let Some(hwnd) = search.found else {
            return false;
        };
        // SAFETY: a window that has gone makes the calls fail harmlessly.
        unsafe {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            let _ = SetForegroundWindow(hwnd);
        }
        true
    }
}

#[cfg(not(windows))]
mod sys {
    pub fn bring_forward(_pid: u32) -> bool {
        false
    }
}
