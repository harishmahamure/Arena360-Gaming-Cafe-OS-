use std::collections::HashSet;

use arena_proto::{
    ActorKind, AppError, ErrorCode, ImportPlanRow, Intent, IntentPayload, MAX_PRICE_MINOR,
    MINUTES_PER_DAY, PLAN_MAX_GROUPS, PLAN_MAX_MINUTES, PLAN_MAX_VALIDITY_DAYS, PlanUpsert,
};
use arena_store::{
    ChangeOp, GroupRow, PlanRow, RowKey, StoreTx, Table, delete_row, find_group, find_plan,
    insert_plan, insert_plan_group, list_plan_groups, read_row, update_plan,
};

use super::mutate::Written;
use super::validate::validate_label;

#[derive(Default)]
pub(crate) struct PlanTarget {
    plan: Option<PlanRow>,
    old_groups: Vec<String>,
    groups: Vec<Option<GroupRow>>,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>, p: &IntentPayload) -> Result<PlanTarget, AppError> {
    let IntentPayload::PlanUpsert(u) = p else {
        return Ok(PlanTarget::default());
    };
    let mut t = PlanTarget::default();
    if let Some(id) = &u.id {
        t.plan = find_plan(tx, id)?;
        if t.plan.is_some() {
            t.old_groups = list_plan_groups(tx, id)?;
        }
    }
    t.groups = u
        .group_ids
        .iter()
        .take(PLAN_MAX_GROUPS + 1)
        .map(|g| Ok(find_group(tx, g)?.filter(|g| g.deleted_at.is_none())))
        .collect::<Result<_, AppError>>()?;
    Ok(t)
}

pub(super) fn base_rev(intent: &Intent, t: &PlanTarget) -> Result<(), AppError> {
    match (t.plan.as_ref().map(|p| p.rev), intent.base_rev) {
        (Some(_), None) if intent.actor.kind != ActorKind::Owner => {
            Err(invalid("base_rev", "editing needs base_rev"))
        }
        (Some(rev), Some(base)) if rev != base => Err(AppError::with_field(
            ErrorCode::Stale,
            "base_rev",
            "this changed since it was read",
        )),
        _ => Ok(()),
    }
}

fn in_range(v: Option<i64>, max: i64) -> bool {
    v.is_some_and(|v| (1..=max).contains(&v))
}

fn window_ok(u: &PlanUpsert) -> Result<(), AppError> {
    let (Some(start), Some(end)) = (u.window_start_min, u.window_end_min) else {
        return Err(invalid("window_start_min", "a start and an end are needed"));
    };
    if !(0..MINUTES_PER_DAY).contains(&start) {
        return Err(invalid(
            "window_start_min",
            "the start must be within a day",
        ));
    }
    if !(0..MINUTES_PER_DAY).contains(&end) {
        return Err(invalid("window_end_min", "the end must be within a day"));
    }
    if start == end {
        return Err(invalid(
            "window_end_min",
            "the end must differ from the start",
        ));
    }
    Ok(())
}

fn terms(u: &PlanUpsert) -> Result<(), AppError> {
    match u.plan_kind.as_str() {
        "pack" => {
            if !in_range(u.minutes, PLAN_MAX_MINUTES) {
                return Err(invalid("minutes", "the pack needs some time"));
            }
            if !in_range(u.validity_days, PLAN_MAX_VALIDITY_DAYS) {
                return Err(invalid("validity_days", "validity must be 1 to 3650 days"));
            }
            if u.window_start_min.is_some() || u.window_end_min.is_some() {
                return Err(invalid("window_start_min", "a pack has no play window"));
            }
            Ok(())
        }
        "block" => {
            if u.minutes.is_some() || u.validity_days.is_some() {
                return Err(invalid("minutes", "a block is set by its window only"));
            }
            window_ok(u)
        }
        "pass" => {
            if u.minutes.is_some() {
                return Err(invalid("minutes", "a pass is set by its window only"));
            }
            if !in_range(u.validity_days, PLAN_MAX_VALIDITY_DAYS) {
                return Err(invalid("validity_days", "validity must be 1 to 3650 days"));
            }
            window_ok(u)
        }
        _ => Err(invalid("plan_kind", "a plan is a pack, a block or a pass")),
    }
}

fn validate_groups(u: &PlanUpsert, t: &PlanTarget) -> Result<(), AppError> {
    let bad = |m: &str| invalid("group_ids", m);
    if u.group_ids.is_empty() {
        return Err(bad("pick at least one group"));
    }
    if u.group_ids.len() > PLAN_MAX_GROUPS {
        return Err(bad("too many groups"));
    }
    let mut seen = HashSet::new();
    if !u.group_ids.iter().all(|g| seen.insert(g)) {
        return Err(bad("a group is listed twice"));
    }
    let Some(groups) = t
        .groups
        .iter()
        .map(Option::as_ref)
        .collect::<Option<Vec<_>>>()
    else {
        return Err(bad("no such group"));
    };
    let consoles = groups.iter().filter(|g| g.kind == "console").count();
    if consoles != 0 && consoles != groups.len() {
        return Err(AppError::with_field(
            ErrorCode::GroupKindMismatch,
            "group_ids",
            "console and PC groups need separate plans",
        ));
    }
    Ok(())
}

pub(super) fn validate(u: &PlanUpsert, t: &PlanTarget) -> Result<(), AppError> {
    validate_label(&u.name).map_err(|m| invalid("name", &m))?;
    if !(1..=MAX_PRICE_MINOR).contains(&u.price_minor) {
        return Err(invalid("price_minor", "the price must be above zero"));
    }
    if let Some(old) = &t.plan {
        if old.kind != u.plan_kind {
            return Err(invalid("plan_kind", "the kind of a plan cannot change"));
        }
    } else if u.id.is_some() {
        return Err(invalid("id", "no such plan"));
    }
    terms(u)?;
    validate_groups(u, t)
}

fn group_write(
    plan_id: &str,
    group_id: &str,
    op: ChangeOp,
    before: Option<arena_store::Row>,
) -> Written {
    Written {
        key: RowKey::PlanGroups {
            plan_id: plan_id.to_owned(),
            group_id: group_id.to_owned(),
        },
        op,
        before,
    }
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    u: &PlanUpsert,
    t: &PlanTarget,
) -> Result<Vec<Written>, AppError> {
    let (id, op) = match &t.plan {
        Some(old) => (old.plan_id.clone(), ChangeOp::Update),
        None => (ulid::Ulid::generate().to_string(), ChangeOp::Insert),
    };
    let row = PlanRow {
        plan_id: id.clone(),
        kind: u.plan_kind.clone(),
        name: u.name.trim().to_owned(),
        price_minor: u.price_minor,
        minutes: u.minutes,
        validity_days: u.validity_days,
        window_start_min: u.window_start_min,
        window_end_min: u.window_end_min,
        active: u.active,
        rev: 0,
    };
    match op {
        ChangeOp::Update => update_plan(tx, &row)?,
        _ => insert_plan(tx, &row)?,
    }
    let mut out = vec![Written {
        key: RowKey::Plans {
            plan_id: id.clone(),
        },
        op,
        before: None,
    }];
    for group_id in t.old_groups.iter().filter(|g| !u.group_ids.contains(g)) {
        let key = RowKey::PlanGroups {
            plan_id: id.clone(),
            group_id: group_id.clone(),
        };
        let before = read_row(tx, &key)?;
        delete_row(tx, Table::PlanGroups, &before)?;
        out.push(group_write(&id, group_id, ChangeOp::Delete, Some(before)));
    }
    for group_id in u.group_ids.iter().filter(|g| !t.old_groups.contains(g)) {
        insert_plan_group(tx, &id, group_id)?;
        out.push(group_write(&id, group_id, ChangeOp::Insert, None));
    }
    Ok(out)
}

/// A plan-import row as the `plan.upsert` it stands for (Story 5.34): an edit of `old` when the
/// name matched a plan (keeping whether it is on sale), else a new active plan.
pub(crate) fn upsert_from_import(row: &ImportPlanRow, old: Option<&PlanRow>) -> PlanUpsert {
    PlanUpsert {
        id: old.map(|o| o.plan_id.clone()),
        plan_kind: row.plan_kind.clone(),
        name: row.name.clone(),
        price_minor: row.price_minor,
        minutes: row.minutes,
        validity_days: row.validity_days,
        window_start_min: row.window_start_min,
        window_end_min: row.window_end_min,
        group_ids: row.group_ids.clone(),
        active: old.is_none_or(|o| o.active),
    }
}

/// What a plan import row is checked against: the plan it updates, that plan's groups and the
/// live groups of the row (`None` for an id that is not one).
pub(crate) fn import_target(
    old: Option<(&PlanRow, &[String])>,
    groups: Vec<Option<GroupRow>>,
) -> PlanTarget {
    PlanTarget {
        plan: old.map(|(p, _)| p.clone()),
        old_groups: old.map(|(_, g)| g.to_vec()).unwrap_or_default(),
        groups,
    }
}

/// The `plan.upsert` rules for one plan-import row (what both the dry-run and the intent run).
pub(crate) fn check_import_row(
    row: &ImportPlanRow,
    old: Option<&PlanRow>,
    groups: Vec<Option<GroupRow>>,
) -> Result<(), AppError> {
    let target = import_target(old.map(|o| (o, &[][..])), groups);
    validate(&upsert_from_import(row, old), &target)
}
