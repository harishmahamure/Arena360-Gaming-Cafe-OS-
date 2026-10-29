//! Player sign-up (Story 6.10): the app's `player_signup` is forwarded to the Hub as the
//! paired-only frame `player.signup`, and the Hub's typed `player.signup_result` goes back to the
//! app as the `reply` body (`{outcome, ...}`).
//!
//! The service decides nothing: the Hub validates, hashes, seals, creates the player as this
//! device and records the login. Nothing is queued: a sign-up needs the Hub now, so with no live
//! connection (or no answer in time) the app gets `HUB_SLOW` and shows "Counter is reconnecting".
//! The password is cleared once it is sent; nothing the player typed is logged, and the service
//! keeps no player context.

use std::sync::Arc;

use arena_kiosk_core::ipc::SignupFields;
use arena_kiosk_core::{ClientError, HubClient};
use arena_proto::{AppError, ErrorCode, FrameBody};
use serde_json::Value;

use crate::runtime::Ctx;

fn unreachable() -> AppError {
    AppError::new(ErrorCode::HubSlow, "the hub is not reachable")
}

/// Asks the Hub to create the player: the `player.signup_result` as the reply body.
pub async fn request(ctx: &Ctx, mut form: SignupFields) -> Result<Value, AppError> {
    let client: Option<Arc<HubClient>> = ctx
        .admin
        .hub
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    let Some(client) = client.filter(|c| !c.is_closed()) else {
        form.password.clear();
        return Err(unreachable());
    };
    let body = serde_json::json!({
        "username": form.username,
        "password": form.password.expose(),
        "phone": form.phone,
        "name": form.name,
        "consent": form.consent,
    });
    form.password.clear();
    match client.request("player.signup", body).await {
        Ok(frame) => match frame.body {
            FrameBody::PlayerSignupResult(result) => serde_json::to_value(result)
                .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string())),
            other => {
                tracing::warn!(kind = other.kind(), "unexpected reply to player.signup");
                Err(AppError::new(
                    ErrorCode::Internal,
                    "the hub answered something else",
                ))
            }
        },
        // The Hub's own refusal (not primary, no player directory, a bad frame) goes through.
        Err(ClientError::Hub(e)) => {
            tracing::info!(code = e.code.as_str(), "the hub refused player.signup");
            Err(e)
        }
        Err(e) => {
            tracing::info!(error = %e, "player.signup did not reach the hub");
            Err(unreachable())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unreachable_hub_is_hub_slow_and_says_nothing_of_the_form() {
        let e = unreachable();
        assert_eq!(e.code, ErrorCode::HubSlow);
        assert!(!e.message.contains("pass"));
    }
}
