use arena_proto::{AppError, ErrorCode, KEY_BRANCH_CODE_LOCKED_AT};
use arena_store::{RowKey, StoreTx, find_setting, update_setting};

pub fn lock_branch_code(tx: &StoreTx<'_>, now_ms: i64) -> Result<Option<RowKey>, AppError> {
    let Some(row) = find_setting(tx, KEY_BRANCH_CODE_LOCKED_AT)? else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "the branch has no code lock to set",
        ));
    };
    if !row.value.is_empty() {
        return Ok(None);
    }
    update_setting(
        tx,
        &row.branch_id,
        &row.key,
        &now_ms.to_string(),
        None,
        None,
    )?;
    Ok(Some(RowKey::Settings {
        branch_id: row.branch_id,
        key: row.key,
    }))
}

pub(super) fn ensure_unlocked(tx: &StoreTx<'_>) -> Result<(), AppError> {
    if find_setting(tx, KEY_BRANCH_CODE_LOCKED_AT)?.is_some_and(|s| !s.value.is_empty()) {
        return Err(AppError::new(
            ErrorCode::BranchCodeLocked,
            "the branch code can't change after the first invoice",
        ));
    }
    Ok(())
}
