//! The account menu's frames (Story 6.17): `player.account`, `player.end_session`,
//! `player.unlock`.
//!
//! Like the plan picker's, none names a player or a session, and none needs a login record: the
//! player is already on a session, so the Hub reads the session running on the paired device.
//!
//! - `player.account` answers the profiled player's balance and packs.
//! - `player.end_session` builds the `session.end` (device actor, the session's own `rev`) and
//!   pushes the lock: `PAY_AT_COUNTER` for a bill, `LOGGED_OUT` (back to attract) when the time
//!   was paid for already.
//! - `player.unlock` checks the password typed on the player's own lock screen against the
//!   session's player, under its own throttle: five wrong tries in a row pause the PC's field for
//!   60 s with no hash checked meanwhile, and raise the `login_lockout` alert once.

use std::sync::Arc;

use arena_proto::{
    Actor, ActorKind, AppError, ErrorCode, IntentKind, IntentPayload, LockReason, PcAction,
    PlayerAccountResult, PlayerEndResult, PlayerUnlock, PlayerUnlockResult, SessionEnd,
};
use tokio::time::Instant;

use super::LanHub;
use super::player_login::{Reservation, intent, raise_lockout};

fn app(code: ErrorCode, message: impl Into<String>) -> AppError {
    AppError::new(code, message)
}

fn primary(hub: &LanHub) -> Result<(), AppError> {
    if hub.is_primary() {
        Ok(())
    } else {
        Err(app(
            ErrorCode::NotPrimary,
            "this hub is not the primary writer",
        ))
    }
}

/// The session running on `device`, read on a blocking thread.
async fn device_session(
    hub: &LanHub,
    device: &str,
) -> Result<Option<arena_core::player_account::DeviceSession>, AppError> {
    let dir = Arc::clone(&hub.devices);
    let device = device.to_owned();
    tokio::task::spawn_blocking(move || dir.device_session(&device))
        .await
        .map_err(|e| app(ErrorCode::Internal, format!("session read failed: {e}")))?
}

/// `player.account`: the balance and packs of the profiled player on this PC's session.
pub(super) async fn account(hub: &LanHub, device: &str) -> Result<PlayerAccountResult, AppError> {
    primary(hub)?;
    let player_id = device_session(hub, device)
        .await?
        .and_then(|s| s.player_id)
        .ok_or_else(|| app(ErrorCode::NotFound, "no player on this session"))?;
    let dir = Arc::clone(&hub.devices);
    let (device, now) = (device.to_owned(), hub.clock.now_ms());
    tokio::task::spawn_blocking(move || dir.player_account(&player_id, &device, now))
        .await
        .map_err(|e| app(ErrorCode::Internal, format!("account read failed: {e}")))?
}

/// `player.end_session`: ends the session on this PC for its player and pushes the lock.
pub(super) async fn end_session(hub: &LanHub, device: &str) -> Result<PlayerEndResult, AppError> {
    primary(hub)?;
    let writer = hub
        .writer
        .clone()
        .ok_or_else(|| app(ErrorCode::NotPrimary, "this hub is not the primary writer"))?;
    let Some(session) = device_session(hub, device).await? else {
        return Ok(PlayerEndResult::Refused {
            code: ErrorCode::NotFound,
            message: "no session on this PC".to_owned(),
        });
    };
    let mut end = intent(
        hub.clock.now_ms(),
        Actor {
            kind: ActorKind::Device,
            id: device.to_owned(),
        },
        IntentKind::SessionEnd,
        IntentPayload::SessionEnd(SessionEnd {
            session_id: session.session_id.clone(),
        }),
    );
    end.base_rev = Some(session.rev);
    let cmd_id = end.idempotency_key.clone();
    match writer.submit_intent(end).await {
        Ok(_) => {
            let reason = if session.prepaid {
                LockReason::LoggedOut
            } else {
                LockReason::PayAtCounter
            };
            let sent =
                hub.presence()
                    .push_pc_command(device, &cmd_id, PcAction::Lock, None, Some(reason));
            tracing::info!(device_id = %device, session_id = %session.session_id, sessions = sent, "player ended their own session");
            Ok(PlayerEndResult::Ok)
        }
        Err(e) if e.code == ErrorCode::NotPrimary => Err(e),
        Err(e) => {
            tracing::info!(device_id = %device, code = e.code.as_str(), "player end refused");
            Ok(PlayerEndResult::Refused {
                code: e.code,
                message: e.message,
            })
        }
    }
}

/// `player.unlock`: checks the password typed on the player's own lock screen.
pub(super) async fn unlock(
    hub: &LanHub,
    device: &str,
    request: PlayerUnlock,
) -> Result<PlayerUnlockResult, AppError> {
    primary(hub)?;
    // Reserved before any check, so parallel guesses count toward the limit.
    if let Err(retry_s) = hub.unlock_throttle.begin(device, Instant::now()) {
        return Ok(PlayerUnlockResult::Locked { retry_s });
    }
    let reservation = Reservation {
        throttle: &hub.unlock_throttle,
        device,
        live: true,
    };
    let Some(player_id) = device_session(hub, device).await?.and_then(|s| s.player_id) else {
        // A guest has no password and no lock screen; nothing is counted.
        return Ok(PlayerUnlockResult::Rejected);
    };
    let _permit = Arc::clone(&hub.verify_permits)
        .acquire_owned()
        .await
        .map_err(|_| app(ErrorCode::Internal, "password checks are shut down"))?;
    let dir = Arc::clone(&hub.devices);
    let matched = tokio::task::spawn_blocking(move || {
        let hash = dir.player_password_hash(&player_id)?;
        Ok::<_, AppError>(arena_core::players::check_password(
            &request.password,
            hash.as_deref(),
        ))
    })
    .await
    .map_err(|e| app(ErrorCode::Internal, format!("password check failed: {e}")))??;
    if matched {
        reservation.finish(true, Instant::now());
        tracing::info!(device_id = %device, "player unlocked their screen");
        return Ok(PlayerUnlockResult::Ok);
    }
    let paused = reservation.finish(false, Instant::now());
    tracing::info!(device_id = %device, paused = paused.is_some(), "player unlock refused");
    Ok(match paused {
        Some(retry_s) => {
            raise_lockout(hub, device).await;
            PlayerUnlockResult::Locked { retry_s }
        }
        None => PlayerUnlockResult::Rejected,
    })
}
