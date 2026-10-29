//! The plan picker's frames (Story 6.9): `player.plans`, `player.start_session`, `player.logout`.
//!
//! All three are paired-only and none names a player: the Hub remembers who logged in on a device
//! (`LoginRecords`, 120 s from the last use, for that connection only) and answers for that
//! player. `player.start_session` builds the `session.start` intent itself, with the paired device
//! as actor, the logged-in player on the session and `pay_from_wallet` set for a block, then
//! pushes `session.started` to the PC exactly as the staff path does. A kiosk cannot send
//! `session.start` (it is outside the paired scope), so it can never name another player.
//!
//! No record, an expired one or one of a closed connection answers an `error` frame whose message
//! is [`PLAYER_LOGIN_REQUIRED`]: the kiosk takes it as "log in again".

use std::sync::Arc;

use arena_proto::{
    Actor, ActorKind, AppError, ErrorCode, IntentKind, IntentPayload, PLAYER_LOGIN_REQUIRED,
    PlayerChoice, PlayerPlansResult, PlayerStartResult, SessionMode, SessionStart,
};
use tokio::time::Instant;

use super::LanHub;
use super::player_login::intent;

fn app(code: ErrorCode, message: impl Into<String>) -> AppError {
    AppError::new(code, message)
}

fn login_required() -> AppError {
    app(ErrorCode::AuthFailed, PLAYER_LOGIN_REQUIRED)
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

/// The player logged in on `device` through `conn`, with the record refreshed.
fn logged_in(hub: &LanHub, device: &str, conn: u64) -> Result<String, AppError> {
    hub.player_logins
        .touch(device, conn, Instant::now())
        .ok_or_else(login_required)
}

/// `player.plans`: the options valid for this PC's group.
pub(super) async fn plans(
    hub: &LanHub,
    device: &str,
    conn: u64,
) -> Result<PlayerPlansResult, AppError> {
    primary(hub)?;
    let player_id = logged_in(hub, device, conn)?;
    let dir = Arc::clone(&hub.devices);
    let (device, now) = (device.to_owned(), hub.clock.now_ms());
    tokio::task::spawn_blocking(move || dir.player_options(&player_id, &device, now))
        .await
        .map_err(|e| app(ErrorCode::Internal, format!("plans read failed: {e}")))?
}

/// The `session.start` a choice becomes for `player_id` on `device`.
fn start_of(device: &str, player_id: &str, choice: PlayerChoice) -> SessionStart {
    let (mode, pack_id, plan_id) = match choice {
        PlayerChoice::Pack { pack_id } => (SessionMode::Pack, Some(pack_id), None),
        PlayerChoice::Pass { pack_id } => (SessionMode::Pass, Some(pack_id), None),
        PlayerChoice::Wallet => (SessionMode::Wallet, None, None),
        PlayerChoice::Block { plan_id } => (SessionMode::Block, None, Some(plan_id)),
    };
    SessionStart {
        device_id: device.to_owned(),
        mode,
        player_id: Some(player_id.to_owned()),
        pack_id,
        plan_id,
        group_with: None,
        pay_from_wallet: mode == SessionMode::Block,
    }
}

/// `player.start_session`: starts the choice for the logged-in player on this PC.
pub(super) async fn start_session(
    hub: &LanHub,
    device: &str,
    conn: u64,
    choice: PlayerChoice,
) -> Result<PlayerStartResult, AppError> {
    primary(hub)?;
    let player_id = logged_in(hub, device, conn)?;
    let writer = hub
        .writer
        .clone()
        .ok_or_else(|| app(ErrorCode::NotPrimary, "this hub is not the primary writer"))?;
    let start = intent(
        hub.clock.now_ms(),
        Actor {
            kind: ActorKind::Device,
            id: device.to_owned(),
        },
        IntentKind::SessionStart,
        IntentPayload::SessionStart(start_of(device, &player_id, choice)),
    );
    let cmd_id = start.idempotency_key.clone();
    match writer.submit_intent(start).await {
        Ok(_) => {
            // The player is on a session now; the Hub forgets the login.
            hub.player_logins.clear(device, conn);
            let reader = Arc::clone(&hub.devices);
            super::push_session_started_after(
                &hub.presence(),
                device,
                &cmd_id,
                hub.clock.now_ms(),
                move |now| reader.session_screens(now),
            )
            .await;
            tracing::info!(device_id = %device, player_id = %player_id, "player started a session");
            Ok(PlayerStartResult::Ok)
        }
        Err(e) if e.code == ErrorCode::NotPrimary => Err(e),
        Err(e) => {
            tracing::info!(device_id = %device, player_id = %player_id, code = e.code.as_str(), "player start refused");
            Ok(PlayerStartResult::Refused {
                code: e.code,
                message: e.message,
            })
        }
    }
}

/// `player.logout`: forgets the login on this device (a no-op when there is none).
pub(super) fn logout(hub: &LanHub, device: &str, conn: u64) {
    hub.player_logins.clear(device, conn);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_block_pays_from_the_wallet() {
        let choices = [
            PlayerChoice::Pack {
                pack_id: "K".into(),
            },
            PlayerChoice::Pass {
                pack_id: "K".into(),
            },
            PlayerChoice::Wallet,
            PlayerChoice::Block {
                plan_id: "B".into(),
            },
        ];
        for choice in choices {
            let s = start_of("D1", "P1", choice);
            assert_eq!(s.device_id, "D1");
            assert_eq!(s.player_id.as_deref(), Some("P1"));
            assert_eq!(s.pay_from_wallet, s.mode == SessionMode::Block);
            assert_ne!(s.mode, SessionMode::Postpaid);
            assert!(s.group_with.is_none());
        }
    }
}
