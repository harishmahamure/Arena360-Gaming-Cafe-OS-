//! `ops.restore` (Story 9.7): the record that a branch was restored from an export file as a new
//! epoch. The Hub submits it once, right after the start-up swap, as the Owner who confirmed the
//! restore (or as itself when that Owner is not in the restored database). It is a record of
//! fact like `backup.export`: it stores `backup.last_restore`, and the change row is the audit
//! entry, carrying the file header, the old and new epoch and the rows set aside.

use arena_proto::{
    ActorKind, AppError, ErrorCode, Intent, KEY_BACKUP_LAST_RESTORE, OpsRestore,
    RESTORE_SYSTEM_ACTOR,
};
use arena_store::{
    ChangeOp, RowKey, SettingRef, StoreTx, find_setting, staff_display_name, update_setting,
};

use super::mutate::Written;

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>) -> Result<Option<SettingRef>, AppError> {
    find_setting(tx, KEY_BACKUP_LAST_RESTORE)
}

pub(super) fn validate(
    intent: &Intent,
    p: &OpsRestore,
    row: Option<&SettingRef>,
) -> Result<(), AppError> {
    let who_ok = match intent.actor.kind {
        ActorKind::Staff => true,
        ActorKind::System => intent.actor.id == RESTORE_SYSTEM_ACTOR,
        _ => false,
    };
    if !who_ok {
        return Err(invalid(
            "actor",
            "a restore is recorded by the Owner who confirmed it, or by the Hub",
        ));
    }
    if !p.is_consistent() {
        return Err(invalid(
            "header",
            "the restore names a plain file, both epochs and a count of rows set aside",
        ));
    }
    let Some(row) = row else {
        return Err(invalid("key", "no such setting"));
    };
    if row.branch_id != p.header.branch_id {
        return Err(invalid("header", "the file is of another branch"));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    p: &OpsRestore,
    row: Option<&SettingRef>,
) -> Result<Vec<Written>, AppError> {
    let row = row.ok_or_else(|| invalid("key", "no such setting"))?;
    let by = match intent.actor.kind {
        ActorKind::Staff => Some(staff_display_name(tx, &intent.actor.id)?),
        _ => None,
    };
    let record = OpsRestore { by, ..p.clone() };
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
