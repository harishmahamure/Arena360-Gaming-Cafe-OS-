//! The service end of the app channel: a Windows named pipe ACL'd to SYSTEM and the player
//! account, or (dev and tests) a 0600 Unix socket in the data directory.

pub mod acl;
#[cfg(windows)]
mod pipe;
#[cfg(unix)]
mod unix;

use std::future::Future;
use std::io;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tokio::io::{AsyncRead, AsyncWrite};

#[cfg(windows)]
pub use pipe::PipeIpcServer;
#[cfg(unix)]
pub use unix::UnixIpcServer;

/// One accepted app connection, split into its read and write halves.
pub struct IpcStream {
    pub reader: Box<dyn AsyncRead + Send + Unpin>,
    pub writer: Box<dyn AsyncWrite + Send + Unpin>,
}

/// Accepts app connections.
pub trait IpcServer: Send + 'static {
    /// The next connection.
    fn accept(&mut self) -> impl Future<Output = io::Result<IpcStream>> + Send;

    /// Whether the transport is open to a resolved player account (read by the watchdog: no
    /// misses count while it is not).
    fn ready(&self) -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(true))
    }
}
