use arena_proto::{AppError, ErrorCode, Intent, PlanSell, PlanSellReverted};
use arena_store::{
    ChangeOp, NewPlanLine, NewPlayerPack, NewTopupOrder, OrderRow, OrderTxn, PlanRow,
    PlayerPackRow, RowKey, StoreTx, Table, adjust_outstanding, adjust_wallet, delete_row,
    effect_recorded, find_credit, find_order, find_pack_for_order, find_plan, find_player,
    forward_intent_id, has_unexpired_pass, insert_plan_line, insert_player_pack,
    insert_topup_order, list_order_line_ids, list_order_wallet_txns, list_payment_ids,
    list_plan_groups, pack_in_use, read_row, set_override_day,
};

use super::mutate::Written;
use super::session::{
    SettleTarget, Stamp, check_rev, in_window, insert_payments, invalid, load_payer,
    record_payer_txns, same_staff, undo_denied, validate_payments,
};

const DAY_MS: i64 = 86_400_000;

pub(crate) struct SellTarget {
    player: bool,
    plan: Option<PlanRow>,
    groups: Vec<String>,
    pass_active: bool,
    payer: SettleTarget,
    coupon: Option<super::coupon::CouponBill>,
}

pub(super) fn load_sell(
    tx: &StoreTx<'_>,
    s: &PlanSell,
    now_ms: i64,
) -> Result<SellTarget, AppError> {
    let player = find_player(tx, &s.player_id)?.is_some();
    let plan = find_plan(tx, &s.plan_id)?;
    let groups = match &plan {
        Some(p) => list_plan_groups(tx, &p.plan_id)?,
        None => Vec::new(),
    };
    let coupon = match (&s.coupon, &plan) {
        (Some(code), Some(p)) => Some(super::coupon::load_plan_bill(
            tx,
            code,
            &p.plan_id,
            p.price_minor,
            now_ms,
        )?),
        _ => None,
    };
    Ok(SellTarget {
        coupon,
        pass_active: player
            && plan.as_ref().is_some_and(|p| p.kind == "pass")
            && has_unexpired_pass(tx, &s.player_id, now_ms)?,
        payer: load_payer(
            tx,
            player.then_some(s.player_id.as_str()),
            &s.payments,
            now_ms,
        )?,
        player,
        plan,
        groups,
    })
}

pub(super) fn validate_sell(intent: &Intent, s: &PlanSell, t: &SellTarget) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "plan.sell takes no base_rev"));
    }
    if !t.player {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "player_id",
            "no such player",
        ));
    }
    let Some(plan) = &t.plan else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "plan_id",
            "no such plan",
        ));
    };
    if !plan.active {
        return Err(invalid("plan_id", "this plan is not offered"));
    }
    if plan.kind == "block" {
        return Err(invalid(
            "plan_id",
            "a block is bought when its session starts",
        ));
    }
    if t.pass_active {
        return Err(AppError::with_field(
            ErrorCode::PassAlreadyActive,
            "plan_id",
            "This player already has an active pass.",
        ));
    }
    let total = net_price(t, plan.price_minor)?.0;
    validate_payments(&s.payments, s.credit_override, &t.payer, true, total)
}

/// The plan's price after the coupon, with the coupon's discount when there is one.
fn net_price(
    t: &SellTarget,
    price_minor: i64,
) -> Result<(i64, Option<super::coupon::Quote>), AppError> {
    match &t.coupon {
        Some(bill) => {
            let q = bill.quote(price_minor)?;
            Ok((price_minor - q.discount_minor, Some(q)))
        }
        None => Ok((price_minor, None)),
    }
}

pub(super) fn mutate_sell(
    tx: &StoreTx<'_>,
    intent: &Intent,
    s: &PlanSell,
    t: &SellTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let Some(plan) = &t.plan else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "a plan sale validated without its plan",
        ));
    };
    let (business_day, fy) = crate::locale::stamp(tx, now_ms)?.ok_or_else(|| {
        AppError::new(ErrorCode::Internal, "the branch has not been bootstrapped")
    })?;
    let (total_minor, quote) = net_price(t, plan.price_minor)?;
    let order_id = ulid::Ulid::generate().to_string();
    insert_topup_order(
        tx,
        &NewTopupOrder {
            kind: "plan_sale",
            id: &order_id,
            player_id: &s.player_id,
            total_minor,
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
    let line_id = ulid::Ulid::generate().to_string();
    insert_plan_line(
        tx,
        &NewPlanLine {
            id: &line_id,
            order_id: &order_id,
            plan_id: &plan.plan_id,
            label: &plan.name,
            minutes: plan.minutes.unwrap_or(0),
            amount_minor: plan.price_minor,
            at: now_ms,
        },
    )?;
    written.push(Written {
        key: RowKey::OrderLines { id: line_id },
        op: ChangeOp::Insert,
        before: None,
    });
    if let Some(q) = &quote {
        super::coupon::redeem(tx, q, &order_id, false, &mut written)?;
    }
    let stamp = Stamp {
        staff_id: &intent.actor.id,
        order_id: &order_id,
        business_day: &business_day,
        fy: &fy,
    };
    insert_payments(tx, &stamp, &s.payments, &mut written)?;
    record_payer_txns(
        tx,
        &stamp,
        &s.payments,
        Some(&s.player_id),
        t.payer.credit.as_ref(),
        &mut written,
    )?;
    let validity_days = plan.validity_days.unwrap_or(1);
    let pack_id = ulid::Ulid::generate().to_string();
    insert_player_pack(
        tx,
        &NewPlayerPack {
            id: &pack_id,
            player_id: &s.player_id,
            plan_id: &plan.plan_id,
            kind: &plan.kind,
            name: &plan.name,
            price_minor: plan.price_minor,
            minutes: plan.minutes,
            validity_days,
            window_start_min: plan.window_start_min,
            window_end_min: plan.window_end_min,
            group_ids: &t.groups,
            bought_at: now_ms,
            expires_at: now_ms + validity_days * DAY_MS,
            order_id: &order_id,
        },
    )?;
    written.push(Written {
        key: RowKey::PlayerPacks { id: pack_id },
        op: ChangeOp::Insert,
        before: None,
    });
    Ok(written)
}

#[derive(Default)]
pub(crate) struct SellRevertTarget {
    order: Option<OrderRow>,
    payments: Vec<String>,
    lines: Vec<String>,
    txns: Vec<OrderTxn>,
    pack: Option<PlayerPackRow>,
    pack_busy: bool,
    credit_blocked: bool,
    effect: bool,
    now: i64,
}

pub(super) fn load_sell_reverted(
    tx: &StoreTx<'_>,
    r: &PlanSellReverted,
    now: i64,
) -> Result<SellRevertTarget, AppError> {
    let Some(order) = find_order(tx, &r.order_id)? else {
        return Ok(SellRevertTarget {
            now,
            ..SellRevertTarget::default()
        });
    };
    let forward = forward_intent_id(tx, "orders", &order.id, "plan.sell")?;
    let txns = list_order_wallet_txns(tx, &order.id)?;
    let mut credit_blocked = false;
    for txn in txns.iter().filter(|t| t.reason == "credit") {
        let owed = find_credit(tx, &txn.player_id)?.map_or(0, |c| c.outstanding_minor);
        credit_blocked |= owed < txn.delta_minor;
    }
    let pack = find_pack_for_order(tx, &order.id)?;
    let pack_busy = match &pack {
        Some(p) => pack_in_use(tx, &p.id)?,
        None => false,
    };
    Ok(SellRevertTarget {
        payments: list_payment_ids(tx, &order.id)?,
        lines: list_order_line_ids(tx, &order.id)?,
        pack,
        pack_busy,
        txns,
        credit_blocked,
        effect: effect_recorded(tx, forward.as_deref(), &order.id)?,
        order: Some(order),
        now,
    })
}

pub(super) fn base_rev_sell_reverted(
    intent: &Intent,
    t: &SellRevertTarget,
) -> Result<(), AppError> {
    let Some(o) = &t.order else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "order_id",
            "no such bill",
        ));
    };
    check_rev(intent, o.rev, "plan.sell.reverted")
}

pub(super) fn validate_sell_reverted(
    intent: &Intent,
    t: &SellRevertTarget,
) -> Result<(), AppError> {
    let Some(o) = &t.order else {
        return Err(invalid("order_id", "no such bill"));
    };
    if o.kind != "plan_sale" {
        return Err(invalid("order_id", "this bill is not a plan sale"));
    }
    if o.status != "settled" {
        return Err(AppError::with_field(
            ErrorCode::Stale,
            "order_id",
            "this sale is not settled",
        ));
    }
    if !same_staff(intent, o.settled_by.as_deref()) {
        return Err(undo_denied("only the staff member who sold it can undo"));
    }
    if !in_window(t.now, o.settled_at) {
        return Err(undo_denied("the undo window has closed"));
    }
    if t.effect {
        return Err(undo_denied("a receipt, drawer or invoice already happened"));
    }
    if t.credit_blocked {
        return Err(undo_denied("the credit has already been collected"));
    }
    let used = t
        .pack
        .as_ref()
        .is_some_and(|p| p.minutes_left != p.minutes_total);
    if used || t.pack_busy {
        return Err(undo_denied("the plan has already been used"));
    }
    Ok(())
}

pub(super) fn mutate_sell_reverted(
    tx: &StoreTx<'_>,
    t: &SellRevertTarget,
) -> Result<Vec<Written>, AppError> {
    let Some(o) = &t.order else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "an undo validated without its target",
        ));
    };
    let mut written = Vec::with_capacity(t.payments.len() + t.txns.len() + 3);
    super::shift::reverse_sales(
        tx,
        o.settled_by.as_deref().unwrap_or_default(),
        &o.id,
        &mut written,
    )?;
    super::receipt_notify::skip_on_revert(tx, &o.id, &mut written)?;
    super::coupon::release_on_revert(tx, &o.id, false, &mut written)?;
    if let Some(pack) = &t.pack {
        let key = RowKey::PlayerPacks {
            id: pack.id.clone(),
        };
        let before = read_row(tx, &key)?;
        delete_row(tx, Table::PlayerPacks, &before)?;
        written.push(Written {
            key,
            op: ChangeOp::Delete,
            before: Some(before),
        });
    }
    delete_sale_rows(tx, &o.id, &t.payments, &t.txns, &t.lines, &mut written)?;
    Ok(written)
}

/// Removes what an Undone sale wrote: its payments, wallet and credit rows (putting the balances
/// back), lines and the order itself.
pub(super) fn delete_sale_rows(
    tx: &StoreTx<'_>,
    order_id: &str,
    payments: &[String],
    txns: &[OrderTxn],
    lines: &[String],
    written: &mut Vec<Written>,
) -> Result<(), AppError> {
    for id in payments {
        let key = RowKey::Payments { id: id.clone() };
        let before = read_row(tx, &key)?;
        delete_row(tx, Table::Payments, &before)?;
        written.push(Written {
            key,
            op: ChangeOp::Delete,
            before: Some(before),
        });
    }
    for txn in txns {
        let key = RowKey::WalletTxns { id: txn.id.clone() };
        let before = read_row(tx, &key)?;
        delete_row(tx, Table::WalletTxns, &before)?;
        written.push(Written {
            key,
            op: ChangeOp::Delete,
            before: Some(before),
        });
        if txn.reason == "credit" {
            adjust_outstanding(tx, &txn.player_id, -txn.delta_minor)?;
            if txn.method.as_deref() == Some("override") {
                set_override_day(tx, &txn.player_id, None)?;
            }
            written.push(Written {
                key: RowKey::CreditBalances {
                    player_id: txn.player_id.clone(),
                },
                op: ChangeOp::Update,
                before: None,
            });
        } else {
            adjust_wallet(tx, &txn.player_id, -txn.delta_minor)?;
            written.push(Written {
                key: RowKey::Wallets {
                    player_id: txn.player_id.clone(),
                },
                op: ChangeOp::Update,
                before: None,
            });
        }
    }
    for id in lines {
        let key = RowKey::OrderLines { id: id.clone() };
        let before = read_row(tx, &key)?;
        delete_row(tx, Table::OrderLines, &before)?;
        written.push(Written {
            key,
            op: ChangeOp::Delete,
            before: Some(before),
        });
    }
    let key = RowKey::Orders {
        id: order_id.to_owned(),
    };
    let before = read_row(tx, &key)?;
    delete_row(tx, Table::Orders, &before)?;
    written.push(Written {
        key,
        op: ChangeOp::Delete,
        before: Some(before),
    });
    Ok(())
}
