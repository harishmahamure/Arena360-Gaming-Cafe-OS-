//! `settings.set`: the first edit intent. It sets one existing setting's value, guarded by
//! `base_rev` (optimistic row version). The row is read once ([`load`]) and handed to the
//! base_rev, validate and mutate steps.
//!
//! `branch.tz` and `branch.day_start_min` move the business day itself, so a change saved during
//! business day D waits for D+1: the old value is kept in `prior_value` and the new one applies
//! from `effective_from` = D+1. A second change while one is waiting keeps the original
//! `prior_value` and replaces the new value (setting it back to `prior_value` cancels the
//! wait). Every other key applies at once.

use arena_proto::{AppError, ErrorCode, Intent, SettingsSet};
use arena_store::{ChangeOp, RowKey, SettingRef, StoreTx, find_setting, update_setting};
use chrono::NaiveDate;

use super::mutate::Written;
use crate::locale;
use crate::settings_defaults::{DEFERRED_KEYS, SettingRule, setting_rule};

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
/// rule and length limit (the global [`MAX_SETTING_VALUE_CHARS`], or the larger one of a kiosk
/// process list, whose own error names the bad rule).
pub(super) fn validate(s: &SettingsSet, row: Option<&SettingRef>) -> Result<(), AppError> {
    let rule = match (row, setting_rule(&s.key)) {
        (Some(_), Some(rule)) => rule,
        _ => return Err(invalid("key", "no such setting")),
    };
    let max = rule.max_chars();
    if s.value.chars().count() > max {
        return Err(invalid(
            "value",
            format!("value must be at most {max} characters"),
        ));
    }
    if let SettingRule::ProcessRules(list) = rule {
        return arena_proto::parse_process_rules(list, &s.value).map(|_| ());
    }
    if !rule.accepts(&s.value) {
        return Err(invalid(
            "value",
            format!("value is not valid for {}", s.key),
        ));
    }
    Ok(())
}

/// **mutate.** Writes the new value (deferred to the next business day for a tz or day-start
/// change); stamp bumps `rev` by 1 and sets `updated_at`.
pub(super) fn mutate(
    tx: &StoreTx<'_>,
    s: &SettingsSet,
    row: Option<&SettingRef>,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let row = row.ok_or_else(|| invalid("key", "no such setting"))?;
    let (prior, from) = if DEFERRED_KEYS.contains(&row.key.as_str()) {
        deferral(tx, row, &s.value, now_ms)?
    } else {
        (None, None)
    };
    update_setting(
        tx,
        &row.branch_id,
        &row.key,
        &s.value,
        prior.as_deref(),
        from.map(|d| d.to_string()).as_deref(),
    )?;
    Ok(vec![Written {
        key: RowKey::Settings {
            branch_id: row.branch_id.clone(),
            key: row.key.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    }])
}

/// The `(prior_value, effective_from)` a tz or day-start change gets, worked out under the rules
/// in effect now (before the write).
fn deferral(
    tx: &StoreTx<'_>,
    row: &SettingRef,
    value: &str,
    now_ms: i64,
) -> Result<(Option<String>, Option<NaiveDate>), AppError> {
    let Some(rows) = locale::read_rows(tx)? else {
        return Ok((None, None));
    };
    let (today, _) = locale::resolve(&rows, now_ms);
    let waiting = match (&row.prior_value, row.effective_from.as_deref()) {
        (Some(prior), Some(from)) => NaiveDate::parse_from_str(from, "%Y-%m-%d")
            .ok()
            .filter(|from| *from > today)
            .map(|from| (prior.clone(), from)),
        _ => None,
    };
    Ok(match waiting {
        // Back to the value in effect: nothing is waiting any more.
        Some((prior, _)) if prior == value => (None, None),
        // A change is already waiting: keep its prior value and day, replace the new value.
        Some((prior, from)) => (Some(prior), Some(from)),
        // The value in effect today is the stored one.
        None if row.value == value => (None, None),
        None => (Some(row.value.clone()), today.succ_opt()),
    })
}
