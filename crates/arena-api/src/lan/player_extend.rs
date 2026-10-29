//! The kiosk's extend frames (Story 6.21): `player.extend_options` and `player.extend`.
//!
//! Like the account menu's, neither names a player or a session and neither needs a login record:
//! the player is on a session, so the Hub reads the session running on the paired device.
//!
//! - `player.extend_options` quotes +30 min, +1 h and +2 h at the live rate and lists the ways the
//!   player can pay (wallet, packs valid for the group, credit headroom).
//! - `player.extend` builds the `session.extend` intent itself (device actor, no credit
//!   override), so the Story 3.28 rules apply as they do at the counter; a refusal comes back
//!   typed (`PAST_CLOSING`, `CREDIT_LIMIT`, `INSUFFICIENT_BALANCE`, ...). On success the PC gets
//!   the session screen again, with its new end, the way a staff extend does.

use std::sync::Arc;

use arena_proto::{
    Actor, ActorKind, AppError, ErrorCode, IntentKind, IntentPayload, PlayerExtend,
    PlayerExtendOptions, PlayerExtendResult, SessionExtend,
};

use super::LanHub;
use super::player_login::intent;

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

/// `player.extend_options`: the quotes and sources for the player on this PC's session.
pub(super) async fn options(hub: &LanHub, device: &str) -> Result<PlayerExtendOptions, AppError> {
    primary(hub)?;
    let dir = Arc::clone(&hub.devices);
    let (device, now) = (device.to_owned(), hub.clock.now_ms());
    tokio::task::spawn_blocking(move || dir.extend_options(&device, now))
        .await
        .map_err(|e| {
            app(
                ErrorCode::Internal,
                format!("extend options read failed: {e}"),
            )
        })?
}

/// The `session.extend` a request becomes for `session_id`: never a credit override.
fn extend_of(session_id: String, request: &PlayerExtend) -> SessionExtend {
    SessionExtend {
        session_id,
        minutes: request.minutes,
        source: request.source,
        pack_id: request.pack_id.clone(),
        credit_override: None,
    }
}

/// `player.extend`: adds the time to the session on this PC, paid as the player chose.
pub(super) async fn extend(
    hub: &LanHub,
    device: &str,
    request: PlayerExtend,
) -> Result<PlayerExtendResult, AppError> {
    primary(hub)?;
    let writer = hub
        .writer
        .clone()
        .ok_or_else(|| app(ErrorCode::NotPrimary, "this hub is not the primary writer"))?;
    let dir = Arc::clone(&hub.devices);
    let target = {
        let device = device.to_owned();
        tokio::task::spawn_blocking(move || dir.extend_target(&device))
            .await
            .map_err(|e| app(ErrorCode::Internal, format!("session read failed: {e}")))?
    };
    let session_id = match target {
        Ok(id) => id,
        Err(e) if e.code == ErrorCode::NotPrimary => return Err(e),
        Err(e) => {
            return Ok(PlayerExtendResult::Refused {
                code: e.code,
                message: e.message,
            });
        }
    };
    let extend = intent(
        hub.clock.now_ms(),
        Actor {
            kind: ActorKind::Device,
            id: device.to_owned(),
        },
        IntentKind::SessionExtend,
        IntentPayload::SessionExtend(extend_of(session_id.clone(), &request)),
    );
    let cmd_id = extend.idempotency_key.clone();
    match writer.submit_intent(extend).await {
        Ok(_) => {
            let reader = Arc::clone(&hub.devices);
            super::push_session_started_after(
                &hub.presence(),
                device,
                &cmd_id,
                hub.clock.now_ms(),
                move |now| reader.session_screens(now),
            )
            .await;
            tracing::info!(device_id = %device, session_id = %session_id, minutes = request.minutes, source = request.source.as_str(), "player extended their session");
            Ok(PlayerExtendResult::Ok)
        }
        Err(e) if e.code == ErrorCode::NotPrimary => Err(e),
        Err(e) => {
            tracing::info!(device_id = %device, code = e.code.as_str(), "player extend refused");
            Ok(PlayerExtendResult::Refused {
                code: e.code,
                message: e.message,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_proto::ExtendSource;

    #[test]
    fn an_extend_never_carries_a_credit_override_or_a_session_from_the_kiosk() {
        for source in [ExtendSource::Wallet, ExtendSource::Credit] {
            let e = extend_of(
                "S1".into(),
                &PlayerExtend {
                    minutes: 60,
                    source,
                    pack_id: None,
                },
            );
            assert_eq!(e.session_id, "S1");
            assert_eq!(e.credit_override, None);
            assert_eq!((e.minutes, e.source), (60, source));
        }
        let pack = extend_of(
            "S1".into(),
            &PlayerExtend {
                minutes: 30,
                source: ExtendSource::Pack,
                pack_id: Some("K".into()),
            },
        );
        assert_eq!(pack.pack_id.as_deref(), Some("K"));
    }
}
