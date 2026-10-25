use arena_proto::{AppError, ErrorCode, StaffSetCredentials};
use arena_store::{ChangeOp, RowKey, StaffRow, StoreTx, find_staff, set_staff_credentials};

use super::mutate::Written;
use crate::devices::is_argon2id_hash;
use crate::staff::setup_code_invalid;

fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

pub(super) fn load(
    tx: &StoreTx<'_>,
    c: &StaffSetCredentials,
) -> Result<Option<StaffRow>, AppError> {
    find_staff(tx, &c.staff_id)
}

pub(super) fn validate(c: &StaffSetCredentials, row: Option<&StaffRow>) -> Result<(), AppError> {
    let usable = row.is_some_and(|r| {
        r.active
            && r.password_hash.is_none()
            && r.setup_code_hash
                .as_deref()
                .is_some_and(|h| same(h, &c.setup_code_hash))
            && r.setup_expires_at.is_some_and(|e| e > c.trusted_now_ms)
    });
    if !usable {
        return Err(setup_code_invalid());
    }
    for (field, hash) in [
        ("password_hash", &c.password_hash),
        ("pin_hash", &c.pin_hash),
    ] {
        if !is_argon2id_hash(hash) {
            return Err(AppError::with_field(
                ErrorCode::ValidationFailed,
                field,
                format!("{field} must be an argon2id PHC string"),
            ));
        }
    }
    Ok(())
}

pub(super) fn mutate(tx: &StoreTx<'_>, c: &StaffSetCredentials) -> Result<Vec<Written>, AppError> {
    set_staff_credentials(tx, &c.staff_id, &c.password_hash, &c.pin_hash)?;
    Ok(vec![Written {
        key: RowKey::Staff {
            staff_id: c.staff_id.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    }])
}
