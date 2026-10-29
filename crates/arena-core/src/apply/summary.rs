//! `summary.publish` (Story 8.4, AD-35) and `rollup.publish` (Story 8.6): the system intents that
//! record a published branch summary or daily rollup. Each writes only its `rollup_log` row (the
//! summary's `latest`, or the rollup's `yyyy-mm-dd`), so the publish appears in the audit, and
//! never touches a business row.

use arena_proto::{AppError, RollupPublish, SUMMARY_LOG_DATE, SummaryPublish, is_rollup_date};
use arena_store::{
    ChangeOp, RollupLogRow, RowKey, StoreTx, find_rollup_log, insert_rollup_log, update_rollup_log,
};

use super::mutate::Written;
use super::session::invalid;

/// The longest object key a publish records.
const MAX_KEY_CHARS: usize = 512;

pub(crate) struct SummaryTarget {
    existing: Option<RollupLogRow>,
}

/// The `rollup_log` row of `date`, if it exists.
pub(super) fn load(tx: &StoreTx<'_>, date: &str) -> Result<SummaryTarget, AppError> {
    Ok(SummaryTarget {
        existing: find_rollup_log(tx, date)?,
    })
}

pub(super) fn validate(
    intent: &arena_proto::Intent,
    p: &SummaryPublish,
    _t: &SummaryTarget,
) -> Result<(), AppError> {
    validate_publish(intent, p.schema, p.built_at, &p.key)
}

pub(super) fn validate_rollup(
    intent: &arena_proto::Intent,
    p: &RollupPublish,
    _t: &SummaryTarget,
) -> Result<(), AppError> {
    if !is_rollup_date(&p.date) {
        return Err(invalid("date", "date must be a business day, yyyy-mm-dd"));
    }
    validate_publish(intent, p.schema, p.built_at, &p.key)
}

fn validate_publish(
    intent: &arena_proto::Intent,
    schema: u32,
    built_at: i64,
    key: &str,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "a publish takes no base_rev"));
    }
    if schema < 1 {
        return Err(invalid("schema", "schema must be at least 1"));
    }
    if built_at < 0 {
        return Err(invalid("built_at", "built_at must not be negative"));
    }
    if key.is_empty() || key.chars().count() > MAX_KEY_CHARS || key.chars().any(char::is_control) {
        return Err(invalid("key", "key must be 1 to 512 printable characters"));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    p: &SummaryPublish,
    t: &SummaryTarget,
) -> Result<Vec<Written>, AppError> {
    upsert(tx, SUMMARY_LOG_DATE, p.schema, p.built_at, &p.key, t)
}

pub(super) fn mutate_rollup(
    tx: &StoreTx<'_>,
    p: &RollupPublish,
    t: &SummaryTarget,
) -> Result<Vec<Written>, AppError> {
    upsert(tx, &p.date, p.schema, p.built_at, &p.key, t)
}

fn upsert(
    tx: &StoreTx<'_>,
    date: &str,
    schema: u32,
    built_at: i64,
    key: &str,
    t: &SummaryTarget,
) -> Result<Vec<Written>, AppError> {
    let op = if t.existing.is_some() {
        update_rollup_log(tx, date, schema, built_at, key)?;
        ChangeOp::Update
    } else {
        insert_rollup_log(tx, date, schema, built_at, key)?;
        ChangeOp::Insert
    };
    Ok(vec![Written {
        key: RowKey::RollupLog {
            date: date.to_owned(),
        },
        op,
        before: None,
    }])
}
