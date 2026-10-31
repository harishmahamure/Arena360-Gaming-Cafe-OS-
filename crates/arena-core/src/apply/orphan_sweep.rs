//! `ops.orphan_sweep` (Story 9.23): the system intent the daily housekeeping submits before it
//! deletes an `orphaned/` database older than 90 days. It stores what the databases held that the
//! live one does not (invoice numbers and amounts, the count of set-aside changes; no personal
//! data) in `ops.last_orphan_sweep`, and its change row is the audit entry, so the accountant can
//! still find the numbers once the files are gone.

use arena_proto::{
    ActorKind, AppError, ErrorCode, Intent, KEY_OPS_LAST_ORPHAN_SWEEP, ORPHAN_SWEEP_SYSTEM_ACTOR,
    OrphanSweep,
};
use arena_store::{ChangeOp, RowKey, SettingRef, StoreTx, find_setting, update_setting};

use super::mutate::Written;

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>) -> Result<Option<SettingRef>, AppError> {
    find_setting(tx, KEY_OPS_LAST_ORPHAN_SWEEP)
}

pub(super) fn validate(
    intent: &Intent,
    p: &OrphanSweep,
    row: Option<&SettingRef>,
) -> Result<(), AppError> {
    if intent.actor.kind != ActorKind::System || intent.actor.id != ORPHAN_SWEEP_SYSTEM_ACTOR {
        return Err(invalid("actor", "only the Hub records an orphan sweep"));
    }
    if !p.is_consistent() {
        return Err(invalid(
            "sweep",
            "the sweep names the databases it deletes and the invoices they held",
        ));
    }
    if row.is_none() {
        return Err(invalid("key", "no such setting"));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    p: &OrphanSweep,
    row: Option<&SettingRef>,
) -> Result<Vec<Written>, AppError> {
    let row = row.ok_or_else(|| invalid("key", "no such setting"))?;
    update_setting(tx, &row.branch_id, &row.key, &p.to_json(), None, None)?;
    Ok(vec![Written {
        key: RowKey::Settings {
            branch_id: row.branch_id.clone(),
            key: row.key.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    }])
}
