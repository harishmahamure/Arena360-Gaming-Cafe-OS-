use arena_proto::{
    AppError, ErrorCode, Intent, PaymentMethod, WALLET_TOPUP_MAX_MINOR, WalletTopup,
    WalletTopupReverted,
};
use arena_store::{
    ChangeOp, NewPayment, NewTopupOrder, NewWalletTxn, OrderRow, RowKey, StoreTx, Table,
    adjust_wallet, delete_row, effect_recorded, find_order, find_player, find_wallet,
    forward_intent_id, insert_payment, insert_topup_order, insert_wallet, insert_wallet_txn,
    list_order_wallet_txns, list_payment_ids, read_row, wallet_balance,
};

use super::mutate::Written;
use super::session::{
    MAX_PAYMENTS, check_reference, check_rev, in_window, invalid, same_staff, undo_denied,
};

#[derive(Default)]
pub(crate) struct TopupTarget {
    player: bool,
    wallet: bool,
}

pub(super) fn load_topup(tx: &StoreTx<'_>, t: &WalletTopup) -> Result<TopupTarget, AppError> {
    Ok(TopupTarget {
        player: find_player(tx, &t.player_id)?.is_some(),
        wallet: find_wallet(tx, &t.player_id)?.is_some(),
    })
}

pub(super) fn validate_topup(
    intent: &Intent,
    t: &WalletTopup,
    target: &TopupTarget,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "wallet.topup takes no base_rev"));
    }
    if !target.player {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "player_id",
            "no such player",
        ));
    }
    if !(1..=WALLET_TOPUP_MAX_MINOR).contains(&t.amount_minor) {
        return Err(invalid("amount_minor", "enter an amount above 0"));
    }
    if t.payments.is_empty() || t.payments.len() > MAX_PAYMENTS {
        return Err(invalid("payments", "add one to eight payments"));
    }
    for (i, p) in t.payments.iter().enumerate() {
        if p.method == PaymentMethod::Wallet {
            return Err(invalid(
                &format!("payments[{i}].method"),
                "a top-up cannot be paid from the wallet",
            ));
        }
        if p.amount_minor <= 0 {
            return Err(invalid(
                &format!("payments[{i}].amount_minor"),
                "each payment must be above 0",
            ));
        }
        check_reference(i, p)?;
    }
    let paid = t
        .payments
        .iter()
        .try_fold(0i64, |sum, p| sum.checked_add(p.amount_minor))
        .ok_or_else(|| invalid("payments", "the payments are too large"))?;
    if paid != t.amount_minor {
        return Err(AppError::with_field(
            ErrorCode::PaymentShort,
            "payments",
            "The payments must add up to the top-up.",
        ));
    }
    Ok(())
}

pub(super) fn mutate_topup(
    tx: &StoreTx<'_>,
    intent: &Intent,
    t: &WalletTopup,
    target: &TopupTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let (business_day, fy) = crate::locale::stamp(tx, now_ms)?.ok_or_else(|| {
        AppError::new(ErrorCode::Internal, "the branch has not been bootstrapped")
    })?;
    let order_id = ulid::Ulid::generate().to_string();
    insert_topup_order(
        tx,
        &NewTopupOrder {
            id: &order_id,
            player_id: &t.player_id,
            total_minor: t.amount_minor,
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
    for (i, p) in t.payments.iter().enumerate() {
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
    let method = match t.payments.first() {
        Some(first) if t.payments.iter().all(|p| p.method == first.method) => first.method.as_str(),
        _ => "mixed",
    };
    let wallet_key = RowKey::Wallets {
        player_id: t.player_id.clone(),
    };
    if target.wallet {
        written.push(Written {
            key: wallet_key,
            op: ChangeOp::Update,
            before: None,
        });
    } else {
        insert_wallet(tx, &t.player_id)?;
        written.push(Written {
            key: wallet_key,
            op: ChangeOp::Insert,
            before: None,
        });
    }
    adjust_wallet(tx, &t.player_id, t.amount_minor)?;
    let txn_id = ulid::Ulid::generate().to_string();
    insert_wallet_txn(
        tx,
        &NewWalletTxn {
            id: &txn_id,
            player_id: &t.player_id,
            delta_minor: t.amount_minor,
            reason: "topup",
            method: Some(method),
            staff_id: &intent.actor.id,
            order_id: &order_id,
            business_day: &business_day,
            fy: &fy,
        },
    )?;
    written.push(Written {
        key: RowKey::WalletTxns { id: txn_id },
        op: ChangeOp::Insert,
        before: None,
    });
    Ok(written)
}

#[derive(Default)]
pub(crate) struct TopupRevertTarget {
    order: Option<OrderRow>,
    payments: Vec<String>,
    txns: Vec<(String, String, i64)>,
    balance: i64,
    effect: bool,
    now: i64,
}

pub(super) fn load_topup_reverted(
    tx: &StoreTx<'_>,
    r: &WalletTopupReverted,
    now: i64,
) -> Result<TopupRevertTarget, AppError> {
    let Some(order) = find_order(tx, &r.order_id)? else {
        return Ok(TopupRevertTarget {
            now,
            ..TopupRevertTarget::default()
        });
    };
    let forward = forward_intent_id(tx, "orders", &order.id, "wallet.topup")?;
    let balance = match &order.player_id {
        Some(p) => wallet_balance(tx, p)?,
        None => 0,
    };
    Ok(TopupRevertTarget {
        payments: list_payment_ids(tx, &order.id)?,
        txns: list_order_wallet_txns(tx, &order.id)?,
        balance,
        effect: effect_recorded(tx, forward.as_deref(), &order.id)?,
        order: Some(order),
        now,
    })
}

pub(super) fn base_rev_topup_reverted(
    intent: &Intent,
    t: &TopupRevertTarget,
) -> Result<(), AppError> {
    let Some(o) = &t.order else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "order_id",
            "no such bill",
        ));
    };
    check_rev(intent, o.rev, "wallet.topup.reverted")
}

pub(super) fn validate_topup_reverted(
    intent: &Intent,
    t: &TopupRevertTarget,
) -> Result<(), AppError> {
    let Some(o) = &t.order else {
        return Err(invalid("order_id", "no such bill"));
    };
    if o.kind != "topup" {
        return Err(invalid("order_id", "this bill is not a top-up"));
    }
    if o.status != "settled" {
        return Err(AppError::with_field(
            ErrorCode::Stale,
            "order_id",
            "this top-up is not settled",
        ));
    }
    if !same_staff(intent, o.settled_by.as_deref()) {
        return Err(undo_denied("only the staff member who topped up can undo"));
    }
    if !in_window(t.now, o.settled_at) {
        return Err(undo_denied("the undo window has closed"));
    }
    if t.effect {
        return Err(undo_denied("a receipt, drawer or invoice already happened"));
    }
    if t.balance < o.total_minor {
        return Err(undo_denied("the money has already been spent"));
    }
    Ok(())
}

pub(super) fn mutate_topup_reverted(
    tx: &StoreTx<'_>,
    t: &TopupRevertTarget,
) -> Result<Vec<Written>, AppError> {
    let (Some(o), Some(player_id)) = (&t.order, t.order.as_ref().and_then(|o| o.player_id.clone()))
    else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "an undo validated without its target",
        ));
    };
    let mut written = Vec::with_capacity(t.payments.len() + t.txns.len() + 2);
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
    for (id, _, delta) in &t.txns {
        let key = RowKey::WalletTxns { id: id.clone() };
        let before = read_row(tx, &key)?;
        delete_row(tx, Table::WalletTxns, &before)?;
        adjust_wallet(tx, &player_id, -delta)?;
        written.push(Written {
            key,
            op: ChangeOp::Delete,
            before: Some(before),
        });
    }
    written.push(Written {
        key: RowKey::Wallets { player_id },
        op: ChangeOp::Update,
        before: None,
    });
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
