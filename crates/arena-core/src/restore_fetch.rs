//! The Hub screen's handle on "Restore from cloud backup" (Story 9.12): the loopback routes call
//! it, `arena-tasks` implements it (the download runs there).

use arena_proto::{RestoreFetchView, UiError};

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
