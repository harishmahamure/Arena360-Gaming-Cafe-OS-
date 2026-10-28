//! Coupons (Story 5.21). `coupon.upsert` stores a coupon and its targets; a settle that names a
//! `coupon` code prices it inside its own transaction ([`quote`]): the discount lands on the
//! eligible lines only, before tax, spread over their tax rates by largest remainder. The settle
//! then raises `coupons.used` ([`redeem`]) and records the code and amount on the order; Undo gives
//! the use back ([`release_on_revert`]). Two settles racing for the last use are serialised by
//! `apply()`: the second finds `used` at the limit and is `COUPON_INVALID`.

use std::collections::BTreeMap;

use arena_proto::{
    ActorKind, AppError, COUPON_MAX_PERCENT_BP, COUPON_MAX_TARGETS, COUPON_TARGET_KINDS,
    CouponPreview, CouponPreviewRequest, CouponUpsert, ErrorCode, Intent, IntentPayload,
    MAX_PRICE_MINOR, PosSaleLine, coupon_code_normal, coupon_code_valid,
};
use arena_store::{
    ChangeOp, CouponLineRow, CouponRow, DbConn, RowKey, StoreTx, Table, clear_order_coupon,
    delete_row, find_coupon, find_coupon_by_code, find_group, find_order, find_plan, find_product,
    give_coupon_use, insert_coupon, insert_coupon_target, list_coupon_lines, list_coupon_targets,
    list_payment_ids, order_coupon, read_row, set_order_coupon, take_coupon_use, update_coupon,
};
use chrono::NaiveDate;

use super::mutate::Written;
use crate::pricing::parse_date;

const BP: i128 = 10_000;

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

/// A coupon the bill cannot take, with the reason staff read.
pub(crate) fn refuse(reason: &str) -> AppError {
    AppError::with_field(ErrorCode::CouponInvalid, "coupon", reason)
}

// ---------------------------------------------------------------------------------------------
// coupon.upsert
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
pub(crate) struct EditTarget {
    coupon: Option<CouponRow>,
    old_targets: Vec<(String, String)>,
    /// The coupon that already holds the code, if any.
    holder: Option<String>,
    /// Per requested target: whether it exists.
    found: Vec<bool>,
}

pub(super) fn load(tx: &StoreTx<'_>, p: &IntentPayload) -> Result<EditTarget, AppError> {
    let IntentPayload::CouponUpsert(u) = p else {
        return Ok(EditTarget::default());
    };
    let mut t = EditTarget::default();
    if let Some(id) = &u.id {
        t.coupon = find_coupon(tx, id)?;
        if t.coupon.is_some() {
            t.old_targets = list_coupon_targets(tx, id)?;
        }
    }
    t.holder = find_coupon_by_code(tx, &coupon_code_normal(&u.code))?.map(|c| c.coupon_id);
    for target in u.targets.iter().take(COUPON_MAX_TARGETS + 1) {
        t.found.push(match target.kind.as_str() {
            "group" => find_group(tx, &target.id)?.is_some_and(|g| g.deleted_at.is_none()),
            "plan" => find_plan(tx, &target.id)?.is_some(),
            "product" => find_product(tx, &target.id)?.is_some_and(|p| p.deleted_at.is_none()),
            _ => false,
        });
    }
    Ok(t)
}

pub(super) fn base_rev(intent: &Intent, t: &EditTarget) -> Result<(), AppError> {
    match (t.coupon.as_ref().map(|c| c.rev), intent.base_rev) {
        (Some(_), None) if intent.actor.kind != ActorKind::Owner => {
            Err(invalid("base_rev", "editing needs base_rev"))
        }
        (Some(rev), Some(base)) if rev != base => Err(AppError::with_field(
            ErrorCode::Stale,
            "base_rev",
            "this changed since it was read",
        )),
        _ => Ok(()),
    }
}

fn terms(u: &CouponUpsert) -> Result<(), AppError> {
    match (u.percent_bp, u.amount_minor) {
        (Some(_), Some(_)) => Err(invalid(
            "percent_bp",
            "a coupon takes a percent or an amount off, not both",
        )),
        (None, None) => Err(invalid(
            "percent_bp",
            "a coupon takes a percent or an amount off",
        )),
        (Some(bp), None) if !(1..=COUPON_MAX_PERCENT_BP).contains(&bp) => {
            Err(invalid("percent_bp", "a percent coupon is 0.01% to 100%"))
        }
        (None, Some(amount)) if !(1..=MAX_PRICE_MINOR).contains(&amount) => {
            Err(invalid("amount_minor", "the amount must be above zero"))
        }
        _ => Ok(()),
    }
}

fn window(u: &CouponUpsert) -> Result<(), AppError> {
    let date = |field: &str, v: &Option<String>| -> Result<Option<NaiveDate>, AppError> {
        v.as_deref()
            .map(|s| parse_date(s).ok_or_else(|| invalid(field, "use a date like 2027-03-10")))
            .transpose()
    };
    let from = date("valid_from", &u.valid_from)?;
    let to = date("valid_to", &u.valid_to)?;
    if let (Some(from), Some(to)) = (from, to)
        && to < from
    {
        return Err(invalid("valid_to", "the last date is before the first"));
    }
    Ok(())
}

pub(super) fn validate(u: &CouponUpsert, t: &EditTarget) -> Result<(), AppError> {
    let code = coupon_code_normal(&u.code);
    if code.is_empty() {
        return Err(invalid("code", "enter a code"));
    }
    if !coupon_code_valid(&code) {
        return Err(invalid("code", "a code is 3 to 24 letters, digits, - or _"));
    }
    if u.id.is_some() && t.coupon.is_none() {
        return Err(invalid("id", "no such coupon"));
    }
    if t.holder
        .as_deref()
        .is_some_and(|h| Some(h) != t.coupon.as_ref().map(|c| c.coupon_id.as_str()))
    {
        return Err(invalid("code", "this code is already used"));
    }
    terms(u)?;
    window(u)?;
    if u.usage_limit
        .is_some_and(|l| !(1..=1_000_000_000).contains(&l))
    {
        return Err(invalid("usage_limit", "the limit is at least 1"));
    }
    if !(0..=MAX_PRICE_MINOR).contains(&u.min_bill_minor) {
        return Err(invalid("min_bill_minor", "the minimum cannot be negative"));
    }
    if u.targets.len() > COUPON_MAX_TARGETS {
        return Err(invalid("targets", "too many targets"));
    }
    for (i, target) in u.targets.iter().enumerate() {
        if !COUPON_TARGET_KINDS.contains(&target.kind.as_str()) {
            return Err(invalid(
                "targets",
                "a target is a group, a plan or a product",
            ));
        }
        if !t.found.get(i).copied().unwrap_or(false) {
            return Err(invalid("targets", "no such group, plan or product"));
        }
        if u.targets[..i].contains(target) {
            return Err(invalid("targets", "a target is listed twice"));
        }
    }
    Ok(())
}

fn target_write(
    coupon_id: &str,
    kind: &str,
    id: &str,
    op: ChangeOp,
    before: Option<arena_store::Row>,
) -> Written {
    Written {
        key: RowKey::CouponTargets {
            coupon_id: coupon_id.to_owned(),
            target_kind: kind.to_owned(),
            target_id: id.to_owned(),
        },
        op,
        before,
    }
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    u: &CouponUpsert,
    t: &EditTarget,
) -> Result<Vec<Written>, AppError> {
    let (id, op) = match &t.coupon {
        Some(old) => (old.coupon_id.clone(), ChangeOp::Update),
        None => (ulid::Ulid::generate().to_string(), ChangeOp::Insert),
    };
    let row = CouponRow {
        coupon_id: id.clone(),
        code: coupon_code_normal(&u.code),
        percent_bp: u.percent_bp,
        amount_minor: u.amount_minor,
        valid_from: u.valid_from.clone(),
        valid_to: u.valid_to.clone(),
        usage_limit: u.usage_limit,
        used: 0,
        min_bill_minor: u.min_bill_minor,
        active: u.active,
        rev: 0,
    };
    match op {
        ChangeOp::Update => update_coupon(tx, &row)?,
        _ => insert_coupon(tx, &row)?,
    }
    let mut out = vec![Written {
        key: RowKey::Coupons {
            coupon_id: id.clone(),
        },
        op,
        before: None,
    }];
    let wanted: Vec<(String, String)> = u
        .targets
        .iter()
        .map(|x| (x.kind.clone(), x.id.clone()))
        .collect();
    for (kind, target_id) in t.old_targets.iter().filter(|o| !wanted.contains(o)) {
        let key = RowKey::CouponTargets {
            coupon_id: id.clone(),
            target_kind: kind.clone(),
            target_id: target_id.clone(),
        };
        let before = read_row(tx, &key)?;
        delete_row(tx, Table::CouponTargets, &before)?;
        out.push(target_write(
            &id,
            kind,
            target_id,
            ChangeOp::Delete,
            Some(before),
        ));
    }
    for (kind, target_id) in wanted.iter().filter(|w| !t.old_targets.contains(w)) {
        insert_coupon_target(tx, &id, kind, target_id)?;
        out.push(target_write(&id, kind, target_id, ChangeOp::Insert, None));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Pricing a coupon on a bill
// ---------------------------------------------------------------------------------------------

/// The coupon a settle names, as read inside the transaction.
pub(crate) struct CouponCtx {
    row: Option<CouponRow>,
    targets: Vec<(String, String)>,
    today: Option<NaiveDate>,
    /// The minimum bill written in the branch's money, for the refusal text.
    min_text: String,
}

/// What a coupon is applied to: its context, the bill's lines and whether the bill is part-paid.
pub(crate) struct CouponBill {
    ctx: CouponCtx,
    lines: Vec<CouponLineRow>,
    partly_paid: bool,
}

/// The discount a coupon gives a bill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Quote {
    pub(crate) coupon_id: String,
    pub(crate) code: String,
    pub(crate) discount_minor: i64,
    /// Per tax rate (ascending): the part of the discount taken off the lines at that rate.
    pub(crate) shares: Vec<(u16, i64)>,
}

fn local_today(conn: &DbConn, now_ms: i64) -> Result<Option<NaiveDate>, AppError> {
    Ok(crate::locale::tz_at(conn, now_ms)?.and_then(|tz| {
        chrono::DateTime::from_timestamp_millis(now_ms).map(|t| t.with_timezone(&tz).date_naive())
    }))
}

/// Reads the coupon `code` (any case) with its targets and the branch's local date.
pub(super) fn load_ctx(conn: &DbConn, code: &str, now_ms: i64) -> Result<CouponCtx, AppError> {
    let row = find_coupon_by_code(conn, &coupon_code_normal(code))?;
    let targets = match &row {
        Some(r) => list_coupon_targets(conn, &r.coupon_id)?,
        None => Vec::new(),
    };
    let min_text = match &row {
        Some(r) if r.min_bill_minor > 0 => {
            crate::locale::format_money(conn, now_ms, r.min_bill_minor)?
        }
        _ => String::new(),
    };
    Ok(CouponCtx {
        row,
        targets,
        today: local_today(conn, now_ms)?,
        min_text,
    })
}

impl CouponCtx {
    fn eligible(&self, line: &CouponLineRow) -> bool {
        self.targets.is_empty()
            || self.targets.iter().any(|(kind, id)| {
                let held = match kind.as_str() {
                    "group" => &line.group_id,
                    "plan" => &line.plan_id,
                    "product" => &line.product_id,
                    _ => return false,
                };
                held.as_deref() == Some(id.as_str())
            })
    }
}

/// Prices `ctx` on a bill of `lines` whose total before the coupon is `bill_total`. The bill is
/// unchanged on any refusal.
pub(crate) fn quote(
    ctx: &CouponCtx,
    lines: &[CouponLineRow],
    bill_total: i64,
    partly_paid: bool,
) -> Result<Quote, AppError> {
    let c = ctx
        .row
        .as_ref()
        .ok_or_else(|| refuse("this coupon code does not exist"))?;
    if !c.active {
        return Err(refuse("this coupon is not active"));
    }
    if let Some(today) = ctx.today {
        let day = |v: &Option<String>| v.as_deref().and_then(parse_date);
        if day(&c.valid_from).is_some_and(|from| today < from) {
            return Err(refuse("this coupon is not valid yet"));
        }
        if day(&c.valid_to).is_some_and(|to| today > to) {
            return Err(refuse("this coupon has expired"));
        }
    }
    if c.usage_limit.is_some_and(|l| c.used >= l) {
        return Err(refuse("this coupon has been used up"));
    }
    if partly_paid {
        return Err(refuse("this bill is partly paid"));
    }
    if bill_total <= 0 {
        return Err(refuse("there is nothing to pay on this bill"));
    }
    if bill_total < c.min_bill_minor {
        return Err(refuse(&format!(
            "the bill is under this coupon's minimum of {}",
            ctx.min_text
        )));
    }
    let whole = [CouponLineRow {
        amount_minor: bill_total,
        rate_bp: 0,
        group_id: None,
        plan_id: None,
        product_id: None,
    }];
    let lines = if lines.is_empty() { &whole[..] } else { lines };
    let mut per_rate: BTreeMap<u16, i128> = BTreeMap::new();
    for line in lines.iter().filter(|l| ctx.eligible(l)) {
        *per_rate
            .entry(u16::try_from(line.rate_bp).unwrap_or(0))
            .or_default() += i128::from(line.amount_minor.max(0));
    }
    let eligible: i128 = per_rate.values().sum();
    if !ctx.targets.is_empty() && per_rate.is_empty() {
        return Err(refuse(
            "this coupon does not apply to anything on this bill",
        ));
    }
    if eligible <= 0 {
        return Err(refuse("there is nothing to discount on this bill"));
    }
    let wanted = match (c.percent_bp, c.amount_minor) {
        (Some(bp), _) => (eligible * i128::from(bp) + BP / 2) / BP,
        (None, Some(amount)) => i128::from(amount),
        (None, None) => 0,
    };
    let discount = wanted.min(eligible);
    if discount <= 0 {
        return Err(refuse("this coupon would not take anything off this bill"));
    }
    Ok(Quote {
        coupon_id: c.coupon_id.clone(),
        code: c.code.clone(),
        discount_minor: i64::try_from(discount).map_err(|_| refuse("the discount is too large"))?,
        shares: spread(discount, eligible, &per_rate),
    })
}

/// Splits `discount` over the rates in proportion to their eligible amounts: each rate gets the
/// floor of its exact share, and the minor units left over go one each to the rates with the
/// largest remainders (ties to the lower rate).
fn spread(discount: i128, eligible: i128, per_rate: &BTreeMap<u16, i128>) -> Vec<(u16, i64)> {
    let mut shares: Vec<(u16, i128, i128)> = per_rate
        .iter()
        .map(|(rate, amount)| {
            let exact = discount * amount;
            (*rate, exact / eligible, exact % eligible)
        })
        .collect();
    let left = discount - shares.iter().map(|s| s.1).sum::<i128>();
    let mut order: Vec<usize> = (0..shares.len()).collect();
    order.sort_by(|a, b| {
        shares[*b]
            .2
            .cmp(&shares[*a].2)
            .then(shares[*a].0.cmp(&shares[*b].0))
    });
    for i in order.into_iter().take(usize::try_from(left).unwrap_or(0)) {
        shares[i].1 += 1;
    }
    shares
        .into_iter()
        .map(|(rate, share, _)| (rate, i64::try_from(share).unwrap_or(0)))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// The three settles
// ---------------------------------------------------------------------------------------------

/// What a session bill's settle reads for its coupon, when it names one.
pub(super) fn load_session_bill(
    tx: &StoreTx<'_>,
    code: &str,
    order_id: &str,
    now_ms: i64,
) -> Result<CouponBill, AppError> {
    Ok(CouponBill {
        ctx: load_ctx(tx, code, now_ms)?,
        lines: list_coupon_lines(tx, order_id)?,
        partly_paid: !list_payment_ids(tx, order_id)?.is_empty(),
    })
}

/// What a plan sale's settle reads for its coupon.
pub(super) fn load_plan_bill(
    tx: &StoreTx<'_>,
    code: &str,
    plan_id: &str,
    price_minor: i64,
    now_ms: i64,
) -> Result<CouponBill, AppError> {
    Ok(CouponBill {
        ctx: load_ctx(tx, code, now_ms)?,
        lines: vec![CouponLineRow {
            amount_minor: price_minor,
            rate_bp: 0,
            group_id: None,
            plan_id: Some(plan_id.to_owned()),
            product_id: None,
        }],
        partly_paid: false,
    })
}

impl CouponBill {
    /// The coupon's discount on a bill whose total before it is `total_minor`.
    pub(super) fn quote(&self, total_minor: i64) -> Result<Quote, AppError> {
        quote(&self.ctx, &self.lines, total_minor, self.partly_paid)
    }
}

/// The counter-sale lines a coupon sees: the amount and rate each was priced at.
pub(super) fn sale_lines(sale: &[PosSaleLine], priced: &[(u16, i64)]) -> Vec<CouponLineRow> {
    sale.iter()
        .zip(priced)
        .map(|(line, (rate_bp, amount))| CouponLineRow {
            amount_minor: *amount,
            rate_bp: i64::from(*rate_bp),
            group_id: None,
            plan_id: None,
            product_id: Some(line.product_id.clone()),
        })
        .collect()
}

/// Takes the coupon's use and records it on the order. A session bill (`reduce_totals`) is lowered
/// by the discount; a counter or plan sale is written net already. A coupon with no use left is
/// `COUPON_INVALID`, so of two settles for its last use the second fails.
pub(super) fn redeem(
    tx: &StoreTx<'_>,
    quote: &Quote,
    order_id: &str,
    reduce_totals: bool,
    written: &mut Vec<Written>,
) -> Result<(), AppError> {
    if !take_coupon_use(tx, &quote.coupon_id)? {
        return Err(refuse("this coupon has been used up"));
    }
    set_order_coupon(
        tx,
        order_id,
        &quote.code,
        quote.discount_minor,
        reduce_totals,
    )?;
    written.push(Written {
        key: RowKey::Coupons {
            coupon_id: quote.coupon_id.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    });
    Ok(())
}

/// Undo of a settle that took a coupon: gives the use back and, for a session bill that goes back
/// to open (`restore_totals`), puts the discount back on its total and clears the coupon columns.
pub(super) fn release_on_revert(
    tx: &StoreTx<'_>,
    order_id: &str,
    restore_totals: bool,
    written: &mut Vec<Written>,
) -> Result<(), AppError> {
    let Some((code, _)) = order_coupon(tx, order_id)? else {
        return Ok(());
    };
    if let Some(c) = find_coupon_by_code(tx, &code)? {
        give_coupon_use(tx, &c.coupon_id)?;
        written.push(Written {
            key: RowKey::Coupons {
                coupon_id: c.coupon_id,
            },
            op: ChangeOp::Update,
            before: None,
        });
    }
    if restore_totals {
        clear_order_coupon(tx, order_id)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------------------------

/// `POST /ui/coupons/preview`: what `req.code` would do to the bill the request names, by the same
/// pricing the settle uses. Nothing is written.
pub fn preview(
    conn: &DbConn,
    req: &CouponPreviewRequest,
    now_ms: i64,
) -> Result<CouponPreview, AppError> {
    let ctx = load_ctx(conn, &req.code, now_ms)?;
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
    let (quote, total) = if let Some(order_id) = &req.order_id {
        let order =
            find_order(conn, order_id)?.ok_or_else(|| invalid("order_id", "no such bill"))?;
        if order.status != "open" || order.kind != "session" {
            return Err(invalid("order_id", "this bill is not open"));
        }
        let bill = CouponBill {
            ctx,
            lines: list_coupon_lines(conn, order_id)?,
            partly_paid: !list_payment_ids(conn, order_id)?.is_empty(),
        };
        let q = bill.quote(order.total_minor)?;
        let total = order.total_minor - q.discount_minor;
        (q, total)
    } else if let Some(lines) = &req.lines {
        let basket = super::pos_sale::load_basket(conn, lines, now_ms)?;
        super::pos_sale::check_lines(lines, &basket)?;
        let (priced, q) = super::pos_sale::price_with_coupon(lines, &basket, Some(&ctx))?;
        let q = q.ok_or_else(|| refuse("this coupon does not apply"))?;
        (q, priced.bill.total_minor)
    } else {
        let plan_id = req.plan_id.as_deref().unwrap_or_default();
        let plan = find_plan(conn, plan_id)?
            .filter(|p| p.active)
            .ok_or_else(|| invalid("plan_id", "no such plan"))?;
        let bill = CouponBill {
            ctx,
            lines: vec![CouponLineRow {
                amount_minor: plan.price_minor,
                rate_bp: 0,
                group_id: None,
                plan_id: Some(plan.plan_id.clone()),
                product_id: None,
            }],
            partly_paid: false,
        };
        let q = bill.quote(plan.price_minor)?;
        let total = plan.price_minor - q.discount_minor;
        (q, total)
    };
    Ok(CouponPreview {
        code: quote.code,
        discount_minor: quote.discount_minor,
        total_minor: total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rates(of: &[(u16, i128)]) -> BTreeMap<u16, i128> {
        of.iter().copied().collect()
    }

    #[test]
    fn the_spare_unit_goes_to_the_largest_remainder_and_ties_to_the_lower_rate() {
        assert_eq!(
            spread(1, 2, &rates(&[(500, 1), (1_800, 1)])),
            [(500, 1), (1_800, 0)]
        );
        assert_eq!(
            spread(129, 1_285, &rates(&[(500, 105), (1_800, 1_180)])),
            [(500, 11), (1_800, 118)]
        );
        assert_eq!(
            spread(10, 30, &rates(&[(0, 10), (500, 10), (1_800, 10)])),
            [(0, 4), (500, 3), (1_800, 3)]
        );
    }

    #[test]
    fn shares_always_add_up_to_the_discount_and_never_exceed_their_rate_amount() {
        for discount in 1..=60 {
            let per_rate = rates(&[(0, 7), (500, 13), (1_200, 21), (1_800, 19)]);
            let eligible: i128 = per_rate.values().sum();
            let shares = spread(discount, eligible, &per_rate);
            assert_eq!(
                shares.iter().map(|s| i128::from(s.1)).sum::<i128>(),
                discount
            );
            for (rate, share) in shares {
                assert!(i128::from(share) <= per_rate[&rate], "{discount} at {rate}");
            }
        }
    }
}
