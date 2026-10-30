//! The branch summary engine (Story 8.4, AD-35): builds `state/latest.json` from a handful of
//! queries over stamped columns, counts and stored totals, on the snapshot reader's own
//! connection. It never runs inside `apply()` and never writes.
//!
//! The file holds no transaction rows and no player data: an attention item names its subject by
//! id and display parameters (a debtor is a player id and two numbers, never a name).

use std::collections::BTreeMap;

use arena_proto::{
    ATTENTION_ALERT, ATTENTION_CREDIT_OVERDUE, ATTENTION_LOW_STOCK, ATTENTION_SHIFT_VARIANCE,
    AppError, BranchSummary, SUMMARY_LIST_CAP, SUMMARY_SCHEMA, SummaryAttention, SummaryShift,
};
use arena_store::Store;

/// Shifts closed within this long of the build count toward "attention" (they are today's).
const RECENT_SHIFT_MS: i64 = 24 * 60 * 60 * 1000;

/// The alert kind whose subject the low-stock items already name.
const STOCK_LOW_ALERT: &str = "stock_low";

fn params(pairs: &[(&str, String)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

/// Builds the summary as of `now_ms` (Hub clock). Each query reads its own snapshot of the
/// database: the file is a picture of right now, not a ledger. `None` before the branch has been
/// bootstrapped.
pub fn build_summary(store: &Store, now_ms: i64) -> Result<Option<BranchSummary>, AppError> {
    let (Some(branch), Some(locale)) = (store.branch_summary()?, store.branch_locale()?) else {
        return Ok(None);
    };
    let business_day = crate::locale::current_business_day(store, now_ms)?.unwrap_or_default();
    let tz = crate::locale::current_tz(store, now_ms)?;
    let sales = store.sales_report_rows(&business_day, &business_day)?;
    let revenue = crate::reports::daily_sales(&sales, &business_day, &business_day, tz);

    let shift = store.current_shift()?;
    let cash_in_drawer_minor = match &shift {
        Some(s) => store.shift_cash_minor(&s.id)?,
        None => 0,
    };
    let mut shifts: Vec<SummaryShift> = shift
        .iter()
        .map(|s| SummaryShift {
            shift_id: s.id.clone(),
            staff_name: s.staff_name.clone(),
            opened_at: s.opened_at,
            variance_minor: None,
            closed_at: None,
        })
        .collect();
    let staff_on_shift = shift.iter().map(|s| s.staff_name.clone()).collect();
    let sessions_live = store.live_session_count()?;

    let mut attention = Vec::new();
    let threshold = store.shift_variance_threshold()?;
    for v in store.recent_shift_variances(now_ms.saturating_sub(RECENT_SHIFT_MS))? {
        // Counted shifts follow the open one, newest first, so the owner sees each shift's signed
        // variance beside the drawer (Story 8.13).
        shifts.push(SummaryShift {
            shift_id: v.shift_id.clone(),
            staff_name: v.staff_name.clone(),
            opened_at: v.opened_at,
            variance_minor: Some(v.variance_minor),
            closed_at: Some(v.closed_at),
        });
        if v.variance_minor.abs() > threshold {
            attention.push(SummaryAttention {
                kind: ATTENTION_SHIFT_VARIANCE.into(),
                reference: v.shift_id,
                text_params: params(&[
                    ("staff", v.staff_name),
                    ("variance_minor", v.variance_minor.to_string()),
                ]),
            });
        }
    }
    for p in store.low_stock()? {
        attention.push(SummaryAttention {
            kind: ATTENTION_LOW_STOCK.into(),
            reference: p.product_id,
            text_params: params(&[
                ("product", p.name),
                ("qty", p.qty.to_string()),
                (
                    "threshold",
                    p.low_stock_qty.map(|t| t.to_string()).unwrap_or_default(),
                ),
            ]),
        });
    }
    for d in store.credit_debtors()? {
        if d.outstanding_minor <= 0 {
            continue;
        }
        let aging = store.credit_aging(&d.player_id, &business_day)?;
        if aging.overdue() {
            attention.push(SummaryAttention {
                kind: ATTENTION_CREDIT_OVERDUE.into(),
                reference: d.player_id,
                text_params: params(&[
                    ("outstanding_minor", d.outstanding_minor.to_string()),
                    ("oldest_age_days", aging.oldest_age_days.to_string()),
                ]),
            });
        }
    }
    for a in store.alerts()? {
        if a.kind == STOCK_LOW_ALERT {
            continue;
        }
        let mut p = params(&[("alert_kind", a.kind.clone())]);
        if let Some(device) = a.device_id {
            p.insert("device_id".into(), device);
        }
        attention.push(SummaryAttention {
            kind: ATTENTION_ALERT.into(),
            reference: a.id,
            text_params: p,
        });
    }
    attention.truncate(SUMMARY_LIST_CAP);
    let reminded_today = store.credit_reminded_on(&business_day)?;

    Ok(Some(BranchSummary {
        schema: SUMMARY_SCHEMA,
        branch_id: branch.branch_id,
        as_of: now_ms,
        business_day,
        open: shift.is_some() || sessions_live > 0,
        sessions_live,
        revenue_so_far_minor: revenue.net_minor,
        currency: locale.currency,
        cash_in_drawer_minor,
        shifts,
        staff_on_shift,
        attention,
        catalogue: store.summary_catalogue()?,
        reminded_today,
    }))
}
