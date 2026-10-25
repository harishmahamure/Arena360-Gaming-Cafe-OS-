//! `staff.upsert` and `role.upsert` (both need `user.manage`).
//!
//! - **authorize** (after load, before base_rev): a built-in role is never changed
//!   (`PERMISSION_DENIED` "built-in roles cannot be changed"). A *restricted* actor — a staff
//!   actor whose role is not the built-in Owner (a Manager, or a custom role with
//!   `user.manage`) — may only assign the built-in Counter or Kitchen roles, only edit staff who
//!   hold one of them, never edit the role it holds itself, and never write a role that allows
//!   a permission its own role does not allow.
//! - **validate**: the last active staff member holding the built-in Owner role cannot be
//!   deactivated or moved to another role; the built-in Kiosk-device role is never assigned to
//!   a person; role names are unique without case and never a built-in name.
//! - **base_rev** is required on an edit (an id that names an existing row) and must match the
//!   row's `rev`, else `STALE`. A create takes none.
//! - Password and PIN hashes are argon2id PHC strings computed by the caller outside the
//!   transaction. A new PIN bumps `pin_verifier_version`.

use std::collections::HashSet;

use arena_proto::{
    ActorKind, AppError, ErrorCode, Intent, PermEffect, Permission, RoleUpsert, StaffUpsert,
};
use arena_store::{
    ChangeOp, RoleRow, RowKey, StaffRow, StaffWrite, StoreTx, Table, count_active_staff,
    count_active_staff_with_role, current_branch_id, delete_row, find_role, find_role_by_name,
    find_staff, find_staff_by_login, insert_role, insert_role_perm, insert_staff, list_role_perms,
    read_row, set_staff_setup_code, update_role_name, update_staff,
};

use super::mutate::Written;
use super::validate::validate_label;
use crate::apply::is_ulid;
use crate::devices::is_argon2id_hash;
use crate::rbac::{
    BUILTIN_ROLES, ROLE_COUNTER, ROLE_KIOSK_DEVICE, ROLE_KITCHEN, ROLE_OWNER, denied,
    staff_builtin_role,
};

/// Login length bounds, in ASCII characters.
pub const MIN_LOGIN_CHARS: usize = 2;
pub const MAX_LOGIN_CHARS: usize = 32;

/// What a restricted actor (see the module docs) is: `None` for an unrestricted one.
#[derive(Debug)]
pub(crate) struct Restriction {
    /// The role the actor holds.
    role_id: String,
    /// The permissions that role allows.
    allowed: HashSet<String>,
}

fn restriction(tx: &StoreTx<'_>, intent: &Intent) -> Result<Option<Restriction>, AppError> {
    if intent.actor.kind != ActorKind::Staff
        || staff_builtin_role(tx, &intent.actor.id)?.as_deref() == Some(ROLE_OWNER)
    {
        return Ok(None);
    }
    // The permission step already required an active staff row.
    let Some(actor) = find_staff(tx, &intent.actor.id)? else {
        return Err(denied());
    };
    let allowed = list_role_perms(tx, &actor.role_id)?
        .into_iter()
        .filter(|(_, effect)| effect == PermEffect::Allow.as_str())
        .map(|(p, _)| p)
        .collect();
    Ok(Some(Restriction {
        role_id: actor.role_id,
        allowed,
    }))
}

fn invalid(field: &str, message: impl Into<String>) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

fn not_bootstrapped() -> AppError {
    AppError::new(
        ErrorCode::ValidationFailed,
        "the branch has not been bootstrapped",
    )
}

fn check_base_rev(intent: &Intent, current: Option<i64>, what: &str) -> Result<(), AppError> {
    match (current, intent.base_rev) {
        (Some(_), None) => Err(invalid(
            "base_rev",
            format!("editing a {what} requires base_rev"),
        )),
        (Some(rev), Some(base)) if rev != base => Err(AppError::with_field(
            ErrorCode::Stale,
            "base_rev",
            format!("the {what} changed since it was read"),
        )),
        _ => Ok(()),
    }
}

/// What `staff.upsert` reads before it decides anything.
#[derive(Debug)]
pub(crate) struct StaffTarget {
    branch: Option<String>,
    row: Option<StaffRow>,
    role: Option<RoleRow>,
    current_role: Option<RoleRow>,
    /// The acting staff member may only manage Counter and Kitchen staff.
    restricted: bool,
    /// The staff member already holding the requested login, if any.
    login_owner: Option<String>,
    /// A new member would exceed the licence's `users` limit.
    full: bool,
    /// When the edited member is an active holder of the built-in Owner role: how many active
    /// staff hold it.
    active_owners: Option<i64>,
}

pub(super) fn load_staff(
    tx: &StoreTx<'_>,
    intent: &Intent,
    s: &StaffUpsert,
) -> Result<StaffTarget, AppError> {
    let row = match &s.staff_id {
        Some(id) => find_staff(tx, id)?,
        None => None,
    };
    let current_role = match &row {
        Some(r) => find_role(tx, &r.role_id)?,
        None => None,
    };
    let restricted = restriction(tx, intent)?.is_some();
    let active_owners = match (&row, &current_role) {
        (Some(r), Some(role)) if r.active && role.builtin && role.name == ROLE_OWNER => {
            Some(count_active_staff_with_role(tx, &role.role_id)?)
        }
        _ => None,
    };
    let full = match (&row, crate::entitlement::limit(tx, "users")?) {
        (None, Some(cap)) => count_active_staff(tx)? >= cap,
        _ => false,
    };
    let role = match find_role(tx, &s.role_id)? {
        Some(r) => Some(r),
        None => find_role_by_name(tx, &s.role_id)?,
    };
    Ok(StaffTarget {
        branch: current_branch_id(tx)?,
        row,
        role,
        current_role,
        restricted,
        login_owner: find_staff_by_login(tx, s.login.trim())?.map(|r| r.staff_id),
        full,
        active_owners,
    })
}

fn is_assignable_by_restricted(role: &RoleRow) -> bool {
    role.builtin && (role.name == ROLE_COUNTER || role.name == ROLE_KITCHEN)
}

pub(super) fn authorize_staff(t: &StaffTarget) -> Result<(), AppError> {
    if !t.restricted {
        return Ok(());
    }
    if t.role
        .as_ref()
        .is_some_and(|r| !is_assignable_by_restricted(r))
        || t.current_role
            .as_ref()
            .is_some_and(|r| !is_assignable_by_restricted(r))
    {
        return Err(denied());
    }
    Ok(())
}

pub(super) fn base_rev_staff(intent: &Intent, t: &StaffTarget) -> Result<(), AppError> {
    check_base_rev(intent, t.row.as_ref().map(|r| r.rev), "staff member")
}

/// A login: an E.164 phone, or 2–32 ASCII letters, digits, `.`, `_` or `-`, so the database's `NOCASE` and the
/// sign-in lockout's lower-casing agree.
fn validate_login(login: &str) -> Result<(), AppError> {
    let phone = login
        .strip_prefix('+')
        .is_some_and(|d| (8..=15).contains(&d.len()) && d.bytes().all(|c| c.is_ascii_digit()));
    let ok = phone
        || ((MIN_LOGIN_CHARS..=MAX_LOGIN_CHARS).contains(&login.len())
            && login
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')));
    if !ok {
        return Err(invalid(
            "login",
            format!(
                "login must be {MIN_LOGIN_CHARS}–{MAX_LOGIN_CHARS} letters, digits, '.', '_' or '-'"
            ),
        ));
    }
    Ok(())
}

pub(super) fn validate_staff(
    intent: &Intent,
    s: &StaffUpsert,
    t: &StaffTarget,
) -> Result<(), AppError> {
    if let Some(id) = &s.staff_id
        && t.row.is_none()
        && !(s.setup_code_hash.is_some() && is_ulid(id))
    {
        return Err(invalid("staff_id", "no such staff member"));
    }
    if s.staff_id.is_none() && intent.base_rev.is_some() {
        return Err(invalid(
            "base_rev",
            "creating a staff member takes no base_rev",
        ));
    }
    if t.branch.is_none() {
        return Err(not_bootstrapped());
    }
    validate_login(s.login.trim())?;
    validate_label(&s.display_name).map_err(|m| invalid("display_name", m))?;
    let Some(role) = &t.role else {
        return Err(invalid("role_id", "no such role"));
    };
    if role.builtin && role.name == ROLE_KIOSK_DEVICE {
        return Err(invalid(
            "role_id",
            "the Kiosk-device role is for devices only",
        ));
    }
    // The last active Owner stays an active Owner (fields `role_id`/`active` are never
    // recorded: the answer depends on the other staff rows).
    if t.active_owners == Some(1) {
        if t.current_role.as_ref().map(|r| &r.role_id) != Some(&role.role_id) {
            return Err(invalid(
                "role_id",
                "the branch needs at least one active Owner",
            ));
        }
        if !s.active {
            return Err(invalid(
                "active",
                "the branch needs at least one active Owner",
            ));
        }
    }
    match &s.password_hash {
        Some(h) if !is_argon2id_hash(h) => {
            return Err(invalid(
                "password_hash",
                "password_hash must be an argon2id PHC string",
            ));
        }
        None if t.row.is_none() && s.setup_code_hash.is_none() => {
            return Err(invalid(
                "password_hash",
                "a new staff member needs a password",
            ));
        }
        _ => {}
    }
    match (&s.setup_code_hash, s.setup_expires_at) {
        (None, None) => {}
        (Some(h), Some(at)) if is_argon2id_hash(h) && at > 0 => {
            if t.row.as_ref().is_some_and(|r| r.password_hash.is_some()) {
                return Err(invalid(
                    "setup_code_hash",
                    "that member has already chosen a password",
                ));
            }
        }
        _ => {
            return Err(invalid(
                "setup_code_hash",
                "a setup code needs an argon2id hash and an expiry",
            ));
        }
    }
    if s.pin_hash.as_deref().is_some_and(|h| !is_argon2id_hash(h)) {
        return Err(invalid(
            "pin_hash",
            "pin_hash must be an argon2id PHC string",
        ));
    }
    if t.login_owner
        .as_ref()
        .is_some_and(|owner| Some(owner) != s.staff_id.as_ref())
    {
        return Err(AppError::with_field(
            ErrorCode::LoginTaken,
            "login",
            "that login is already in use",
        ));
    }
    if t.full {
        return Err(AppError::new(
            ErrorCode::UserLimit,
            "the staff limit for this branch is reached",
        ));
    }
    Ok(())
}

pub(super) fn mutate_staff(
    tx: &StoreTx<'_>,
    s: &StaffUpsert,
    t: &StaffTarget,
) -> Result<Vec<Written>, AppError> {
    let branch_id = t.branch.as_deref().ok_or_else(not_bootstrapped)?;
    let login = s.login.trim();
    let display_name = s.display_name.trim();
    let role_id = t
        .role
        .as_ref()
        .map_or_else(|| s.role_id.clone(), |r| r.role_id.clone());
    match &t.row {
        None => {
            let staff_id = s
                .staff_id
                .clone()
                .unwrap_or_else(|| ulid::Ulid::generate().to_string());
            insert_staff(
                tx,
                &StaffWrite {
                    staff_id: &staff_id,
                    branch_id,
                    login,
                    display_name,
                    role_id: &role_id,
                    password_hash: s.password_hash.as_deref(),
                    pin_hash: s.pin_hash.as_deref(),
                    pin_verifier_version: 1,
                    active: s.active,
                },
            )?;
            set_code(tx, &staff_id, s)?;
            Ok(vec![Written {
                key: RowKey::Staff { staff_id },
                op: ChangeOp::Insert,
                before: None,
            }])
        }
        Some(row) => {
            let (pin_hash, pin_verifier_version) = match &s.pin_hash {
                Some(new) => (Some(new.as_str()), row.pin_verifier_version + 1),
                None => (row.pin_hash.as_deref(), row.pin_verifier_version),
            };
            update_staff(
                tx,
                &StaffWrite {
                    staff_id: &row.staff_id,
                    branch_id: &row.branch_id,
                    login,
                    display_name,
                    role_id: &role_id,
                    password_hash: s.password_hash.as_deref().or(row.password_hash.as_deref()),
                    pin_hash,
                    pin_verifier_version,
                    active: s.active,
                },
            )?;
            set_code(tx, &row.staff_id, s)?;
            Ok(vec![Written {
                key: RowKey::Staff {
                    staff_id: row.staff_id.clone(),
                },
                op: ChangeOp::Update,
                before: None,
            }])
        }
    }
}

fn set_code(tx: &StoreTx<'_>, staff_id: &str, s: &StaffUpsert) -> Result<(), AppError> {
    match (&s.setup_code_hash, s.setup_expires_at) {
        (Some(h), Some(at)) => set_staff_setup_code(tx, staff_id, h, at),
        _ => Ok(()),
    }
}

/// What `role.upsert` reads before it decides anything.
#[derive(Debug)]
pub(crate) struct RoleTarget {
    branch: Option<String>,
    row: Option<RoleRow>,
    /// The role already holding the requested name, if any.
    name_owner: Option<String>,
    /// The edited role's current `(permission, effect)` rows.
    perms: Vec<(String, String)>,
    restriction: Option<Restriction>,
}

pub(super) fn load_role(
    tx: &StoreTx<'_>,
    intent: &Intent,
    r: &RoleUpsert,
) -> Result<RoleTarget, AppError> {
    let row = match &r.role_id {
        Some(id) => find_role(tx, id)?,
        None => None,
    };
    let perms = match &row {
        Some(row) => list_role_perms(tx, &row.role_id)?,
        None => Vec::new(),
    };
    Ok(RoleTarget {
        branch: current_branch_id(tx)?,
        row,
        name_owner: find_role_by_name(tx, r.name.trim())?.map(|r| r.role_id),
        perms,
        restriction: restriction(tx, intent)?,
    })
}

pub(super) fn authorize_role(r: &RoleUpsert, t: &RoleTarget) -> Result<(), AppError> {
    if t.row.as_ref().is_some_and(|r| r.builtin) {
        return Err(AppError::new(
            ErrorCode::PermissionDenied,
            "built-in roles cannot be changed",
        ));
    }
    if let Some(limit) = &t.restriction {
        let own_role = t
            .row
            .as_ref()
            .is_some_and(|row| row.role_id == limit.role_id);
        // Unknown permission strings are left to validate (`perms[i]`).
        let beyond_own = r.perms.iter().any(|p| {
            p.effect == PermEffect::Allow
                && Permission::parse(&p.permission).is_some()
                && !limit.allowed.contains(&p.permission)
        });
        if own_role || beyond_own {
            return Err(denied());
        }
    }
    Ok(())
}

pub(super) fn base_rev_role(intent: &Intent, t: &RoleTarget) -> Result<(), AppError> {
    check_base_rev(intent, t.row.as_ref().map(|r| r.rev), "role")
}

pub(super) fn validate_role(
    intent: &Intent,
    r: &RoleUpsert,
    t: &RoleTarget,
) -> Result<(), AppError> {
    if r.role_id.is_some() && t.row.is_none() {
        return Err(invalid("role_id", "no such role"));
    }
    if r.role_id.is_none() && intent.base_rev.is_some() {
        return Err(invalid("base_rev", "creating a role takes no base_rev"));
    }
    if t.branch.is_none() {
        return Err(not_bootstrapped());
    }
    validate_label(&r.name).map_err(|m| invalid("name", m))?;
    if BUILTIN_ROLES
        .iter()
        .any(|b| b.eq_ignore_ascii_case(r.name.trim()))
    {
        return Err(invalid("name", "that name belongs to a built-in role"));
    }
    let mut seen = HashSet::new();
    for (i, p) in r.perms.iter().enumerate() {
        let field = format!("perms[{i}]");
        let Some(perm) = Permission::parse(&p.permission) else {
            return Err(invalid(&field, "not a catalogue permission"));
        };
        if !seen.insert(perm) {
            return Err(invalid(&field, "this permission is listed twice"));
        }
    }
    if t.name_owner
        .as_ref()
        .is_some_and(|owner| Some(owner) != r.role_id.as_ref())
    {
        // `name_taken`, not `name`: the answer depends on other rows, so it is never recorded.
        return Err(invalid(
            "name_taken",
            "a role with that name already exists",
        ));
    }
    Ok(())
}

pub(super) fn mutate_role(
    tx: &StoreTx<'_>,
    r: &RoleUpsert,
    t: &RoleTarget,
) -> Result<Vec<Written>, AppError> {
    let branch_id = t.branch.as_deref().ok_or_else(not_bootstrapped)?;
    let name = r.name.trim();
    let mut written = Vec::new();
    let role_id = match &t.row {
        None => {
            let role_id = ulid::Ulid::generate().to_string();
            insert_role(tx, &role_id, branch_id, name, false)?;
            written.push(Written {
                key: RowKey::Roles {
                    role_id: role_id.clone(),
                },
                op: ChangeOp::Insert,
                before: None,
            });
            role_id
        }
        Some(row) => {
            update_role_name(tx, &row.role_id, name)?;
            written.push(Written {
                key: RowKey::Roles {
                    role_id: row.role_id.clone(),
                },
                op: ChangeOp::Update,
                before: None,
            });
            // The permission rows are replaced: each old row is deleted (its change carries the
            // row as it was), then the new set is inserted.
            for (permission, _) in &t.perms {
                let key = RowKey::RolePerms {
                    role_id: row.role_id.clone(),
                    permission: permission.clone(),
                };
                let before = read_row(tx, &key)?;
                delete_row(tx, Table::RolePerms, &before)?;
                written.push(Written {
                    key,
                    op: ChangeOp::Delete,
                    before: Some(before),
                });
            }
            row.role_id.clone()
        }
    };
    for p in &r.perms {
        insert_role_perm(tx, &role_id, &p.permission, p.effect.as_str())?;
        written.push(Written {
            key: RowKey::RolePerms {
                role_id: role_id.clone(),
                permission: p.permission.clone(),
            },
            op: ChangeOp::Insert,
            before: None,
        });
    }
    Ok(written)
}
