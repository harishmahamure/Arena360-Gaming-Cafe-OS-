//! The Windows cleanup host (Story 6.11).
//!
//! The service is LocalSystem, but the clipboard and the volume belong to the player's
//! interactive session. So the service does `temp_files` itself (the player's profile is a
//! folder it can reach) and runs `clipboard` and `volume` through a short-lived helper,
//! `arena-kiosk-svc cleanup-helper --clipboard | --volume N`, started in the player's session
//! with `WTSQueryUserToken` + `CreateProcessAsUserW` the way [`launcher`](crate::launcher)
//! starts the app. The service waits for it ([`HELPER_TIMEOUT_MS`]) and ends it if it hangs.

use std::path::PathBuf;

use windows::Win32::Foundation::WAIT_OBJECT_0;
use windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, GetExitCodeProcess,
    PROCESS_INFORMATION, STARTUPINFOW, TerminateProcess, WaitForSingleObject,
};
use windows::core::{PCWSTR, PWSTR};

use crate::cleanup::{CleanupHost, clear_dir_contents};
use crate::cli::HelperTask;
use crate::launcher::{Owned, console_player_token};
use crate::winsec::wide;

/// How long the helper gets before the service ends it.
pub const HELPER_TIMEOUT_MS: u32 = 20_000;
/// The player's temp folder, below its profile folder.
const TEMP_BELOW_PROFILE: &str = r"AppData\Local\Temp";

/// The real PC.
pub struct WinCleanupHost {
    config: PathBuf,
}

impl WinCleanupHost {
    /// A host for the player account named in the config at `config`.
    pub fn new(config: PathBuf) -> Self {
        Self { config }
    }

    fn player(&self) -> String {
        crate::config::KioskConfig::load(&self.config)
            .ok()
            .flatten()
            .map(|c| c.player_user)
            .unwrap_or_else(|| crate::config::DEFAULT_PLAYER_USER.to_owned())
    }

    /// The player account's temp folder, from its profile (no logon needed).
    fn temp_dir(&self) -> Result<Option<PathBuf>, String> {
        let player = self.player();
        let sid = crate::winsec::user_sid(&player)
            .map_err(|_| "the player account cannot be found".to_owned())?;
        let profile = match crate::lockdown::windows::profile_dir(&sid) {
            Ok(p) if !p.trim().is_empty() => PathBuf::from(p.trim()),
            // No profile yet: nothing was ever left there.
            _ => return Ok(None),
        };
        // Never the root of a drive or a folder that is not below a users folder.
        if profile.parent().is_none_or(|p| p.parent().is_none()) {
            return Err("the player profile folder looks wrong".into());
        }
        Ok(Some(profile.join(TEMP_BELOW_PROFILE)))
    }

    /// Runs the helper in the player's session and waits for it.
    fn helper(&self, args: &str) -> Result<(), String> {
        let player = self.player();
        let token = match console_player_token(&player) {
            Ok(t) => t,
            Err(e) => {
                // Nobody is signed in as the player: no clipboard or volume to leave behind.
                tracing::warn!(error = %e, "no player session; the helper is skipped");
                return Ok(());
            }
        };
        let exe = std::env::current_exe().map_err(|e| format!("no service path: {e}"))?;
        let cmd = format!("\"{}\" cleanup-helper {args}", exe.display());
        run_as(&token, &cmd, exe.parent().map(|d| d.display().to_string()))
    }
}

impl CleanupHost for WinCleanupHost {
    fn clear_clipboard(&self) -> Result<(), String> {
        self.helper("--clipboard")
    }

    fn clear_temp_files(&self) -> Result<(), String> {
        match self.temp_dir()? {
            Some(dir) => clear_dir_contents(&dir),
            None => Ok(()),
        }
    }

    fn set_volume(&self, percent: u8) -> Result<(), String> {
        self.helper(&format!("--volume {percent}"))
    }
}

/// Starts `cmd` as the token's user on `winsta0\default` with no window and waits for its exit
/// code; a helper that hangs is ended.
fn run_as(token: &Owned, cmd: &str, cwd: Option<String>) -> Result<(), String> {
    // SAFETY: plain Win32 calls; every handle and block is released.
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
        let cwd = cwd.map(|c| wide(&c));
        let created = CreateProcessAsUserW(
            Some(token.0),
            PCWSTR::null(),
            Some(PWSTR(cmd.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
            env_ok.then_some(env as *const core::ffi::c_void),
            cwd.as_ref().map_or(PCWSTR::null(), |c| PCWSTR(c.as_ptr())),
            &si,
            &mut pi,
        );
        if env_ok {
            let _ = DestroyEnvironmentBlock(env);
        }
        created.map_err(|e| format!("could not start the helper: {e}"))?;
        let _thread = Owned(pi.hThread);
        let process = Owned(pi.hProcess);
        if WaitForSingleObject(process.0, HELPER_TIMEOUT_MS) != WAIT_OBJECT_0 {
            let _ = TerminateProcess(process.0, 1);
            return Err("the helper did not finish in time".into());
        }
        let mut code = 0u32;
        GetExitCodeProcess(process.0, &mut code)
            .map_err(|e| format!("could not read the helper's result: {e}"))?;
        if code == 0 {
            Ok(())
        } else {
            Err(format!("the helper failed (code {code})"))
        }
    }
}

/// `arena-kiosk-svc cleanup-helper`: runs in the player's session.
pub fn run_helper(task: HelperTask) -> Result<(), String> {
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
    // SAFETY: balanced with `CoUninitialize` below.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if com.is_err() {
        return Err(format!("COM did not start: {com:?}"));
    }
    let result = match task {
        HelperTask::Clipboard => clear_clipboard(),
        HelperTask::Volume(percent) => set_volume(percent),
    };
    // SAFETY: COM was initialised on this thread above.
    unsafe { CoUninitialize() };
    result
}

/// Empties the clipboard (another process may hold it for a moment) and its history.
fn clear_clipboard() -> Result<(), String> {
    use windows::ApplicationModel::DataTransfer::Clipboard;
    use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard};
    let mut opened = false;
    for _ in 0..10 {
        // SAFETY: a plain Win32 call with no window; closed below.
        if unsafe { OpenClipboard(None) }.is_ok() {
            opened = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if !opened {
        return Err("the clipboard is busy".into());
    }
    // SAFETY: the clipboard was opened above.
    let emptied = unsafe { EmptyClipboard() };
    // SAFETY: as above.
    unsafe {
        let _ = CloseClipboard();
    }
    emptied.map_err(|e| format!("the clipboard did not empty: {e}"))?;
    // Clipboard history (when it is on): clears everything not pinned.
    Clipboard::ClearHistory().map_err(|e| format!("the clipboard history did not clear: {e}"))?;
    Ok(())
}

/// Sets the default output device's master volume and unmutes it. A PC with no sound device
/// has nothing to set.
fn set_volume(percent: u8) -> Result<(), String> {
    use windows::Win32::Foundation::ERROR_NOT_FOUND;
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender,
    };
    use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
    // SAFETY: COM calls on interfaces owned by this function.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("no audio service: {e}"))?;
        let device = match enumerator.GetDefaultAudioEndpoint(eRender, eConsole) {
            Ok(d) => d,
            Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => {
                eprintln!("no sound device; the volume is left alone");
                return Ok(());
            }
            Err(e) => return Err(format!("no output device: {e}")),
        };
        let volume: IAudioEndpointVolume = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| format!("no volume control: {e}"))?;
        volume
            .SetMasterVolumeLevelScalar(f32::from(percent) / 100.0, std::ptr::null())
            .map_err(|e| format!("the volume did not set: {e}"))?;
        volume
            .SetMute(false, std::ptr::null())
            .map_err(|e| format!("the volume did not unmute: {e}"))?;
    }
    Ok(())
}
