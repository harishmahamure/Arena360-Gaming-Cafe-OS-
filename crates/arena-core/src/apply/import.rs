//! `import.products` (Story 5.33): a chunk of up to 500 rows of a product CSV. Every row is
//! checked with the same rules the dry-run used ([`crate::import_products::check_row`]), then
//! each creates a product or updates the one it matches and books its opening stock as a
//! stock-in lot. All rows apply or none do, and the chunk's idempotency key makes a retry a no-op.

use std::collections::HashMap;

use arena_proto::{AppError, ErrorCode, IMPORT_CHUNK_ROWS, ImportProducts, Intent, KEY_TAX};
use arena_store::{RowKey, StoreTx, find_setting, find_stock, list_products};

use super::mutate::Written;
use super::{products, stock};
use crate::import_products::{ImportContext, Problem, check_row, duplicates, resolve};

pub(crate) struct ImportTarget {
    ctx: ImportContext,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

/// A row's problem as the error of the whole chunk, keeping the codes a `product.upsert` gives.
fn refused(row: usize, p: Problem) -> AppError {
    let code = match p.reason {
        "needs_hsn" => ErrorCode::HsnRequired,
        "barcode_taken" => ErrorCode::BarcodeTaken,
        _ => ErrorCode::ValidationFailed,
    };
    AppError::with_field(code, format!("rows[{row}].{}", p.field), p.message)
}

pub(super) fn load(tx: &StoreTx<'_>) -> Result<ImportTarget, AppError> {
    let existing = list_products(tx)?;
    let tax = find_setting(tx, KEY_TAX)?.map(|s| s.value);
    let mut stock = HashMap::new();
    for p in existing.iter().filter(|p| p.track_stock && !p.bundle) {
        if let Some(s) = find_stock(tx, &p.product_id)? {
            stock.insert(p.product_id.clone(), s.qty);
        }
    }
    Ok(ImportTarget {
        ctx: ImportContext::new(existing, tax.as_deref(), stock)?,
    })
}

pub(super) fn validate(p: &ImportProducts, t: &ImportTarget) -> Result<(), AppError> {
    if p.rows.is_empty() || p.rows.len() > IMPORT_CHUNK_ROWS {
        return Err(invalid("rows", "an import chunk carries 1 to 500 rows"));
    }
    let mut items = Vec::with_capacity(p.rows.len());
    for (i, row) in p.rows.iter().enumerate() {
        let target = check_row(&t.ctx, row).map_err(|e| refused(i, e))?;
        items.push((i64::try_from(i + 1).unwrap_or(i64::MAX), row, target));
    }
    match duplicates(&items).into_iter().next() {
        Some((n, e)) => Err(refused(n, e)),
        None => Ok(()),
    }
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    p: &ImportProducts,
    t: &ImportTarget,
    now: i64,
) -> Result<Vec<Written>, AppError> {
    let mut out = Vec::new();
    for row in &p.rows {
        let old = resolve(&t.ctx, row).map(|i| &t.ctx.existing[i]);
        let upsert = products::upsert_from_import(row, old);
        let target = products::import_target(old, None, t.ctx.tax.as_ref());
        let written = products::mutate_upsert(tx, intent, &upsert, &target, now)?;
        let Some(RowKey::Products { product_id }) = written.first().map(|w| w.key.clone()) else {
            return Err(AppError::new(
                ErrorCode::Internal,
                "a product write did not name its product",
            ));
        };
        out.extend(written);
        if row.opening_qty > 0 {
            out.extend(stock::opening_stock(
                tx,
                intent,
                &product_id,
                row.opening_qty,
                row.unit_cost_minor,
                now,
            )?);
        }
    }
    Ok(out)
}
