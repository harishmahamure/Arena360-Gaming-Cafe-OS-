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
/// The branch's clean browser templates, next to the service's config.
const BROWSER_TEMPLATES: &str = "browser-templates";

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

    /// The player account's profile folder, from the system (no logon needed).
    fn profile(&self) -> Result<Option<PathBuf>, String> {
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
        Ok(Some(profile))
    }

    /// The player account's temp folder.
    fn temp_dir(&self) -> Result<Option<PathBuf>, String> {
        Ok(self.profile()?.map(|p| p.join(TEMP_BELOW_PROFILE)))
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

    fn profile_dir(&self) -> Result<Option<PathBuf>, String> {
        self.profile()
    }

    fn steam_dir(&self) -> Option<PathBuf> {
        steam_install_dir()
    }

    fn browser_templates(&self) -> Option<PathBuf> {
        self.config.parent().map(|d| d.join(BROWSER_TEMPLATES))
    }

    fn clear_steam_login(&self) -> Result<(), String> {
        self.helper("--steam-login")
    }

    fn clear_credentials(&self) -> Result<(), String> {
        self.helper("--credentials")
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
        HelperTask::SteamLogin => clear_steam_login(),
        HelperTask::Credentials => clear_credentials(),
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

/// The Steam install folder (`HKLM\SOFTWARE\WOW6432Node\Valve\Steam`, `InstallPath`), if Steam is
/// installed and the folder is there.
fn steam_install_dir() -> Option<PathBuf> {
    use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    let key = wide(r"SOFTWARE\WOW6432Node\Valve\Steam");
    let name = wide("InstallPath");
    let mut buf = [0u16; 520];
    let mut bytes = std::mem::size_of_val(&buf) as u32;
    // SAFETY: `buf` and `bytes` describe the same buffer; both names are NUL-terminated.
    let rc = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    };
    if rc.0 != 0 {
        return None;
    }
    let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    let path = PathBuf::from(String::from_utf16_lossy(&buf[..len]));
    // Never the root of a drive.
    (path.is_dir() && path.parent().is_some()).then_some(path)
}

/// Forgets the remembered Steam account of the player running this helper
/// (`HKCU\Software\Valve\Steam`): no automatic sign-in, no remembered password.
fn clear_steam_login() -> Result<(), String> {
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_DWORD, RegCloseKey, RegDeleteValueW,
        RegOpenKeyExW, RegSetValueExW,
    };
    let sub = wide(r"Software\Valve\Steam");
    let mut key = HKEY::default();
    // SAFETY: `sub` is NUL-terminated; `key` is closed below on success.
    let rc = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(sub.as_ptr()),
            None,
            KEY_SET_VALUE,
            &mut key,
        )
    };
    match rc {
        ERROR_SUCCESS => {}
        // Steam never ran for this player.
        ERROR_FILE_NOT_FOUND => return Ok(()),
        rc => return Err(format!("the Steam settings cannot be opened ({})", rc.0)),
    }
    let auto = wide("AutoLoginUser");
    let remember = wide("RememberPassword");
    // SAFETY: `key` is open with `KEY_SET_VALUE`; names are NUL-terminated.
    let (deleted, set) = unsafe {
        (
            RegDeleteValueW(key, PCWSTR(auto.as_ptr())),
            RegSetValueExW(
                key,
                PCWSTR(remember.as_ptr()),
                None,
                REG_DWORD,
                Some(&0u32.to_le_bytes()),
            ),
        )
    };
    // SAFETY: the key was opened above.
    unsafe {
        let _ = RegCloseKey(key);
    }
    if deleted != ERROR_SUCCESS && deleted != ERROR_FILE_NOT_FOUND {
        return Err(format!("Steam's remembered account stayed ({})", deleted.0));
    }
    if set != ERROR_SUCCESS {
        return Err(format!("Steam's remembered password stayed ({})", set.0));
    }
    Ok(())
}

/// Removes every Credential Manager entry of the player running this helper.
fn clear_credentials() -> Result<(), String> {
    use windows::Win32::Foundation::ERROR_NOT_FOUND;
    use windows::Win32::Security::Credentials::{
        CRED_ENUMERATE_ALL_CREDENTIALS, CREDENTIALW, CredDeleteW, CredEnumerateW, CredFree,
    };
    let mut count = 0u32;
    let mut list: *mut *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: on success `list` holds `count` entries owned by the system until `CredFree`.
    let enumerated = unsafe {
        CredEnumerateW(
            PCWSTR::null(),
            Some(CRED_ENUMERATE_ALL_CREDENTIALS),
            &mut count,
            &mut list,
        )
    };
    if let Err(e) = enumerated {
        // No entries is not a failure.
        return if e.code() == ERROR_NOT_FOUND.to_hresult() {
            Ok(())
        } else {
            Err(format!("the saved credentials cannot be listed: {e}"))
        };
    }
    let mut left = 0usize;
    for i in 0..count as usize {
        // SAFETY: `i < count`; the entry and its strings live until `CredFree` below.
        let cred = unsafe { &**list.add(i) };
        // SAFETY: `TargetName` is a NUL-terminated string owned by the entry.
        let gone = unsafe { CredDeleteW(PCWSTR(cred.TargetName.0), cred.Type, None) };
        if let Err(e) = gone
            && e.code() != ERROR_NOT_FOUND.to_hresult()
        {
            left += 1;
        }
    }
    // SAFETY: `list` came from `CredEnumerateW`.
    unsafe { CredFree(list.cast()) };
    match left {
        0 => Ok(()),
        1 => Err("1 saved credential could not be removed".into()),
        n => Err(format!("{n} saved credentials could not be removed")),
    }
}
