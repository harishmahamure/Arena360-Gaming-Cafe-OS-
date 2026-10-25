use arena_proto::{
    ActorKind, AppError, DeviceMaintenance, ErrorCode, Intent, IntentPayload, MAINTENANCE_NOTE_MAX,
};
use arena_store::{ChangeOp, DeviceMetaRow, RowKey, StoreTx, find_device_meta, set_device_state};

use super::mutate::Written;

#[derive(Default)]
pub(crate) struct MaintenanceTarget {
    device: Option<DeviceMetaRow>,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>, p: &IntentPayload) -> Result<MaintenanceTarget, AppError> {
    let IntentPayload::DeviceMaintenance(m) = p else {
        return Ok(MaintenanceTarget::default());
    };
    let device = find_device_meta(tx, &m.device_id)?.filter(|d| d.deleted_at.is_none());
    Ok(MaintenanceTarget { device })
}

pub(super) fn base_rev(intent: &Intent, t: &MaintenanceTarget) -> Result<(), AppError> {
    if intent.actor.kind == ActorKind::Owner {
        return Ok(());
    }
    match (t.device.as_ref(), intent.base_rev) {
        (Some(_), None) => Err(invalid("base_rev", "editing needs base_rev")),
        (Some(d), Some(base)) if d.rev != base => Err(AppError::with_field(
            ErrorCode::Stale,
            "base_rev",
            "this changed since it was read",
        )),
        _ => Ok(()),
    }
}

pub(super) fn validate(m: &DeviceMaintenance, t: &MaintenanceTarget) -> Result<(), AppError> {
    let Some(device) = &t.device else {
        return Err(invalid("device_id", "no such device"));
    };
    if m.on && m.note.as_deref().map_or(0, |n| n.trim().chars().count()) > MAINTENANCE_NOTE_MAX {
        return Err(invalid("note", "the note is too long"));
    }
    if m.on
        && m.note
            .as_deref()
            .is_some_and(|n| n.chars().any(char::is_control))
    {
        return Err(invalid("note", "the note has control characters"));
    }
    if !m.on && device.state != "maintenance" {
        return Err(invalid("device_id", "the device is not in maintenance"));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    m: &DeviceMaintenance,
    t: &MaintenanceTarget,
) -> Result<Vec<Written>, AppError> {
    let Some(d) = &t.device else {
        return Err(invalid("device_id", "no such device"));
    };
    if m.on {
        let note = m.note.as_deref().map(str::trim).filter(|n| !n.is_empty());
        let prior = d
            .prior_state
            .as_deref()
            .or(Some(d.state.as_str()).filter(|s| *s != "maintenance"))
            .unwrap_or("online");
        set_device_state(tx, &d.device_id, "maintenance", Some(prior), note)?;
    } else {
        let back = d.prior_state.as_deref().unwrap_or("online");
        set_device_state(tx, &d.device_id, back, None, None)?;
    }
    Ok(vec![Written {
        key: RowKey::DevicesMeta {
            device_id: d.device_id.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    }])
}
