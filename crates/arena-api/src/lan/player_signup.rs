//! `player.signup` (Story 6.10): a new player's sign-up typed at a paired kiosk.
//!
//! The Hub runs the Story 3.6 checks (`PlayerForm::check`), hashes with Argon2id on a blocking
//! thread under the verify permits, seals name and phone with `arena_core::players::seal` (a fresh
//! player key wrapped by the branch KEK) and applies `player.create` through the writer as the
//! paired device. The policy version is the Hub's own constant and the consent box must be ticked
//! (`consent: false` is refused on field `consent` and writes nothing). On success the Hub stores
//! the same login record as a correct `player.authenticate`, so `player.plans` and
//! `player.start_session` work at once; no `kiosk.log` is written (the `player.create` change rows
//! already show player and device).
//!
//! The answer is `player.signup_result{outcome}`: `ok{player}` or `refused{code, field?}`. A Hub
//! that is not primary or has no player directory answers an `error` frame. The credentials and
//! personal data are never logged.

use std::sync::Arc;

use arena_proto::{
    Actor, ActorKind, AppError, ErrorCode, IntentKind, IntentPayload, PLAYER_POLICY_VERSION,
    PlayerContext, PlayerForm, PlayerSignup, PlayerSignupResult,
};
use tokio::time::Instant;

use super::LanHub;
use super::player_login::intent;

fn app(code: ErrorCode, message: impl Into<String>) -> AppError {
    AppError::new(code, message)
}

fn refused(e: AppError) -> PlayerSignupResult {
    PlayerSignupResult::Refused {
        code: e.code,
        field: e.field,
    }
}

/// Creates the player for `device` and logs them in on `conn`.
pub(super) async fn signup_player(
    hub: &LanHub,
    device: &str,
    conn: u64,
    request: PlayerSignup,
) -> Result<PlayerSignupResult, AppError> {
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
    let writer = hub
        .writer
        .clone()
        .ok_or_else(|| app(ErrorCode::NotPrimary, "this hub is not the primary writer"))?;
    if !request.consent {
        return Ok(refused(AppError::with_field(
            ErrorCode::ValidationFailed,
            "consent",
            "the consent box must be ticked",
        )));
    }
    let now = hub.clock.now_ms();
    let form = PlayerForm {
        username: request.username,
        password: request.password,
        phone: request.phone,
        name: request.name,
        email: String::new(),
        dob: String::new(),
        notes: String::new(),
        tags: Vec::new(),
        consent: true,
        policy_version: PLAYER_POLICY_VERSION.to_owned(),
    };
    if let Err(e) = form.check(&arena_core::players::utc_day(now)) {
        return Ok(refused(e));
    }
    let name = form.name.trim().to_owned();
    let permit = Arc::clone(&hub.verify_permits)
        .acquire_owned()
        .await
        .map_err(|_| app(ErrorCode::Internal, "password checks are shut down"))?;
    let player_id = ulid::Ulid::generate().to_string();
    let id = player_id.clone();
    let sealed = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let hash = arena_core::staff::hash_secret(&form.password)?;
        arena_core::players::seal(&kek.kek()?, &form, &id, hash)
    })
    .await
    .map_err(|e| app(ErrorCode::Internal, format!("sign-up failed: {e}")))??;
    let create = intent(
        hub.clock.now_ms(),
        Actor {
            kind: ActorKind::Device,
            id: device.to_owned(),
        },
        IntentKind::PlayerCreate,
        IntentPayload::PlayerCreate(sealed),
    );
    match writer.submit_intent(create).await {
        Ok(_) => {
            hub.player_logins
                .set(device, &player_id, conn, Instant::now());
            tracing::info!(device_id = %device, player_id = %player_id, "player signed up");
            Ok(PlayerSignupResult::Ok {
                player: PlayerContext { player_id, name },
            })
        }
        Err(e) if e.code == ErrorCode::NotPrimary => Err(e),
        Err(e) => {
            tracing::info!(device_id = %device, code = e.code.as_str(), "player sign-up refused");
            Ok(refused(e))
        }
    }
}
