use arena_proto::{
    AppError, CREDIT_DAY_MAX, CREDIT_DAYS_MAX, CREDIT_LIMIT_MAX_MINOR, CreditSetLimit, ErrorCode,
    Intent,
};
use arena_store::{
    ChangeOp, CreditRow, RowKey, StoreTx, find_credit, find_player, insert_credit,
    update_credit_limit,
};

use super::mutate::Written;
use super::session::invalid;

#[derive(Default)]
pub(crate) struct LimitTarget {
    player: bool,
    credit: Option<CreditRow>,
}

pub(super) fn load_set_limit(
    tx: &StoreTx<'_>,
    c: &CreditSetLimit,
) -> Result<LimitTarget, AppError> {
    Ok(LimitTarget {
        player: find_player(tx, &c.player_id)?.is_some(),
        credit: find_credit(tx, &c.player_id)?,
    })
}

pub(super) fn validate_set_limit(
    intent: &Intent,
    c: &CreditSetLimit,
    t: &LimitTarget,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "credit.set_limit takes no base_rev"));
    }
    if !t.player {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "player_id",
            "no such player",
        ));
    }
    if !(0..=CREDIT_LIMIT_MAX_MINOR).contains(&c.limit_minor) {
        return Err(invalid("limit_minor", "enter a limit of 0 or more"));
    }
    let days = &c.reminder_days;
    if days.is_empty()
        || days.len() > CREDIT_DAYS_MAX
        || days.iter().any(|d| !(1..=CREDIT_DAY_MAX).contains(d))
        || days.windows(2).any(|w| w[0] >= w[1])
    {
        return Err(invalid(
            "reminder_days",
            "list one to ten different days, smallest first",
        ));
    }
    if let Some(credit) = &t.credit
        && c.limit_minor < credit.outstanding_minor
    {
        return Err(AppError::with_field(
            ErrorCode::CreditBelowOutstanding,
            "limit_minor",
            "The limit is below what the player owes.",
        ));
    }
    Ok(())
}

pub(super) fn mutate_set_limit(
    tx: &StoreTx<'_>,
    c: &CreditSetLimit,
    t: &LimitTarget,
) -> Result<Vec<Written>, AppError> {
    let key = RowKey::CreditBalances {
        player_id: c.player_id.clone(),
    };
    if t.credit.is_some() {
        update_credit_limit(tx, &c.player_id, c.limit_minor, &c.reminder_days)?;
        Ok(vec![Written {
            key,
            op: ChangeOp::Update,
            before: None,
        }])
    } else {
        insert_credit(tx, &c.player_id, c.limit_minor, &c.reminder_days)?;
        Ok(vec![Written {
            key,
            op: ChangeOp::Insert,
            before: None,
        }])
    }
}
