//! `pos.sale` and `pos.sale.reverted` (Story 5.11): a walk-up sale of products and bundles that
//! settles in the same intent and takes stock. One transaction writes the settled order (with the
//! per-rate tax breakup), its lines, the payments (the shift gate and cash events of Story 5.10
//! apply), the wallet or credit rows of an attached player, and the `sale` stock moves. The Undo
//! window is 8 s: it writes compensating cash and stock rows and removes the order.

use std::collections::{BTreeMap, BTreeSet};

use arena_proto::{
    AppError, ErrorCode, Intent, KEY_TAX, MAX_SALE_LINES, MAX_SALE_QTY, MOVE_SALE, PosSale,
    PosSaleLine, PosSaleReverted, TaxMode, TaxSettings, lot_expired,
};
use arena_store::{
    ChangeOp, LotRow, MoveRow, NewPosOrder, NewProductLine, OrderRow, OrderTxn, ProductRow, RowKey,
    StockRow, StoreTx, effect_recorded, find_credit, find_order, find_player, find_product,
    find_setting, find_stock, forward_intent_id, insert_pos_order, insert_product_line,
    list_components, list_lots, list_order_line_ids, list_order_moves, list_order_wallet_txns,
    list_payment_ids, open_hold_qty,
};

use super::mutate::Written;
use super::session::{
    SettleTarget, Stamp, check_rev, in_window, insert_payments, invalid, load_payer,
    record_payer_txns, same_staff, undo_denied, validate_payments,
};
pub(super) use super::stock::StockFor;
use crate::invoice::tax::{Bill, Line, compute};

/// A bundle's component, as the sale found it.
pub(super) struct Part {
    pub(super) product: Option<ProductRow>,
    pub(super) qty: i64,
}

/// A sale line's product, with its components when it is a bundle.
pub(super) struct Item {
    pub(super) product: Option<ProductRow>,
    pub(super) parts: Vec<Part>,
}

/// The stock of a tracked product at the moment of the sale.
pub(super) struct Held {
    pub(super) qty: i64,
    /// What may be sold: the quantity less lots past their expiry date and less the units open
    /// holds claim.
    pub(super) usable: i64,
}

/// What a set of sale lines reads from the branch: each line's product (and bundle parts), the
/// stock held and the tax settings.
pub(super) struct Basket {
    pub(super) items: Vec<Item>,
    pub(super) held: BTreeMap<String, Held>,
    pub(super) tax: TaxSettings,
}

pub(crate) struct SaleTarget {
    basket: Basket,
    player: bool,
    payer: SettleTarget,
    coupon: Option<super::coupon::CouponCtx>,
}

pub(super) fn internal(message: &str) -> AppError {
    AppError::new(ErrorCode::Internal, message)
}

pub(super) fn load_held(
    tx: &arena_store::DbConn,
    product_id: &str,
    today: Option<&str>,
) -> Result<Held, AppError> {
    let qty = find_stock(tx, product_id)?.map_or(0, |s| s.qty);
    let expired: i64 = list_lots(tx, product_id)?
        .iter()
        .filter(|l| match (&l.expiry, today) {
            (Some(e), Some(day)) => lot_expired(e, day),
            _ => false,
        })
        .map(|l| l.qty_remaining)
        .sum();
    let held = open_hold_qty(tx, product_id)?;
    Ok(Held {
        qty,
        usable: (qty - expired - held).max(0),
    })
}

pub(super) fn load_basket(
    tx: &arena_store::DbConn,
    lines: &[PosSaleLine],
    now_ms: i64,
) -> Result<Basket, AppError> {
    let tax = match find_setting(tx, KEY_TAX)? {
        Some(row) => TaxSettings::parse(&row.value)
            .ok_or_else(|| internal("the tax settings cannot be read"))?,
        None => super::validate::default_tax(),
    };
    let today = crate::locale::stamp(tx, now_ms)?.map(|(day, _)| day);
    let mut items = Vec::new();
    let mut held = BTreeMap::new();
    for line in lines.iter().take(MAX_SALE_LINES + 1) {
        let product = find_product(tx, &line.product_id)?;
        let mut parts = Vec::new();
        if let Some(p) = product.as_ref().filter(|p| p.bundle) {
            for (id, qty) in list_components(tx, &p.product_id)? {
                parts.push(Part {
                    product: find_product(tx, &id)?,
                    qty,
                });
            }
        }
        let tracked = std::iter::once(product.as_ref())
            .chain(parts.iter().map(|x| x.product.as_ref()))
            .flatten()
            .filter(|p| !p.bundle && p.track_stock);
        for p in tracked {
            if !held.contains_key(&p.product_id) {
                held.insert(
                    p.product_id.clone(),
                    load_held(tx, &p.product_id, today.as_deref())?,
                );
            }
        }
        items.push(Item { product, parts });
    }
    Ok(Basket { items, held, tax })
}

pub(super) fn load_sale(
    tx: &StoreTx<'_>,
    s: &PosSale,
    now_ms: i64,
) -> Result<SaleTarget, AppError> {
    let basket = load_basket(tx, &s.lines, now_ms)?;
    let player_id = s.player_id.as_deref();
    let player = match player_id {
        Some(id) => find_player(tx, id)?.is_some(),
        None => false,
    };
    let payer = load_payer(tx, player_id.filter(|_| player), &s.payments, now_ms)?;
    let coupon = s
        .coupon
        .as_deref()
        .map(|code| super::coupon::load_ctx(tx, code, now_ms))
        .transpose()?;
    Ok(SaleTarget {
        basket,
        player,
        payer,
        coupon,
    })
}

fn sellable(p: &Option<ProductRow>) -> bool {
    p.as_ref()
        .is_some_and(|p| p.active && p.deleted_at.is_none())
}

/// The shape of the lines and that every product (and bundle component) can be sold.
pub(super) fn check_lines(lines: &[PosSaleLine], t: &Basket) -> Result<(), AppError> {
    if lines.is_empty() || lines.len() > MAX_SALE_LINES {
        return Err(invalid("lines", "a sale takes 1 to 100 lines"));
    }
    let mut seen = BTreeSet::new();
    for (i, (line, item)) in lines.iter().zip(&t.items).enumerate() {
        let field = format!("lines[{i}]");
        if !(1..=MAX_SALE_QTY).contains(&line.qty) {
            return Err(invalid(&field, "quantity must be 1 to 999"));
        }
        if !seen.insert(line.product_id.as_str()) {
            return Err(invalid(&field, "each product appears once"));
        }
        if !sellable(&item.product) {
            return Err(invalid(&field, "this product cannot be sold"));
        }
        if item.parts.iter().any(|part| !sellable(&part.product)) {
            return Err(invalid(&field, "a part of this bundle cannot be sold"));
        }
    }
    Ok(())
}

/// What the sale costs: each line at its unit price and rate, the tax computed once per rate.
pub(super) struct Priced {
    pub(super) bill: Bill,
    /// Per line: the rate it was taxed at and its amount.
    pub(super) lines: Vec<(u16, i64)>,
}

pub(super) fn price(sale: &[PosSaleLine], t: &Basket) -> Result<Priced, AppError> {
    price_net(sale, t, &[])
}

/// The sale priced with the coupon in `ctx` and the manual `discount` (when there are any): the net
/// bill and what was taken off. A coupon the bill cannot take is `COUPON_INVALID`, a discount it
/// cannot take `VALIDATION_FAILED` on `discount`.
pub(super) fn price_adjusted(
    sale: &[PosSaleLine],
    t: &Basket,
    ctx: Option<&super::coupon::CouponCtx>,
    discount: Option<&arena_proto::ManualDiscount>,
) -> Result<(Priced, super::discount::Adjust), AppError> {
    let gross = price(sale, t)?;
    if ctx.is_none() && discount.is_none() {
        return Ok((gross, super::discount::Adjust::default()));
    }
    let lines = super::coupon::sale_lines(sale, &gross.lines);
    let adjust = super::discount::price(ctx, discount, &lines, gross.bill.total_minor, false)?;
    Ok((price_net(sale, t, &adjust.shares())?, adjust))
}

/// [`price`] with the coupon and discount shares taken off before tax: each rate's share reduces
/// that rate's taxable amount, and tax is computed once per rate on what is left.
fn price_net(sale: &[PosSaleLine], t: &Basket, shares: &[(u16, i64)]) -> Result<Priced, AppError> {
    let gst = t.tax.tax_mode() == TaxMode::GstIn;
    let mut lines = Vec::with_capacity(sale.len());
    let mut taxed = Vec::with_capacity(sale.len());
    for (i, (line, item)) in sale.iter().zip(&t.items).enumerate() {
        let p = item
            .product
            .as_ref()
            .ok_or_else(|| internal("a sale line priced without its product"))?;
        let rate_bp = if gst {
            u16::try_from(p.tax_rate_bp).unwrap_or(0)
        } else {
            t.tax.rate_bp
        };
        let amount = p
            .price_minor
            .checked_mul(line.qty)
            .ok_or_else(|| invalid(&format!("lines[{i}]"), "the amount is too large"))?;
        lines.push((rate_bp, amount));
        taxed.push(Line {
            rate_bp,
            amount_minor: amount,
        });
    }
    for (rate_bp, share) in shares {
        taxed.push(Line {
            rate_bp: *rate_bp,
            amount_minor: -share,
        });
    }
    let bill = compute(&taxed, t.tax.inclusive, gst)
        .ok_or_else(|| invalid("lines", "the total is too large"))?;
    Ok(Priced { bill, lines })
}

/// What the sale does to stock: units taken per product, the units an override raises first and the
/// units held for lines that are not served yet.
pub(super) struct StockPlan {
    pub(super) takes: Vec<(String, i64)>,
    pub(super) raises: BTreeMap<String, i64>,
    pub(super) holds: Vec<PlannedHold>,
}

/// One tracked item of a deferred line whose stock is held instead of taken.
pub(super) struct PlannedHold {
    /// The index of the line in the order's lines.
    pub(super) line: usize,
    pub(super) product_id: String,
    pub(super) qty: i64,
}

/// Plans a sale's stock. `defer[i]` is true for a line whose stock is held until it is served;
/// an empty slice defers nothing. Every line is checked against what is available (on hand less
/// expired lots less open holds), deferred or not.
pub(super) fn plan_stock(
    sale: &[PosSaleLine],
    t: &Basket,
    over: bool,
    defer: &[bool],
) -> Result<StockPlan, AppError> {
    let tracked = |p: &ProductRow| !p.bundle && p.track_stock;
    let mut need: BTreeMap<&str, i64> = BTreeMap::new();
    let mut takes_need: BTreeMap<&str, i64> = BTreeMap::new();
    let mut order: Vec<&str> = Vec::new();
    let mut in_bundle: BTreeSet<&str> = BTreeSet::new();
    let mut holds = Vec::new();
    // What the plain lines alone ask of a product: a bundle's part may be raised by an
    // override, a plain line never.
    let mut plain: BTreeMap<&str, i64> = BTreeMap::new();
    for (i, (line, item)) in sale.iter().zip(&t.items).enumerate() {
        let Some(p) = &item.product else { continue };
        let deferred = defer.get(i).copied().unwrap_or(false);
        let demand: Vec<(&ProductRow, i64)> = if p.bundle {
            item.parts
                .iter()
                .filter_map(|part| {
                    part.product
                        .as_ref()
                        .filter(|c| tracked(c))
                        .map(|c| (c, part.qty * line.qty))
                })
                .collect()
        } else if tracked(p) {
            vec![(p, line.qty)]
        } else {
            Vec::new()
        };
        for (c, qty) in demand {
            *need.entry(c.product_id.as_str()).or_default() += qty;
            if p.bundle {
                in_bundle.insert(c.product_id.as_str());
            } else {
                *plain.entry(c.product_id.as_str()).or_default() += qty;
            }
            if deferred {
                holds.push(PlannedHold {
                    line: i,
                    product_id: c.product_id.clone(),
                    qty,
                });
            } else {
                if !takes_need.contains_key(c.product_id.as_str()) {
                    order.push(c.product_id.as_str());
                }
                *takes_need.entry(c.product_id.as_str()).or_default() += qty;
            }
        }
    }
    let mut raises = BTreeMap::new();
    for (i, (_, item)) in sale.iter().zip(&t.items).enumerate() {
        let Some(p) = &item.product else { continue };
        let checked: Vec<&ProductRow> = if p.bundle {
            item.parts
                .iter()
                .filter_map(|x| x.product.as_ref())
                .filter(|c| tracked(c))
                .collect()
        } else if tracked(p) {
            vec![p]
        } else {
            Vec::new()
        };
        for c in checked {
            let usable = t.held.get(&c.product_id).map_or(0, |h| h.usable);
            let wanted = need.get(c.product_id.as_str()).copied().unwrap_or(0);
            if wanted <= usable {
                continue;
            }
            let field = format!("lines[{i}]");
            let plain_wanted = plain.get(c.product_id.as_str()).copied().unwrap_or(0);
            if !in_bundle.contains(c.product_id.as_str()) || plain_wanted > usable {
                return Err(AppError::with_field(
                    ErrorCode::OutOfStock,
                    field,
                    format!("Only {usable} {} left.", c.name),
                ));
            }
            if !over {
                return Err(AppError::with_field(
                    ErrorCode::BundleShort,
                    field,
                    format!("Not enough {} for {}.", c.name, p.name),
                ));
            }
            raises.insert(c.product_id.clone(), wanted - usable);
        }
    }
    let takes = order
        .into_iter()
        .map(|id| (id.to_owned(), takes_need[id]))
        .collect();
    Ok(StockPlan {
        takes,
        raises,
        holds,
    })
}

pub(super) fn validate_sale(intent: &Intent, s: &PosSale, t: &SaleTarget) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "pos.sale takes no base_rev"));
    }
    check_lines(&s.lines, &t.basket)?;
    if s.player_id.is_some() && !t.player {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "player_id",
            "no such player",
        ));
    }
    let (priced, _) = price_adjusted(&s.lines, &t.basket, t.coupon.as_ref(), s.discount.as_ref())?;
    validate_payments(
        &s.payments,
        s.credit_override,
        &t.payer,
        s.player_id.is_some(),
        priced.bill.total_minor,
    )?;
    plan_stock(&s.lines, &t.basket, s.stock_override == Some(true), &[])?;
    Ok(())
}

pub(super) fn mutate_sale(
    tx: &StoreTx<'_>,
    intent: &Intent,
    s: &PosSale,
    t: &SaleTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let (business_day, fy) = crate::locale::stamp(tx, now_ms)?
        .ok_or_else(|| internal("the branch has not been bootstrapped"))?;
    let (priced, adjust) =
        price_adjusted(&s.lines, &t.basket, t.coupon.as_ref(), s.discount.as_ref())?;
    let plan = plan_stock(&s.lines, &t.basket, s.stock_override == Some(true), &[])?;
    let breakup = serde_json::to_string(&priced.bill.breakup)
        .map_err(|_| internal("the tax breakup cannot be written"))?;
    let order_id = ulid::Ulid::generate().to_string();
    insert_pos_order(
        tx,
        &NewPosOrder {
            id: &order_id,
            player_id: s.player_id.as_deref(),
            subtotal_minor: priced.bill.subtotal_minor,
            tax_minor: priced.bill.tax_minor,
            total_minor: priced.bill.total_minor,
            tax_breakup: &breakup,
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
    for (i, ((line, item), (rate_bp, amount))) in s
        .lines
        .iter()
        .zip(&t.basket.items)
        .zip(&priced.lines)
        .enumerate()
    {
        let p = item
            .product
            .as_ref()
            .ok_or_else(|| internal("a sale line written without its product"))?;
        let id = ulid::Ulid::generate().to_string();
        insert_product_line(
            tx,
            &NewProductLine {
                id: &id,
                order_id: &order_id,
                product_id: &p.product_id,
                label: &p.name,
                qty: line.qty,
                unit_price_minor: p.price_minor,
                tax_rate_bp: i64::from(*rate_bp),
                amount_minor: *amount,
                position: i as i64,
                at: now_ms,
                placement: None,
                served: true,
            },
        )?;
        written.push(Written {
            key: RowKey::OrderLines { id },
            op: ChangeOp::Insert,
            before: None,
        });
    }
    if let Some(q) = &adjust.coupon {
        super::coupon::redeem(tx, q, &order_id, false, &mut written)?;
    }
    if let Some(d) = &adjust.discount {
        let by = intent
            .elevated_by
            .as_ref()
            .map_or(intent.actor.id.as_str(), |e| e.id.as_str());
        super::discount::record(tx, d, &order_id, by, false)?;
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
        s.player_id.as_deref(),
        t.payer.credit.as_ref(),
        &mut written,
    )?;
    move_stock(
        tx,
        intent,
        &t.basket,
        &plan,
        &StockFor {
            order_id: &order_id,
            placement_id: None,
            today: &business_day,
        },
        now_ms,
        &mut written,
    )?;
    Ok(written)
}

/// Carries out a stock plan: each override raise first, then each take from the lots, with the
/// low-stock hook after a product changes.
pub(super) fn move_stock(
    tx: &StoreTx<'_>,
    intent: &Intent,
    basket: &Basket,
    plan: &StockPlan,
    sale: &StockFor<'_>,
    now_ms: i64,
    written: &mut Vec<Written>,
) -> Result<(), AppError> {
    let elevated_by = intent.elevated_by.as_ref().map(|e| e.id.as_str());
    for (product_id, raise) in &plan.raises {
        written.extend(super::stock::raise_for_sale(
            tx,
            product_id,
            *raise,
            sale.order_id,
            &intent.actor.id,
            elevated_by,
            now_ms,
        )?);
    }
    for (product_id, qty) in &plan.takes {
        written.extend(super::stock::take_for_sale(
            tx,
            product_id,
            *qty,
            sale,
            &intent.actor.id,
            now_ms,
        )?);
        let before = basket.held.get(product_id).map_or(0, |h| h.qty);
        let after = before + plan.raises.get(product_id).copied().unwrap_or(0) - qty;
        written.extend(super::stock::low_after_change(
            tx,
            product_id,
            before,
            after,
            &intent.actor.id,
            now_ms,
        )?);
    }
    Ok(())
}

#[derive(Default)]
pub(crate) struct RevertTarget {
    order: Option<OrderRow>,
    payments: Vec<String>,
    lines: Vec<String>,
    txns: Vec<OrderTxn>,
    moves: Vec<MoveRow>,
    stocks: BTreeMap<String, StockRow>,
    lots: BTreeMap<String, Vec<LotRow>>,
    credit_blocked: bool,
    effect: bool,
    now: i64,
}

pub(super) fn load_sale_reverted(
    tx: &StoreTx<'_>,
    r: &PosSaleReverted,
    now: i64,
) -> Result<RevertTarget, AppError> {
    let Some(order) = find_order(tx, &r.order_id)? else {
        return Ok(RevertTarget {
            now,
            ..RevertTarget::default()
        });
    };
    let forward = forward_intent_id(tx, "orders", &order.id, "pos.sale")?;
    let txns = list_order_wallet_txns(tx, &order.id)?;
    let mut credit_blocked = false;
    for txn in txns.iter().filter(|t| t.reason == "credit") {
        let owed = find_credit(tx, &txn.player_id)?.map_or(0, |c| c.outstanding_minor);
        credit_blocked |= owed < txn.delta_minor;
    }
    let moves = list_order_moves(tx, &order.id, MOVE_SALE)?;
    let mut stocks = BTreeMap::new();
    let mut lots = BTreeMap::new();
    for m in &moves {
        if let Some(stock) = find_stock(tx, &m.product_id)? {
            stocks.insert(m.product_id.clone(), stock);
        }
        lots.insert(m.product_id.clone(), list_lots(tx, &m.product_id)?);
    }
    Ok(RevertTarget {
        payments: list_payment_ids(tx, &order.id)?,
        lines: list_order_line_ids(tx, &order.id)?,
        txns,
        moves,
        stocks,
        lots,
        credit_blocked,
        effect: effect_recorded(tx, forward.as_deref(), &order.id)?,
        order: Some(order),
        now,
    })
}

pub(super) fn base_rev_sale_reverted(intent: &Intent, t: &RevertTarget) -> Result<(), AppError> {
    let Some(o) = &t.order else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "order_id",
            "no such bill",
        ));
    };
    check_rev(intent, o.rev, "pos.sale.reverted")
}

pub(super) fn validate_sale_reverted(intent: &Intent, t: &RevertTarget) -> Result<(), AppError> {
    let Some(o) = &t.order else {
        return Err(invalid("order_id", "no such bill"));
    };
    if o.kind != "pos_sale" {
        return Err(invalid("order_id", "this bill is not a counter sale"));
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
    if !super::stock::sale_restorable(&t.moves, &t.stocks, &t.lots) {
        return Err(undo_denied("the stock has changed since the sale"));
    }
    Ok(())
}

pub(super) fn mutate_sale_reverted(
    tx: &StoreTx<'_>,
    intent: &Intent,
    t: &RevertTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let Some(o) = &t.order else {
        return Err(internal("an undo validated without its target"));
    };
    let mut written = Vec::with_capacity(t.payments.len() + t.txns.len() + 3);
    super::shift::reverse_sales(
        tx,
        o.settled_by.as_deref().unwrap_or_default(),
        &o.id,
        &mut written,
    )?;
    super::receipt_notify::skip_on_revert(tx, &o.id, &mut written)?;
    let (stock_rows, changed) =
        super::stock::restore_sale_moves(tx, &t.moves, &o.id, None, &intent.actor.id, now_ms)?;
    written.extend(stock_rows);
    for (product_id, before, after) in changed {
        written.extend(super::stock::low_after_change(
            tx,
            &product_id,
            before,
            after,
            &intent.actor.id,
            now_ms,
        )?);
    }
    super::coupon::release_on_revert(tx, &o.id, false, &mut written)?;
    super::plan_sales::delete_sale_rows(tx, &o.id, &t.payments, &t.txns, &t.lines, &mut written)?;
    Ok(written)
}
