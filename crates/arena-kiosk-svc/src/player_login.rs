//! Player login (Story 6.8): the app's `player_login{username, password}` is forwarded to the Hub
//! as the paired-only frame `player.authenticate`, and the Hub's typed `player.auth_result` goes
//! back to the app as the `reply` body (`{outcome, ...}`).
//!
//! The service decides nothing: the Hub checks the Argon2id hash, throttles by device, records the
//! login and answers `ok`, `rejected`, `locked` or `busy`. Nothing is queued: a login needs the
//! Hub now, so with no live connection (or no answer in time) the app gets `HUB_SLOW` and shows
//! "Counter is reconnecting". The password is cleared after it is sent and never logged, and the
//! service keeps no player context.

use std::sync::Arc;

use arena_kiosk_core::ipc::SecretPin;
use arena_kiosk_core::{ClientError, HubClient};
use arena_proto::{AppError, ErrorCode, FrameBody};
use serde_json::Value;

use crate::runtime::Ctx;

fn unreachable() -> AppError {
    AppError::new(ErrorCode::HubSlow, "the hub is not reachable")
}

/// Asks the Hub to check `username` and `password`: the outcome as the reply body.
pub async fn request(
    ctx: &Ctx,
    username: String,
    mut password: SecretPin,
) -> Result<Value, AppError> {
    let client: Option<Arc<HubClient>> = ctx
        .admin
        .hub
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    let Some(client) = client.filter(|c| !c.is_closed()) else {
        password.clear();
        return Err(unreachable());
    };
    let body = serde_json::json!({ "username": username, "password": password.expose() });
    password.clear();
    match client.request("player.authenticate", body).await {
        Ok(frame) => match frame.body {
            FrameBody::PlayerAuthResult(result) => serde_json::to_value(result)
                .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string())),
            other => {
                tracing::warn!(
                    kind = other.kind(),
                    "unexpected reply to player.authenticate"
                );
                Err(AppError::new(
                    ErrorCode::Internal,
                    "the hub answered something else",
                ))
            }
        },
        // The Hub's own refusal (not primary, no player directory, a bad frame) goes through.
        Err(ClientError::Hub(e)) => {
            tracing::info!(
                code = e.code.as_str(),
                "the hub refused player.authenticate"
            );
            Err(e)
        }
        Err(e) => {
            tracing::info!(error = %e, "player.authenticate did not reach the hub");
            Err(unreachable())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unreachable_hub_is_hub_slow_and_says_nothing_of_the_login() {
        let e = unreachable();
        assert_eq!(e.code, ErrorCode::HubSlow);
        assert!(!e.message.contains("pass"));
    }
}
