//! The Windows game host (Story 6.23): starts a library game in the player's session.
//!
//! The service is LocalSystem, so a game starts the way the app does: `WTSQueryUserToken` for the
//! console session's user (who must be the player account) and `CreateProcessAsUserW` on
//! `winsta0\default`. A launch URI (`steam://...`) is started through the program Windows has
//! registered for its scheme (`HKCR\<scheme>\shell\open\command`), because a service in session
//! 0 has no shell to hand a URI to. The handle of every process started is kept, which is how
//! [`alive`](GameHost::alive) knows when a game has exited.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use windows::Win32::Foundation::WAIT_TIMEOUT;
use windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use windows::Win32::System::Registry::{
    HKEY_CLASSES_ROOT, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegGetValueW,
};
use windows::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, PROCESS_INFORMATION, STARTUPINFOW,
    WaitForSingleObject,
};
use windows::core::{PCWSTR, PWSTR};

use crate::games::{GameHost, LaunchSpec};
use crate::launcher::{Owned, console_player_token};
use crate::winsec::wide;

/// The real PC.
pub struct WinGames {
    config: PathBuf,
    /// The processes started and not yet seen to exit, by process id.
    procs: Mutex<HashMap<u32, Owned>>,
}

impl WinGames {
    /// A host for the player account named in the config at `config`.
    pub fn new(config: PathBuf) -> Self {
        Self {
            config,
            procs: Mutex::new(HashMap::new()),
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

impl GameHost for WinGames {
    fn start(&self, spec: &LaunchSpec) -> Result<u32, String> {
        let token = console_player_token(&self.player())?;
        let (cmd, cwd) = command_of(spec)?;
        // SAFETY: plain Win32 calls; every block and handle is released or owned below.
        let (pid, process) = unsafe {
            let mut env: *mut core::ffi::c_void = std::ptr::null_mut();
            let env_ok = CreateEnvironmentBlock(&mut env, Some(token.0), false).is_ok();
            let mut desktop = wide(r"winsta0\default");
            let si = STARTUPINFOW {
                cb: std::mem::size_of::<STARTUPINFOW>() as u32,
                lpDesktop: PWSTR(desktop.as_mut_ptr()),
                ..Default::default()
            };
            let mut pi = PROCESS_INFORMATION::default();
            let mut cmd = wide(&cmd);
            let cwd = cwd.map(|c| wide(&c));
            let created = CreateProcessAsUserW(
                Some(token.0),
                PCWSTR::null(),
                Some(PWSTR(cmd.as_mut_ptr())),
                None,
                None,
                false,
                CREATE_UNICODE_ENVIRONMENT,
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
            (pi.dwProcessId, Owned(pi.hProcess))
        };
        let mut procs = self.procs.lock().unwrap_or_else(|p| p.into_inner());
        procs.insert(pid, process);
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
}
