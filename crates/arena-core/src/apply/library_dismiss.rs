//! `library.dismiss` and `library.dismiss_clear` (Story 6.5, `games.manage`): the proposals the
//! Manager hid from Settings → Games → "Found on PCs". One `library_dismissals` row per proposal
//! key; later scans that report the same executables do not propose them again until the list
//! is cleared.

use std::collections::HashSet;

use arena_proto::{AppError, ErrorCode, Intent, LibraryDismiss, MAX_DISMISSED};
use arena_store::{
    ChangeOp, RowKey, StoreTx, Table, delete_row, insert_library_dismissal,
    list_library_dismissals, read_row,
};

use super::Target;
use super::mutate::Written;

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>) -> Result<Target, AppError> {
    Ok(Target::Dismissals(
        list_library_dismissals(tx)?.into_iter().collect(),
    ))
}

fn no_base_rev(intent: &Intent) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid(
            "base_rev",
            &format!("{} takes no base_rev", intent.kind.as_str()),
        ));
    }
    Ok(())
}

/// The keys of `p` that are not dismissed yet, once each, in order.
fn fresh<'a>(p: &'a LibraryDismiss, current: &HashSet<String>) -> Vec<&'a String> {
    let mut seen = HashSet::new();
    p.keys
        .iter()
        .filter(|k| !current.contains(*k) && seen.insert(k.as_str()))
        .collect()
}

pub(super) fn validate_dismiss(
    intent: &Intent,
    p: &LibraryDismiss,
    current: &HashSet<String>,
) -> Result<(), AppError> {
    no_base_rev(intent)?;
    p.validate()?;
    let new = fresh(p, current);
    if new.is_empty() {
        return Err(invalid("keys", "these proposals are already dismissed"));
    }
    if current.len() + new.len() > MAX_DISMISSED {
        return Err(invalid(
            "keys",
            "too many dismissed proposals; clear the list",
        ));
    }
    Ok(())
}

pub(super) fn mutate_dismiss(
    tx: &StoreTx<'_>,
    p: &LibraryDismiss,
    current: &HashSet<String>,
) -> Result<Vec<Written>, AppError> {
    let mut out = Vec::new();
    for key in fresh(p, current) {
        insert_library_dismissal(tx, key)?;
        out.push(Written {
            key: RowKey::LibraryDismissals { key: key.clone() },
            op: ChangeOp::Insert,
            before: None,
        });
    }
    Ok(out)
}

pub(super) fn validate_clear(intent: &Intent, current: &HashSet<String>) -> Result<(), AppError> {
    no_base_rev(intent)?;
    if current.is_empty() {
        return Err(invalid("keys", "no proposal is dismissed"));
    }
    Ok(())
}

pub(super) fn mutate_clear(
    tx: &StoreTx<'_>,
    current: &HashSet<String>,
) -> Result<Vec<Written>, AppError> {
    let mut keys: Vec<&String> = current.iter().collect();
    keys.sort();
    let mut out = Vec::with_capacity(keys.len());
    for key in keys {
        let row_key = RowKey::LibraryDismissals { key: key.clone() };
        let before = read_row(tx, &row_key)?;
        delete_row(tx, Table::LibraryDismissals, &before)?;
        out.push(Written {
            key: row_key,
            op: ChangeOp::Delete,
            before: Some(before),
        });
    }
    Ok(out)
}
