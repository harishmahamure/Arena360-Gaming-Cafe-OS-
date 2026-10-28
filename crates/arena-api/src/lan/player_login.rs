//! `player.authenticate` (Story 6.8): a player's username and password typed at a paired kiosk.
//!
//! The Hub answers `player.auth_result{outcome}`:
//!
//! - `ok{player}` for the right password. The correct login is recorded as a `kiosk.log`
//!   (`info`, `kiosk.player_login`, detail `player <player_id>`) through the writer, with the
//!   device as actor, so the change log and the audit viewer show player and device. If the
//!   record cannot be written the login fails with that error rather than go unrecorded.
//! - `rejected` for an unknown username, a deleted player, an imported player with no password
//!   and a wrong password alike; a dummy argon2id check keeps the time alike. Only `$argon2id$`
//!   hashes verify.
//! - `locked{retry_s}` while the device is paused: five wrong tries in a row pause that device for
//!   60 s, with no hash checked meanwhile. The fifth wrong try answers `locked` itself and raises
//!   the `login_lockout` alert (tag `locked-out`) for the device, once while it is open. The
//!   count resets after the pause and on a correct login. The throttle is the admin-PIN one (a
//!   second instance) and counts checks in flight, so parallel guesses cannot beat the limit.
//! - `busy{device_name}` for a correct login by a player with a live session on another PC; it
//!   is not a failure and records nothing.
//!
//! A Hub that is not primary, has no player directory, or is busy answers an `error` frame. The
//! credentials are never logged. The checks run on a blocking thread under the Hub's verify
//! permits.

use std::sync::Arc;

use arena_proto::{
    Actor, ActorKind, AlertKind, AlertRaise, AppError, ErrorCode, Intent, IntentKind,
    IntentPayload, KioskLog, KioskLogLevel, LOGIN_LOCK_SECONDS, PLAYER_LOGIN_CODE,
    PlayerAuthResult, PlayerAuthenticate, PlayerContext, normalize_username,
};
use tokio::time::Instant;

use super::LanHub;
use super::session::LAN_SYSTEM_ACTOR;
use super::throttle::AdminPinThrottle;

/// A reserved throttle check. If the request is dropped before it is finished (the session ended
/// or its task was aborted mid-check) the reservation is given back, so abandoned checks cannot
/// pile up until the device is locked for good.
struct Reservation<'a> {
    throttle: &'a AdminPinThrottle,
    device: &'a str,
    live: bool,
}

impl Reservation<'_> {
    fn finish(mut self, ok: bool, now: Instant) -> Option<u32> {
        self.live = false;
        self.throttle.finish(self.device, ok, now)
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if self.live {
            self.throttle.cancel(self.device);
        }
    }
}

fn app(code: ErrorCode, message: impl Into<String>) -> AppError {
    AppError::new(code, message)
}

fn intent(now_ms: i64, actor: Actor, kind: IntentKind, payload: IntentPayload) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: now_ms,
        base_rev: None,
        actor,
        elevated_by: None,
        reason: None,
        payload,
    }
}

/// Checks the player's credentials for `device` and answers the typed outcome.
pub(super) async fn authenticate_player(
    hub: &LanHub,
    device: &str,
    request: PlayerAuthenticate,
) -> Result<PlayerAuthResult, AppError> {
    if !hub.is_primary() {
        return Err(app(
            ErrorCode::NotPrimary,
            "this hub is not the primary writer",
        ));
    }
    let Some(kek) = hub.player_kek.clone() else {
        return Err(app(
            ErrorCode::Unsupported,
            "this hub has no player directory",
        ));
    };
    // Reserved before any check, so parallel guesses count toward the limit.
    if let Err(retry_s) = hub.login_throttle.begin(device, Instant::now()) {
        return Ok(PlayerAuthResult::Locked { retry_s });
    }
    let reservation = Reservation {
        throttle: &hub.login_throttle,
        device,
        live: true,
    };
    let checked = async {
        let _permit = Arc::clone(&hub.verify_permits)
            .acquire_owned()
            .await
            .map_err(|_| app(ErrorCode::Internal, "password checks are shut down"))?;
        let dir = Arc::clone(&hub.devices);
        tokio::task::spawn_blocking(move || {
            let PlayerAuthenticate { username, password } = request;
            let candidate = dir.login_candidate(&normalize_username(&username))?;
            let matched = arena_core::players::check_password(
                &password,
                candidate.as_ref().map(|c| c.password_hash.as_str()),
            );
            Ok::<_, AppError>((candidate, matched))
        })
        .await
        .map_err(|e| app(ErrorCode::Internal, format!("password check failed: {e}")))?
    }
    .await;
    let (candidate, matched) = match checked {
        Ok(c) => c,
        // A check that could not run counts neither way (the guard gives the reservation back).
        Err(e) => return Err(e),
    };
    let Some(candidate) = candidate.filter(|_| matched) else {
        let paused = reservation.finish(false, Instant::now());
        tracing::info!(device_id = %device, paused = paused.is_some(), "player login refused");
        return Ok(match paused {
            Some(retry_s) => {
                raise_lockout(hub, device).await;
                PlayerAuthResult::Locked { retry_s }
            }
            None => PlayerAuthResult::Rejected,
        });
    };
    reservation.finish(true, Instant::now());
    if let Some(device_name) = candidate.busy_device_name {
        tracing::info!(device_id = %device, player_id = %candidate.player_id, "player login refused: already playing");
        return Ok(PlayerAuthResult::Busy { device_name });
    }
    let name = tokio::task::spawn_blocking({
        let player_id = candidate.player_id.clone();
        move || {
            let kek = kek.kek()?;
            arena_core::players::open_name(
                &kek,
                &player_id,
                &candidate.wrapped_key,
                &candidate.name_enc,
            )
        }
    })
    .await
    .map_err(|e| app(ErrorCode::Internal, format!("name read failed: {e}")))??;
    record_login(hub, device, &candidate.player_id).await?;
    tracing::info!(device_id = %device, player_id = %candidate.player_id, "player logged in");
    Ok(PlayerAuthResult::Ok {
        player: PlayerContext {
            player_id: candidate.player_id,
            name,
        },
    })
}

/// Writes the `kiosk.log` of a correct login (device actor).
async fn record_login(hub: &LanHub, device: &str, player_id: &str) -> Result<(), AppError> {
    let writer = hub
        .writer
        .clone()
        .ok_or_else(|| app(ErrorCode::NotPrimary, "this hub is not the primary writer"))?;
    let log = intent(
        hub.clock.now_ms(),
        Actor {
            kind: ActorKind::Device,
            id: device.to_owned(),
        },
        IntentKind::KioskLog,
        IntentPayload::KioskLog(KioskLog {
            level: KioskLogLevel::Info,
            code: PLAYER_LOGIN_CODE.to_owned(),
            detail: format!("player {player_id}"),
        }),
    );
    writer.submit_intent(log).await.map(|_| ())
}

/// Raises the `login_lockout` alert for `device` (system actor); one that is already open stays as
/// it is.
async fn raise_lockout(hub: &LanHub, device: &str) {
    let Some(writer) = hub.writer.clone() else {
        return;
    };
    let raise = intent(
        hub.clock.now_ms(),
        Actor {
            kind: ActorKind::System,
            id: LAN_SYSTEM_ACTOR.to_owned(),
        },
        IntentKind::AlertRaise,
        IntentPayload::AlertRaise(AlertRaise::new(
            AlertKind::LoginLockout,
            Some(device.to_owned()),
        )),
    );
    match writer.submit_intent(raise).await {
        Ok(_) => {
            tracing::warn!(device_id = %device, seconds = LOGIN_LOCK_SECONDS, "player login paused after wrong tries");
        }
        Err(e) if e.code == ErrorCode::Stale => {}
        Err(e) => {
            tracing::warn!(device_id = %device, code = e.code.as_str(), "the login lockout alert was not raised");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::throttle::{ADMIN_PIN_MAX_FAILURES, ADMIN_PIN_PAUSE};

    #[test]
    fn the_shared_throttle_has_the_protocol_s_limits() {
        assert_eq!(ADMIN_PIN_MAX_FAILURES, arena_proto::LOGIN_MAX_FAILURES);
        assert_eq!(
            ADMIN_PIN_PAUSE.as_secs(),
            u64::from(arena_proto::LOGIN_LOCK_SECONDS)
        );
    }
}
