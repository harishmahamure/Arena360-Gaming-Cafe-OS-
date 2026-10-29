//! The kitchen board a `kds` session reads (Story 7.7): the Hub's tickets, read on a blocking
//! task, with the Hub's clock so a display's elapsed times never use the tablet's. A display asks
//! for it with `kitchen.board` and is pushed it as `event{kind:"kitchen.board"}` after any change
//! to a ticket, so it never has to merge changes.

use std::sync::Arc;

use arena_proto::{AppError, ErrorCode, KitchenBoard};

use super::LanHub;

/// Reads the board at the Hub's clock.
pub(crate) async fn read(hub: &LanHub) -> Result<KitchenBoard, AppError> {
    let dir = Arc::clone(&hub.devices);
    let now = hub.clock.now_ms();
    tokio::task::spawn_blocking(move || dir.kitchen_board(now))
        .await
        .map_err(|e| {
            AppError::new(
                ErrorCode::Internal,
                format!("kitchen board read failed: {e}"),
            )
        })?
}
