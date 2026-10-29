//! `device.call_staff` (Story 6.15): a player at a paired kiosk pressed "Call staff".
//!
//! The Hub raises a `call_staff` alert for that device (system actor, so the tile shows `call` and
//! the Hub sounds within a snapshot push) and answers `ack`. A call already open for the PC is as
//! good as a new one: the raise is `STALE`, nothing more is written, and the answer is still `ack`
//! so the kiosk says "Staff notified." again. A Hub that is not primary, or has no writer, answers
//! an `error` frame and the kiosk tells the player the counter is reconnecting.

use arena_proto::{
    Actor, ActorKind, AlertKind, AlertRaise, AppError, ErrorCode, IntentKind, IntentPayload,
};

use super::LanHub;
use super::player_login::intent;
use super::session::LAN_SYSTEM_ACTOR;

/// Raises the `call_staff` alert for `device`; one that is already open stays as it is.
pub(super) async fn call_staff(hub: &LanHub, device: &str) -> Result<(), AppError> {
    if !hub.is_primary() {
        return Err(AppError::new(
            ErrorCode::NotPrimary,
            "this hub is not the primary writer",
        ));
    }
    let writer = hub.writer.clone().ok_or_else(|| {
        AppError::new(ErrorCode::NotPrimary, "this hub is not the primary writer")
    })?;
    let raise = intent(
        hub.clock.now_ms(),
        Actor {
            kind: ActorKind::System,
            id: LAN_SYSTEM_ACTOR.to_owned(),
        },
        IntentKind::AlertRaise,
        IntentPayload::AlertRaise(AlertRaise::new(
            AlertKind::CallStaff,
            Some(device.to_owned()),
        )),
    );
    match writer.submit_intent(raise).await {
        Ok(_) => {
            tracing::info!(device_id = %device, "a player called staff");
            Ok(())
        }
        Err(e) if e.code == ErrorCode::Stale => Ok(()),
        Err(e) => {
            tracing::warn!(device_id = %device, code = e.code.as_str(), "the staff call was not raised");
            Err(e)
        }
    }
}
