use arena_proto::{
    AppError, CreditCollect, CreditCollectReverted, ErrorCode, Intent, PaymentMethod,
};
use arena_store::{
    ChangeOp, CreditRow, NewPayment, NewTopupOrder, NewWalletTxn, OrderRow, OrderTxn, RowKey,
    StoreTx, Table, adjust_outstanding, adjust_wallet, credit_aging_with, delete_row,
    effect_recorded, find_credit, find_order, find_wallet, forward_intent_id, insert_payment,
    insert_topup_order, insert_wallet_txn, list_order_wallet_txns, list_payment_ids, read_row,
    set_credit_paused,
};

use super::mutate::Written;
use super::session::{
    MAX_PAYMENTS, check_reference, check_rev, in_window, invalid, same_staff, undo_denied,
};

const REOPENED: &str = "reopened";

#[derive(Default)]
pub(crate) struct CollectTarget {
    credit: Option<CreditRow>,
    wallet_minor: i64,
    pause_rule: bool,
}

pub(super) fn load_collect(tx: &StoreTx<'_>, c: &CreditCollect) -> Result<CollectTarget, AppError> {
    Ok(CollectTarget {
        credit: find_credit(tx, &c.player_id)?,
        wallet_minor: find_wallet(tx, &c.player_id)?.map_or(0, |w| w.balance_minor),
        pause_rule: super::credit_pause::rule_on(tx)?,
    })
}

pub(super) fn validate_collect(
    intent: &Intent,
    c: &CreditCollect,
    t: &CollectTarget,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "credit.collect takes no base_rev"));
    }
    let Some(credit) = &t.credit else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "player_id",
            "this player has no credit",
        ));
    };
    if c.amount_minor <= 0 {
        return Err(invalid("amount_minor", "enter an amount above 0"));
    }
    if c.amount_minor > credit.outstanding_minor {
        return Err(invalid(
            "amount_minor",
            "the amount is more than the player owes",
        ));
    }
    if c.payments.is_empty() || c.payments.len() > MAX_PAYMENTS {
        return Err(invalid("payments", "add one to eight payments"));
    }
    let mut wallet_rows = 0;
    for (i, p) in c.payments.iter().enumerate() {
        if p.method == PaymentMethod::Credit {
            return Err(invalid(
                &format!("payments[{i}].method"),
                "credit cannot be collected on credit",
            ));
        }
        if p.amount_minor <= 0 {
            return Err(invalid(
                &format!("payments[{i}].amount_minor"),
                "each payment must be above 0",
            ));
        }
        check_reference(i, p)?;
        if p.method == PaymentMethod::Wallet {
            wallet_rows += 1;
            if wallet_rows > 1 {
                return Err(invalid(
                    &format!("payments[{i}].method"),
                    "use the wallet once on a collection",
                ));
            }
            if p.amount_minor > t.wallet_minor {
                return Err(AppError::with_field(
                    ErrorCode::InsufficientBalance,
                    format!("payments[{i}].amount_minor"),
                    "The wallet balance is too low.",
                ));
            }
        }
    }
    let paid = c
        .payments
        .iter()
        .try_fold(0i64, |sum, p| sum.checked_add(p.amount_minor))
        .ok_or_else(|| invalid("payments", "the payments are too large"))?;
    if paid != c.amount_minor {
        return Err(AppError::with_field(
            ErrorCode::PaymentShort,
            "payments",
            "The payments must add up to the amount collected.",
        ));
    }
    Ok(())
}

pub(super) fn mutate_collect(
    tx: &StoreTx<'_>,
    intent: &Intent,
    c: &CreditCollect,
    t: &CollectTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let (business_day, fy) = crate::locale::stamp(tx, now_ms)?.ok_or_else(|| {
        AppError::new(ErrorCode::Internal, "the branch has not been bootstrapped")
    })?;
    let order_id = ulid::Ulid::generate().to_string();
    insert_topup_order(
        tx,
        &NewTopupOrder {
            kind: "credit_collect",
            id: &order_id,
            player_id: &c.player_id,
            total_minor: c.amount_minor,
            settled_at: now_ms,
            settled_by: &intent.actor.id,
            business_day: &business_day,
            fy: &fy,
        },
    )?;
    let mut written = vec![Written {
        key: RowKey::Orders {
            id: order_id.clone(),
        },
        op: ChangeOp::Insert,
        before: None,
    }];
    for (i, p) in c.payments.iter().enumerate() {
        let id = ulid::Ulid::generate().to_string();
        let reference = p
            .reference
            .as_deref()
            .map(str::trim)
            .filter(|r| !r.is_empty());
        insert_payment(
            tx,
            &NewPayment {
                id: &id,
                order_id: &order_id,
                method: p.method.as_str(),
                amount_minor: p.amount_minor,
                reference,
                staff_id: &intent.actor.id,
                position: i as i64,
                business_day: &business_day,
                fy: &fy,
            },
        )?;
        written.push(Written {
            key: RowKey::Payments { id },
            op: ChangeOp::Insert,
            before: None,
        });
    }
    if let Some(w) = c
        .payments
        .iter()
        .find(|p| p.method == PaymentMethod::Wallet)
    {
        let txn_id = ulid::Ulid::generate().to_string();
        insert_wallet_txn(
            tx,
            &NewWalletTxn {
                id: &txn_id,
                player_id: &c.player_id,
                delta_minor: -w.amount_minor,
                reason: "bill",
                method: None,
                staff_id: &intent.actor.id,
                order_id: &order_id,
                business_day: &business_day,
                fy: &fy,
            },
        )?;
        adjust_wallet(tx, &c.player_id, -w.amount_minor)?;
        written.push(Written {
            key: RowKey::WalletTxns { id: txn_id },
            op: ChangeOp::Insert,
            before: None,
        });
        written.push(Written {
            key: RowKey::Wallets {
                player_id: c.player_id.clone(),
            },
            op: ChangeOp::Update,
            before: None,
        });
    }
    let txn_id = ulid::Ulid::generate().to_string();
    let reopen = t.credit.as_ref().is_some_and(|credit| credit.paused)
        && (!t.pause_rule
            || !credit_aging_with(tx, &c.player_id, &business_day, c.amount_minor)?.overdue());
    insert_wallet_txn(
        tx,
        &NewWalletTxn {
            id: &txn_id,
            player_id: &c.player_id,
            delta_minor: -c.amount_minor,
            reason: "credit",
            method: reopen.then_some(REOPENED),
            staff_id: &intent.actor.id,
            order_id: &order_id,
            business_day: &business_day,
            fy: &fy,
        },
    )?;
    adjust_outstanding(tx, &c.player_id, -c.amount_minor)?;
    if reopen {
        set_credit_paused(tx, &c.player_id, false)?;
    }
    written.push(Written {
        key: RowKey::WalletTxns { id: txn_id },
        op: ChangeOp::Insert,
        before: None,
    });
    written.push(Written {
        key: RowKey::CreditBalances {
            player_id: c.player_id.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    });
    Ok(written)
}

#[derive(Default)]
pub(crate) struct CollectRevertTarget {
    order: Option<OrderRow>,
    payments: Vec<String>,
    txns: Vec<OrderTxn>,
    credit: Option<CreditRow>,
    effect: bool,
    now: i64,
}

pub(super) fn load_collect_reverted(
    tx: &StoreTx<'_>,
    r: &CreditCollectReverted,
    now: i64,
) -> Result<CollectRevertTarget, AppError> {
    let Some(order) = find_order(tx, &r.order_id)? else {
        return Ok(CollectRevertTarget {
            now,
            ..CollectRevertTarget::default()
        });
    };
    let forward = forward_intent_id(tx, "orders", &order.id, "credit.collect")?;
    let credit = match &order.player_id {
        Some(p) => find_credit(tx, p)?,
        None => None,
    };
    Ok(CollectRevertTarget {
        payments: list_payment_ids(tx, &order.id)?,
        txns: list_order_wallet_txns(tx, &order.id)?,
        credit,
        effect: effect_recorded(tx, forward.as_deref(), &order.id)?,
        order: Some(order),
        now,
    })
}

pub(super) fn base_rev_collect_reverted(
    intent: &Intent,
    t: &CollectRevertTarget,
) -> Result<(), AppError> {
    let Some(o) = &t.order else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "order_id",
            "no such collection",
        ));
    };
    check_rev(intent, o.rev, "credit.collect.reverted")
}

pub(super) fn validate_collect_reverted(
    intent: &Intent,
    t: &CollectRevertTarget,
) -> Result<(), AppError> {
    let Some(o) = &t.order else {
        return Err(invalid("order_id", "no such collection"));
    };
    if o.kind != "credit_collect" {
        return Err(invalid("order_id", "this bill is not a credit collection"));
    }
    if o.status != "settled" {
        return Err(AppError::with_field(
            ErrorCode::Stale,
            "order_id",
            "this collection is not settled",
        ));
    }
    if !same_staff(intent, o.settled_by.as_deref()) {
        return Err(undo_denied("only the staff member who collected can undo"));
    }
    if !in_window(t.now, o.settled_at) {
        return Err(undo_denied("the undo window has closed"));
    }
    if t.effect {
        return Err(undo_denied("a receipt, drawer or invoice already happened"));
    }
    let Some(credit) = &t.credit else {
        return Err(undo_denied("the player no longer has credit"));
    };
    if credit.outstanding_minor + o.total_minor > credit.limit_minor {
        return Err(undo_denied("the credit limit was lowered since"));
    }
    Ok(())
}

pub(super) fn mutate_collect_reverted(
    tx: &StoreTx<'_>,
    t: &CollectRevertTarget,
) -> Result<Vec<Written>, AppError> {
    let (Some(o), Some(player_id)) = (&t.order, t.order.as_ref().and_then(|o| o.player_id.clone()))
    else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "an undo validated without its target",
        ));
    };
    let mut written = Vec::with_capacity(t.payments.len() + t.txns.len() + 3);
    for id in &t.payments {
        let key = RowKey::Payments { id: id.clone() };
        let before = read_row(tx, &key)?;
        delete_row(tx, Table::Payments, &before)?;
        written.push(Written {
            key,
            op: ChangeOp::Delete,
            before: Some(before),
        });
    }
    for txn in &t.txns {
        let key = RowKey::WalletTxns { id: txn.id.clone() };
        let before = read_row(tx, &key)?;
        delete_row(tx, Table::WalletTxns, &before)?;
        written.push(Written {
            key,
            op: ChangeOp::Delete,
            before: Some(before),
        });
        if txn.reason == "credit" {
            adjust_outstanding(tx, &player_id, -txn.delta_minor)?;
            if txn.method.as_deref() == Some(REOPENED) {
                set_credit_paused(tx, &player_id, true)?;
            }
            written.push(Written {
                key: RowKey::CreditBalances {
                    player_id: player_id.clone(),
                },
                op: ChangeOp::Update,
                before: None,
            });
        } else {
            adjust_wallet(tx, &player_id, -txn.delta_minor)?;
            written.push(Written {
                key: RowKey::Wallets {
                    player_id: player_id.clone(),
                },
                op: ChangeOp::Update,
                before: None,
            });
        }
    }
    let key = RowKey::Orders { id: o.id.clone() };
    let before = read_row(tx, &key)?;
    delete_row(tx, Table::Orders, &before)?;
    written.push(Written {
        key,
        op: ChangeOp::Delete,
        before: Some(before),
    });
    Ok(written)
}
