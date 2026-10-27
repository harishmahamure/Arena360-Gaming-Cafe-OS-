//! `staff.deactivate`: an Owner ends a staff member's access and frees their slot under the
//! licence's `users` limit.
//!
//! - **authorize**: only a staff actor holding the built-in Owner role (a Manager with
//!   `user.manage` can list staff but not remove them).
//! - **validate**: the member exists and is active; nobody deactivates themselves; the last
//!   active Owner stays.
//! - **mutate**: the row is kept (history shows the name) with `active = 0`. Hub sign-in ends
//!   at once because every authenticated request re-reads the staff row.

use arena_proto::{ActorKind, AppError, ErrorCode, Intent, StaffDeactivate};
use arena_store::{
    ChangeOp, RowKey, StaffRow, StoreTx, count_active_staff_with_role, deactivate_staff, find_role,
    find_staff,
};

use super::mutate::Written;
use crate::rbac::{ROLE_OWNER, denied, staff_builtin_role};

/// What `staff.deactivate` reads before it decides anything.
#[derive(Debug)]
pub(crate) struct DeactivateTarget {
    row: Option<StaffRow>,
    actor_is_owner: bool,
    /// When the member is an active holder of the built-in Owner role: how many hold it.
    active_owners: Option<i64>,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(
    tx: &StoreTx<'_>,
    intent: &Intent,
    d: &StaffDeactivate,
) -> Result<DeactivateTarget, AppError> {
    let row = find_staff(tx, &d.staff_id)?;
    let actor_is_owner = intent.actor.kind == ActorKind::Staff
        && staff_builtin_role(tx, &intent.actor.id)?.as_deref() == Some(ROLE_OWNER);
    let active_owners = match &row {
        Some(r) if r.active => match find_role(tx, &r.role_id)? {
            Some(role) if role.builtin && role.name == ROLE_OWNER => {
                Some(count_active_staff_with_role(tx, &role.role_id)?)
            }
            _ => None,
        },
        _ => None,
    };
    Ok(DeactivateTarget {
        row,
        actor_is_owner,
        active_owners,
    })
}

pub(super) fn authorize(t: &DeactivateTarget) -> Result<(), AppError> {
    if t.actor_is_owner {
        Ok(())
    } else {
        Err(denied())
    }
}

pub(super) fn validate(
    intent: &Intent,
    d: &StaffDeactivate,
    t: &DeactivateTarget,
) -> Result<(), AppError> {
    let Some(row) = &t.row else {
        return Err(invalid("staff_id", "no such staff member"));
    };
    if row.staff_id == intent.actor.id {
        return Err(invalid("staff_id", "you cannot deactivate yourself"));
    }
    if !row.active {
        return Err(invalid("staff_id", "that staff member is already inactive"));
    }
    if t.active_owners == Some(1) {
        return Err(invalid(
            "staff_id",
            "the branch needs at least one active Owner",
        ));
    }
    debug_assert_eq!(row.staff_id, d.staff_id);
    Ok(())
}

pub(super) fn mutate(tx: &StoreTx<'_>, d: &StaffDeactivate) -> Result<Vec<Written>, AppError> {
    deactivate_staff(tx, &d.staff_id)?;
    Ok(vec![Written {
        key: RowKey::Staff {
            staff_id: d.staff_id.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    }])
}
