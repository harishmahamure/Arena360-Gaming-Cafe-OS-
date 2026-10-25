use arena_proto::{AppError, ErrorCode, Intent, LicenceApply};
use arena_store::{ChangeOp, RowKey, StoreTx, find_setting, insert_setting, update_setting};

use super::mutate::Written;

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(crate) fn pending(tx: &StoreTx<'_>, p: &LicenceApply) -> Result<bool, AppError> {
    for (key, value) in p.rows() {
        if find_setting(tx, key)?.is_none_or(|s| s.value != value) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn validate(
    intent: &Intent,
    p: &LicenceApply,
    branch: Option<&String>,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "licence.apply takes no base_rev"));
    }
    if branch.is_none() {
        return Err(invalid(
            "branch",
            "there is no branch to apply a licence to",
        ));
    }
    if !p.flags_valid() {
        return Err(invalid(
            "flags",
            "flags are lower-case names joined by commas",
        ));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    p: &LicenceApply,
    branch: Option<&String>,
) -> Result<Vec<Written>, AppError> {
    let branch = branch.ok_or_else(|| invalid("branch", "there is no branch"))?;
    let mut written = Vec::new();
    for (key, value) in p.rows() {
        let op = match find_setting(tx, key)? {
            Some(s) if s.value == value => continue,
            Some(s) => {
                update_setting(tx, &s.branch_id, key, &value, None, None)?;
                ChangeOp::Update
            }
            None => {
                insert_setting(tx, branch, key, &value)?;
                ChangeOp::Insert
            }
        };
        written.push(Written {
            key: RowKey::Settings {
                branch_id: branch.clone(),
                key: key.to_owned(),
            },
            op,
            before: None,
        });
    }
    Ok(written)
}
