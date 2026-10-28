//! `import.plans` (Story 5.34): a chunk of up to 500 rows of a plan CSV. Every row is checked
//! with the same rules the dry-run used ([`crate::import_plans::check_row`], which is the
//! `plan.upsert` rules), then creates a plan or updates the one with exactly its name through
//! `plans::mutate`. All rows apply or none do, and the chunk's idempotency key makes a retry a
//! no-op.

use arena_proto::{AppError, ErrorCode, IMPORT_CHUNK_ROWS, ImportPlans};
use arena_store::{StoreTx, list_groups, list_plan_groups, list_plans};

use super::mutate::Written;
use super::plans;
use crate::import_plans::{PlanContext, check_row, duplicates};

pub(crate) struct PlansTarget {
    ctx: PlanContext,
}

pub(super) fn load(tx: &StoreTx<'_>) -> Result<PlansTarget, AppError> {
    let plans = list_plans(tx)?
        .into_iter()
        .map(|p| {
            let groups = list_plan_groups(tx, &p.plan_id)?;
            Ok((p, groups))
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    Ok(PlansTarget {
        ctx: PlanContext {
            plans,
            groups: list_groups(tx)?,
        },
    })
}

pub(super) fn validate(p: &ImportPlans, t: &PlansTarget) -> Result<(), AppError> {
    if p.rows.is_empty() || p.rows.len() > IMPORT_CHUNK_ROWS {
        return Err(AppError::with_field(
            ErrorCode::ValidationFailed,
            "rows",
            "an import chunk carries 1 to 500 rows",
        ));
    }
    let refused = |row: usize, field: &str, message: String| {
        AppError::with_field(
            ErrorCode::ValidationFailed,
            format!("rows[{row}].{field}"),
            message,
        )
    };
    for (i, row) in p.rows.iter().enumerate() {
        check_row(&t.ctx, row).map_err(|e| refused(i, &e.field, e.message))?;
    }
    let items: Vec<_> = p
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| (i64::try_from(i + 1).unwrap_or(i64::MAX), r))
        .collect();
    match duplicates(&items).into_iter().next() {
        Some((n, e)) => Err(refused(n, &e.field, e.message)),
        None => Ok(()),
    }
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    p: &ImportPlans,
    t: &PlansTarget,
) -> Result<Vec<Written>, AppError> {
    let mut out = Vec::new();
    for row in &p.rows {
        let old = t.ctx.resolve(row.name.trim()).map(|i| &t.ctx.plans[i]);
        let upsert = plans::upsert_from_import(row, old.map(|(p, _)| p));
        let groups = row
            .group_ids
            .iter()
            .map(|id| t.ctx.groups.iter().find(|g| g.group_id == *id).cloned())
            .collect();
        let target = plans::import_target(old.map(|(p, g)| (p, g.as_slice())), groups);
        out.extend(plans::mutate(tx, &upsert, &target)?);
    }
    Ok(out)
}
