//! The plan picker (Story 6.9): the app's `player_plans`, `player_start` and `player_logout` are
//! forwarded to the Hub as the paired-only frames `player.plans`, `player.start_session` and
//! `player.logout`, and the Hub's typed answer goes back as the `reply` body.
//!
//! The service decides nothing and keeps no player context: the Hub remembers who logged in on this
//! PC and builds the session itself, so the app never names a player. Nothing is queued: with no
//! live connection (or no answer in time) the app gets `HUB_SLOW` and shows "Counter is
//! reconnecting". A Hub that no longer holds the login answers `AUTH_FAILED` with the message
//! `player login required`; it passes through and the app returns to attract.

use std::sync::Arc;

use arena_kiosk_core::{ClientError, HubClient};
use arena_proto::{AppError, ErrorCode, FrameBody, PlayerChoice};
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
