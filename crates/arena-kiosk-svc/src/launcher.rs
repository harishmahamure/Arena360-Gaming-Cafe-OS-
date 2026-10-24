//! Restarting the Kiosk app.
//!
//! - Windows ([`SessionLauncher`]): only when the active console session's user is the
//!   configured `player_user`. The previous launch's job object is terminated (taking WebView2's
//!   child processes with it) and waited for, any other `arena-kiosk-ui.exe` is killed, then the
//!   app starts in that session as the player (`WTSGetActiveConsoleSessionId` +
//!   `WTSQueryUserToken` + `CreateProcessAsUserW`), suspended, inside a fresh kill-on-close job,
//!   then resumed. The service is LocalSystem; the app never is.
//! - Dev console mode ([`CommandLauncher`]): kill and respawn a child process.
//! - Tests: any mock.
//!
//! Admin exit (Story 1.20): [`AppLauncher::start_desktop`] starts Explorer in the player's
//! session the same way, in its own job, and [`AppLauncher::stop_desktop`] ends that job.
//!
//! The desktop is never exposed while the app restarts because the player's shell is the kiosk
//! itself (Story 1.17's shell replacement: no Explorer behind it).

use std::path::PathBuf;
use std::sync::Mutex;

/// Kills and relaunches the app.
pub trait AppLauncher: Send + Sync {
    fn restart(&self) -> Result<(), String>;

    /// Whether the app can run now (Windows: a console session whose user is the player). While
    /// it cannot, the watchdog counts no misses and never restarts.
    fn ready(&self) -> bool {
        true
    }

    /// Admin exit (Story 1.20): starts Explorer in the player's session. The default has no
    /// desktop to start.
    fn start_desktop(&self) -> Result<(), String> {
        Err("no desktop launcher on this host".into())
    }

    /// Ends the admin exit's Explorer (and what it started). The default does nothing.
    fn stop_desktop(&self) {}
}

/// Does nothing but log (no app configured, e.g. a console run without `--app`).
#[derive(Debug, Default)]
pub struct NoopLauncher;

impl AppLauncher for NoopLauncher {
    fn restart(&self) -> Result<(), String> {
        tracing::warn!("the app should be restarted, but no launcher is configured");
        Ok(())
    }
}

/// Dev console mode: the app is a child process of the service.
pub struct CommandLauncher {
    program: PathBuf,
    args: Vec<String>,
    child: Mutex<Option<std::process::Child>>,
}

impl CommandLauncher {
    pub fn new(program: impl Into<PathBuf>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
            child: Mutex::new(None),
        }
    }
}

impl AppLauncher for CommandLauncher {
    fn restart(&self) -> Result<(), String> {
        let mut child = self.child.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(mut old) = child.take() {
            let _ = old.kill();
            let _ = old.wait();
        }
        let spawned = std::process::Command::new(&self.program)
            .args(&self.args)
            .spawn()
            .map_err(|e| format!("spawn {}: {e}", self.program.display()))?;
        *child = Some(spawned);
        Ok(())
    }
}

impl Drop for CommandLauncher {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock()
            && let Some(mut c) = child.take()
        {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// The app executable: `app_path` from the config at `config`, else [`default_app_path`].
pub fn configured_app_path(config: &std::path::Path) -> PathBuf {
    crate::config::KioskConfig::load(config)
        .ok()
        .flatten()
        .and_then(|c| c.app_path)
        .map(PathBuf::from)
        .unwrap_or_else(default_app_path)
}

/// The app executable's default location: next to the service.
pub fn default_app_path() -> PathBuf {
    let name = if cfg!(windows) {
        "arena-kiosk-ui.exe"
    } else {
        "arena-kiosk-ui"
    };
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

#[cfg(windows)]
pub use session::SessionLauncher;

#[cfg(windows)]
mod session {
    use super::*;
    use crate::winsec::{sid_string, user_sid, wide};
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TOKEN_USER, TokenUser};
    use windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };
    use windows::Win32::System::RemoteDesktop::{WTSGetActiveConsoleSessionId, WTSQueryUserToken};
    use windows::Win32::System::Threading::{
        CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, PROCESS_INFORMATION,
        ResumeThread, STARTUPINFOW, TerminateProcess, WaitForSingleObject,
    };
    use windows::core::{PCWSTR, PWSTR};

    /// How long a terminated app gets to exit before the relaunch.
    const EXIT_WAIT_MS: u32 = 5_000;

    /// A handle closed on drop.
    struct Owned(HANDLE);
    // SAFETY: kernel handles may be used from any thread.
    unsafe impl Send for Owned {}
    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: owned handle.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    /// The running app: its job and its process.
    struct Running {
        job: Owned,
        process: Owned,
    }

    /// Windows: the app in the active console session, as the player.
    pub struct SessionLauncher {
        exe: PathBuf,
        config: PathBuf,
        running: Mutex<Option<Running>>,
        /// The admin exit's Explorer, in its own job.
        desktop: Mutex<Option<Running>>,
    }

    impl SessionLauncher {
        pub fn new(exe: PathBuf, config: PathBuf) -> Self {
            Self {
                exe,
                config,
                running: Mutex::new(None),
                desktop: Mutex::new(None),
            }
        }

        fn player(&self) -> String {
            crate::config::KioskConfig::load(&self.config)
                .ok()
                .flatten()
                .map(|c| c.player_user)
                .unwrap_or_else(|| crate::config::DEFAULT_PLAYER_USER.to_owned())
        }

        /// The console session's user token, when that user is the player.
        fn player_token(&self) -> Result<Owned, String> {
            // SAFETY: plain Win32 calls; the token is owned by the returned guard.
            unsafe {
                let session = WTSGetActiveConsoleSessionId();
                if session == u32::MAX {
                    return Err("no active console session".into());
                }
                let mut token = HANDLE::default();
                WTSQueryUserToken(session, &mut token)
                    .map_err(|e| format!("no user in the console session: {e}"))?;
                let token = Owned(token);
                let mut buf = vec![0u8; 256];
                let mut len = 0u32;
                GetTokenInformation(
                    token.0,
                    TokenUser,
                    Some(buf.as_mut_ptr().cast()),
                    buf.len() as u32,
                    &mut len,
                )
                .map_err(|e| format!("GetTokenInformation: {e}"))?;
                let user = &*(buf.as_ptr() as *const TOKEN_USER);
                let session_sid = sid_string(user.User.Sid).map_err(|e| e.to_string())?;
                let player = self.player();
                let player_sid = user_sid(&player).map_err(|e| e.to_string())?;
                if session_sid != player_sid {
                    return Err(format!("the console session is not the player {player}"));
                }
                Ok(token)
            }
        }

        /// Ends the previous launch: its whole job, then waits for the process.
        fn stop_previous(&self) {
            let previous = self
                .running
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take();
            if let Some(r) = previous {
                // SAFETY: owned handles.
                unsafe {
                    let _ = TerminateJobObject(r.job.0, 1);
                    let _ = WaitForSingleObject(r.process.0, EXIT_WAIT_MS);
                }
            }
            if let Some(name) = self.exe.file_name().and_then(|n| n.to_str()) {
                // One started outside this service (by the shell): not running is fine.
                let _ = std::process::Command::new("taskkill")
                    .args(["/F", "/T", "/IM", name])
                    .status();
            }
        }
    }

    impl AppLauncher for SessionLauncher {
        fn ready(&self) -> bool {
            self.player_token().is_ok()
        }

        fn restart(&self) -> Result<(), String> {
            let token = self.player_token()?;
            self.stop_previous();
            let cmd = format!("\"{}\"", self.exe.display());
            let cwd = self.exe.parent().map(|d| d.display().to_string());
            let running = spawn_in_job(&token, &cmd, cwd.as_deref())?;
            *self.running.lock().unwrap_or_else(|p| p.into_inner()) = Some(running);
            Ok(())
        }

        fn start_desktop(&self) -> Result<(), String> {
            let token = self.player_token()?;
            self.stop_desktop();
            let windows = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
            let cmd = format!("\"{windows}\\explorer.exe\"");
            let running = spawn_in_job(&token, &cmd, Some(&windows))?;
            *self.desktop.lock().unwrap_or_else(|p| p.into_inner()) = Some(running);
            Ok(())
        }

        fn stop_desktop(&self) {
            let previous = self
                .desktop
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take();
            if let Some(r) = previous {
                // SAFETY: owned handles.
                unsafe {
                    let _ = TerminateJobObject(r.job.0, 1);
                    let _ = WaitForSingleObject(r.process.0, EXIT_WAIT_MS);
                }
            }
        }
    }

    /// Starts `cmd` as the token's user on `winsta0\default`, suspended, inside a fresh
    /// kill-on-close job (so ending the job ends everything it started), then resumes it.
    fn spawn_in_job(token: &Owned, cmd: &str, cwd: Option<&str>) -> Result<Running, String> {
        // SAFETY: plain Win32 calls; every handle and block is released.
        unsafe {
            let job = Owned(
                CreateJobObjectW(None, PCWSTR::null())
                    .map_err(|e| format!("CreateJobObjectW: {e}"))?,
            );
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .map_err(|e| format!("SetInformationJobObject: {e}"))?;
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
            let created = CreateProcessAsUserW(
                Some(token.0),
                PCWSTR::null(),
                Some(PWSTR(cmd.as_mut_ptr())),
                None,
                None,
                false,
                CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED,
                env_ok.then_some(env as *const core::ffi::c_void),
                cwd.as_ref().map_or(PCWSTR::null(), |c| PCWSTR(c.as_ptr())),
                &si,
                &mut pi,
            );
            if env_ok {
                let _ = DestroyEnvironmentBlock(env);
            }
            created.map_err(|e| format!("CreateProcessAsUserW: {e}"))?;
            let thread = Owned(pi.hThread);
            let process = Owned(pi.hProcess);
            if let Err(e) = AssignProcessToJobObject(job.0, process.0) {
                let _ = TerminateProcess(process.0, 1);
                return Err(format!("AssignProcessToJobObject: {e}"));
            }
            ResumeThread(thread.0);
            Ok(Running { job, process })
        }
    }
}
