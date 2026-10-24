//! Kiosk admin exits (Story 1.20): `device.admin_exit`, `device.admin_relock` and
//! `device.admin_unlock`, each writing one `device_admin_exits` row (insert, or the re-lock
//! update).
//!
//! - **`device.admin_exit`** (the kiosk, elevated by a manager): the actor is the kiosk device
//!   itself and the elevating staff member must hold `device.remote`. Its elevation may be
//!   `cached_verifier` (the only intent that accepts one, AD-30): the kiosk checked the PIN
//!   offline. The exit starts at the intent's `issued_at` and re-locks [`ADMIN_EXIT_MS`] later.
//!   When the verifier version the PIN was checked against is older than the version in effect
//!   at `issued_at`, the exit is still recorded, with `flagged = 1` and
//!   `flag_reason = stale_verifier`. A manager who has since lost the permission (or left) is
//!   accepted only on such a stale offline check, and only when the verifier version used
//!   (`verifier_history.staff_ids`) really listed them.
//! - **`device.admin_relock`** (the kiosk): sets `relocked_at` on one of its own exits.
//! - **`device.admin_unlock`** (staff with `device.remote`, or elevated to it): the Hub mints a
//!   `remote` exit for a live paired device, starting now, approved by the elevating member (or
//!   the actor). The LAN layer then pushes it to the kiosk.

use arena_proto::{
    ADMIN_EXIT_MS, ActorKind, AdminExitMode, AppError, ClientClass, DeviceAdminExit,
    DeviceAdminRelock, DeviceAdminUnlock, ElevatedBy, ElevationKind, ElevationMode, ErrorCode,
    FLAG_STALE_VERIFIER, Intent, Permission, validate_exit_id,
};
use arena_store::{
    AdminExitRow, ChangeOp, DeviceRow, NewAdminExit, RowKey, StoreTx, current_branch_id,
    find_admin_exit, find_device, find_staff, insert_admin_exit, set_admin_exit_relocked,
    verifier_staff, verifier_version_at,
};

use super::Target;
use super::mutate::Written;
use crate::rbac;

fn invalid(field: &str, message: impl Into<String>) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

fn no_branch() -> AppError {
    AppError::new(
        ErrorCode::ValidationFailed,
        "the branch has not been bootstrapped",
    )
}

/// What `device.admin_exit` needs: the branch and whether the exit id is taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExitTarget {
    pub branch: Option<String>,
    pub taken: bool,
}

/// What `device.admin_unlock` needs: the branch and the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnlockTarget {
    pub branch: Option<String>,
    pub device: Option<DeviceRow>,
}

pub(super) fn load_exit(tx: &StoreTx<'_>, p: &DeviceAdminExit) -> Result<Target, AppError> {
    Ok(Target::AdminExit(Box::new(ExitTarget {
        branch: current_branch_id(tx)?,
        taken: find_admin_exit(tx, &p.exit_id)?.is_some(),
    })))
}

pub(super) fn load_relock(tx: &StoreTx<'_>, p: &DeviceAdminRelock) -> Result<Target, AppError> {
    Ok(Target::AdminRelock(find_admin_exit(tx, &p.exit_id)?))
}

pub(super) fn load_unlock(tx: &StoreTx<'_>, p: &DeviceAdminUnlock) -> Result<Target, AppError> {
    Ok(Target::AdminUnlock(Box::new(UnlockTarget {
        branch: current_branch_id(tx)?,
        device: find_device(tx, &p.device_id)?,
    })))
}

/// Whether the verifier `e` was checked against was already out of date at `issued_at`.
fn stale(tx: &StoreTx<'_>, e: &ElevatedBy, issued_at: i64) -> Result<bool, AppError> {
    Ok(e.verifier_version < verifier_version_at(tx, issued_at)?)
}

/// **permission** for `device.admin_exit`: the kiosk device acts (checked like any device
/// intent), elevated by a staff member's PIN in mode `hub` or `cached_verifier` matching the
/// payload, who holds `device.remote` now, or held it in the stale verifier the kiosk used
/// offline. Every refusal is `PERMISSION_DENIED`.
pub(super) fn permission(tx: &StoreTx<'_>, intent: &Intent) -> Result<(), AppError> {
    rbac::check(tx, intent)?;
    let arena_proto::IntentPayload::DeviceAdminExit(p) = &intent.payload else {
        return Err(rbac::denied());
    };
    let Some(e) = &intent.elevated_by else {
        return Err(rbac::denied());
    };
    let mode_ok = matches!(
        (e.mode, p.mode),
        (ElevationMode::Hub, AdminExitMode::Hub)
            | (ElevationMode::CachedVerifier, AdminExitMode::CachedVerifier)
    );
    if e.kind != ElevationKind::Pin || !mode_ok || e.id.trim().is_empty() {
        return Err(rbac::denied());
    }
    let Some(staff) = find_staff(tx, &e.id)? else {
        return Err(rbac::denied());
    };
    if staff.pin_hash.is_some() && rbac::staff_allows(tx, &e.id, Permission::DeviceRemote)? {
        return Ok(());
    }
    // Offline against a verifier that was already out of date: accepted only for someone that
    // verifier really listed (the kiosk could not have matched anyone else's PIN).
    if e.mode == ElevationMode::CachedVerifier
        && stale(tx, e, intent.issued_at)?
        && verifier_staff(tx, e.verifier_version)?.is_some_and(|ids| ids.contains(&e.id))
    {
        return Ok(());
    }
    Err(rbac::denied())
}

/// **authorize** for `device.admin_relock`: only the kiosk that made the exit re-locks it.
pub(super) fn authorize_relock(
    intent: &Intent,
    row: Option<&AdminExitRow>,
) -> Result<(), AppError> {
    match row {
        Some(r) if intent.actor.kind == ActorKind::Device && r.device_id != intent.actor.id => {
            Err(rbac::denied())
        }
        _ => Ok(()),
    }
}

pub(super) fn validate_exit(
    intent: &Intent,
    p: &DeviceAdminExit,
    t: &ExitTarget,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "device.admin_exit takes no base_rev"));
    }
    validate_exit_id(&p.exit_id)?;
    if p.mode == AdminExitMode::Remote {
        return Err(invalid(
            "mode",
            "a kiosk exit is granted by the Hub or the cached verifier",
        ));
    }
    if t.taken {
        return Err(invalid("exit_id", "this exit is already recorded"));
    }
    if t.branch.is_none() {
        return Err(no_branch());
    }
    Ok(())
}

pub(super) fn validate_relock(
    intent: &Intent,
    p: &DeviceAdminRelock,
    row: Option<&AdminExitRow>,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "device.admin_relock takes no base_rev"));
    }
    validate_exit_id(&p.exit_id)?;
    let Some(row) = row else {
        return Err(invalid("exit_id", "there is no such exit"));
    };
    if row.relocked_at.is_some() {
        return Err(invalid("exit_id", "this exit is already re-locked"));
    }
    Ok(())
}

pub(super) fn validate_unlock(
    intent: &Intent,
    p: &DeviceAdminUnlock,
    t: &UnlockTarget,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "device.admin_unlock takes no base_rev"));
    }
    if t.branch.is_none() {
        return Err(no_branch());
    }
    match &t.device {
        Some(d) if d.revoked_at.is_none() && d.class == ClientClass::Paired.as_str() => Ok(()),
        Some(d) if d.revoked_at.is_some() => Err(invalid("device_id", "the device is revoked")),
        Some(_) => Err(invalid("device_id", "only a paired kiosk can be unlocked")),
        None => Err(invalid(
            "device_id",
            format!(
                "no device {}",
                p.device_id.chars().take(64).collect::<String>()
            ),
        )),
    }
}

fn inserted(exit_id: String) -> Vec<Written> {
    vec![Written {
        key: RowKey::DeviceAdminExits { exit_id },
        op: ChangeOp::Insert,
        before: None,
    }]
}

pub(super) fn mutate_exit(
    tx: &StoreTx<'_>,
    intent: &Intent,
    p: &DeviceAdminExit,
    t: &ExitTarget,
) -> Result<Vec<Written>, AppError> {
    let branch_id = t
        .branch
        .as_deref()
        .ok_or_else(|| AppError::new(ErrorCode::Internal, "an exit validated without a branch"))?;
    let e = intent.elevated_by.as_ref().ok_or_else(|| {
        AppError::new(
            ErrorCode::Internal,
            "an exit permitted without an elevation",
        )
    })?;
    let flagged = stale(tx, e, intent.issued_at)?;
    if flagged {
        tracing::warn!(
            exit_id = %p.exit_id,
            device_id = %intent.actor.id,
            verifier_version = e.verifier_version,
            "an offline admin exit used a stale verifier; flagged for review"
        );
    }
    insert_admin_exit(
        tx,
        &NewAdminExit {
            exit_id: &p.exit_id,
            branch_id,
            device_id: &intent.actor.id,
            staff_id: &e.id,
            mode: p.mode.as_str(),
            started_at: intent.issued_at,
            relock_at: intent.issued_at.saturating_add(ADMIN_EXIT_MS),
            flagged,
            flag_reason: flagged.then_some(FLAG_STALE_VERIFIER),
        },
    )?;
    Ok(inserted(p.exit_id.clone()))
}

pub(super) fn mutate_relock(
    tx: &StoreTx<'_>,
    intent: &Intent,
    p: &DeviceAdminRelock,
) -> Result<Vec<Written>, AppError> {
    set_admin_exit_relocked(tx, &p.exit_id, intent.issued_at)?;
    Ok(vec![Written {
        key: RowKey::DeviceAdminExits {
            exit_id: p.exit_id.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    }])
}

pub(super) fn mutate_unlock(
    tx: &StoreTx<'_>,
    intent: &Intent,
    p: &DeviceAdminUnlock,
    t: &UnlockTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let branch_id = t.branch.as_deref().ok_or_else(|| {
        AppError::new(ErrorCode::Internal, "an unlock validated without a branch")
    })?;
    let exit_id = ulid::Ulid::generate().to_string();
    let approver = intent
        .elevated_by
        .as_ref()
        .map_or(intent.actor.id.as_str(), |e| e.id.as_str());
    insert_admin_exit(
        tx,
        &NewAdminExit {
            exit_id: &exit_id,
            branch_id,
            device_id: &p.device_id,
            staff_id: approver,
            mode: AdminExitMode::Remote.as_str(),
            started_at: now_ms,
            relock_at: now_ms.saturating_add(ADMIN_EXIT_MS),
            flagged: false,
            flag_reason: None,
        },
    )?;
    Ok(inserted(exit_id))
}
