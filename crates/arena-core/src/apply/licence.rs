use arena_proto::{
    AlertKind, AppError, ErrorCode, Intent, KEY_LICENCE_STATUS, KEY_LICENCE_TIER, LicenceApply,
    LicenceStanding, Tier,
};
use arena_store::{
    ChangeOp, NewAlert, RowKey, StoreTx, find_open_alert, find_setting, insert_alert,
    insert_setting, update_setting,
};

use super::mutate::Written;

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(crate) fn pending(tx: &StoreTx<'_>, p: &LicenceApply) -> Result<bool, AppError> {
    for (key, value) in p.rows() {
        if find_setting(tx, key)?.is_none_or(|s| s.value != value) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn validate(
    intent: &Intent,
    p: &LicenceApply,
    branch: Option<&String>,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "licence.apply takes no base_rev"));
    }
    if branch.is_none() {
        return Err(invalid(
            "branch",
            "there is no branch to apply a licence to",
        ));
    }
    if !p.flags_valid() {
        return Err(invalid(
            "flags",
            "flags are lower-case names joined by commas",
        ));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    p: &LicenceApply,
    branch: Option<&String>,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let branch = branch.ok_or_else(|| invalid("branch", "there is no branch"))?;
    let plan_changed = plan_changed(tx, p)?;
    let mut written = Vec::new();
    for (key, value) in p.rows() {
        let op = match find_setting(tx, key)? {
            Some(s) if s.value == value => continue,
            Some(s) => {
                update_setting(tx, &s.branch_id, key, &value, None, None)?;
                ChangeOp::Update
            }
            None => {
                insert_setting(tx, branch, key, &value)?;
                ChangeOp::Insert
            }
        };
        written.push(Written {
            key: RowKey::Settings {
                branch_id: branch.clone(),
                key: key.to_owned(),
            },
            op,
            before: None,
        });
    }
    if plan_changed && find_open_alert(tx, AlertKind::PlanUpdated.as_str(), None)?.is_none() {
        let id = ulid::Ulid::generate().to_string();
        insert_alert(
            tx,
            &NewAlert {
                id: &id,
                kind: AlertKind::PlanUpdated.as_str(),
                device_id: None,
                raised_at: now_ms,
                product_id: None,
                qty: None,
                threshold: None,
                ticket_id: None,
            },
        )?;
        written.push(Written {
            key: RowKey::Alerts { id },
            op: ChangeOp::Insert,
            before: None,
        });
    }
    Ok(written)
}

/// Whether this licence moves a Hub that already knew a plan onto a different, verified paid one
/// (Story 10.6): the one-time "Plan updated" notice. A first licence (pairing), a Free or failed
/// check, and the recovery from an unverified one are not a plan change.
fn plan_changed(tx: &StoreTx<'_>, p: &LicenceApply) -> Result<bool, AppError> {
    if p.standing != LicenceStanding::Verified || p.tier == Tier::Free {
        return Ok(false);
    }
    let Some(old) = find_setting(tx, KEY_LICENCE_TIER)? else {
        return Ok(false);
    };
    let was_unverified = find_setting(tx, KEY_LICENCE_STATUS)?
        .is_some_and(|s| s.value == LicenceStanding::Unverified.as_str());
    Ok(old.value != p.tier.as_str() && !was_unverified)
}
