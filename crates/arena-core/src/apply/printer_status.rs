use arena_proto::{
    ActorKind, AppError, ErrorCode, Intent, KEY_PRINTER_STATUS, PrinterState, PrinterStatus,
    PrinterStatusSet, valid_printer_outcome,
};
use arena_store::{ChangeOp, RowKey, SettingRef, StoreTx, find_setting, update_setting};

use super::mutate::Written;

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>) -> Result<Option<SettingRef>, AppError> {
    find_setting(tx, KEY_PRINTER_STATUS)
}

pub(super) fn validate(
    intent: &Intent,
    p: &PrinterStatusSet,
    row: Option<&SettingRef>,
) -> Result<(), AppError> {
    if intent.actor.kind != ActorKind::Staff {
        return Err(invalid(
            "actor",
            "the printer status is recorded by a staff member",
        ));
    }
    if p.state == PrinterState::Unset {
        return Err(invalid(
            "state",
            "the printer status is ok, failed or skipped",
        ));
    }
    if !valid_printer_outcome(p.state, p.error_kind, p.error.as_deref()) {
        let field = if p.error_kind.is_some() != (p.state == PrinterState::Failed) {
            "error_kind"
        } else {
            "error"
        };
        return Err(invalid(
            field,
            "a failed test print has an error kind and at most 200 characters of detail; other states have neither",
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
    p: &PrinterStatusSet,
    row: Option<&SettingRef>,
) -> Result<Vec<Written>, AppError> {
    let row = row.ok_or_else(|| invalid("key", "no such setting"))?;
    let status = PrinterStatus {
        state: p.state,
        error_kind: p.error_kind,
        error: p.error.clone(),
        at: Some(intent.issued_at),
    };
    update_setting(tx, &row.branch_id, &row.key, &status.to_json(), None, None)?;
    Ok(vec![Written {
        key: RowKey::Settings {
            branch_id: row.branch_id.clone(),
            key: row.key.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    }])
}
