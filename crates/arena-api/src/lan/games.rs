//! The kiosk's `games.most_played` frame (Story 6.27): the branch's top games over the last 7
//! days and the live player's own last-played times.
//!
//! It names no player and no session in the request: the Hub reads the session running on the
//! paired device. It is a read, so a follower answers it from its replicated database as the
//! primary does; the top list is recomputed at most every 15 minutes per Hub.

use std::sync::Arc;

use arena_proto::{AppError, ErrorCode, MostPlayed};

use super::LanHub;

/// `games.most_played` for the PC `device`.
pub(super) async fn most_played(hub: &LanHub, device: &str) -> Result<MostPlayed, AppError> {
    let dir = Arc::clone(&hub.devices);
    let (device, now) = (device.to_owned(), hub.clock.now_ms());
    tokio::task::spawn_blocking(move || dir.most_played(&device, now))
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, format!("most played read failed: {e}")))?
}
