//! `summary.publish` (Story 8.4, AD-35): the system intent that records a published branch
//! summary. It writes only the `rollup_log` row `latest` (so the publish appears in the audit)
//! and never touches a business row.

use arena_proto::{AppError, SUMMARY_LOG_DATE, SummaryPublish};
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

pub(super) fn load(tx: &StoreTx<'_>) -> Result<SummaryTarget, AppError> {
    Ok(SummaryTarget {
        existing: find_rollup_log(tx, SUMMARY_LOG_DATE)?,
    })
}

pub(super) fn validate(
    intent: &arena_proto::Intent,
    p: &SummaryPublish,
    _t: &SummaryTarget,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "summary.publish takes no base_rev"));
    }
    if p.schema < 1 {
        return Err(invalid("schema", "schema must be at least 1"));
    }
    if p.built_at < 0 {
        return Err(invalid("built_at", "built_at must not be negative"));
    }
    if p.key.is_empty()
        || p.key.chars().count() > MAX_KEY_CHARS
        || p.key.chars().any(char::is_control)
    {
        return Err(invalid("key", "key must be 1 to 512 printable characters"));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    p: &SummaryPublish,
    t: &SummaryTarget,
) -> Result<Vec<Written>, AppError> {
    let op = if t.existing.is_some() {
        update_rollup_log(tx, SUMMARY_LOG_DATE, p.schema, p.built_at, &p.key)?;
        ChangeOp::Update
    } else {
        insert_rollup_log(tx, SUMMARY_LOG_DATE, p.schema, p.built_at, &p.key)?;
        ChangeOp::Insert
    };
    Ok(vec![Written {
        key: RowKey::RollupLog {
            date: SUMMARY_LOG_DATE.to_owned(),
        },
        op,
        before: None,
    }])
}
