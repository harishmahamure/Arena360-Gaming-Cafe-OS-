//! Role-based access control: the built-in roles and the permission step of `apply()`.
//!
//! Every intent kind has one [`Requirement`]. `Perm(p)` needs a `staff` actor with an active
//! staff row whose role allows `p` (and does not deny it), or an `owner` actor, who holds every
//! permission. An `admin` actor holds none yet. `System` and `Device` need that actor kind;
//! `Phone` is refused until phone actors exist. Every refusal is `PERMISSION_DENIED` with a
//! message that never names anyone.

use arena_proto::{ActorKind, AppError, ErrorCode, Intent, PermEffect, Permission, Requirement};
use arena_store::{StoreTx, find_role, find_session, find_staff, role_perm_effect};

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

const COUNTER_PERMS: [Permission; 15] = [
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
    Permission::KitchenUse,
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

/// The Hub recording its own daily export (Story 9.5).
fn is_auto_export(intent: &Intent) -> bool {
    intent.actor.kind == ActorKind::System
        && intent.actor.id == arena_proto::AUTO_EXPORT_ACTOR
        && matches!(
            &intent.payload,
            arena_proto::IntentPayload::BackupExport(p) if arena_proto::is_auto_export_file(&p.file_name)
        )
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

/// Whether a paired device may apply `intent` itself (Story 6.10): the one player right a device
/// holds is creating a profile with the consent box ticked (`player.signup` at the kiosk). The Hub
/// builds that intent; `player.create` is outside the paired scope, so a kiosk cannot send it. A
/// `consent: false` create, an update, a password reset and an import stay denied.
fn device_creates_player(p: Permission, intent: &Intent) -> bool {
    use arena_proto::IntentPayload;
    p == Permission::PlayerManage
        && matches!(&intent.payload, IntentPayload::PlayerCreate(c) if c.consent)
}

/// Whether a paired device may change a password for the player on its own PC (Story 6.34): the
/// player typed the current password and a new one at the kiosk's Account page. The Hub checked
/// the current password and hashed the new one before building this `player.password_change`; a
/// kiosk cannot send it (it is outside the paired scope). Another player's password stays denied,
/// and so does a reset: only the change, for the player running on this PC.
fn device_changes_own_password(
    tx: &StoreTx<'_>,
    p: Permission,
    intent: &Intent,
) -> Result<bool, AppError> {
    use arena_proto::IntentPayload;
    let IntentPayload::PlayerPasswordChange(change) = &intent.payload else {
        return Ok(false);
    };
    if p != Permission::PlayerManage {
        return Ok(false);
    }
    Ok(arena_store::live_session_on_device(tx, &intent.actor.id)?
        .is_some_and(|(_, player)| player.as_deref() == Some(change.player_id.as_str())))
}

/// Whether a paired device may save PC settings for the player on its own PC (Story 6.35): the
/// player changed a mouse, display, sound or keyboard setting at the kiosk's PC settings panel.
/// The Hub builds this `player.pc_settings_set` from the device's session; a kiosk cannot send it
/// (it is outside the paired scope). Another player's settings stay denied.
fn device_saves_own_pc_settings(
    tx: &StoreTx<'_>,
    p: Permission,
    intent: &Intent,
) -> Result<bool, AppError> {
    use arena_proto::IntentPayload;
    let IntentPayload::PlayerPcSettingsSet(save) = &intent.payload else {
        return Ok(false);
    };
    if p != Permission::PlayerManage {
        return Ok(false);
    }
    Ok(arena_store::live_session_on_device(tx, &intent.actor.id)?
        .is_some_and(|(_, player)| player.as_deref() == Some(save.player_id.as_str())))
}

/// Whether a paired device may end the session on its own PC (Story 6.17): the player chose Log
/// out at the kiosk. The Hub builds that `session.end` from the device's session; a kiosk cannot
/// send it (it is outside the paired scope). Another PC's session stays denied.
fn device_ends_own_session(
    tx: &StoreTx<'_>,
    p: Permission,
    intent: &Intent,
) -> Result<bool, AppError> {
    use arena_proto::IntentPayload;
    let IntentPayload::SessionEnd(end) = &intent.payload else {
        return Ok(false);
    };
    if p != Permission::SessionEnd {
        return Ok(false);
    }
    Ok(find_session(tx, &end.session_id)?.is_some_and(|s| s.device_id == intent.actor.id))
}

/// Whether a paired device may extend the session on its own PC (Story 6.21): the player chose
/// Extend at the kiosk. The Hub builds that `session.extend` from the device's session; a kiosk
/// cannot send it (it is outside the paired scope). Another PC's session stays denied, and so does
/// a credit override: the kiosk has no manager override.
fn device_extends_own_session(
    tx: &StoreTx<'_>,
    p: Permission,
    intent: &Intent,
) -> Result<bool, AppError> {
    use arena_proto::IntentPayload;
    let IntentPayload::SessionExtend(extend) = &intent.payload else {
        return Ok(false);
    };
    if p != Permission::SessionExtend || extend.credit_override == Some(true) {
        return Ok(false);
    }
    Ok(find_session(tx, &extend.session_id)?.is_some_and(|s| s.device_id == intent.actor.id))
}

/// A kitchen display (class `kds`, a device actor) moves a ticket and takes the move back
/// (Story 7.3). The LAN class scope keeps every other device from submitting these.
fn device_moves_ticket(p: Permission, intent: &Intent) -> bool {
    p == Permission::KitchenUse
        && matches!(
            &intent.payload,
            arena_proto::IntentPayload::KitchenAdvance(_)
                | arena_proto::IntentPayload::KitchenAdvanceReverted(_)
        )
}

/// Whether a paired device may place an order on the session on its own PC (Story 6.31): the
/// player confirmed their cart at the kiosk. The Hub builds that `order.place` from the device's
/// session; a kiosk cannot send it (it is outside the paired scope). Another PC's session stays
/// denied, and so does a stock override: the kiosk has no manager override. Story 6.32 adds the
/// unpaid counter order (tied to this PC and the player who last had its session) and the
/// wallet sale (`pos.sale` with `kiosk`, for the player running on this PC).
fn device_places_own_order(
    tx: &StoreTx<'_>,
    p: Permission,
    intent: &Intent,
) -> Result<bool, AppError> {
    use arena_proto::IntentPayload;
    if p != Permission::PosSell {
        return Ok(false);
    }
    match &intent.payload {
        IntentPayload::OrderPlace(place) if place.stock_override != Some(true) => {
            match &place.counter {
                Some(c) => Ok(place.session_id.is_empty()
                    && c.device_id == intent.actor.id
                    && arena_store::last_session_player(tx, &intent.actor.id)? == c.player_id),
                None => Ok(find_session(tx, &place.session_id)?
                    .is_some_and(|s| s.device_id == intent.actor.id)),
            }
        }
        IntentPayload::PosSale(sale) => {
            let Some(k) = &sale.kiosk else {
                return Ok(false);
            };
            let live = arena_store::list_live_sessions(tx)?
                .into_iter()
                .find(|s| s.device_id == intent.actor.id);
            Ok(k.device_id == intent.actor.id
                && sale.player_id.is_some()
                && live.is_some_and(|s| s.player_id == sale.player_id))
        }
        _ => Ok(false),
    }
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
                // The owner's one-tap credit reminder (Story 8.32): the only `notify.requested`
                // an owner may submit; every other kind stays system-only.
                || (actor.kind == ActorKind::Owner
                    && matches!(
                        &intent.payload,
                        arena_proto::IntentPayload::NotifyRequested(n)
                            if n.kind == arena_proto::NOTIFY_KIND_CREDIT_REMINDER
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
            ActorKind::System => is_pairing(intent) || is_auto_export(intent),
            ActorKind::Admin => false,
            ActorKind::Device => {
                device_starts_own_session(p, intent)
                    || device_creates_player(p, intent)
                    || device_ends_own_session(tx, p, intent)?
                    || device_extends_own_session(tx, p, intent)?
                    || device_moves_ticket(p, intent)
                    || device_places_own_order(tx, p, intent)?
                    || device_changes_own_password(tx, p, intent)?
                    || device_saves_own_pc_settings(tx, p, intent)?
            }
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
        assert_eq!(builtin_perms(ROLE_COUNTER).len(), 15);
        assert!(!builtin_perms(ROLE_COUNTER).contains(&Permission::SettingsEdit));
        assert_eq!(builtin_perms(ROLE_KITCHEN), vec![Permission::KitchenUse]);
        assert!(builtin_perms(ROLE_KIOSK_DEVICE).is_empty());
    }
}
