use arena_proto::{
    ADS_POLICY_VERSION, ActorKind, AdsConsent, AdsConsentSet, AppError, ErrorCode, Intent,
    KEY_ADS_CONSENT,
};
use arena_store::{ChangeOp, RowKey, SettingRef, StoreTx, find_setting, update_setting};

use super::mutate::Written;
use crate::setup::{HubTier, hub_tier};

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>) -> Result<Option<SettingRef>, AppError> {
    find_setting(tx, KEY_ADS_CONSENT)
}

pub(super) fn validate(
    intent: &Intent,
    p: &AdsConsentSet,
    row: Option<&SettingRef>,
) -> Result<(), AppError> {
    if intent.actor.kind != ActorKind::Staff {
        return Err(invalid("actor", "ad consent is given by a staff member"));
    }
    if hub_tier() != HubTier::Free {
        return Err(invalid("tier", "ad consent applies only on the Free tier"));
    }
    if p.policy_version != ADS_POLICY_VERSION {
        return Err(invalid(
            "policy_version",
            "consent must be given to the current ad policy",
        ));
    }
    if row.is_none() {
        return Err(invalid("key", "no such setting"));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    p: &AdsConsentSet,
    row: Option<&SettingRef>,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let row = row.ok_or_else(|| invalid("key", "no such setting"))?;
    let consent = AdsConsent {
        given: p.given,
        at: Some(now_ms),
        staff_id: Some(intent.actor.id.clone()),
        policy_version: Some(p.policy_version),
    };
    update_setting(tx, &row.branch_id, &row.key, &consent.to_json(), None, None)?;
    Ok(vec![Written {
        key: RowKey::Settings {
            branch_id: row.branch_id.clone(),
            key: row.key.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    }])
}
