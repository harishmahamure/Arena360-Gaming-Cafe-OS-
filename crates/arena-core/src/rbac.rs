//! Role-based access control: the built-in roles and the permission step of `apply()`.
//!
//! Every intent kind has one [`Requirement`]. `Perm(p)` needs a `staff` actor with an active
//! staff row whose role allows `p` (and does not deny it), or an `owner` actor, who holds every
//! permission. An `admin` actor holds none yet. `System` and `Device` need that actor kind;
//! `Phone` is refused until phone actors exist. Every refusal is `PERMISSION_DENIED` with a
//! message that never names anyone.

use arena_proto::{ActorKind, AppError, ErrorCode, Intent, PermEffect, Permission, Requirement};
use arena_store::{StoreTx, find_role, find_staff, role_perm_effect};

/// The built-in role names. Built-in roles are seeded by `branch.bootstrap` and cannot be
/// changed.
pub const ROLE_OWNER: &str = "Owner";
pub const ROLE_MANAGER: &str = "Manager";
pub const ROLE_COUNTER: &str = "Counter";
pub const ROLE_KITCHEN: &str = "Kitchen";
pub const ROLE_KIOSK_DEVICE: &str = "Kiosk-device";

/// Every built-in role, in the order bootstrap inserts them.
pub const BUILTIN_ROLES: [&str; 5] = [
    ROLE_OWNER,
    ROLE_MANAGER,
    ROLE_COUNTER,
    ROLE_KITCHEN,
    ROLE_KIOSK_DEVICE,
];

const COUNTER_PERMS: [Permission; 14] = [
    Permission::SessionStart,
    Permission::SessionPause,
    Permission::SessionExtend,
    Permission::SessionTransfer,
    Permission::SessionEnd,
    Permission::PosSell,
    Permission::WalletTopup,
    Permission::CreditCollect,
    Permission::ShiftOpen,
    Permission::ShiftClose,
    Permission::StockIn,
    Permission::PlayerManage,
    Permission::PlayerView,
    Permission::DeviceScreen,
];

/// The permissions a built-in role allows (every row is an `allow`). Kiosk-device has none:
/// a device's rights are implicit.
pub fn builtin_perms(role: &str) -> Vec<Permission> {
    match role {
        ROLE_OWNER => Permission::ALL.to_vec(),
        ROLE_MANAGER => Permission::ALL
            .into_iter()
            .filter(|p| *p != Permission::OpsRestore)
            .collect(),
        ROLE_COUNTER => COUNTER_PERMS.to_vec(),
        ROLE_KITCHEN => vec![Permission::KitchenUse],
        _ => Vec::new(),
    }
}

/// The one refusal every permission failure returns. It never names anyone.
pub fn denied() -> AppError {
    AppError::new(
        ErrorCode::PermissionDenied,
        "you don't have permission to do this",
    )
}

/// Whether the staff member `staff_id` is active and their role allows `p`.
pub fn staff_allows(tx: &StoreTx<'_>, staff_id: &str, p: Permission) -> Result<bool, AppError> {
    let Some(staff) = find_staff(tx, staff_id)? else {
        return Ok(false);
    };
    if !staff.active {
        return Ok(false);
    }
    let effect = role_perm_effect(tx, &staff.role_id, p.as_str())?;
    Ok(effect.as_deref() == Some(PermEffect::Allow.as_str()))
}

/// The role of an active staff actor, if it is a built-in role: its name. An inactive or
/// missing staff member has none.
pub fn staff_builtin_role(tx: &StoreTx<'_>, staff_id: &str) -> Result<Option<String>, AppError> {
    let Some(staff) = find_staff(tx, staff_id)?.filter(|s| s.active) else {
        return Ok(None);
    };
    Ok(find_role(tx, &staff.role_id)?
        .filter(|r| r.builtin)
        .map(|r| r.name))
}

pub fn is_pairing(intent: &Intent) -> bool {
    intent.actor.kind == ActorKind::System
        && intent.actor.id == arena_proto::PAIR_SYSTEM_ACTOR
        && matches!(
            &intent.payload,
            arena_proto::IntentPayload::DeviceRegister(d) if d.device_id.is_none()
        )
}

/// Whether a paired device may apply `intent` itself (Story 6.9): the one thing a device actor may do
/// under a staff permission is start a prepaid session for a player on its own PC (pack, pass,
/// wallet or block). The Hub builds that intent from its login record; a kiosk cannot send
/// `session.start` (it is outside the paired scope), and a postpaid or another PC's start is
/// denied here too.
fn device_starts_own_session(p: Permission, intent: &Intent) -> bool {
    use arena_proto::{IntentPayload, SessionMode};
    p == Permission::SessionStart
        && matches!(
            &intent.payload,
            IntentPayload::SessionStart(s)
                if s.device_id == intent.actor.id
                    && s.player_id.is_some()
                    && matches!(
                        s.mode,
                        SessionMode::Pack | SessionMode::Pass | SessionMode::Wallet | SessionMode::Block
                    )
        )
}

/// **permission.** Checks the intent's [`Requirement`] against its actor. The kind is taken
/// from the payload, so a mislabelled envelope cannot borrow another kind's requirement.
pub fn check(tx: &StoreTx<'_>, intent: &Intent) -> Result<(), AppError> {
    let actor = &intent.actor;
    let ok = match intent.payload.kind().requirement() {
        Requirement::System => {
            actor.kind == ActorKind::System
                || (actor.kind == ActorKind::Staff
                    && matches!(
                        &intent.payload,
                        arena_proto::IntentPayload::StaffSetCredentials(c) if c.staff_id == actor.id
                    ))
                || (actor.kind == ActorKind::Owner
                    && matches!(
                        &intent.payload,
                        arena_proto::IntentPayload::BranchBootstrap(b) if b.onboarding.is_some()
                    ))
        }
        Requirement::Device => {
            actor.kind == ActorKind::Device
                || (actor.kind == ActorKind::System
                    && matches!(
                        &intent.payload,
                        arena_proto::IntentPayload::DeviceTestSessionCompleted(_)
                    ))
        }
        Requirement::Phone => false,
        Requirement::Perm(p) => match actor.kind {
            ActorKind::Owner => true,
            ActorKind::Staff => staff_allows(tx, &actor.id, p)?,
            ActorKind::System => is_pairing(intent),
            ActorKind::Admin => false,
            ActorKind::Device => device_starts_own_session(p, intent),
        },
    };
    if ok { Ok(()) } else { Err(denied()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_role_permissions() {
        assert_eq!(builtin_perms(ROLE_OWNER).len(), Permission::ALL.len());
        let manager = builtin_perms(ROLE_MANAGER);
        assert_eq!(manager.len(), Permission::ALL.len() - 1);
        assert!(!manager.contains(&Permission::OpsRestore));
        assert_eq!(builtin_perms(ROLE_COUNTER).len(), 14);
        assert!(!builtin_perms(ROLE_COUNTER).contains(&Permission::SettingsEdit));
        assert_eq!(builtin_perms(ROLE_KITCHEN), vec![Permission::KitchenUse]);
        assert!(builtin_perms(ROLE_KIOSK_DEVICE).is_empty());
    }
}
