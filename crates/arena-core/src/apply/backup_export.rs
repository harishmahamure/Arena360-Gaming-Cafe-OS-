//! `backup.export` (Story 9.4): the record that an export of the whole database finished. The
//! Hub applies it only after the `.a360` file is closed, so `backup.last_export` always names a
//! file that exists. It is a record of fact, like `printer.status`: staff do not set it by
//! `settings.set`. The change row carries the acting staff member and, for a Counter, the
//! manager whose PIN approved, so the audit log shows both.

use arena_proto::{
    ActorKind, AppError, BackupExport, EXPORT_HISTORY, ErrorCode, Intent, KEY_BACKUP_LAST_EXPORT,
    is_auto_export_file,
};
use arena_store::{
    ChangeOp, RowKey, SettingRef, StoreTx, find_setting, staff_display_name, update_setting,
};

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
    // A staff member records the export they tapped; the Hub itself records its daily auto
    // export (Story 9.5), and only under an auto file name, so the two never pass for each other.
    let auto = is_auto_export_file(&p.file_name);
    let who_ok = match intent.actor.kind {
        ActorKind::Staff => !auto,
        ActorKind::System => auto && intent.actor.id == arena_proto::AUTO_EXPORT_ACTOR,
        _ => false,
    };
    if !who_ok {
        return Err(invalid(
            "actor",
            "an export is recorded by the staff member who made it, or by the Hub for an auto export",
        ));
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
    intent: &Intent,
    p: &BackupExport,
    row: Option<&SettingRef>,
) -> Result<Vec<Written>, AppError> {
    let row = row.ok_or_else(|| invalid("key", "no such setting"))?;
    // The record keeps the list System → Backup shows: who made this one, then the ones before.
    let by = match intent.actor.kind {
        ActorKind::Staff => Some(staff_display_name(tx, &intent.actor.id)?),
        _ => None,
    };
    let mut recent = BackupExport::parse(&row.value)
        .map(|last| last.entries())
        .unwrap_or_default();
    recent.truncate(EXPORT_HISTORY - 1);
    let record = BackupExport {
        by,
        recent,
        ..p.clone()
    };
    update_setting(tx, &row.branch_id, &row.key, &record.to_json(), None, None)?;
    Ok(vec![Written {
        key: RowKey::Settings {
            branch_id: row.branch_id.clone(),
            key: row.key.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    }])
}
