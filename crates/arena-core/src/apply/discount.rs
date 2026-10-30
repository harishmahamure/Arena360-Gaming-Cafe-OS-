//! Manual discounts (Story 5.22). A settle that carries a `discount` takes it off after the coupon,
//! before tax, spread over the tax rates in proportion to what is left of each (largest remainder
//! in minor units, as for a coupon). The actor needs `discount.manual`, or a manager whose PIN
//! approved the intent does (checked in `guards`). The amount, reason, note and approver are
//! written on the order; Undo of a session settle puts the amount back on the total.

use std::collections::BTreeMap;

use arena_proto::{
    AppError, DISCOUNT_MAX_PERCENT_BP, DISCOUNT_NOTE_MAX_CHARS, DiscountPreview,
    DiscountPreviewRequest, DiscountReason, ErrorCode, MAX_PRICE_MINOR, ManualDiscount,
    OwnerAlertKind,
};
use arena_store::{
    CouponLineRow, DbConn, clear_order_discount, find_order, find_plan, list_coupon_lines,
    list_payment_ids, set_order_discount,
};

use super::coupon::{self, CouponBill, CouponCtx, Quote};
use super::mutate::Written;
use super::owner_alert;

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

/// A manual discount priced on a bill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Taken {
    pub(crate) minor: i64,
    pub(crate) reason: DiscountReason,
    /// The text of an `other` reason, trimmed.
    pub(crate) note: Option<String>,
    /// Per tax rate (ascending): the part of the discount taken off the lines at that rate.
    pub(crate) shares: Vec<(u16, i64)>,
    /// The bill the discount came off, after the coupon and before the discount.
    pub(crate) before_minor: i64,
}

/// What a bill's coupon and manual discount take off it together.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Adjust {
    pub(crate) coupon: Option<Quote>,
    pub(crate) discount: Option<Taken>,
}

impl Adjust {
    /// Everything taken off the bill before tax.
    pub(crate) fn off(&self) -> i64 {
        self.coupon.as_ref().map_or(0, |q| q.discount_minor)
            + self.discount.as_ref().map_or(0, |d| d.minor)
    }

    /// The amount taken off each tax rate (ascending), coupon and discount together.
    pub(crate) fn shares(&self) -> Vec<(u16, i64)> {
        let mut merged: BTreeMap<u16, i64> = BTreeMap::new();
        let parts = self
            .coupon
            .iter()
            .flat_map(|q| q.shares.iter())
            .chain(self.discount.iter().flat_map(|d| d.shares.iter()));
        for (rate, share) in parts {
            *merged.entry(*rate).or_default() += share;
        }
        merged.into_iter().collect()
    }
}

/// The shape of a discount: a percent or an amount (not both) and a reason, with text for `other`.
/// Returns the trimmed note to store.
fn check(d: &ManualDiscount) -> Result<Option<String>, AppError> {
    match (d.percent_bp, d.amount_minor) {
        (Some(_), Some(_)) => {
            return Err(invalid(
                "discount.percent_bp",
                "a discount takes a percent or an amount off, not both",
            ));
        }
        (None, None) => {
            return Err(invalid(
                "discount.percent_bp",
                "a discount takes a percent or an amount off",
            ));
        }
        (Some(bp), None) if !(1..=DISCOUNT_MAX_PERCENT_BP).contains(&bp) => {
            return Err(invalid(
                "discount.percent_bp",
                "a percent discount is 0.01% to 100%",
            ));
        }
        (None, Some(amount)) if !(1..=MAX_PRICE_MINOR).contains(&amount) => {
            return Err(invalid(
                "discount.amount_minor",
                "the amount must be above zero",
            ));
        }
        _ => {}
    }
    if d.reason != DiscountReason::Other {
        return Ok(None);
    }
    let note = d.note.as_deref().map(str::trim).unwrap_or_default();
    if note.is_empty() {
        return Err(invalid("discount.note", "say why the discount is given"));
    }
    if note.chars().count() > DISCOUNT_NOTE_MAX_CHARS || note.chars().any(char::is_control) {
        return Err(invalid(
            "discount.note",
            "the reason is plain text of up to 200 characters",
        ));
    }
    Ok(Some(note.to_owned()))
}

/// The most the discount may take: what is left of the lines after the coupon, per rate.
fn left_after(
    lines: &[CouponLineRow],
    bill_total: i64,
    coupon: Option<&Quote>,
) -> (BTreeMap<u16, i128>, i128) {
    let whole = [CouponLineRow {
        amount_minor: bill_total,
        rate_bp: 0,
        group_id: None,
        plan_id: None,
        product_id: None,
    }];
    let lines = if lines.is_empty() { &whole[..] } else { lines };
    let mut per_rate: BTreeMap<u16, i128> = BTreeMap::new();
    for line in lines {
        *per_rate
            .entry(u16::try_from(line.rate_bp).unwrap_or(0))
            .or_default() += i128::from(line.amount_minor.max(0));
    }
    for (rate, share) in coupon.iter().flat_map(|q| q.shares.iter()) {
        if let Some(left) = per_rate.get_mut(rate) {
            *left = (*left - i128::from(*share)).max(0);
        }
    }
    let base = per_rate.values().sum();
    (per_rate, base)
}

/// Prices `d` on a bill of `lines` whose total before the coupon is `bill_total`, after `coupon`.
pub(crate) fn quote(
    d: &ManualDiscount,
    lines: &[CouponLineRow],
    bill_total: i64,
    coupon: Option<&Quote>,
    partly_paid: bool,
) -> Result<Taken, AppError> {
    let note = check(d)?;
    if partly_paid {
        return Err(invalid("discount", "this bill is partly paid"));
    }
    let (left, base) = left_after(lines, bill_total, coupon);
    let after_coupon = i128::from(bill_total) - i128::from(coupon.map_or(0, |q| q.discount_minor));
    let cap = base.min(after_coupon);
    if cap <= 0 {
        return Err(invalid("discount", "there is nothing left to discount"));
    }
    let wanted = match (d.percent_bp, d.amount_minor) {
        (Some(bp), _) => ((base * i128::from(bp) + 5_000) / 10_000).min(cap),
        (None, Some(amount)) if i128::from(amount) > cap => {
            return Err(invalid(
                "discount.amount_minor",
                "the discount is more than the bill",
            ));
        }
        (None, Some(amount)) => i128::from(amount),
        (None, None) => 0,
    };
    if wanted <= 0 {
        return Err(invalid(
            "discount",
            "this discount would not take anything off the bill",
        ));
    }
    Ok(Taken {
        minor: i64::try_from(wanted)
            .map_err(|_| invalid("discount", "the discount is too large"))?,
        reason: d.reason,
        note,
        shares: coupon::spread(wanted, base, &left),
        before_minor: i64::try_from(after_coupon).unwrap_or(i64::MAX),
    })
}

/// The coupon (when `ctx` names one) and then the manual discount (when there is one) on a bill.
/// A coupon the bill cannot take is `COUPON_INVALID`; a discount it cannot take is
/// `VALIDATION_FAILED` on `discount`. The bill is unchanged on any refusal.
pub(crate) fn price(
    ctx: Option<&CouponCtx>,
    discount: Option<&ManualDiscount>,
    lines: &[CouponLineRow],
    bill_total: i64,
    partly_paid: bool,
) -> Result<Adjust, AppError> {
    let coupon = ctx
        .map(|c| coupon::quote(c, lines, bill_total, partly_paid))
        .transpose()?;
    let discount = discount
        .map(|d| quote(d, lines, bill_total, coupon.as_ref(), partly_paid))
        .transpose()?;
    Ok(Adjust { coupon, discount })
}

/// Writes the discount on the order at settle. A session bill (`reduce_totals`) is lowered by it;
/// a counter or plan sale is written net already. `by` is the approving staff member. A discount
/// over the owner's amount or percent limit also queues the owner's `large_discount` alert,
/// tied to the order so the Undo of the settle skips it.
pub(super) fn record(
    tx: &arena_store::StoreTx<'_>,
    taken: &Taken,
    order_id: &str,
    by: &str,
    reduce_totals: bool,
    now_ms: i64,
    written: &mut Vec<Written>,
) -> Result<(), AppError> {
    set_order_discount(
        tx,
        order_id,
        taken.minor,
        taken.reason.as_str(),
        taken.note.as_deref(),
        by,
        reduce_totals,
    )?;
    let limits = owner_alert::thresholds(tx)?;
    let share_of_bill = i128::from(taken.minor) * 100
        > i128::from(limits.large_discount_percent) * i128::from(taken.before_minor);
    if taken.minor > limits.large_discount_minor || share_of_bill {
        owner_alert::queue(
            tx,
            &owner_alert::Alert {
                kind: OwnerAlertKind::LargeDiscount,
                source_ref: order_id,
                order_id: Some(order_id),
                player_id: "",
                params: BTreeMap::from([("amount_minor", taken.minor.to_string())]),
            },
            now_ms,
            written,
        )?;
    }
    Ok(())
}

/// Undo of a session settle that took a discount: the amount goes back onto the bill's total and
/// the discount columns clear. A counter or plan sale keeps its record (it is reverted whole).
pub(super) fn release_on_revert(
    tx: &arena_store::StoreTx<'_>,
    order_id: &str,
    restore_totals: bool,
) -> Result<(), AppError> {
    if restore_totals {
        clear_order_discount(tx, order_id)?;
    }
    Ok(())
}

/// `POST /ui/discount/preview`: what the discount (after the coupon the request names, if any)
/// would do to the bill the request names, by the same pricing the settle uses. Nothing is written.
pub fn preview(
    conn: &DbConn,
    req: &DiscountPreviewRequest,
    now_ms: i64,
) -> Result<DiscountPreview, AppError> {
    let ctx = req
        .coupon
        .as_deref()
        .map(|code| coupon::load_ctx(conn, code, now_ms))
        .transpose()?;
    let sources = [
        req.order_id.is_some(),
        req.lines.is_some(),
        req.plan_id.is_some(),
    ];
    if sources.iter().filter(|s| **s).count() != 1 {
        return Err(invalid(
            "order_id",
            "name one bill: an order, a cart or a plan",
        ));
    }
    let discount = Some(&req.discount);
    let (adjust, total) = if let Some(order_id) = &req.order_id {
        let order =
            find_order(conn, order_id)?.ok_or_else(|| invalid("order_id", "no such bill"))?;
        if order.status != "open" || order.kind != "session" {
            return Err(invalid("order_id", "this bill is not open"));
        }
        let lines = list_coupon_lines(conn, order_id)?;
        let partly_paid = !list_payment_ids(conn, order_id)?.is_empty();
        let a = price(
            ctx.as_ref(),
            discount,
            &lines,
            order.total_minor,
            partly_paid,
        )?;
        let total = order.total_minor - a.off();
        (a, total)
    } else if let Some(lines) = &req.lines {
        let basket = super::pos_sale::load_basket(conn, lines, now_ms)?;
        super::pos_sale::check_lines(lines, &basket)?;
        let (priced, a) = super::pos_sale::price_adjusted(lines, &basket, ctx.as_ref(), discount)?;
        (a, priced.bill.total_minor)
    } else {
        let plan_id = req.plan_id.as_deref().unwrap_or_default();
        let plan = find_plan(conn, plan_id)?
            .filter(|p| p.active)
            .ok_or_else(|| invalid("plan_id", "no such plan"))?;
        let lines = [CouponLineRow {
            amount_minor: plan.price_minor,
            rate_bp: 0,
            group_id: None,
            plan_id: Some(plan.plan_id.clone()),
            product_id: None,
        }];
        let a = price(ctx.as_ref(), discount, &lines, plan.price_minor, false)?;
        let total = plan.price_minor - a.off();
        (a, total)
    };
    Ok(DiscountPreview {
        discount_minor: adjust.discount.map_or(0, |d| d.minor),
        total_minor: total,
    })
}

impl CouponBill {
    /// The coupon and discount on a bill whose total before them is `total_minor`.
    pub(super) fn price(
        &self,
        total_minor: i64,
        discount: Option<&ManualDiscount>,
    ) -> Result<Adjust, AppError> {
        price(
            self.ctx(),
            discount,
            self.lines(),
            total_minor,
            self.partly_paid(),
        )
    }
}
