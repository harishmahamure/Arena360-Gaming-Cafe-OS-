//! The Windows game host (Story 6.23): starts a library game in the player's session.
//!
//! The service is LocalSystem, so a game starts the way the app does: `WTSQueryUserToken` for the
//! console session's user (who must be the player account) and `CreateProcessAsUserW` on
//! `winsta0\default`. A launch URI (`steam://...`) is started through the program Windows has
//! registered for its scheme (`HKCR\<scheme>\shell\open\command`), because a service in session
//! 0 has no shell to hand a URI to. The handle of every process started is kept, which is how
//! [`alive`](GameHost::alive) knows when a game has exited.
//!
//! The booster (Story 6.25) adds three things. The priority is set on the process handle kept
//! here. The power plan is read and set with `powercfg`. A game's first window is seen by a
//! helper, `arena-kiosk-svc window-watch <pid>`, started in the player's session (a service in
//! session 0 cannot enumerate the player's windows); [`has_window`](GameHost::has_window) is
//! whether that helper has exited with [`WINDOW_FOUND`].

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use arena_proto::BoosterPriority;
use windows::Win32::Foundation::{HWND, LPARAM, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use windows::Win32::System::Registry::{
    HKEY_CLASSES_ROOT, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegGetValueW,
};
use windows::Win32::System::Threading::{
    ABOVE_NORMAL_PRIORITY_CLASS, CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT,
    CreateProcessAsUserW, GetExitCodeProcess, HIGH_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
    OpenProcess, PROCESS_INFORMATION, PROCESS_SYNCHRONIZE, STARTUPINFOW, SetPriorityClass,
    WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GWL_EXSTYLE, GetWindow, GetWindowLongPtrW, GetWindowThreadProcessId,
    IsWindowVisible, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, PCWSTR, PWSTR};

use crate::games::{GameHost, LaunchSpec};
use crate::launcher::{Owned, console_player_token};
use crate::winsec::wide;

/// The real PC.
pub struct WinGames {
    config: PathBuf,
    /// The processes started and not yet seen to exit, by process id.
    procs: Mutex<HashMap<u32, Owned>>,
    /// The window watchers (Story 6.25) of the games running, by the game's process id.
    watchers: Mutex<HashMap<u32, Owned>>,
}

impl WinGames {
    /// A host for the player account named in the config at `config`.
    pub fn new(config: PathBuf) -> Self {
        Self {
            config,
            procs: Mutex::new(HashMap::new()),
            watchers: Mutex::new(HashMap::new()),
        }
    }

    fn player(&self) -> String {
        crate::config::KioskConfig::load(&self.config)
            .ok()
            .flatten()
            .map(|c| c.player_user)
            .unwrap_or_else(|| crate::config::DEFAULT_PLAYER_USER.to_owned())
    }
}

/// The command line and working folder `spec` starts with.
fn command_of(spec: &LaunchSpec) -> Result<(String, Option<String>), String> {
    if let Some(exe) = &spec.exe {
        let mut cmd = format!("\"{exe}\"");
        if !spec.args.trim().is_empty() {
            cmd.push(' ');
            cmd.push_str(spec.args.trim());
        }
        let cwd = std::path::Path::new(exe)
            .parent()
            .map(|d| d.display().to_string());
        return Ok((cmd, cwd));
    }
    let uri = spec.uri.as_deref().ok_or("nothing to start")?;
    let scheme = uri.split_once(':').map_or("", |(s, _)| s);
    let handler = scheme_handler(scheme)
        .ok_or_else(|| format!("no program is registered for {scheme}: links"))?;
    Ok((fill_uri(&handler, uri), None))
}

/// `command` (a registered `shell\open\command`) with its `%1` / `%L` placeholders replaced by
/// `uri`, or `uri` added after it when it has none. The URI holds no quote (checked before).
fn fill_uri(command: &str, uri: &str) -> String {
    if command.contains("%1") || command.contains("%L") || command.contains("%l") {
        command
            .replace("%1", uri)
            .replace("%L", uri)
            .replace("%l", uri)
    } else {
        format!("{command} \"{uri}\"")
    }
}

/// The program Windows has registered for `scheme:` links, as a command line.
fn scheme_handler(scheme: &str) -> Option<String> {
    let key = wide(&format!(r"{scheme}\shell\open\command"));
    let mut buf = vec![0u16; 2048];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY: `buf` holds `len` bytes; REG_EXPAND_SZ is expanded by RegGetValueW.
    let rc = unsafe {
        RegGetValueW(
            HKEY_CLASSES_ROOT,
            PCWSTR(key.as_ptr()),
            PCWSTR::null(),
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
    };
    if rc.is_err() {
        return None;
    }
    let units = (len as usize / 2).min(buf.len());
    let command = String::from_utf16_lossy(&buf[..units])
        .trim_end_matches('\0')
        .trim()
        .to_owned();
    (!command.is_empty()).then_some(command)
}

/// Starts `cmd` as the token's user on `winsta0\default`; the process id and the process handle.
fn spawn_as(
    token: &Owned,
    cmd: &str,
    cwd: Option<&str>,
    no_window: bool,
) -> Result<(u32, Owned), String> {
    // SAFETY: plain Win32 calls; every block and handle is released or owned below.
    unsafe {
        let mut env: *mut core::ffi::c_void = std::ptr::null_mut();
        let env_ok = CreateEnvironmentBlock(&mut env, Some(token.0), false).is_ok();
        let mut desktop = wide(r"winsta0\default");
        let si = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            lpDesktop: PWSTR(desktop.as_mut_ptr()),
            ..Default::default()
        };
        let mut pi = PROCESS_INFORMATION::default();
        let mut cmd = wide(cmd);
        let cwd = cwd.map(wide);
        let flags = if no_window {
            CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW
        } else {
            CREATE_UNICODE_ENVIRONMENT
        };
        let created = CreateProcessAsUserW(
            Some(token.0),
            PCWSTR::null(),
            Some(PWSTR(cmd.as_mut_ptr())),
            None,
            None,
            false,
            flags,
            env_ok.then_some(env as *const core::ffi::c_void),
            cwd.as_ref().map_or(PCWSTR::null(), |c| PCWSTR(c.as_ptr())),
            &si,
            &mut pi,
        );
        if env_ok {
            let _ = DestroyEnvironmentBlock(env);
        }
        created.map_err(|e| format!("CreateProcessAsUserW: {e}"))?;
        let _thread = Owned(pi.hThread);
        Ok((pi.dwProcessId, Owned(pi.hProcess)))
    }
}

impl WinGames {
    /// Starts the helper that waits for process `pid` to show a window, in the player's session
    /// (a service in session 0 cannot see the player's windows). A helper that cannot start
    /// only costs the launch time.
    fn watch_window(&self, token: &Owned, pid: u32) {
        let result = std::env::current_exe()
            .map_err(|e| format!("no service path: {e}"))
            .and_then(|exe| {
                let cmd = format!("\"{}\" window-watch {pid}", exe.display());
                spawn_as(
                    token,
                    &cmd,
                    exe.parent().map(|d| d.display().to_string()).as_deref(),
                    true,
                )
            });
        match result {
            Ok((_, helper)) => {
                let mut watchers = self.watchers.lock().unwrap_or_else(|p| p.into_inner());
                watchers.insert(pid, helper);
            }
            Err(e) => tracing::warn!(pid, error = %e, "the window watcher did not start"),
        }
    }
}

impl GameHost for WinGames {
    fn start(&self, spec: &LaunchSpec) -> Result<u32, String> {
        let token = console_player_token(&self.player())?;
        let (cmd, cwd) = command_of(spec)?;
        let (pid, process) = spawn_as(&token, &cmd, cwd.as_deref(), false)?;
        self.procs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(pid, process);
        self.watch_window(&token, pid);
        Ok(pid)
    }

    fn alive(&self, pid: u32) -> bool {
        let mut procs = self.procs.lock().unwrap_or_else(|p| p.into_inner());
        let Some(process) = procs.get(&pid) else {
            return false;
        };
        // SAFETY: an owned process handle.
        let running = unsafe { WaitForSingleObject(process.0, 0) } == WAIT_TIMEOUT;
        if !running {
            procs.remove(&pid);
        }
        running
    }

    fn set_priority(&self, pid: u32, priority: BoosterPriority) -> Result<(), String> {
        let procs = self.procs.lock().unwrap_or_else(|p| p.into_inner());
        let process = procs.get(&pid).ok_or("the game has exited")?;
        let class = match priority {
            BoosterPriority::Normal => NORMAL_PRIORITY_CLASS,
            BoosterPriority::AboveNormal => ABOVE_NORMAL_PRIORITY_CLASS,
            BoosterPriority::High => HIGH_PRIORITY_CLASS,
        };
        // SAFETY: an owned process handle created with full access.
        unsafe { SetPriorityClass(process.0, class) }.map_err(|e| format!("SetPriorityClass: {e}"))
    }

    fn has_window(&self, pid: u32) -> bool {
        let mut watchers = self.watchers.lock().unwrap_or_else(|p| p.into_inner());
        let Some(helper) = watchers.get(&pid) else {
            return false;
        };
        // SAFETY: an owned process handle.
        let (done, code) = unsafe {
            let done = WaitForSingleObject(helper.0, 0) == WAIT_OBJECT_0;
            let mut code = 1u32;
            if done {
                let _ = GetExitCodeProcess(helper.0, &mut code);
            }
            (done, code)
        };
        if done {
            watchers.remove(&pid);
        }
        done && code == WINDOW_FOUND
    }

    fn power_plan(&self) -> Result<String, String> {
        let out = powercfg(&["/getactivescheme"])?;
        parse_active_plan(&out).ok_or_else(|| "powercfg did not name the active plan".into())
    }

    fn set_power_plan(&self, id: &str) -> Result<(), String> {
        if !is_plan_id(id) {
            return Err("not a power plan id".into());
        }
        powercfg(&["/setactive", id]).map(|_| ())
    }

    fn find_process(&self, images: &[&str]) -> Option<u32> {
        find_image(images)
    }
}

/// The id of a running process whose image file name is one of `images` (any case).
fn find_image(images: &[&str]) -> Option<u32> {
    // SAFETY: a snapshot walked with a correctly sized entry, then closed.
    unsafe {
        let snap = Owned(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?);
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap.0, &mut entry).is_err() {
            return None;
        }
        loop {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let image = String::from_utf16_lossy(&entry.szExeFile[..len]);
            if images.iter().any(|i| i.eq_ignore_ascii_case(&image)) {
                return Some(entry.th32ProcessID);
            }
            if Process32NextW(snap.0, &mut entry).is_err() {
                return None;
            }
        }
    }
}

/// The exit code of `window-watch` when the process showed a window.
pub const WINDOW_FOUND: u32 = 0;
/// The exit code of `window-watch` when the process exited or the time ran out.
pub const WINDOW_NOT_FOUND: u32 = 2;
/// How often `window-watch` looks.
const WATCH_POLL_MS: u64 = 100;
/// A bit past the 3 minutes a launch is measured for, so the service gives up first.
const WATCH_LIMIT_MS: u64 = 190_000;

/// Runs `powercfg` and answers what it printed.
fn powercfg(args: &[&str]) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("powercfg")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("powercfg did not run: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!("powercfg failed ({})", out.status))
    }
}

/// Whether `id` is a GUID in the `8-4-4-4-12` form with hex digits.
fn is_plan_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('-').collect();
    parts.len() == 5
        && parts
            .iter()
            .zip([8usize, 4, 4, 4, 12])
            .all(|(p, n)| p.len() == n && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The plan id in `powercfg /getactivescheme`'s line (`Power Scheme GUID: <id>  (<name>)`).
fn parse_active_plan(output: &str) -> Option<String> {
    output
        .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .find(|w| is_plan_id(w))
        .map(str::to_ascii_lowercase)
}

/// `arena-kiosk-svc window-watch <pid>`, run in the player's session: waits until process `pid`
/// has a visible top-level window of its own (exit 0), or until it exits or the limit passes
/// (exit 2).
pub fn run_window_watch(pid: u32) -> u32 {
    let mut waited = 0;
    loop {
        if has_main_window(pid) {
            return WINDOW_FOUND;
        }
        if waited >= WATCH_LIMIT_MS || !process_running(pid) {
            return WINDOW_NOT_FOUND;
        }
        std::thread::sleep(std::time::Duration::from_millis(WATCH_POLL_MS));
        waited += WATCH_POLL_MS;
    }
}

fn process_running(pid: u32) -> bool {
    // SAFETY: a plain open, wait and close of a process handle.
    unsafe {
        let Ok(process) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) else {
            return false;
        };
        let process = Owned(process);
        WaitForSingleObject(process.0, 0) == WAIT_TIMEOUT
    }
}

struct Search {
    pid: u32,
    found: bool,
}

unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam` is the `Search` `has_main_window` passes and outlives the enumeration.
    let search = unsafe { &mut *(lparam.0 as *mut Search) };
    let mut owner = 0u32;
    // SAFETY: plain queries on a handle Windows just gave us.
    let (visible, unowned, tool) = unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut owner));
        (
            IsWindowVisible(hwnd).as_bool(),
            GetWindow(hwnd, GW_OWNER).is_err(),
            GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0,
        )
    };
    if owner == search.pid && visible && unowned && !tool {
        search.found = true;
        return BOOL(0);
    }
    BOOL(1)
}

/// Whether process `pid` has a visible, unowned top-level window that is not a tool window.
fn has_main_window(pid: u32) -> bool {
    let mut search = Search { pid, found: false };
    // SAFETY: the callback only touches `search`, which lives across the call. The call reports
    // an error when the callback stops it early, which is how a window is found.
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize)) };
    search.found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_executable_starts_quoted_with_its_arguments_in_its_own_folder() {
        let spec = LaunchSpec {
            exe: Some(r"C:\Games\My Game\game.exe".into()),
            args: " -windowed ".into(),
            uri: None,
        };
        let (cmd, cwd) = command_of(&spec).unwrap();
        assert_eq!(cmd, r#""C:\Games\My Game\game.exe" -windowed"#);
        assert_eq!(cwd.as_deref(), Some(r"C:\Games\My Game"));
    }

    #[test]
    fn a_uri_fills_the_registered_handlers_placeholder_or_follows_it() {
        let uri = "steam://rungameid/730";
        assert_eq!(
            fill_uri(r#""C:\Steam\steam.exe" -- "%1""#, uri),
            r#""C:\Steam\steam.exe" -- "steam://rungameid/730""#
        );
        assert_eq!(
            fill_uri(r#""C:\Epic\launcher.exe""#, uri),
            r#""C:\Epic\launcher.exe" "steam://rungameid/730""#
        );
    }

    #[test]
    fn the_active_plan_is_read_from_powercfgs_line_and_only_a_guid_is_passed_back() {
        let line = "Power Scheme GUID: 381B4222-F694-41F0-9685-FF5BB260DF2E  (Balanced)\r\n";
        assert_eq!(
            parse_active_plan(line).as_deref(),
            Some("381b4222-f694-41f0-9685-ff5bb260df2e")
        );
        assert_eq!(parse_active_plan("Aktives Schema: nothing here"), None);
        assert!(is_plan_id(crate::games::HIGH_PERFORMANCE_PLAN));
        for bad in [
            "",
            "high",
            "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635",
            "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635g",
            "x & calc",
        ] {
            assert!(!is_plan_id(bad), "{bad:?}");
        }
    }
}
