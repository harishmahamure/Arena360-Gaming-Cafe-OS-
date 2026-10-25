//! The device intents: `device.register`, `device.revoke` and `device.version_reported`.
//!
//! - **permission**: `device.register` needs `device.manage`, `device.revoke` needs
//!   `device.remote` and `device.version_reported` the system actor (see `rbac`).
//! - **base_rev** is optional on revoke and version_reported: when present it must match the
//!   device row's `rev`, else `STALE`. `device.register` creates a row and takes none.
//! - `device.register` stores the argon2id hash computed by the caller outside the
//!   transaction; the writer mints the `device_id`. A paired device also gets its
//!   `devices_meta` row and its `display.hc.<device_id>` setting row (`false`) in the same
//!   change. With a `device_id` it edits that device's `devices_meta` row instead (and needs
//!   `base_rev`, checked against that row's `rev`).
//! - A register with an empty `token_hash` is a manual device: a console, VR rig or simulator
//!   with no token, so it can never sign in. A console needs a `tv_number`, unique among live
//!   consoles; `DEVICE_LIMIT` when the cap in `licence.device_cap` is reached (manual devices only;
//!   Story 2.19 extends the cap to pairing). `device.revoke`
//!   also soft-deletes the device's `devices_meta` row, which frees the slot and the TV number.

use arena_proto::{
    ActorKind, AppError, ClientClass, DeviceKind, DeviceRegister, DeviceRevoke,
    DeviceVersionReported, ErrorCode, HC_DEFAULT, Intent, hc_key,
};
use arena_store::{
    ChangeOp, DeviceMetaRow, DeviceRow, GroupRow, NewDevice, RowKey, StoreTx, count_live_devices,
    current_branch_id, find_device, find_device_meta, find_group, insert_device,
    insert_device_meta, insert_setting, read_row, set_device_label, set_device_meta_deleted,
    set_device_reported_version, set_device_revoked, tv_number_taken, update_device_meta,
};

use super::Target;
use super::groups::fits;
use super::mutate::Written;
use super::validate::validate_label;
use crate::devices::is_argon2id_hash;

/// Longest reported version, in bytes.
pub const MAX_VERSION_LEN: usize = 32;

fn invalid(field: &str, message: impl Into<String>) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

fn tv_taken(tx: &StoreTx<'_>, d: &DeviceRegister, except: Option<&str>) -> Result<bool, AppError> {
    match (d.device_kind, d.tv_number) {
        (DeviceKind::Console, Some(tv)) => tv_number_taken(tx, tv, except),
        _ => Ok(false),
    }
}

pub(super) fn load_register(tx: &StoreTx<'_>, d: &DeviceRegister) -> Result<Target, AppError> {
    match &d.device_id {
        None => {
            let cap = crate::entitlement::limit(tx, "devices")?;
            Ok(Target::NewDevice(Box::new(NewDev {
                branch: current_branch_id(tx)?,
                full: match cap {
                    Some(cap) if d.token_hash.is_empty() => count_live_devices(tx)? >= cap,
                    _ => false,
                },
                tv_taken: tv_taken(tx, d, None)?,
                group: match &d.group_id {
                    Some(id) => find_group(tx, id)?.filter(|g| g.deleted_at.is_none()),
                    None => None,
                },
            })))
        }
        Some(id) => Ok(Target::RegisterEdit(Box::new(RegisterEdit {
            device: find_device(tx, id)?,
            meta: find_device_meta(tx, id)?,
            tv_taken: tv_taken(tx, d, Some(id))?,
        }))),
    }
}

#[derive(Default)]
pub(crate) struct NewDev {
    branch: Option<String>,
    full: bool,
    tv_taken: bool,
    group: Option<GroupRow>,
}

pub(crate) struct RegisterEdit {
    pub device: Option<DeviceRow>,
    pub meta: Option<DeviceMetaRow>,
    pub tv_taken: bool,
}

fn validate_tv(
    kind: DeviceKind,
    tv: Option<i64>,
    required: bool,
    taken: bool,
) -> Result<(), AppError> {
    match (kind, tv) {
        (DeviceKind::Console, None) if required => {
            Err(invalid("tv_number", "a console needs a TV number"))
        }
        (DeviceKind::Console, Some(tv)) if !(1..=999).contains(&tv) => {
            Err(invalid("tv_number", "the TV number is 1 to 999"))
        }
        (DeviceKind::Console, Some(_)) if taken => {
            Err(invalid("tv_number", "this TV number is already used"))
        }
        (DeviceKind::Console, _) => Ok(()),
        (_, Some(_)) => Err(invalid("tv_number", "only a console has a TV number")),
        _ => Ok(()),
    }
}

pub(super) fn base_rev_edit(intent: &Intent, t: &RegisterEdit) -> Result<(), AppError> {
    match (intent.base_rev, t.meta.as_ref()) {
        (Some(base), Some(meta)) if base != meta.rev => Err(AppError::with_field(
            ErrorCode::Stale,
            "base_rev",
            "the device changed since it was read",
        )),
        _ => Ok(()),
    }
}

pub(super) fn validate_edit(
    intent: &Intent,
    d: &DeviceRegister,
    t: &RegisterEdit,
) -> Result<(), AppError> {
    if intent.base_rev.is_none() {
        return Err(invalid("base_rev", "editing a device needs base_rev"));
    }
    let device = t
        .device
        .as_ref()
        .filter(|r| r.revoked_at.is_none())
        .ok_or_else(|| invalid("device_id", "no such device"))?;
    if t.meta.as_ref().is_none_or(|m| m.deleted_at.is_some()) {
        return Err(invalid("device_id", "no such device"));
    }
    if device.class != d.class.as_str() {
        return Err(invalid("class", "a device's class cannot change"));
    }
    if d.group_id.is_some() {
        return Err(invalid("group_id", "move a device with device.assign"));
    }
    if t.meta
        .as_ref()
        .is_some_and(|m| m.group_id.is_some() && m.kind != d.device_kind.as_str())
    {
        return Err(AppError::with_field(
            ErrorCode::GroupKindMismatch,
            "kind",
            "move the device out of its group before changing its kind",
        ));
    }
    if device.token_hash.is_empty() && d.device_kind == DeviceKind::Pc {
        return Err(invalid("kind", "a device added by hand cannot be a PC"));
    }
    validate_tv(
        d.device_kind,
        d.tv_number,
        device.token_hash.is_empty(),
        t.tv_taken,
    )?;
    if !d.token_hash.is_empty() {
        return Err(invalid("token_hash", "editing a device keeps its token"));
    }
    validate_label(&d.label).map_err(|m| invalid("label", m))?;
    Ok(())
}

pub(super) fn mutate_edit(
    tx: &StoreTx<'_>,
    d: &DeviceRegister,
    t: &RegisterEdit,
) -> Result<Vec<Written>, AppError> {
    let device = t
        .device
        .as_ref()
        .ok_or_else(|| invalid("device_id", "no such device"))?;
    let label = d.label.trim();
    let meta_key = RowKey::DevicesMeta {
        device_id: device.device_id.clone(),
    };
    let meta_before = read_row(tx, &meta_key)?;
    update_device_meta(
        tx,
        &device.device_id,
        d.device_kind.as_str(),
        label,
        d.tv_number,
    )?;
    let mut written = vec![Written {
        key: meta_key,
        op: ChangeOp::Update,
        before: Some(meta_before),
    }];
    if device.label != label {
        let device_key = RowKey::Devices {
            device_id: device.device_id.clone(),
        };
        let device_before = read_row(tx, &device_key)?;
        set_device_label(tx, &device.device_id, label)?;
        written.push(Written {
            key: device_key,
            op: ChangeOp::Update,
            before: Some(device_before),
        });
    }
    Ok(written)
}

pub(super) fn load_device(tx: &StoreTx<'_>, device_id: &str) -> Result<Target, AppError> {
    Ok(Target::Device(find_device(tx, device_id)?))
}

/// **base_rev** for an existing device row.
pub(super) fn base_rev(intent: &Intent, row: Option<&DeviceRow>) -> Result<(), AppError> {
    match (intent.base_rev, row) {
        (Some(base), Some(row)) if base != row.rev => Err(AppError::with_field(
            ErrorCode::Stale,
            "base_rev",
            "the device changed since it was read",
        )),
        _ => Ok(()),
    }
}

/// Whether `v` is `x.y.z` with numeric parts.
pub(super) fn is_semver(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    v.len() <= MAX_VERSION_LEN
        && parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit()))
}

pub(super) fn validate_register(
    intent: &Intent,
    d: &DeviceRegister,
    t: &NewDev,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "device.register takes no base_rev"));
    }
    if !d.class.uses_device_token() {
        return Err(invalid(
            "class",
            "only paired and kds devices are registered",
        ));
    }
    if d.class != ClientClass::Paired && d.device_kind != DeviceKind::Pc {
        return Err(invalid("kind", "only a paired device has a kind"));
    }
    validate_label(&d.label).map_err(|m| invalid("label", m))?;
    let manual = d.token_hash.is_empty();
    if manual {
        if d.class != ClientClass::Paired
            || d.device_kind == DeviceKind::Pc
            || intent.actor.kind == ActorKind::System
        {
            return Err(invalid(
                "kind",
                "register a console, VR rig or simulator by hand; a PC pairs with a code",
            ));
        }
    } else if !is_argon2id_hash(&d.token_hash) {
        return Err(invalid(
            "token_hash",
            "token_hash must be an argon2id PHC string",
        ));
    }
    validate_tv(d.device_kind, d.tv_number, manual, t.tv_taken)?;
    match (&d.group_id, &t.group) {
        (Some(_), None) => return Err(invalid("group_id", "no such group")),
        (Some(_), Some(g)) if !fits(d.device_kind.as_str(), &g.kind) => {
            return Err(AppError::with_field(
                ErrorCode::GroupKindMismatch,
                "group_id",
                "this device does not fit that kind of group",
            ));
        }
        _ => {}
    }
    if t.branch.is_none() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "the branch has not been bootstrapped",
        ));
    }
    if t.full {
        return Err(AppError::new(
            ErrorCode::DeviceLimit,
            "the device limit for this branch is reached",
        ));
    }
    Ok(())
}

fn existing(row: Option<&DeviceRow>) -> Result<&DeviceRow, AppError> {
    row.ok_or_else(|| invalid("device_id", "no such device"))
}

pub(super) fn validate_revoke(_d: &DeviceRevoke, row: Option<&DeviceRow>) -> Result<(), AppError> {
    if existing(row)?.revoked_at.is_some() {
        return Err(invalid("device_id", "the device is already revoked"));
    }
    Ok(())
}

pub(super) fn validate_version(
    d: &DeviceVersionReported,
    row: Option<&DeviceRow>,
) -> Result<(), AppError> {
    existing(row)?;
    if !is_semver(&d.version) {
        return Err(invalid("version", "version must be x.y.z"));
    }
    Ok(())
}

fn updated(device_id: &str) -> Vec<Written> {
    vec![Written {
        key: RowKey::Devices {
            device_id: device_id.to_owned(),
        },
        op: ChangeOp::Update,
        before: None,
    }]
}

pub(super) fn mutate_register(
    tx: &StoreTx<'_>,
    d: &DeviceRegister,
    t: &NewDev,
) -> Result<Vec<Written>, AppError> {
    let branch_id = t.branch.as_ref().ok_or_else(|| {
        AppError::new(
            ErrorCode::ValidationFailed,
            "the branch has not been bootstrapped",
        )
    })?;
    let device_id = ulid::Ulid::generate().to_string();
    let class = match d.class {
        ClientClass::Paired => "paired",
        ClientClass::Kds => "kds",
        ClientClass::Phone => return Err(invalid("class", "phones are not registered")),
    };
    insert_device(
        tx,
        &NewDevice {
            device_id: &device_id,
            branch_id,
            class,
            label: d.label.trim(),
            token_hash: &d.token_hash,
        },
    )?;
    let mut written = vec![Written {
        key: RowKey::Devices {
            device_id: device_id.clone(),
        },
        op: ChangeOp::Insert,
        before: None,
    }];
    if d.class == ClientClass::Paired {
        insert_device_meta(
            tx,
            &device_id,
            d.device_kind.as_str(),
            d.label.trim(),
            d.group_id.as_deref(),
            d.tv_number,
        )?;
        written.push(Written {
            key: RowKey::DevicesMeta {
                device_id: device_id.clone(),
            },
            op: ChangeOp::Insert,
            before: None,
        });
    }
    // Story 1.21: a paired device starts with high contrast off, in the same change.
    if d.class == ClientClass::Paired && !d.token_hash.is_empty() {
        let key = hc_key(&device_id);
        insert_setting(tx, branch_id, &key, HC_DEFAULT)?;
        written.push(Written {
            key: RowKey::Settings {
                branch_id: branch_id.clone(),
                key,
            },
            op: ChangeOp::Insert,
            before: None,
        });
    }
    Ok(written)
}

pub(super) fn mutate_revoke(
    tx: &StoreTx<'_>,
    row: Option<&DeviceRow>,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let row = existing(row)?;
    set_device_revoked(tx, &row.device_id, now_ms)?;
    let mut written = updated(&row.device_id);
    let meta_key = RowKey::DevicesMeta {
        device_id: row.device_id.clone(),
    };
    if find_device_meta(tx, &row.device_id)?.is_some_and(|m| m.deleted_at.is_none()) {
        let before = read_row(tx, &meta_key)?;
        set_device_meta_deleted(tx, &row.device_id, now_ms)?;
        written.push(Written {
            key: meta_key,
            op: ChangeOp::Update,
            before: Some(before),
        });
    }
    Ok(written)
}

pub(super) fn mutate_version(
    tx: &StoreTx<'_>,
    d: &DeviceVersionReported,
    row: Option<&DeviceRow>,
) -> Result<Vec<Written>, AppError> {
    let row = existing(row)?;
    set_device_reported_version(tx, &row.device_id, &d.version)?;
    Ok(updated(&row.device_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_shapes() {
        for ok in ["0.1.0", "10.20.30", "1.2.3"] {
            assert!(is_semver(ok), "{ok}");
        }
        for bad in [
            "",
            "1.2",
            "1.2.3.4",
            "a.b.c",
            "1..3",
            "1.2.3-beta",
            "1.2.9999999999",
        ] {
            assert!(!is_semver(bad), "{bad}");
        }
    }
}
