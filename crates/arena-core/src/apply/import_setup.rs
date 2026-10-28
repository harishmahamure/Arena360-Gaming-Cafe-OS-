//! `import.setup` (Story 5.36): a branch setup file. The file is read against the Hub with the
//! same [`analyse`] the dry-run uses, then each step it lists is run through the single-item
//! intent's own load, validate and mutate (`group.upsert`, `rate.upsert`, `product.upsert`,
//! `plan.upsert`, `rule.upsert`, `coupon.upsert`), in dependency order, inside this one
//! transaction. All of it applies or none of it does, and the idempotency key makes a retry a
//! no-op. Entries that exist by name are left alone, never updated.

use arena_proto::{AppError, ErrorCode, Intent, IntentPayload, SetupFile};
use arena_store::{StoreTx, find_group_by_name, list_plans, list_products};

use super::mutate::Written;
use super::{coupon, groups, plans, products, rates, rules};
use crate::setup_file::{
    Analysis, Context, Step, analyse, bundle_upsert, coupon_upsert, group_upsert, plan_upsert,
    product_upsert, rule_upsert, target_name,
};

pub(crate) struct SetupTarget {
    analysis: Analysis,
}

pub(super) fn load(
    tx: &StoreTx<'_>,
    file: &SetupFile,
    now_ms: i64,
) -> Result<SetupTarget, AppError> {
    let ctx = Context::load(tx, now_ms)?;
    Ok(SetupTarget {
        analysis: analyse(file, &ctx)?,
    })
}

pub(super) fn validate(t: &SetupTarget) -> Result<(), AppError> {
    if t.analysis.steps.is_empty() {
        return Err(AppError::with_field(
            ErrorCode::ValidationFailed,
            "file",
            "everything in this file is already set up, or cannot be added here",
        ));
    }
    Ok(())
}

/// A step's refusal, said with the entry it was about.
fn at(kind: &str, name: &str, e: AppError) -> AppError {
    AppError::with_field(e.code, format!("{kind} {name}"), e.message)
}

fn group_id(tx: &StoreTx<'_>, name: &str) -> Result<String, AppError> {
    find_group_by_name(tx, name.trim())?
        .map(|g| g.group_id)
        .ok_or_else(|| AppError::new(ErrorCode::Internal, "a group of the file is missing"))
}

fn product_id(tx: &StoreTx<'_>, name: &str) -> Result<String, AppError> {
    let name = name.trim();
    let all = list_products(tx)?;
    let mut same = all.iter().filter(|p| p.name.trim() == name);
    let first = same.next();
    first
        .filter(|p| p.active)
        .or_else(|| same.find(|p| p.active).or(first))
        .map(|p| p.product_id.clone())
        .ok_or_else(|| AppError::new(ErrorCode::Internal, "a product of the file is missing"))
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    file: &SetupFile,
    t: &SetupTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let mut out = Vec::new();
    for step in &t.analysis.steps {
        match *step {
            Step::Group(i) => {
                let g = &file.groups[i];
                let u = group_upsert(g);
                let target = groups::load(tx, &IntentPayload::GroupUpsert(u.clone()))?;
                groups::validate_upsert(&u, &target).map_err(|e| at("group", &g.name, e))?;
                out.extend(groups::mutate_upsert(tx, &u, &target)?);
            }
            Step::Rate(i) => {
                let g = &file.groups[i];
                let u = arena_proto::RateUpsert {
                    group_id: group_id(tx, &g.name)?,
                    price_per_hour_minor: g.rate_minor.unwrap_or_default(),
                };
                let target = rates::load(tx, &IntentPayload::RateUpsert(u.clone()))?;
                rates::validate(&u, &target).map_err(|e| at("rate", &g.name, e))?;
                out.extend(rates::mutate(tx, &u, &target)?);
            }
            Step::Product(i) => {
                let p = &file.products[i];
                let u = product_upsert(p);
                let target = products::load(tx, &IntentPayload::ProductUpsert(u.clone()))?;
                products::validate_upsert(&u, &target).map_err(|e| at("product", &p.name, e))?;
                out.extend(products::mutate_upsert(tx, intent, &u, &target, now_ms)?);
            }
            Step::Bundle(i) => {
                let b = &file.bundles[i];
                let ids = b
                    .components
                    .iter()
                    .map(|c| product_id(tx, &c.product))
                    .collect::<Result<Vec<_>, _>>()?;
                let u = bundle_upsert(b, &ids);
                let target = products::load(tx, &IntentPayload::ProductUpsert(u.clone()))?;
                products::validate_upsert(&u, &target).map_err(|e| at("bundle", &b.name, e))?;
                out.extend(products::mutate_upsert(tx, intent, &u, &target, now_ms)?);
            }
            Step::Plan(i) => {
                let p = &file.plans[i];
                let ids = p
                    .groups
                    .iter()
                    .map(|n| group_id(tx, n))
                    .collect::<Result<Vec<_>, _>>()?;
                let u = plan_upsert(p, ids);
                let target = plans::load(tx, &IntentPayload::PlanUpsert(u.clone()))?;
                plans::validate(&u, &target).map_err(|e| at("plan", &p.name, e))?;
                out.extend(plans::mutate(tx, &u, &target)?);
            }
            Step::Rule(i) => {
                let r = &file.rules[i];
                let u = rule_upsert(r, group_id(tx, &r.group)?);
                let target = rules::load(tx, &IntentPayload::RuleUpsert(u.clone()))?;
                rules::validate(&u, &target).map_err(|e| at("rule", &r.name, e))?;
                out.extend(rules::mutate(tx, &u, &target)?);
            }
            Step::Coupon(i) => {
                let c = &file.coupons[i];
                let mut ids = Vec::with_capacity(c.targets.len());
                for target in &c.targets {
                    let name = target_name(target);
                    ids.push(match target.kind.as_str() {
                        "group" => group_id(tx, name)?,
                        "plan" => list_plans(tx)?
                            .into_iter()
                            .find(|p| p.name == name)
                            .map(|p| p.plan_id)
                            .ok_or_else(|| {
                                AppError::new(ErrorCode::Internal, "a plan of the file is missing")
                            })?,
                        _ => product_id(tx, name)?,
                    });
                }
                let u = coupon_upsert(c, ids);
                let target = coupon::load(tx, &IntentPayload::CouponUpsert(u.clone()))?;
                coupon::validate(&u, &target).map_err(|e| at("coupon", &c.code, e))?;
                out.extend(coupon::mutate(tx, &u, &target)?);
            }
        }
    }
    Ok(out)
}
