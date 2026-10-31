//! The Hub screen's handle on "Restore from cloud backup" (Story 9.12): the loopback routes call
//! it, `arena-tasks` implements it (the download runs there).

use arena_proto::{RestoreFetchView, RestoreRebuildView, UiError};

pub trait RestoreFetchControl: Send + Sync {
    /// Where the download stands; `None` before one starts, and after Cancel.
    fn view(&self) -> Option<RestoreFetchView>;
    /// Starts the download, or resumes it from the files already staged. Refused unless the Hub
    /// waits in "Restore this branch" with the branch key released, on a paid tier.
    fn start(&self) -> Result<(), UiError>;
    /// Stops the download and removes the staging folder. The Hub stays in "Restore this branch".
    fn cancel(&self);
    /// After `RESTORE_GAP`: keeps what is staged before the bad range and drops the rest.
    fn restore_partial(&self) -> Result<(), UiError>;
}

/// The Hub screen's handle on "Rebuild this branch" from the downloaded backup (Story 9.13): the
/// loopback routes call it, `arena-hub` implements it (the replay runs there, on a blocking task).
pub trait RestoreRebuildControl: Send + Sync {
    /// Where the rebuild stands; `None` before one starts, and after Cancel.
    fn view(&self) -> Option<RestoreRebuildView>;
    /// Starts the rebuild from the staged backup. Refused unless the download is complete
    /// (`ready`) on a Windows Hub that waits in "Restore this branch" with the branch key released.
    fn start(&self) -> Result<(), UiError>;
    /// Stops the rebuild before the Hub restarts and removes the rebuilt file. The staged backup
    /// is kept and the Hub stays in "Restore this branch".
    fn cancel(&self);
}

/// What the cloud key backup says about a branch (Story 9.27), asked before an export is restored
/// on a paid Hub: `arena-hub` implements it (the fetch runs in `arena-tasks`, the key opens it).
pub trait KeyBackupLookup: Send + Sync {
    /// The ids of the players whose key the backup holds. `None` when the Hub is not paid, the
    /// cloud cannot be reached, there is no backup, or it does not open with the branch key: the
    /// restore then changes nothing about who is erased. Never waits long.
    fn live_keys(&self) -> KeyLookupFuture<'_>;
}

pub type KeyLookupFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Option<Vec<String>>> + Send + 'a>>;
