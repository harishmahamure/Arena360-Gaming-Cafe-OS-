//! `group.upsert`, `group.delete` and `device.assign` (all need `device.manage`).
//!
//! - **base_rev** is required on an edit and must match the edited row's `rev` (the group's, or
//!   the device's `devices_meta` row for `device.assign`), else `STALE`. An owner actor
//!   (a setup command) may omit it or send an old one: the last write wins.
//! - A group's name is unique among live groups without case; a deleted group keeps its row
//!   (`deleted_at`) and frees its name. A group with devices cannot be deleted
//!   (`GROUP_NOT_EMPTY`) or change kind.
//! - A device joins only a group of its kind (`GROUP_KIND_MISMATCH`): pc to pc, console to
//!   console, vr to vr, sim to other.

use arena_proto::{
    ActorKind, AppError, DeviceAssign, ErrorCode, GROUP_COLOURS, GROUP_KINDS, GroupDelete,
    GroupUpsert, Intent, IntentPayload,
};
use arena_store::{
    ChangeOp, DeviceMetaRow, GroupRow, RowKey, StoreTx, count_group_devices, find_device_meta,
    find_group, find_group_by_name, insert_group, set_device_group, set_group_deleted,
    update_group,
};

use super::mutate::Written;
use super::validate::validate_label;

#[derive(Default)]
pub(crate) struct GroupTarget {
    group: Option<GroupRow>,
    taken: bool,
    devices: i64,
    device: Option<DeviceMetaRow>,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

fn live(group: Option<GroupRow>) -> Option<GroupRow> {
    group.filter(|g| g.deleted_at.is_none())
}

pub(super) fn load(tx: &StoreTx<'_>, p: &IntentPayload) -> Result<GroupTarget, AppError> {
    let mut t = GroupTarget::default();
    match p {
        IntentPayload::GroupUpsert(g) => {
            if let Some(id) = &g.id {
                t.group = live(find_group(tx, id)?);
            }
            t.taken = find_group_by_name(tx, g.name.trim())?
                .is_some_and(|o| Some(&o.group_id) != g.id.as_ref());
        }
        IntentPayload::GroupDelete(g) => t.group = live(find_group(tx, &g.id)?),
        IntentPayload::DeviceAssign(a) => {
            t.device = find_device_meta(tx, &a.device_id)?.filter(|m| m.deleted_at.is_none());
            if let Some(id) = &a.group_id {
                t.group = live(find_group(tx, id)?);
            }
        }
        _ => {}
    }
    if !matches!(p, IntentPayload::DeviceAssign(_))
        && let Some(g) = &t.group
    {
        t.devices = count_group_devices(tx, &g.group_id)?;
    }
    Ok(t)
}

pub(super) fn base_rev(intent: &Intent, t: &GroupTarget) -> Result<(), AppError> {
    let current = match &intent.payload {
        IntentPayload::DeviceAssign(_) => t.device.as_ref().map(|d| d.rev),
        _ => t.group.as_ref().map(|g| g.rev),
    };
    match (current, intent.base_rev) {
        (Some(_), None) if intent.actor.kind != ActorKind::Owner => {
            Err(invalid("base_rev", "editing needs base_rev"))
        }
        (Some(rev), Some(base)) if rev != base && intent.actor.kind != ActorKind::Owner => {
            Err(AppError::with_field(
                ErrorCode::Stale,
                "base_rev",
                "this changed since it was read",
            ))
        }
        _ => Ok(()),
    }
}

pub(super) fn validate_upsert(g: &GroupUpsert, t: &GroupTarget) -> Result<(), AppError> {
    validate_label(&g.name)
        .map_err(|m| AppError::with_field(ErrorCode::ValidationFailed, "name", m))?;
    if t.taken {
        return Err(invalid("name", "a group with this name exists"));
    }
    if !GROUP_KINDS.contains(&g.group_kind.as_str()) {
        return Err(invalid("kind", "kind must be pc, console, vr or other"));
    }
    if !GROUP_COLOURS.contains(&g.colour.as_str()) {
        return Err(invalid("colour", "colour must be one of the six swatches"));
    }
    match (&g.id, &t.group) {
        (Some(_), None) => Err(invalid("id", "no such group")),
        (Some(_), Some(old)) if old.kind != g.group_kind && t.devices > 0 => {
            Err(invalid("kind", "a group with devices keeps its kind"))
        }
        _ => Ok(()),
    }
}

/// The `group.upsert` rules for a new group of a setup file (Story 5.36); the caller has already
/// checked that no live group has its name.
pub(crate) fn check_import(g: &GroupUpsert) -> Result<(), AppError> {
    validate_upsert(g, &GroupTarget::default())
}

pub(super) fn validate_delete(t: &GroupTarget) -> Result<(), AppError> {
    if t.group.is_none() {
        return Err(invalid("id", "no such group"));
    }
    if t.devices > 0 {
        return Err(AppError::new(
            ErrorCode::GroupNotEmpty,
            "move the devices out of this group first",
        ));
    }
    Ok(())
}

pub(super) fn fits(device: &str, group: &str) -> bool {
    matches!(
        (device, group),
        ("pc", "pc") | ("console", "console") | ("vr", "vr") | ("sim", "other")
    )
}

pub(super) fn validate_assign(a: &DeviceAssign, t: &GroupTarget) -> Result<(), AppError> {
    let device = t
        .device
        .as_ref()
        .ok_or_else(|| invalid("device_id", "no such device"))?;
    match (&a.group_id, &t.group) {
        (Some(_), None) => Err(invalid("group_id", "no such group")),
        (Some(_), Some(g)) if !fits(&device.kind, &g.kind) => Err(AppError::with_field(
            ErrorCode::GroupKindMismatch,
            "group_id",
            "this device does not fit that kind of group",
        )),
        _ => Ok(()),
    }
}

fn written(key: RowKey, op: ChangeOp) -> Vec<Written> {
    vec![Written {
        key,
        op,
        before: None,
    }]
}

pub(super) fn mutate_upsert(
    tx: &StoreTx<'_>,
    g: &GroupUpsert,
    t: &GroupTarget,
) -> Result<Vec<Written>, AppError> {
    let name = g.name.trim();
    let (id, op) = match &t.group {
        Some(old) => {
            update_group(
                tx,
                &old.group_id,
                name,
                &g.group_kind,
                &g.colour,
                g.sort_order,
            )?;
            (old.group_id.clone(), ChangeOp::Update)
        }
        None => {
            let id = ulid::Ulid::generate().to_string();
            insert_group(tx, &id, name, &g.group_kind, &g.colour, g.sort_order)?;
            (id, ChangeOp::Insert)
        }
    };
    Ok(written(RowKey::DeviceGroups { group_id: id }, op))
}

pub(super) fn mutate_delete(
    tx: &StoreTx<'_>,
    g: &GroupDelete,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    set_group_deleted(tx, &g.id, now_ms)?;
    Ok(written(
        RowKey::DeviceGroups {
            group_id: g.id.clone(),
        },
        ChangeOp::Update,
    ))
}

pub(super) fn mutate_assign(tx: &StoreTx<'_>, a: &DeviceAssign) -> Result<Vec<Written>, AppError> {
    set_device_group(tx, &a.device_id, a.group_id.as_deref())?;
    Ok(written(
        RowKey::DevicesMeta {
            device_id: a.device_id.clone(),
        },
        ChangeOp::Update,
    ))
}
