//! The volume slider (Story 6.15): the app's `volume` request reads the level the service last set
//! and `volume{percent}` sets the system volume through the cleanup host (Windows: the helper that
//! runs in the player's session, since the service itself is LocalSystem).
//!
//! A new session starts at the branch default whatever the previous player left
//! ([`reset_for_session`]); the session-end cleanup (Story 6.11) resets it too. The level is the
//! one the service last set, not a reading of the mixer: a player who changes the volume with a
//! keyboard key moves the mixer but not this value, and the slider shows the last value it set.

use std::sync::{Arc, Mutex};

use arena_proto::{AppError, ErrorCode};
use serde::Deserialize;
use serde_json::Value;

use crate::cleanup::{CleanupHost, DEFAULT_VOLUME};
use crate::runtime::Ctx;

/// What the service remembers about the volume.
#[derive(Default)]
pub struct VolumeState {
    host: Mutex<Option<Arc<dyn CleanupHost>>>,
    level: Mutex<Option<u8>>,
}

impl VolumeState {
    /// Gives the volume the host the cleanup runs on.
    pub(crate) fn attach(&self, host: Arc<dyn CleanupHost>) {
        *self.host.lock().unwrap_or_else(|p| p.into_inner()) = Some(host);
    }

    fn host(&self) -> Option<Arc<dyn CleanupHost>> {
        self.host.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn remember(&self, percent: u8) {
        *self.level.lock().unwrap_or_else(|p| p.into_inner()) = Some(percent);
    }
}

#[derive(Deserialize)]
struct SetBody {
    percent: u8,
}

/// The branch default volume (the Story 2.26 setting), or [`DEFAULT_VOLUME`] when it never synced.
pub fn branch_default(ctx: &Ctx) -> u8 {
    crate::operating::load(&ctx.queue.lock().unwrap_or_else(|p| p.into_inner()))
        .volume_default
        .unwrap_or(DEFAULT_VOLUME)
}

/// The level the slider shows: the last one set, else the branch default.
pub fn level(ctx: &Ctx) -> u8 {
    let last = *ctx
        .cleanup
        .volume
        .level
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    last.unwrap_or_else(|| branch_default(ctx))
}

/// Sets the system volume to `percent` and remembers it.
pub async fn set(ctx: &Ctx, percent: u8) -> Result<u8, AppError> {
    if percent > 100 {
        return Err(AppError::with_field(
            ErrorCode::ValidationFailed,
            "percent",
            "the volume is 0 to 100",
        ));
    }
    let host =
        ctx.cleanup.volume.host().ok_or_else(|| {
            AppError::new(ErrorCode::Unsupported, "this PC has no volume control")
        })?;
    tokio::task::spawn_blocking(move || host.set_volume(percent))
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string()))?
        .map_err(|e| {
            tracing::warn!(error = %e, "the volume could not be set");
            AppError::new(ErrorCode::Internal, "the volume could not be set")
        })?;
    ctx.cleanup.volume.remember(percent);
    Ok(percent)
}

/// A session started: the volume goes back to the branch default, whatever the last player set.
/// Best effort and off the caller's thread; a PC with no volume control is left alone.
pub fn reset_for_session(ctx: &Arc<Ctx>) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let ctx = Arc::clone(ctx);
    runtime.spawn(async move {
        let default = branch_default(&ctx);
        if ctx.cleanup.volume.host().is_some() && set(&ctx, default).await.is_err() {
            tracing::warn!("the volume was not reset to the branch default at session start");
        }
    });
}

/// The `volume` request: with `{percent}` it sets the volume, with nothing it reads the level.
/// The answer is `{percent}` either way.
pub async fn request(ctx: &Ctx, body: Value) -> Result<Value, AppError> {
    let percent = if body.is_null() || body.as_object().is_some_and(|o| o.is_empty()) {
        level(ctx)
    } else {
        let b: SetBody = serde_json::from_value(body)
            .map_err(|e| AppError::new(ErrorCode::BadFrame, format!("volume body: {e}")))?;
        set(ctx, b.percent).await?
    };
    Ok(serde_json::json!({ "percent": percent }))
}
