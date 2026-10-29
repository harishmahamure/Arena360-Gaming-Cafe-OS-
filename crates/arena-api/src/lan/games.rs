//! The kiosk's `games.most_played` frame (Story 6.27): the branch's top games over the last 7
//! days and the live player's own last-played times.
//!
//! It names no player and no session in the request: the Hub reads the session running on the
//! paired device. It is a read, so a follower answers it from its replicated database as the
//! primary does; the top list is recomputed at most every 15 minutes per Hub. The answer also
//! carries the stored media of the library games for the kiosk's Home hero (Story 6.29).

use std::sync::Arc;

use arena_proto::{AppError, ErrorCode, MostPlayed};

use super::LanHub;

/// `games.most_played` for the PC `device`.
pub(super) async fn most_played(hub: &LanHub, device: &str) -> Result<MostPlayed, AppError> {
    let dir = Arc::clone(&hub.devices);
    let cache = hub.media.clone();
    let (device, now) = (device.to_owned(), hub.clock.now_ms());
    tokio::task::spawn_blocking(move || {
        let mut played = dir.most_played(&device, now)?;
        let lookup = |game: &str| {
            cache
                .as_ref()
                .map(|c| arena_core::attract::GameMediaHashes {
                    video: c.video_sha256(game).ok().flatten(),
                    cover: c.cover_sha256(game).ok().flatten(),
                })
                .unwrap_or_default()
        };
        played.media = dir.game_media(&lookup)?;
        Ok(played)
    })
    .await
    .map_err(|e| AppError::new(ErrorCode::Internal, format!("most played read failed: {e}")))?
}

/// `session.sync` for the PC `device` (Story 6.36): the Hub's record of its session. A read, so
/// a follower answers it from its replicated database as the primary does.
pub(super) async fn session_sync(
    hub: &LanHub,
    device: &str,
) -> Result<arena_proto::SessionSync, AppError> {
    let dir = Arc::clone(&hub.devices);
    let (device, now) = (device.to_owned(), hub.clock.now_ms());
    tokio::task::spawn_blocking(move || dir.session_sync(&device, now))
        .await
        .map_err(|e| {
            AppError::new(
                ErrorCode::Internal,
                format!("session sync read failed: {e}"),
            )
        })?
}
