//! The Windows transport: `\\.\pipe\arena-kiosk-v1`, created with a security descriptor that
//! allows only SYSTEM and the player account ([`super::acl`]), remote clients rejected, and the
//! first instance created with `FILE_FLAG_FIRST_PIPE_INSTANCE` so nobody can squat the name.
//!
//! - **Peer check.** A connected client is accepted only when its process image
//!   (`GetNamedPipeClientProcessId`) is the installed `arena-kiosk-ui.exe`; anything else is
//!   disconnected and logged.
//! - **Rebind.** Every [`RECHECK`] the configured `player_user` is read again; when it changed,
//!   or could not be resolved before, the SID is resolved again and new instances get the new
//!   descriptor. Until the player resolves the pipe is SYSTEM-only and [`IpcServer::ready`] is
//!   false (the watchdog then counts no misses).

use std::io;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::time::Instant;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::core::{PCWSTR, PWSTR};

use super::acl::PipeAcl;
use super::{IpcServer, IpcStream};
use crate::winsec::{user_sid, wide, win};

/// How often the player account is looked up again.
pub const RECHECK: Duration = Duration::from_secs(30);

/// A self-relative security descriptor allocated by Windows (freed on drop).
pub struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

// SAFETY: the descriptor is immutable after creation and only read by CreateNamedPipeW.
unsafe impl Send for SecurityDescriptor {}
unsafe impl Sync for SecurityDescriptor {}

impl SecurityDescriptor {
    pub fn from_sddl(sddl: &str) -> io::Result<Self> {
        let w = wide(sddl);
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `w` is NUL-terminated and outlives the call; `sd` receives a LocalAlloc'd
        // descriptor.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(w.as_ptr()),
                SDDL_REVISION_1,
                &mut sd,
                None,
            )
        }
        .map_err(win)?;
        Ok(Self(sd))
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0.0)));
        }
    }
}

/// The ACL for `player` (SYSTEM-only, and `false`, when the account does not resolve to a user).
fn acl_for(player: &str) -> (PipeAcl, bool) {
    match user_sid(player).map(|sid| PipeAcl::for_player(&sid)) {
        Ok(Ok(acl)) => (acl, true),
        Ok(Err(e)) => {
            tracing::error!(detail = %e, "the player account SID is unusable; the pipe is SYSTEM-only");
            (PipeAcl::system_only(), false)
        }
        Err(e) => {
            tracing::error!(error = %e, "the player account was not found; the pipe is SYSTEM-only");
            (PipeAcl::system_only(), false)
        }
    }
}

fn configured_player(config: &Path) -> String {
    crate::config::KioskConfig::load(config)
        .ok()
        .flatten()
        .map(|c| c.player_user)
        .unwrap_or_else(|| crate::config::DEFAULT_PLAYER_USER.to_owned())
}

/// The image path of process `pid`.
fn process_image(pid: u32) -> io::Result<PathBuf> {
    // SAFETY: the handle is closed below.
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.map_err(win)?;
    let mut buf = vec![0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` holds `len` u16s.
    let r = unsafe {
        QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
    };
    unsafe {
        let _ = CloseHandle(h);
    }
    r.map_err(win)?;
    Ok(PathBuf::from(String::from_utf16_lossy(
        &buf[..len as usize],
    )))
}

fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let s = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_owned());
        s.display()
            .to_string()
            .trim_start_matches(r"\\?\")
            .to_lowercase()
    };
    norm(a) == norm(b)
}

/// The pipe listener.
pub struct PipeIpcServer {
    name: String,
    config: PathBuf,
    app_exe: PathBuf,
    player: String,
    sd: SecurityDescriptor,
    ready: Arc<AtomicBool>,
    next: Option<NamedPipeServer>,
    checked: Instant,
}

impl PipeIpcServer {
    /// Creates the first instance of `name`, open to SYSTEM and the `player_user` of
    /// `config` only, accepting only clients whose image is `app_exe`.
    pub fn bind(name: &str, config: &Path, app_exe: &Path) -> io::Result<Self> {
        let player = configured_player(config);
        let (acl, resolved) = acl_for(&player);
        let mut server = Self {
            name: name.to_owned(),
            config: config.to_owned(),
            app_exe: app_exe.to_owned(),
            player,
            sd: SecurityDescriptor::from_sddl(&acl.sddl())?,
            ready: Arc::new(AtomicBool::new(resolved)),
            next: None,
            checked: Instant::now(),
        };
        server.next = Some(server.create(true)?);
        Ok(server)
    }

    fn create(&self, first: bool) -> io::Result<NamedPipeServer> {
        let mut attrs = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.sd.0.0,
            bInheritHandle: false.into(),
        };
        // SAFETY: `attrs` and the descriptor it points to outlive the call.
        unsafe {
            ServerOptions::new()
                .first_pipe_instance(first)
                .reject_remote_clients(true)
                .create_with_security_attributes_raw(
                    &self.name,
                    (&mut attrs as *mut SECURITY_ATTRIBUTES).cast(),
                )
        }
    }

    /// Every [`RECHECK`]: re-resolve the player when it changed or never resolved, and give
    /// new instances the new descriptor.
    fn recheck(&mut self) -> io::Result<()> {
        if self.checked.elapsed() < RECHECK {
            return Ok(());
        }
        self.checked = Instant::now();
        let player = configured_player(&self.config);
        if player == self.player && self.ready.load(Ordering::SeqCst) {
            return Ok(());
        }
        let (acl, resolved) = acl_for(&player);
        tracing::info!(resolved, "rebinding the app pipe for the configured player");
        self.sd = SecurityDescriptor::from_sddl(&acl.sddl())?;
        self.player = player;
        self.ready.store(resolved, Ordering::SeqCst);
        // Replace the waiting instance; a connected one keeps its old descriptor until it closes.
        self.next = None;
        self.next = Some(self.create(false)?);
        Ok(())
    }

    /// Whether the connected client is the installed Kiosk app.
    fn peer_ok(&self, server: &NamedPipeServer) -> bool {
        let mut pid = 0u32;
        let handle = HANDLE(server.as_raw_handle());
        // SAFETY: `handle` is the connected pipe instance.
        if let Err(e) = unsafe { GetNamedPipeClientProcessId(handle, &mut pid) } {
            tracing::warn!(error = %e, "app pipe: no client process id");
            return false;
        }
        match process_image(pid) {
            Ok(image) if same_path(&image, &self.app_exe) => true,
            Ok(image) => {
                tracing::warn!(pid, image = %image.display(), "app pipe: refusing a client that is not the kiosk app");
                false
            }
            Err(e) => {
                tracing::warn!(pid, error = %e, "app pipe: cannot read the client image");
                false
            }
        }
    }

    #[cfg(test)]
    fn waiting(&self) -> &NamedPipeServer {
        self.next.as_ref().expect("a waiting instance")
    }
}

impl IpcServer for PipeIpcServer {
    fn ready(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.ready)
    }

    async fn accept(&mut self) -> io::Result<IpcStream> {
        loop {
            self.recheck()?;
            let server = match self.next.take() {
                Some(s) => s,
                None => self.create(false)?,
            };
            let connected = tokio::select! {
                r = server.connect() => Some(r),
                () = tokio::time::sleep(RECHECK) => None,
            };
            match connected {
                None => {
                    self.next = Some(server);
                    continue;
                }
                Some(Err(e)) => return Err(e),
                Some(Ok(())) => {}
            }
            // The next instance exists before this one is handed off, so a client never finds
            // the name missing.
            self.next = Some(self.create(false)?);
            if !self.peer_ok(&server) {
                let _ = server.disconnect();
                continue;
            }
            let (r, w) = tokio::io::split(server);
            return Ok(IpcStream {
                reader: Box::new(r),
                writer: Box::new(w),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::windows::named_pipe::ClientOptions;
    use windows::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SE_KERNEL_OBJECT,
    };
    use windows::Win32::Security::DACL_SECURITY_INFORMATION;

    fn current_user() -> String {
        std::env::var("USERNAME").unwrap()
    }

    fn config_for(dir: &Path, player: &str) -> PathBuf {
        let path = dir.join("config.json");
        crate::config::KioskConfig {
            hub_addr: "127.0.0.1:7400".into(),
            hub_fingerprint: "ab".repeat(32),
            device_id: "D".into(),
            player_user: player.into(),
            app_path: None,
        }
        .save(&path)
        .unwrap();
        path
    }

    fn pipe_name() -> String {
        format!(r"\\.\pipe\arena-kiosk-test-{}", ulid::Ulid::generate())
    }

    fn dacl_of(server: &NamedPipeServer) -> String {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        let h = HANDLE(server.as_raw_handle());
        // SAFETY: test-only reads of a live handle.
        unsafe {
            GetSecurityInfo(
                h,
                SE_KERNEL_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                None,
                None,
                Some(&mut sd),
            )
            .ok()
            .unwrap();
            let mut text = PWSTR::null();
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                sd,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut text,
                None,
            )
            .unwrap();
            let s = text.to_string().unwrap();
            let _ = LocalFree(Some(HLOCAL(text.0.cast())));
            let _ = LocalFree(Some(HLOCAL(sd.0)));
            s
        }
    }

    #[tokio::test]
    async fn the_pipe_dacl_is_system_and_the_player_and_squatting_is_denied() {
        let dir = tempfile::tempdir().unwrap();
        let config = config_for(dir.path(), &current_user());
        let name = pipe_name();
        let exe = std::env::current_exe().unwrap();
        let server = PipeIpcServer::bind(&name, &config, &exe).unwrap();
        assert!(server.ready().load(Ordering::SeqCst));
        let sid = user_sid(&current_user()).unwrap();
        let sddl = dacl_of(server.waiting());
        assert!(sddl.starts_with("D:P"), "{sddl}");
        assert_eq!(sddl.matches("(A;").count(), 2, "{sddl}");
        assert!(sddl.contains(";;;SY)"), "{sddl}");
        assert!(sddl.contains(&format!("(A;;0x12018b;;;{sid})")), "{sddl}");
        // The player (this test's user) may not create another instance.
        let squat = ServerOptions::new()
            .first_pipe_instance(false)
            .create(&name);
        assert_eq!(
            squat.err().map(|e| e.kind()),
            Some(io::ErrorKind::PermissionDenied)
        );
    }

    #[tokio::test]
    async fn two_sequential_clients_connect() {
        let dir = tempfile::tempdir().unwrap();
        let config = config_for(dir.path(), &current_user());
        let name = pipe_name();
        let exe = std::env::current_exe().unwrap();
        let mut server = PipeIpcServer::bind(&name, &config, &exe).unwrap();
        for _ in 0..2 {
            let n = name.clone();
            let client = tokio::spawn(async move {
                loop {
                    match ClientOptions::new().open(&n) {
                        Ok(c) => return c,
                        Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
                    }
                }
            });
            let stream = tokio::time::timeout(Duration::from_secs(5), server.accept())
                .await
                .unwrap()
                .unwrap();
            let c = client.await.unwrap();
            drop(stream);
            drop(c);
        }
    }

    #[tokio::test]
    async fn a_client_that_is_not_the_app_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let config = config_for(dir.path(), &current_user());
        let name = pipe_name();
        let mut server =
            PipeIpcServer::bind(&name, &config, Path::new(r"C:\nope\arena-kiosk-ui.exe")).unwrap();
        let n = name.clone();
        tokio::spawn(async move {
            let _c = ClientOptions::new().open(&n).unwrap();
            tokio::time::sleep(Duration::from_secs(10)).await;
        });
        assert!(
            tokio::time::timeout(Duration::from_secs(1), server.accept())
                .await
                .is_err(),
            "a foreign client is never handed to the supervisor"
        );
    }

    #[test]
    fn an_unknown_player_gives_a_system_only_pipe_that_is_not_ready() {
        let dir = tempfile::tempdir().unwrap();
        let config = config_for(dir.path(), "no-such-player-xyz");
        let server =
            PipeIpcServer::bind(&pipe_name(), &config, &std::env::current_exe().unwrap()).unwrap();
        assert!(!server.ready().load(Ordering::SeqCst));
    }
}
