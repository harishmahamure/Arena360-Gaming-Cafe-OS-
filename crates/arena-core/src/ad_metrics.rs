//! The ad rows after a cursor, as the day-and-slot totals a Free branch's heartbeat carries
//! (Story 4.14), and the cursor move when the cloud acknowledges them.
//!
//! The business day of a row comes from the branch locale in effect when the row happened
//! ([`locale::resolve`]), read when the heartbeat is built. One block totals at most
//! [`AD_BATCH_ROWS`] rows; the rest follow in the next heartbeat.

use std::collections::BTreeMap;

use arena_proto::{AD_BATCH_ROWS, AdCounts, AdDayCount, AdSlot, AppError, ErrorCode};
use arena_store::{
    AdKind, AdRow, StoreTx, ad_acknowledge, ad_cursor, ad_rows_after, find_branch_locale,
    list_settings,
};

use crate::locale::{self, LocaleSetting};

/// What the next heartbeat carries: a block for each table with rows after its cursor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdPending {
    pub impressions: Option<AdCounts>,
    pub requests: Option<AdCounts>,
}

/// The rows of `rows` (ids after `after`, oldest first) as one block, or `None` when empty.
fn block(rows: &[AdRow], after: i64, locale_rows: &[LocaleSetting]) -> Option<AdCounts> {
    let through_id = rows.last()?.id;
    let mut totals: BTreeMap<(String, AdSlot), i64> = BTreeMap::new();
    for row in rows {
        // A slot this build does not know cannot be stored (CHECK); skip it rather than fail.
        let Some(slot) = AdSlot::parse(&row.slot) else {
            continue;
        };
        let (day, _) = locale::resolve(locale_rows, row.at);
        *totals.entry((day.to_string(), slot)).or_insert(0) += 1;
    }
    Some(AdCounts {
        after_id: Some(after),
        through_id,
        counts: totals
            .into_iter()
            .map(|((business_day, slot), n)| AdDayCount {
                business_day,
                slot,
                n,
            })
            .collect(),
    })
}

pub(crate) fn pending(tx: &StoreTx<'_>) -> Result<AdPending, AppError> {
    let Some(branch) = find_branch_locale(tx)? else {
        return Ok(AdPending::default());
    };
    let locale_rows = locale::locale_rows(&branch, &list_settings(tx)?);
    let read = |kind| -> Result<Option<AdCounts>, AppError> {
        let after = ad_cursor(tx, kind)?;
        let rows = ad_rows_after(tx, kind, after, AD_BATCH_ROWS)?;
        Ok(block(&rows, after, &locale_rows))
    };
    Ok(AdPending {
        impressions: read(AdKind::Impression)?,
        requests: read(AdKind::Request)?,
    })
}

/// Moves each acknowledged cursor (and drops the rows it covers).
pub(crate) fn acknowledge(
    tx: &StoreTx<'_>,
    impressions: Option<i64>,
    requests: Option<i64>,
) -> Result<(), AppError> {
    for (kind, through) in [
        (AdKind::Impression, impressions),
        (AdKind::Request, requests),
    ] {
        if let Some(through) = through {
            if through < 0 {
                return Err(AppError::new(
                    ErrorCode::ValidationFailed,
                    "an ad cursor cannot be negative",
                ));
            }
            ad_acknowledge(tx, kind, through)?;
        }
    }
    Ok(())
}
