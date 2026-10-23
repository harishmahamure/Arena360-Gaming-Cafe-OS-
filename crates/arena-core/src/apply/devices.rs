//! The device intents: `device.register`, `device.revoke` and `device.version_reported`.
//!
//! - **permission**: `device.register` needs `device.manage`, `device.revoke` needs
//!   `device.remote` and `device.version_reported` the system actor (see `rbac`).
//! - **base_rev** is optional on revoke and version_reported: when present it must match the
//!   device row's `rev`, else `STALE`. `device.register` creates a row and takes none.
//! - `device.register` stores the argon2id hash computed by the caller outside the
//!   transaction; the writer mints the `device_id`.

use arena_proto::{
    AppError, ClientClass, DeviceRegister, DeviceRevoke, DeviceVersionReported, ErrorCode, Intent,
};
use arena_store::{
    ChangeOp, DeviceRow, NewDevice, RowKey, StoreTx, current_branch_id, find_device, insert_device,
    set_device_reported_version, set_device_revoked,
};

use super::Target;
use super::mutate::Written;
use super::validate::validate_label;
use crate::devices::is_argon2id_hash;

/// Longest reported version, in bytes.
pub const MAX_VERSION_LEN: usize = 32;

fn invalid(field: &str, message: impl Into<String>) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load_register(tx: &StoreTx<'_>) -> Result<Target, AppError> {
    Ok(Target::NewDevice(current_branch_id(tx)?))
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
    branch: Option<&String>,
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
    validate_label(&d.label).map_err(|m| invalid("label", m))?;
    if !is_argon2id_hash(&d.token_hash) {
        return Err(invalid(
            "token_hash",
            "token_hash must be an argon2id PHC string",
        ));
    }
    if branch.is_none() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "the branch has not been bootstrapped",
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
    branch: Option<&String>,
) -> Result<Vec<Written>, AppError> {
    let branch_id = branch.ok_or_else(|| {
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
    Ok(vec![Written {
        key: RowKey::Devices { device_id },
        op: ChangeOp::Insert,
        before: None,
    }])
}

pub(super) fn mutate_revoke(
    tx: &StoreTx<'_>,
    row: Option<&DeviceRow>,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let row = existing(row)?;
    set_device_revoked(tx, &row.device_id, now_ms)?;
    Ok(updated(&row.device_id))
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
