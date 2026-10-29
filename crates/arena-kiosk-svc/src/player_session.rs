//! The plan picker (Story 6.9): the app's `player_plans`, `player_start` and `player_logout` are
//! forwarded to the Hub as the paired-only frames `player.plans`, `player.start_session` and
//! `player.logout`, and the Hub's typed answer goes back as the `reply` body.
//!
//! The account menu (Story 6.17) adds `player_account`, `player_end` (Log out) and
//! `player_unlock`, forwarded as `player.account`, `player.end_session` and `player.unlock`, and
//! `player_lock`, which the service does itself: the player's own lock covers the screen with the
//! reason `LockedByPlayer`, the session and its clock go on, and only the Hub's `ok` for the
//! player's password (or staff Unlock) lifts it.
//!
//! Extend (Story 6.21) adds `player_extend_options` and `player_extend`, forwarded as
//! `player.extend_options` and `player.extend`. The service only checks that the extend is one of
//! the offered steps before it leaves the PC; the Hub quotes, charges and refuses.
//!
//! The service decides nothing and keeps no player context: the Hub remembers who logged in on this
//! PC and builds the session itself, so the app never names a player. Nothing is queued: with no
//! live connection (or no answer in time) the app gets `HUB_SLOW` and shows "Counter is
//! reconnecting". A Hub that no longer holds the login answers `AUTH_FAILED` with the message
//! `player login required`; it passes through and the app returns to attract.

use std::sync::Arc;

use arena_kiosk_core::ipc::SecretPin;
use arena_kiosk_core::{ClientError, HubClient};
use arena_proto::{
    AppError, ErrorCode, FrameBody, LockReason, PlayerChoice, PlayerExtend, PlayerUnlockResult,
};
use serde_json::Value;

use crate::runtime::Ctx;

fn unreachable() -> AppError {
    AppError::new(ErrorCode::HubSlow, "the hub is not reachable")
}

fn client(ctx: &Ctx) -> Result<Arc<HubClient>, AppError> {
    let client: Option<Arc<HubClient>> = ctx
        .admin
        .hub
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    client.filter(|c| !c.is_closed()).ok_or_else(unreachable)
}

fn failed(kind: &str, e: ClientError) -> AppError {
    match e {
        // The Hub's own refusal (not primary, login required, a bad frame) goes through.
        ClientError::Hub(e) => {
            tracing::info!(code = e.code.as_str(), "the hub refused {kind}");
            e
        }
        e => {
            tracing::info!(error = %e, "{kind} did not reach the hub");
            unreachable()
        }
    }
}

fn unexpected(kind: &str, other: FrameBody) -> AppError {
    tracing::warn!(kind = other.kind(), "unexpected reply to {kind}");
    AppError::new(ErrorCode::Internal, "the hub answered something else")
}

/// Asks the Hub for the options valid for this PC: the `player.plans_result` as the reply body.
pub async fn plans(ctx: &Ctx) -> Result<Value, AppError> {
    let client = client(ctx)?;
    match client.request("player.plans", Value::Null).await {
        Ok(frame) => match frame.body {
            FrameBody::PlayerPlansResult(r) => serde_json::to_value(r)
                .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string())),
            other => Err(unexpected("player.plans", other)),
        },
        Err(e) => Err(failed("player.plans", e)),
    }
}

/// Asks the Hub to start `choice` for the player logged in on this PC: the `player.start_result`
/// as the reply body.
pub async fn start(ctx: &Ctx, choice: PlayerChoice) -> Result<Value, AppError> {
    let client = client(ctx)?;
    let body = serde_json::json!({ "choice": choice });
    match client.request("player.start_session", body).await {
        Ok(frame) => match frame.body {
            FrameBody::PlayerStartResult(r) => serde_json::to_value(r)
                .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string())),
            other => Err(unexpected("player.start_session", other)),
        },
        Err(e) => Err(failed("player.start_session", e)),
    }
}

/// Tells the Hub to drop the login on this PC. Best effort: the Hub also forgets it after 120 s.
pub async fn logout(ctx: &Ctx) -> Result<Value, AppError> {
    let client = client(ctx)?;
    match client.request("player.logout", Value::Null).await {
        Ok(_) => Ok(Value::Null),
        Err(e) => Err(failed("player.logout", e)),
    }
}

/// Asks the Hub what the player on this PC's session holds (Story 6.17): the
/// `player.account_result` as the reply body.
pub async fn account(ctx: &Ctx) -> Result<Value, AppError> {
    let client = client(ctx)?;
    match client.request("player.account", Value::Null).await {
        Ok(frame) => match frame.body {
            FrameBody::PlayerAccountResult(r) => serde_json::to_value(r)
                .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string())),
            other => Err(unexpected("player.account", other)),
        },
        Err(e) => Err(failed("player.account", e)),
    }
}

/// Asks the Hub to end the player's own session (Story 6.17, Log out): the
/// `player.end_result` as the reply body. The lock follows from the Hub as a PC command.
pub async fn end(ctx: &Ctx) -> Result<Value, AppError> {
    let client = client(ctx)?;
    match client.request("player.end_session", Value::Null).await {
        Ok(frame) => match frame.body {
            FrameBody::PlayerEndResult(r) => serde_json::to_value(r)
                .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string())),
            other => Err(unexpected("player.end_session", other)),
        },
        Err(e) => Err(failed("player.end_session", e)),
    }
}

/// Asks the Hub what the player on this PC can add and what it costs (Story 6.21): the
/// `player.extend_options_result` as the reply body. Nothing is queued: with no live link the app
/// gets `HUB_SLOW` and disables every option.
pub async fn extend_options(ctx: &Ctx) -> Result<Value, AppError> {
    let client = client(ctx)?;
    match client.request("player.extend_options", Value::Null).await {
        Ok(frame) => match frame.body {
            FrameBody::PlayerExtendOptionsResult(r) => serde_json::to_value(r)
                .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string())),
            other => Err(unexpected("player.extend_options", other)),
        },
        Err(e) => Err(failed("player.extend_options", e)),
    }
}

/// The extend the app asked for, as long as it is one the Hub offers (a step of 30 min, 1 h or
/// 2 h, a pack named only for a pack source). Anything else never leaves the PC.
fn extend_request(body: &Value) -> Result<PlayerExtend, AppError> {
    let request: PlayerExtend = serde_json::from_value(body.clone())
        .map_err(|_| AppError::new(ErrorCode::ValidationFailed, "malformed extend request"))?;
    request
        .validate()
        .map_err(|e| AppError::new(ErrorCode::ValidationFailed, e.message))?;
    Ok(request)
}

/// Asks the Hub to add time to the player's session (Story 6.21): the `player.extend_result` as
/// the reply body. Nothing is queued and nothing is retried; the Hub decides, and a refusal comes
/// back typed for the panel to show.
pub async fn extend(ctx: &Ctx, body: &Value) -> Result<Value, AppError> {
    let request = extend_request(body)?;
    let client = client(ctx)?;
    let body = serde_json::to_value(&request)
        .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string()))?;
    match client.request("player.extend", body).await {
        Ok(frame) => match frame.body {
            FrameBody::PlayerExtendResult(r) => serde_json::to_value(r)
                .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string())),
            other => Err(unexpected("player.extend", other)),
        },
        Err(e) => Err(failed("player.extend", e)),
    }
}

/// Locks the screen for the player (Story 6.17). Only a profiled player's session on screen can
/// be locked: a guest has no password to unlock with. Time keeps running; nothing is sent.
pub fn lock(ctx: &Ctx) -> Result<Value, AppError> {
    let profiled = {
        let screen = ctx.pc.screen.borrow();
        !screen.locked
            && screen
                .session
                .as_ref()
                .is_some_and(|s| s.username.is_some())
    };
    if !profiled {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "only a signed-in player's session can be locked",
        ));
    }
    crate::pc::set_locked(ctx, true, Some(LockReason::LockedByPlayer));
    Ok(Value::Null)
}

/// Sends the password typed on the player's own lock screen to the Hub (Story 6.17) and lifts
/// the lock when the Hub says it is right: the `player.unlock_result` as the reply body. Nothing
/// is queued, and a screen locked by anyone else is not opened by a password.
pub async fn unlock(ctx: &Ctx, mut password: SecretPin) -> Result<Value, AppError> {
    let mine = {
        let screen = ctx.pc.screen.borrow();
        screen.locked && screen.reason == Some(LockReason::LockedByPlayer)
    };
    if !mine {
        password.clear();
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "this screen is not locked by the player",
        ));
    }
    let client = match client(ctx) {
        Ok(c) => c,
        Err(e) => {
            password.clear();
            return Err(e);
        }
    };
    let body = serde_json::json!({ "password": password.expose() });
    password.clear();
    match client.request("player.unlock", body).await {
        Ok(frame) => match frame.body {
            FrameBody::PlayerUnlockResult(r) => {
                if r == PlayerUnlockResult::Ok {
                    crate::pc::set_locked(ctx, false, None);
                }
                serde_json::to_value(r)
                    .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string()))
            }
            other => Err(unexpected("player.unlock", other)),
        },
        Err(e) => Err(failed("player.unlock", e)),
    }
}

/// Tells the Hub the player at this PC wants staff (Story 6.15): `device.call_staff`. Nothing is
/// queued; with no live connection the app gets `HUB_SLOW` and tells the player the counter is
/// reconnecting.
pub async fn call_staff(ctx: &Ctx) -> Result<Value, AppError> {
    let client = client(ctx)?;
    match client.request("device.call_staff", Value::Null).await {
        Ok(_) => Ok(Value::Null),
        Err(e) => Err(failed("device.call_staff", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_extend_that_is_not_an_offered_step_never_leaves_the_pc() {
        use serde_json::json;
        let ok = extend_request(&json!({"minutes": 60, "source": "wallet"})).unwrap();
        assert_eq!(ok.minutes, 60);
        let pack = json!({"minutes": 30, "source": "pack", "pack_id": "K1"});
        assert_eq!(
            extend_request(&pack).unwrap().pack_id.as_deref(),
            Some("K1")
        );
        for bad in [
            json!({"minutes": 45, "source": "wallet"}),
            json!({"minutes": 30, "source": "pack"}),
            json!({"minutes": 30, "source": "wallet", "pack_id": "K1"}),
            json!({"minutes": 30, "source": "cash"}),
            json!({"source": "wallet"}),
            Value::Null,
        ] {
            assert_eq!(
                extend_request(&bad).unwrap_err().code,
                ErrorCode::ValidationFailed,
                "{bad}"
            );
        }
    }

    #[test]
    fn an_unreachable_hub_is_hub_slow() {
        assert_eq!(unreachable().code, ErrorCode::HubSlow);
    }

    #[test]
    fn a_hub_refusal_passes_through_and_a_dead_link_is_hub_slow() {
        let refusal = AppError::new(ErrorCode::AuthFailed, arena_proto::PLAYER_LOGIN_REQUIRED);
        let e = failed("player.plans", ClientError::Hub(refusal.clone()));
        assert_eq!(e, refusal);
        assert_eq!(
            failed("player.plans", ClientError::Closed).code,
            ErrorCode::HubSlow
        );
        assert_eq!(
            failed("player.plans", ClientError::HubSlow).code,
            ErrorCode::HubSlow
        );
    }
}
