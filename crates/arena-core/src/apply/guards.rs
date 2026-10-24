//! The steps before any write: permission, epoch and base_rev (ttl_dedupe is in
//! `ttl_dedupe.rs`).

use arena_proto::{
    ActorKind, AppError, ElevatedBy, ElevationKind, ElevationMode, ErrorCode, Intent, Requirement,
};
use arena_store::{StoreTx, find_device, find_staff, read_lease};

use super::{ApplyCtx, Target, admin_exit, devices, settings_set, staff_roles};
use crate::rbac;
use crate::staff::pin_required;
use crate::status::{Epoch, Role};

fn refused(message: &str) -> AppError {
    AppError::new(ErrorCode::PermissionDenied, message)
}

/// **permission.** Runs first, before any write: the actor must have an id, a device actor's
/// row must exist and not be revoked, and then the kind's requirement must hold — against the
/// actor, or, when the intent is elevated, against the elevating staff member
/// ([`elevation`]). Every failure is `PERMISSION_DENIED`, except a PIN that changed since it was
/// checked (`PIN_REQUIRED`).
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
    // The kiosk's admin exit: the device acts on a manager's PIN, checked at the Hub or, only
    // here, against the cached verifier.
    if intent.payload.kind().elevator_permission().is_some() {
        return admin_exit::permission(tx, intent);
    }
    match &intent.elevated_by {
        Some(e) => elevation(tx, intent, e),
        None => rbac::check(tx, intent),
    }
}

/// The permission step of an elevated intent. Only the Hub's ticket path sets `elevated_by`
/// (every edge strips a client's), and this re-checks everything the ticket was issued on:
///
/// - the elevation is a PIN verified at the Hub (`mfa` is a later story; `cached_verifier` is
///   accepted only for `device.admin_exit`, which never comes here);
/// - the actor is an active staff member, and the elevating member is someone else;
/// - the kind's requirement is a permission, and the elevating member is active and holds it;
/// - the elevating member's `pin_verifier_version` is still the one the PIN was checked
///   against, else `PIN_REQUIRED` (the PIN rotated: enter it again).
fn elevation(tx: &StoreTx<'_>, intent: &Intent, e: &ElevatedBy) -> Result<(), AppError> {
    let Requirement::Perm(p) = intent.payload.kind().requirement() else {
        return Err(rbac::denied());
    };
    if e.kind != ElevationKind::Pin || e.mode != ElevationMode::Hub {
        return Err(rbac::denied());
    }
    if intent.actor.kind != ActorKind::Staff || intent.actor.id == e.id {
        return Err(rbac::denied());
    }
    if find_staff(tx, &intent.actor.id)?.is_none_or(|s| !s.active) {
        return Err(rbac::denied());
    }
    if !rbac::staff_allows(tx, &e.id, p)? {
        return Err(rbac::denied());
    }
    match find_staff(tx, &e.id)? {
        Some(s) if s.pin_hash.is_some() && s.pin_verifier_version == e.verifier_version => Ok(()),
        _ => Err(pin_required()),
    }
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
        (Target::AdminRelock(row), _) => admin_exit::authorize_relock(intent, row.as_ref()),
        (
            Target::None
            | Target::Setting(_)
            | Target::NewDevice(_)
            | Target::Device(_)
            | Target::Branch(_)
            | Target::AdminExit(_)
            | Target::AdminUnlock(_),
            _,
        ) => Ok(()),
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
        Target::NewDevice(_)
        | Target::Branch(_)
        | Target::AdminExit(_)
        | Target::AdminRelock(_)
        | Target::AdminUnlock(_) => Ok(()),
        Target::Device(row) => devices::base_rev(intent, row.as_ref()),
        Target::Staff(t) => staff_roles::base_rev_staff(intent, t),
        Target::Role(t) => staff_roles::base_rev_role(intent, t),
    }
}
