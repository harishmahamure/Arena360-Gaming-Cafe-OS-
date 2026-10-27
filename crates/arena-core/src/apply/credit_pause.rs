use arena_proto::{AppError, CreditAutoPause, ErrorCode, Intent, KEY_BILLING_CREDIT_AUTO_PAUSE};
use arena_store::{
    ChangeOp, RowKey, StoreTx, credit_aging, find_credit, find_setting, list_credit_debtors,
    set_credit_paused,
};

use super::mutate::Written;
use super::session::invalid;

pub(super) fn rule_on(tx: &StoreTx<'_>) -> Result<bool, AppError> {
    Ok(find_setting(tx, KEY_BILLING_CREDIT_AUTO_PAUSE)?.is_none_or(|s| s.value != "false"))
}

pub(super) fn overdue(tx: &StoreTx<'_>, player_id: &str, today: &str) -> Result<bool, AppError> {
    Ok(credit_aging(tx, player_id, today)?.overdue())
}

#[derive(Default)]
pub(crate) struct PauseTarget {
    changes: Vec<(String, bool)>,
}

pub(crate) fn load(
    tx: &StoreTx<'_>,
    p: &CreditAutoPause,
    now_ms: i64,
) -> Result<PauseTarget, AppError> {
    let rows = match &p.player_id {
        Some(id) => find_credit(tx, id)?.into_iter().collect(),
        None => list_credit_debtors(tx)?,
    };
    let rule = rule_on(tx)?;
    let today = crate::locale::stamp(tx, now_ms)?
        .map(|(day, _)| day)
        .unwrap_or_default();
    let mut changes = Vec::new();
    for row in rows {
        let want = rule && row.outstanding_minor > 0 && overdue(tx, &row.player_id, &today)?;
        if want != row.paused {
            changes.push((row.player_id, want));
        }
    }
    Ok(PauseTarget { changes })
}

pub(crate) fn is_due(t: &PauseTarget) -> bool {
    !t.changes.is_empty()
}

pub(super) fn validate(intent: &Intent, t: &PauseTarget) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "credit.auto_pause takes no base_rev"));
    }
    if t.changes.is_empty() {
        return Err(AppError::new(
            ErrorCode::Stale,
            "no credit needs to be paused or reopened",
        ));
    }
    Ok(())
}

pub(super) fn mutate(tx: &StoreTx<'_>, t: &PauseTarget) -> Result<Vec<Written>, AppError> {
    let mut written = Vec::with_capacity(t.changes.len());
    for (player_id, paused) in &t.changes {
        set_credit_paused(tx, player_id, *paused)?;
        written.push(Written {
            key: RowKey::CreditBalances {
                player_id: player_id.clone(),
            },
            op: ChangeOp::Update,
            before: None,
        });
    }
    Ok(written)
}
