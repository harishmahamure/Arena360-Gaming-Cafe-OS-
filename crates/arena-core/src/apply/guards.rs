//! The steps before any write: permission, epoch, ttl_dedupe and base_rev.

use arena_proto::{ActorKind, AppError, ErrorCode, Intent, IntentKind};
use arena_store::{StoreTx, read_lease};

use super::ApplyCtx;
use crate::status::{Epoch, Role};

/// **permission.** In this story `branch.bootstrap` requires a `system` actor. Story 1.11
/// replaces this with the permission catalogue. The kind is taken from the payload, so a
/// mislabelled envelope cannot borrow another kind's permission.
pub(super) fn permission(intent: &Intent) -> Result<(), AppError> {
    if intent.actor.id.trim().is_empty() {
        return Err(AppError::new(ErrorCode::Forbidden, "the actor has no id"));
    }
    // Hub-side PIN verification arrives in Story 1.13; until then an elevation claim cannot be
    // trusted, so it is refused rather than written into the audit log.
    if intent.elevated_by.is_some() {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            "elevation not verified",
        ));
    }
    let allowed = match intent.payload.kind() {
        IntentKind::BranchBootstrap => intent.actor.kind == ActorKind::System,
    };
    if allowed {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::Forbidden,
            format!(
                "{} is not permitted for this actor",
                intent.payload.kind().as_str()
            ),
        ))
    }
}

fn not_primary(message: &str) -> AppError {
    AppError::new(ErrorCode::NotPrimary, message)
}

/// **epoch.** Fails `NOT_PRIMARY` unless this writer is primary and the lease row, read inside
/// the transaction, still holds its epoch and has not expired. Returns that epoch.
pub(super) fn epoch(tx: &StoreTx<'_>, ctx: &ApplyCtx<'_>) -> Result<Epoch, AppError> {
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

/// **ttl_dedupe.** A pass-through stub: Story 1.8 adds the 7-day TTL (`EXPIRED`) and the
/// `applied_intents` dedupe records.
pub(super) fn ttl_dedupe(
    _tx: &StoreTx<'_>,
    _intent: &Intent,
    _now_ms: i64,
) -> Result<(), AppError> {
    Ok(())
}

/// **base_rev.** Edit intents carry the `rev` they were based on, and a mismatch fails
/// `STALE`. `branch.bootstrap` creates rows, so it has none to check.
pub(super) fn base_rev(_tx: &StoreTx<'_>, intent: &Intent) -> Result<(), AppError> {
    match intent.payload.kind() {
        IntentKind::BranchBootstrap => Ok(()),
    }
}
