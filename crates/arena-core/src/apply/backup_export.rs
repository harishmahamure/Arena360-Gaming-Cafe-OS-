//! `backup.export` (Story 9.4): the record that an export of the whole database finished. The
//! Hub applies it only after the `.a360` file is closed, so `backup.last_export` always names a
//! file that exists. It is a record of fact, like `printer.status`: staff do not set it by
//! `settings.set`. The change row carries the acting staff member and, for a Counter, the
//! manager whose PIN approved, so the audit log shows both.

use arena_proto::{ActorKind, AppError, BackupExport, ErrorCode, Intent, KEY_BACKUP_LAST_EXPORT};
use arena_store::{ChangeOp, RowKey, SettingRef, StoreTx, find_setting, update_setting};

use super::mutate::Written;

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>) -> Result<Option<SettingRef>, AppError> {
    find_setting(tx, KEY_BACKUP_LAST_EXPORT)
}

pub(super) fn validate(
    intent: &Intent,
    p: &BackupExport,
    row: Option<&SettingRef>,
) -> Result<(), AppError> {
    if intent.actor.kind != ActorKind::Staff {
        return Err(invalid("actor", "an export is recorded by a staff member"));
    }
    if !p.is_consistent() {
        return Err(invalid(
            "file_name",
            "the export has a plain .a360 file name, a size, a sequence and a time",
        ));
    }
    if row.is_none() {
        return Err(invalid("key", "no such setting"));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    p: &BackupExport,
    row: Option<&SettingRef>,
) -> Result<Vec<Written>, AppError> {
    let row = row.ok_or_else(|| invalid("key", "no such setting"))?;
    update_setting(tx, &row.branch_id, &row.key, &p.to_json(), None, None)?;
    Ok(vec![Written {
        key: RowKey::Settings {
            branch_id: row.branch_id.clone(),
            key: row.key.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    }])
}
