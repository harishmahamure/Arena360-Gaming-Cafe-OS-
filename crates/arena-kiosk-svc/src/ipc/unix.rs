//! Non-Windows dev and test transport: a Unix socket with mode 0600 inside a 0700 directory
//! (owner only: the directory closes the window between `bind` and `chmod`).

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use tokio::net::UnixListener;

use super::{IpcServer, IpcStream};

/// The socket listener. Dropping it removes the socket file.
pub struct UnixIpcServer {
    listener: UnixListener,
    path: PathBuf,
}

impl UnixIpcServer {
    /// Binds `path` (a stale socket file from an earlier run is replaced) and restricts it to
    /// the owner.
    pub fn bind(path: &Path) -> io::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let listener = UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Self {
            listener,
            path: path.to_owned(),
        })
    }
}

impl Drop for UnixIpcServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl IpcServer for UnixIpcServer {
    async fn accept(&mut self) -> io::Result<IpcStream> {
        let (stream, _) = self.listener.accept().await?;
        let (r, w) = stream.into_split();
        Ok(IpcStream {
            reader: Box::new(r),
            writer: Box::new(w),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_socket_is_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = arena_kiosk_core::ipc::socket_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"stale").unwrap();
        let server = UnixIpcServer::bind(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let dir_mode = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(dir_mode & 0o777, 0o700);
        drop(server);
        assert!(!path.exists());
    }
}
