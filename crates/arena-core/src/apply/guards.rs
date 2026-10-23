//! The steps before any write: permission, epoch and base_rev (ttl_dedupe is in
//! `ttl_dedupe.rs`).

use arena_proto::{ActorKind, AppError, ErrorCode, Intent};
use arena_store::{StoreTx, find_device, read_lease};

use super::{ApplyCtx, Target, devices, settings_set, staff_roles};
use crate::rbac;
use crate::status::{Epoch, Role};

fn refused(message: &str) -> AppError {
    AppError::new(ErrorCode::PermissionDenied, message)
}

/// **permission.** Runs first, before any write: the actor must have an id, a device actor's
/// row must exist and not be revoked, an elevation claim is refused (Hub-side PIN verification
/// arrives in Story 1.13), and then the kind's requirement must hold (`rbac::check`). Every
/// failure is `PERMISSION_DENIED`.
pub(super) fn permission(tx: &StoreTx<'_>, intent: &Intent) -> Result<(), AppError> {
    if intent.actor.id.trim().is_empty() {
        return Err(refused("the actor has no id"));
    }
    // A device acts only while its row exists and is not revoked: this closes the window
    // between a revoke committing and the LAN fan-out closing the device's sessions.
    if intent.actor.kind == ActorKind::Device
        && find_device(tx, &intent.actor.id)?.is_none_or(|d| d.revoked_at.is_some())
    {
        return Err(refused("the device is revoked or unknown"));
    }
    // An elevation claim cannot be trusted until the Hub verifies PINs, so it is refused rather
    // than written into the audit log.
    if intent.elevated_by.is_some() {
        return Err(refused("elevation not verified"));
    }
    rbac::check(tx, intent)
}

/// **authorize.** The permission rules that depend on the loaded target.
pub(super) fn authorize(intent: &Intent, target: &Target) -> Result<(), AppError> {
    match (target, &intent.payload) {
        (Target::Staff(t), _) => staff_roles::authorize_staff(t),
        (Target::Role(t), arena_proto::IntentPayload::RoleUpsert(r)) => {
            staff_roles::authorize_role(r, t)
        }
        (Target::Role(_), _) => Err(AppError::new(
            ErrorCode::Internal,
            "the loaded target does not match the payload",
        )),
        (Target::None | Target::Setting(_) | Target::NewDevice(_) | Target::Device(_), _) => Ok(()),
    }
}

fn not_primary(message: &str) -> AppError {
    AppError::new(ErrorCode::NotPrimary, message)
}

/// **epoch.** Fails `NOT_PRIMARY` unless this writer is primary and the lease row, read inside
/// the transaction, still holds its epoch and has not expired. Returns that epoch.
pub(crate) fn epoch(tx: &StoreTx<'_>, ctx: &ApplyCtx<'_>) -> Result<Epoch, AppError> {
    if ctx.role != Role::Primary {
        return Err(not_primary("this hub is not the primary writer"));
    }
    let Some(own) = ctx.epoch.clone() else {
        return Err(not_primary("this hub holds no lease"));
    };
    match read_lease(tx)? {
        Some(row) if row.counter == own.counter && row.hub_instance_id == own.hub_instance_id => {
            if row.expires_at > ctx.clock.now_ms() {
                Ok(own)
            } else {
                Err(not_primary("this hub's lease has expired"))
            }
        }
        _ => Err(not_primary("the lease is held by another epoch")),
    }
}

/// **base_rev.** Edit intents carry the `rev` they were based on, and a mismatch fails
/// `STALE`. `branch.bootstrap` creates rows, so it has none to check (validate rejects one).
pub(super) fn base_rev(intent: &Intent, target: &Target) -> Result<(), AppError> {
    match target {
        Target::None => Ok(()),
        Target::Setting(row) => settings_set::base_rev(intent, row.as_ref()),
        Target::NewDevice(_) => Ok(()),
        Target::Device(row) => devices::base_rev(intent, row.as_ref()),
        Target::Staff(t) => staff_roles::base_rev_staff(intent, t),
        Target::Role(t) => staff_roles::base_rev_role(intent, t),
    }
}
