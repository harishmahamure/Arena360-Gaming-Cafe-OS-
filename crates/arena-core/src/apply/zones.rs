use arena_proto::{
    ActorKind, AppError, DevicePlace, ErrorCode, FLOOR_COLS, FLOOR_ROWS, Intent, IntentPayload,
    ZoneDelete, ZoneUpsert,
};
use arena_store::{
    ChangeOp, DeviceMetaRow, RowKey, StoreTx, ZoneRow, cell_taken, count_zone_devices,
    find_device_meta, find_zone, find_zone_by_name, insert_zone, set_device_place,
    set_zone_deleted, update_zone,
};

use super::mutate::Written;
use super::validate::validate_label;

#[derive(Default)]
pub(crate) struct ZoneTarget {
    zone: Option<ZoneRow>,
    taken: bool,
    devices: i64,
    device: Option<DeviceMetaRow>,
    cell_taken: bool,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

fn live(zone: Option<ZoneRow>) -> Option<ZoneRow> {
    zone.filter(|z| z.deleted_at.is_none())
}

pub(super) fn load(tx: &StoreTx<'_>, p: &IntentPayload) -> Result<ZoneTarget, AppError> {
    let mut t = ZoneTarget::default();
    match p {
        IntentPayload::ZoneUpsert(z) => {
            if let Some(id) = &z.id {
                t.zone = live(find_zone(tx, id)?);
            }
            t.taken = find_zone_by_name(tx, z.name.trim())?
                .is_some_and(|o| Some(&o.zone_id) != z.id.as_ref());
        }
        IntentPayload::ZoneDelete(z) => {
            t.zone = live(find_zone(tx, &z.id)?);
            if let Some(z) = &t.zone {
                t.devices = count_zone_devices(tx, &z.zone_id)?;
            }
        }
        IntentPayload::DevicePlace(d) => {
            t.device = find_device_meta(tx, &d.device_id)?.filter(|m| m.deleted_at.is_none());
            if let Some(id) = &d.zone_id {
                t.zone = live(find_zone(tx, id)?);
            }
            if let (Some(z), Some(x), Some(y)) = (&t.zone, d.floor_x, d.floor_y) {
                t.cell_taken = cell_taken(tx, &z.zone_id, x, y, &d.device_id)?;
            }
        }
        _ => {}
    }
    Ok(t)
}

pub(super) fn base_rev(intent: &Intent, t: &ZoneTarget) -> Result<(), AppError> {
    let current = match &intent.payload {
        IntentPayload::DevicePlace(_) => t.device.as_ref().map(|d| d.rev),
        _ => t.zone.as_ref().map(|z| z.rev),
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

pub(super) fn validate_upsert(z: &ZoneUpsert, t: &ZoneTarget) -> Result<(), AppError> {
    validate_label(&z.name)
        .map_err(|m| AppError::with_field(ErrorCode::ValidationFailed, "name", m))?;
    if t.taken {
        return Err(invalid("name", "a zone with this name exists"));
    }
    match (&z.id, &t.zone) {
        (Some(_), None) => Err(invalid("id", "no such zone")),
        _ => Ok(()),
    }
}

pub(super) fn validate_delete(t: &ZoneTarget) -> Result<(), AppError> {
    if t.zone.is_none() {
        return Err(invalid("id", "no such zone"));
    }
    if t.devices > 0 {
        return Err(AppError::new(
            ErrorCode::ZoneNotEmpty,
            "move the devices out of this zone first",
        ));
    }
    Ok(())
}

pub(super) fn validate_place(d: &DevicePlace, t: &ZoneTarget) -> Result<(), AppError> {
    if t.device.is_none() {
        return Err(invalid("device_id", "no such device"));
    }
    match (&d.zone_id, d.floor_x, d.floor_y) {
        (None, None, None) => Ok(()),
        (None, _, _) => Err(invalid("zone_id", "a cell needs a zone")),
        (Some(_), None, _) => Err(invalid("floor_x", "choose a column")),
        (Some(_), _, None) => Err(invalid("floor_y", "choose a row")),
        (Some(_), Some(x), Some(y)) => {
            if t.zone.is_none() {
                return Err(invalid("zone_id", "no such zone"));
            }
            if !(0..FLOOR_COLS).contains(&x) {
                return Err(invalid("floor_x", "column is off the grid"));
            }
            if !(0..FLOOR_ROWS).contains(&y) {
                return Err(invalid("floor_y", "row is off the grid"));
            }
            if t.cell_taken {
                return Err(AppError::with_field(
                    ErrorCode::CellTaken,
                    "floor_x",
                    "another device is in that cell",
                ));
            }
            Ok(())
        }
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
    z: &ZoneUpsert,
    t: &ZoneTarget,
) -> Result<Vec<Written>, AppError> {
    let name = z.name.trim();
    let (id, op) = match &t.zone {
        Some(old) => {
            update_zone(tx, &old.zone_id, name, z.sort_order)?;
            (old.zone_id.clone(), ChangeOp::Update)
        }
        None => {
            let id = ulid::Ulid::generate().to_string();
            insert_zone(tx, &id, name, z.sort_order)?;
            (id, ChangeOp::Insert)
        }
    };
    Ok(written(RowKey::FloorZones { zone_id: id }, op))
}

pub(super) fn mutate_delete(
    tx: &StoreTx<'_>,
    z: &ZoneDelete,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    set_zone_deleted(tx, &z.id, now_ms)?;
    Ok(written(
        RowKey::FloorZones {
            zone_id: z.id.clone(),
        },
        ChangeOp::Update,
    ))
}

pub(super) fn mutate_place(tx: &StoreTx<'_>, d: &DevicePlace) -> Result<Vec<Written>, AppError> {
    set_device_place(tx, &d.device_id, d.zone_id.as_deref(), d.floor_x, d.floor_y)?;
    Ok(written(
        RowKey::DevicesMeta {
            device_id: d.device_id.clone(),
        },
        ChangeOp::Update,
    ))
}
