//! `lease.promote` (Story 9.21): the record that a standby was promoted to main as a new epoch.
//! The Hub submits it once, right after its writer opens, as the Manager who confirmed the
//! promote. It is a record of fact like `ops.restore`: it stores `ops.last_promote`, and the change
//! row is the audit entry, carrying the old and new epoch, the fork point and the lag at promotion.
//!
//! A forced promote (Story 9.22, `force: true`) also queues the owner alert `standby_forced` in the
//! same transaction, once per new epoch, through the path of Story 8.34: the drain posts it when
//! the WAN is next up.

use std::collections::BTreeMap;

use arena_proto::{
    ActorKind, AppError, ErrorCode, Intent, KEY_OPS_LAST_PROMOTE, LeasePromote, OwnerAlertKind,
};
use arena_store::{
    ChangeOp, RowKey, SettingRef, StoreTx, find_setting, staff_display_name, update_setting,
};

use super::mutate::Written;
use super::owner_alert;

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>) -> Result<Option<SettingRef>, AppError> {
    find_setting(tx, KEY_OPS_LAST_PROMOTE)
}

pub(super) fn validate(
    intent: &Intent,
    p: &LeasePromote,
    row: Option<&SettingRef>,
) -> Result<(), AppError> {
    if intent.actor.kind != ActorKind::Staff {
        return Err(invalid(
            "actor",
            "a promote is recorded by the Manager who made it",
        ));
    }
    if !p.is_consistent() {
        return Err(invalid(
            "promote",
            "the promote names both epochs, the fork point and the lag",
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
    p: &LeasePromote,
    row: Option<&SettingRef>,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let row = row.ok_or_else(|| invalid("key", "no such setting"))?;
    let name = staff_display_name(tx, &intent.actor.id)?;
    let record = LeasePromote {
        by: Some(name.clone()),
        ..p.clone()
    };
    update_setting(tx, &row.branch_id, &row.key, &record.to_json(), None, None)?;
    let mut written = vec![Written {
        key: RowKey::Settings {
            branch_id: row.branch_id.clone(),
            key: row.key.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    }];
    if p.force {
        owner_alert::queue(
            tx,
            &owner_alert::Alert {
                kind: OwnerAlertKind::StandbyForced,
                source_ref: &p.new_epoch,
                order_id: None,
                player_id: "",
                params: BTreeMap::from([("manager", owner_alert::text_param(&name))]),
            },
            now_ms,
            &mut written,
        )?;
    }
    Ok(written)
}
