use arena_proto::{
    ActorKind, AppError, ErrorCode, Intent, IntentPayload, MAX_PRICE_MINOR, MINUTES_PER_DAY,
    RATE_MAX_MINOR, RULE_ALL_DAYS, RULE_MAX_DATE_SPAN_DAYS, RULE_MAX_PERCENT_BP, RULE_MAX_PRIORITY,
    RULE_MIN_PERCENT_BP, RuleUpsert,
};
use arena_store::{
    ChangeOp, GroupRow, PricingRuleRow, RowKey, StoreTx, find_group, find_pricing_rule,
    insert_pricing_rule, update_pricing_rule,
};

use super::mutate::Written;
use super::validate::validate_label;
use crate::pricing::{ADJUST_OVERRIDE, ADJUST_PERCENT, KIND_DATE, KIND_WEEKLY, parse_date};

#[derive(Default)]
pub(crate) struct RuleTarget {
    rule: Option<PricingRuleRow>,
    group: Option<GroupRow>,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>, p: &IntentPayload) -> Result<RuleTarget, AppError> {
    let IntentPayload::RuleUpsert(u) = p else {
        return Ok(RuleTarget::default());
    };
    let mut t = RuleTarget::default();
    if let Some(id) = &u.id {
        t.rule = find_pricing_rule(tx, id)?;
    }
    t.group = find_group(tx, &u.group_id)?.filter(|g| g.deleted_at.is_none());
    Ok(t)
}

pub(super) fn base_rev(intent: &Intent, t: &RuleTarget) -> Result<(), AppError> {
    match (t.rule.as_ref().map(|r| r.rev), intent.base_rev) {
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

fn weekly_terms(u: &RuleUpsert) -> Result<(), AppError> {
    if u.date_from.is_some() || u.date_to.is_some() {
        return Err(invalid("date_from", "a weekly rule has no dates"));
    }
    match u.days_mask {
        Some(mask) if (1..=RULE_ALL_DAYS).contains(&mask) => {}
        _ => return Err(invalid("days_mask", "pick at least one weekday")),
    }
    let (Some(start), Some(end)) = (u.start_min, u.end_min) else {
        return Err(invalid("start_min", "a start and an end are needed"));
    };
    if !(0..MINUTES_PER_DAY).contains(&start) {
        return Err(invalid("start_min", "the start must be within a day"));
    }
    if !(0..MINUTES_PER_DAY).contains(&end) {
        return Err(invalid("end_min", "the end must be within a day"));
    }
    if start == end {
        return Err(invalid("end_min", "the end must differ from the start"));
    }
    Ok(())
}

fn date_terms(u: &RuleUpsert) -> Result<(), AppError> {
    if u.days_mask.is_some() || u.start_min.is_some() || u.end_min.is_some() {
        return Err(invalid("days_mask", "a date rule covers whole days"));
    }
    let date = |field: &str, v: &Option<String>| {
        v.as_deref()
            .and_then(parse_date)
            .ok_or_else(|| invalid(field, "use a date like 2027-03-10"))
    };
    let from = date("date_from", &u.date_from)?;
    let to = date("date_to", &u.date_to)?;
    if to < from {
        return Err(invalid("date_to", "the last date is before the first"));
    }
    if (to - from).num_days() >= RULE_MAX_DATE_SPAN_DAYS {
        return Err(invalid("date_to", "a date rule can span one year at most"));
    }
    Ok(())
}

fn adjustment(u: &RuleUpsert) -> Result<(), AppError> {
    match u.adjust_kind.as_str() {
        ADJUST_OVERRIDE => {
            if !(1..=RATE_MAX_MINOR.min(MAX_PRICE_MINOR)).contains(&u.adjust_value) {
                return Err(invalid("adjust_value", "the rate must be above zero"));
            }
        }
        ADJUST_PERCENT => {
            if !(RULE_MIN_PERCENT_BP..=RULE_MAX_PERCENT_BP).contains(&u.adjust_value) {
                return Err(invalid(
                    "adjust_value",
                    "a discount cannot be more than 100%",
                ));
            }
        }
        _ => {
            return Err(invalid(
                "adjust_kind",
                "a rule sets a rate or moves it by a percentage",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate(u: &RuleUpsert, t: &RuleTarget) -> Result<(), AppError> {
    validate_label(&u.name).map_err(|m| invalid("name", &m))?;
    if u.id.is_some() && t.rule.is_none() {
        return Err(invalid("id", "no such rule"));
    }
    if t.group.is_none() {
        return Err(invalid("group_id", "no such group"));
    }
    if !(0..=RULE_MAX_PRIORITY).contains(&u.priority) {
        return Err(invalid("priority", "priority is 0 to 1000"));
    }
    adjustment(u)?;
    match u.rule_kind.as_str() {
        KIND_WEEKLY => weekly_terms(u),
        KIND_DATE => date_terms(u),
        _ => Err(invalid("rule_kind", "a rule is weekly or by date")),
    }
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    u: &RuleUpsert,
    t: &RuleTarget,
) -> Result<Vec<Written>, AppError> {
    let (id, op) = match &t.rule {
        Some(old) => (old.rule_id.clone(), ChangeOp::Update),
        None => (ulid::Ulid::generate().to_string(), ChangeOp::Insert),
    };
    let row = PricingRuleRow {
        rule_id: id.clone(),
        name: u.name.trim().to_owned(),
        group_id: u.group_id.clone(),
        rule_kind: u.rule_kind.clone(),
        days_mask: u.days_mask,
        start_min: u.start_min,
        end_min: u.end_min,
        date_from: u.date_from.clone(),
        date_to: u.date_to.clone(),
        adjust_kind: u.adjust_kind.clone(),
        adjust_value: u.adjust_value,
        priority: u.priority,
        active: u.active,
        rev: 0,
        updated_at: 0,
    };
    match op {
        ChangeOp::Update => update_pricing_rule(tx, &row)?,
        _ => insert_pricing_rule(tx, &row)?,
    }
    Ok(vec![Written {
        key: RowKey::PricingRules { rule_id: id },
        op,
        before: None,
    }])
}
