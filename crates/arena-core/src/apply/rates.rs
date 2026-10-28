use arena_proto::{AppError, ErrorCode, IntentPayload, RATE_MAX_MINOR, RateUpsert};
use arena_store::{
    ChangeOp, GroupRow, RateRow, RowKey, StoreTx, deactivate_rate, find_active_rate, find_group,
    insert_rate,
};

use super::mutate::Written;

#[derive(Default)]
pub(crate) struct RateTarget {
    group: Option<GroupRow>,
    active: Option<RateRow>,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>, p: &IntentPayload) -> Result<RateTarget, AppError> {
    let IntentPayload::RateUpsert(r) = p else {
        return Ok(RateTarget::default());
    };
    let group = find_group(tx, &r.group_id)?.filter(|g| g.deleted_at.is_none());
    let active = find_active_rate(tx, &r.group_id)?;
    Ok(RateTarget { group, active })
}

pub(super) fn validate(r: &RateUpsert, t: &RateTarget) -> Result<(), AppError> {
    if !(1..=RATE_MAX_MINOR).contains(&r.price_per_hour_minor) {
        return Err(invalid(
            "price_per_hour_minor",
            "the rate must be above zero",
        ));
    }
    if t.group.is_none() {
        return Err(invalid("group_id", "no such group"));
    }
    Ok(())
}

/// Whether `price` is a rate a group can take (Story 5.36).
pub(crate) fn price_ok(price: i64) -> bool {
    (1..=RATE_MAX_MINOR).contains(&price)
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    r: &RateUpsert,
    t: &RateTarget,
) -> Result<Vec<Written>, AppError> {
    let mut written = Vec::with_capacity(2);
    if let Some(old) = &t.active {
        deactivate_rate(tx, &old.rate_id)?;
        written.push(Written {
            key: RowKey::Rates {
                rate_id: old.rate_id.clone(),
            },
            op: ChangeOp::Update,
            before: None,
        });
    }
    let id = ulid::Ulid::generate().to_string();
    insert_rate(tx, &id, &r.group_id, r.price_per_hour_minor)?;
    written.push(Written {
        key: RowKey::Rates { rate_id: id },
        op: ChangeOp::Insert,
        before: None,
    });
    Ok(written)
}
