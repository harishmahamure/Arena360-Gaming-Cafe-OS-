//! `settings.set`: the first edit intent. It sets one existing setting's value, guarded by
//! `base_rev` (optimistic row version). The row is read once ([`load`]) and handed to the
//! base_rev, validate and mutate steps.
//!
//! `branch.tz` and `branch.day_start_min` move the business day itself, so a change saved during
//! business day D waits for D+1: the old value is kept in `prior_value` and the new one applies
//! from `effective_from` = D+1. A second change while one is waiting keeps the original
//! `prior_value` and replaces the new value (setting it back to `prior_value` cancels the
//! wait). Every other key applies at once.

use arena_proto::{
    AppError, ErrorCode, Intent, KEY_KITCHEN_KOT_PRINTER, KEY_OPENING_HOURS, KEY_OPENING_HOURS_SET,
    KEY_PRINTER_PAPER_MM, KEY_PRINTER_SELECTED, KEY_PRINTER_STATUS, KEY_TAX, PrinterSelected,
    PrinterStatus, SettingsSet, TaxSettings,
};
use arena_store::{ChangeOp, RowKey, SettingRef, StoreTx, find_setting, update_setting};
use chrono::NaiveDate;

use super::mutate::Written;
use crate::locale;
use crate::settings_defaults::{
    DEFERRED_KEYS, KEYED_FIELD_KEYS, SYSTEM_MANAGED_KEYS, SettingRule, setting_rule,
};

/// Longest value, in characters.
pub const MAX_SETTING_VALUE_CHARS: usize = 4096;

fn invalid(field: &str, message: impl Into<String>) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

/// Reads the target row (`None` if the branch has no such setting).
pub(super) fn load(tx: &StoreTx<'_>, s: &SettingsSet) -> Result<Option<SettingRef>, AppError> {
    if s.key == arena_proto::KEY_BRANCH_CODE {
        super::branch_code::ensure_unlocked(tx)?;
    }
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
    if SYSTEM_MANAGED_KEYS.contains(&s.key.as_str()) {
        return Err(invalid("key", "this setting is not set by settings.set"));
    }
    let field = if KEYED_FIELD_KEYS.contains(&s.key.as_str()) {
        s.key.as_str()
    } else {
        "value"
    };
    let max = rule.max_chars();
    if s.value.chars().count() > max {
        return Err(invalid(
            field,
            format!("value must be at most {max} characters"),
        ));
    }
    match rule {
        SettingRule::ProcessRules(list) => {
            return arena_proto::parse_process_rules(list, &s.value).map(|_| ());
        }
        SettingRule::OpeningHours => {
            let week = arena_proto::parse_opening_hours(&s.value)
                .map_err(|e| invalid(&e.field(), e.message()))?;
            if !arena_proto::has_open_day(&week) {
                return Err(invalid(KEY_OPENING_HOURS, "at least one day must be open"));
            }
            return Ok(());
        }
        SettingRule::Path if !rule.accepts(&s.value) => {
            return Err(invalid(
                field,
                arena_proto::BackupDirProblem::NotFound.as_str(),
            ));
        }
        SettingRule::KioskLogo => {
            return arena_proto::logo_problem(&s.value).map_or(Ok(()), |m| Err(invalid(field, m)));
        }
        SettingRule::Tax => {
            let tax = TaxSettings::parse(&s.value)
                .ok_or_else(|| invalid(field, "tax settings are malformed"))?;
            return tax.check().map_err(|(f, m)| invalid(f, m));
        }
        SettingRule::WarnMarks => {
            return arena_proto::parse_warn_marks(&s.value)
                .map(|_| ())
                .map_err(|p| {
                    invalid(
                        field,
                        match p {
                            arena_proto::WarnMarksProblem::Shape => {
                                "give one to three whole minutes separated by commas"
                            }
                            arena_proto::WarnMarksProblem::Range => {
                                "each mark must be from 1 to 30 minutes"
                            }
                            arena_proto::WarnMarksProblem::Duplicate => {
                                "each mark must be different"
                            }
                        },
                    )
                });
        }
        SettingRule::BoosterPrograms => {
            return arena_proto::parse_booster_programs(&s.value)
                .map(|_| ())
                .map_err(|p| invalid(field, p.message()));
        }
        SettingRule::DisplayModes => {
            return arena_proto::parse_display_modes(&s.value)
                .map(|_| ())
                .map_err(|p| invalid(field, p.message()));
        }
        SettingRule::PrinterSelected => {
            return PrinterSelected::parse(&s.value)
                .map(|_| ())
                .map_err(|e| invalid(e.field(), e.message()));
        }
        _ => {}
    }
    if !rule.accepts(&s.value) {
        return Err(invalid(field, format!("value is not valid for {}", s.key)));
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
    let opening_hours = (row.key == KEY_OPENING_HOURS)
        .then(|| arena_proto::parse_opening_hours(&s.value).ok())
        .flatten();
    let printer = (row.key == KEY_PRINTER_SELECTED || row.key == KEY_KITCHEN_KOT_PRINTER)
        .then(|| PrinterSelected::parse(&s.value).ok().flatten())
        .flatten();
    let tax = (row.key == KEY_TAX)
        .then(|| TaxSettings::parse(&s.value))
        .flatten();
    // The marks are stored longest first, one canonical string whoever wrote them.
    let marks = (row.key == arena_proto::KEY_BILLING_WARN_MARKS)
        .then(|| arena_proto::parse_warn_marks(&s.value).ok())
        .flatten();
    let value = match (&opening_hours, &printer, &tax) {
        (Some(week), _, _) => arena_proto::opening_hours_json(week),
        (None, Some(p), _) => p.to_json(),
        (None, None, Some(t)) => t.to_json(),
        _ => marks.map_or_else(
            || s.value.clone(),
            |m| m.iter().map(u8::to_string).collect::<Vec<_>>().join(","),
        ),
    };
    update_setting(
        tx,
        &row.branch_id,
        &row.key,
        &value,
        prior.as_deref(),
        from.map(|d| d.to_string()).as_deref(),
    )?;
    let mut written = vec![Written {
        key: RowKey::Settings {
            branch_id: row.branch_id.clone(),
            key: row.key.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    }];
    if (row.key == KEY_PRINTER_SELECTED || row.key == KEY_PRINTER_PAPER_MM)
        && value != row.value
        && let Some(status) = find_setting(tx, KEY_PRINTER_STATUS)?
    {
        let unset = PrinterStatus::unset().to_json();
        if status.value != unset {
            update_setting(tx, &status.branch_id, &status.key, &unset, None, None)?;
            written.push(Written {
                key: RowKey::Settings {
                    branch_id: status.branch_id,
                    key: status.key,
                },
                op: ChangeOp::Update,
                before: None,
            });
        }
    }
    if opening_hours.is_some()
        && let Some(flag) = find_setting(tx, KEY_OPENING_HOURS_SET)?
        && flag.value != "true"
    {
        update_setting(tx, &flag.branch_id, &flag.key, "true", None, None)?;
        written.push(Written {
            key: RowKey::Settings {
                branch_id: flag.branch_id,
                key: flag.key,
            },
            op: ChangeOp::Update,
            before: None,
        });
    }
    Ok(written)
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
