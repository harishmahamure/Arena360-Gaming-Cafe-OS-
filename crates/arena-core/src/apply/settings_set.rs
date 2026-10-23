//! `settings.set`: the first edit intent. It sets one existing setting's value, guarded by
//! `base_rev` (optimistic row version). The row is read once ([`load`]) and handed to the
//! base_rev, validate and mutate steps.

use arena_proto::{AppError, ErrorCode, Intent, SettingsSet};
use arena_store::{ChangeOp, RowKey, SettingRef, StoreTx, find_setting, update_setting_value};

use super::mutate::Written;
use crate::settings_defaults::setting_rule;

/// Longest value, in characters.
pub const MAX_SETTING_VALUE_CHARS: usize = 4096;

fn invalid(field: &str, message: impl Into<String>) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

/// Reads the target row (`None` if the branch has no such setting).
pub(super) fn load(tx: &StoreTx<'_>, s: &SettingsSet) -> Result<Option<SettingRef>, AppError> {
    find_setting(tx, &s.key)
}

/// **base_rev.** `base_rev` is required (`VALIDATION_FAILED` on `base_rev` when missing) and
/// must equal the row's `rev`, else `STALE`. A missing row is left to validate (`key`).
pub(super) fn base_rev(intent: &Intent, row: Option<&SettingRef>) -> Result<(), AppError> {
    let Some(base_rev) = intent.base_rev else {
        return Err(invalid("base_rev", "settings.set requires base_rev"));
    };
    match row {
        Some(row) if row.rev != base_rev => Err(AppError::with_field(
            ErrorCode::Stale,
            "base_rev",
            "the setting changed since it was read",
        )),
        _ => Ok(()),
    }
}

/// **validate.** The key must name an existing, known setting; the value must fit the key's
/// rule and the global length limit.
pub(super) fn validate(s: &SettingsSet, row: Option<&SettingRef>) -> Result<(), AppError> {
    let rule = match (row, setting_rule(&s.key)) {
        (Some(_), Some(rule)) => rule,
        _ => return Err(invalid("key", "no such setting")),
    };
    if s.value.chars().count() > MAX_SETTING_VALUE_CHARS {
        return Err(invalid(
            "value",
            format!("value must be at most {MAX_SETTING_VALUE_CHARS} characters"),
        ));
    }
    if !rule.accepts(&s.value) {
        return Err(invalid(
            "value",
            format!("value is not valid for {}", s.key),
        ));
    }
    Ok(())
}

/// **mutate.** Writes the new value; stamp bumps `rev` by 1 and sets `updated_at`.
pub(super) fn mutate(
    tx: &StoreTx<'_>,
    s: &SettingsSet,
    row: Option<&SettingRef>,
) -> Result<Vec<Written>, AppError> {
    let row = row.ok_or_else(|| invalid("key", "no such setting"))?;
    update_setting_value(tx, &row.branch_id, &row.key, &s.value)?;
    Ok(vec![Written {
        key: RowKey::Settings {
            branch_id: row.branch_id.clone(),
            key: row.key.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    }])
}
