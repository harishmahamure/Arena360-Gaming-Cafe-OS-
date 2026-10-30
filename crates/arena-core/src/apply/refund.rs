//! `refund.create` (Story 5.24): selected lines of a settled, invoiced order are paid back against
//! a credit note.
//!
//! The invoice is found by its number (`INVOICE_NOT_FOUND`). What a line is worth to the customer
//! is its share of what the order actually charged: the order total (or, on a sale that carries a
//! per-rate tax breakup, each rate's pre-tax amount) is split over the lines in proportion to their
//! gross amounts by largest remainder, so a coupon or manual discount is refunded pro rata and a
//! full refund of every line comes back to exactly the amount paid. A partial quantity takes the
//! half-up share of its line; the last units take whatever is left. Tax is recomputed once per rate
//! on the refunded amounts (Story 5.2), negative, for a sale that has a breakup; a session or plan
//! order carries no tax and neither does its refund.
//!
//! The refund is capped at what was paid less earlier refunds (`REFUND_EXCEEDS_PAID`). A credit
//! portion of the original (not yet collected) is cut from the player's outstanding credit first;
//! only the rest goes back by the chosen method: cash from an open shift (a negative `refund` cash
//! event), UPI with the last 4 digits, card with its terminal reference, or the wallet of a
//! profiled player. Ticked tracked lines return their units to stock (`return` moves); a plan line
//! can only be refunded whole while its pack or pass is unused, and then it is deactivated. The
//! credit note number comes from the same `(fy, segment)` counter as invoices. A refund has no
//! Undo.

use std::collections::BTreeMap;

use arena_proto::{
    AppError, CASH_REFUND, ErrorCode, Intent, KEY_TAX, MAX_REFUND_LINES, OwnerAlertKind,
    PaymentMethod, PaymentRow, REFUND_NOTE_MAX_CHARS, ReasonCode, ReceiptChoice, RefundCreate,
    TaxMode, TaxSettings,
};
use arena_store::{
    BranchSummary, ChangeOp, CreditRow, KIND_PRINT, NewCashEvent, NewRefund, NewRefundPayment,
    NewWalletTxn, RefundLineRow, RefundOrder, RowKey, StoreTx, add_line_refunded,
    adjust_outstanding, adjust_wallet, deactivate_pack, find_branch_summary, find_credit,
    find_order_by_invoice, find_pack_for_order, find_product, find_setting, insert_cash_event,
    insert_outbox, insert_refund, insert_refund_payment, insert_wallet_txn, list_components,
    list_refund_lines, open_shift_id, pack_ever_used, paid_by_method, sum_refunded, take_sequence,
};
use serde::{Deserialize, Serialize};

use super::mutate::Written;
use super::pos_sale::internal;
use super::session::{check_reference, invalid};
use super::stock::StockFor;
use crate::invoice::number;
use crate::invoice::tax::{Line, RateTax, compute};
use crate::status::Epoch;

/// A refunded line as stored on the credit note and printed from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreditNoteLine {
    pub line_id: String,
    pub label: String,
    pub qty: i64,
    /// What the line is refunded for, net of coupon and discount and before tax.
    pub amount_minor: i64,
    pub return_to_stock: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hsn: Option<String>,
}

/// An owned plan a plan line sold.
struct PackInfo {
    id: String,
    used: bool,
}

pub(crate) struct RefundTarget {
    order: Option<RefundOrder>,
    lines: Vec<RefundLineRow>,
    /// Money taken on the order, by method (positive rows only).
    paid: Vec<(String, i64)>,
    /// Earlier refunds: total, and the part of it that cut outstanding credit.
    refunded: (i64, i64),
    shift_id: Option<String>,
    tax: TaxSettings,
    credit: Option<CreditRow>,
    branch: Option<BranchSummary>,
    /// The pack a plan order sold, for its plan lines.
    pack: Option<PackInfo>,
    /// For a product line, the tracked items one unit of it takes from stock.
    stock: BTreeMap<String, Vec<(String, i64)>>,
}

pub(super) fn load(tx: &StoreTx<'_>, r: &RefundCreate) -> Result<RefundTarget, AppError> {
    let tax = match find_setting(tx, KEY_TAX)? {
        Some(row) => TaxSettings::parse(&row.value)
            .ok_or_else(|| internal("the tax settings cannot be read"))?,
        None => super::validate::default_tax(),
    };
    let mut t = RefundTarget {
        order: None,
        lines: Vec::new(),
        paid: Vec::new(),
        refunded: (0, 0),
        shift_id: None,
        tax,
        credit: None,
        branch: find_branch_summary(tx)?,
        pack: None,
        stock: BTreeMap::new(),
    };
    let Some(order) = find_order_by_invoice(tx, r.invoice_no.trim())? else {
        return Ok(t);
    };
    t.lines = list_refund_lines(tx, &order.id)?;
    t.paid = paid_by_method(tx, &order.id)?;
    t.refunded = sum_refunded(tx, &order.id)?;
    t.shift_id = open_shift_id(tx)?;
    if let Some(player) = &order.player_id {
        t.credit = find_credit(tx, player)?;
    }
    if t.lines.iter().any(|l| l.kind == "plan") {
        t.pack = find_pack_for_order(tx, &order.id)?.map(|p| PackInfo {
            used: p.minutes_total != p.minutes_left || pack_ever_used(tx, &p.id).unwrap_or(true),
            id: p.id,
        });
    }
    for line in t.lines.iter().filter(|l| l.kind == "product") {
        let Some(product_id) = &line.product_id else {
            continue;
        };
        let mut items = Vec::new();
        if line.bundle {
            for (id, qty) in list_components(tx, product_id)? {
                if let Some(p) = find_product(tx, &id)?.filter(|p| !p.bundle && p.track_stock) {
                    items.push((p.product_id, qty));
                }
            }
        } else if line.tracked {
            items.push((product_id.clone(), 1));
        }
        t.stock.insert(line.id.clone(), items);
    }
    t.order = Some(order);
    Ok(t)
}

/// What a plan line cannot be refunded for, if anything.
fn plan_refusal(t: &RefundTarget) -> Option<&'static str> {
    match &t.pack {
        None => Some("plan_not_refundable"),
        Some(p) if p.used => Some("plan_used"),
        Some(_) => None,
    }
}

/// Splits `total` over `weights` in proportion: each gets the floor of its exact share and the
/// spare minor units go one each to the largest remainders (ties to the earlier line).
fn allocate(total: i64, weights: &[i64]) -> Vec<i64> {
    let sum: i128 = weights.iter().map(|w| i128::from(*w)).sum();
    if sum <= 0 || total <= 0 {
        return vec![0; weights.len()];
    }
    let mut shares: Vec<(i128, i128)> = weights
        .iter()
        .map(|w| {
            let exact = i128::from(total) * i128::from(*w);
            (exact / sum, exact % sum)
        })
        .collect();
    let left = i128::from(total) - shares.iter().map(|s| s.0).sum::<i128>();
    let mut order: Vec<usize> = (0..shares.len()).collect();
    order.sort_by(|a, b| shares[*b].1.cmp(&shares[*a].1).then(a.cmp(b)));
    for i in order.into_iter().take(usize::try_from(left).unwrap_or(0)) {
        shares[i].0 += 1;
    }
    shares
        .into_iter()
        .map(|s| i64::try_from(s.0).unwrap_or(0))
        .collect()
}

/// What the order charged for each line: its share of the order total, or of each rate's pre-tax
/// amount on a sale with a tax breakup.
fn line_nets(order: &RefundOrder, lines: &[RefundLineRow], tax: &TaxSettings) -> Vec<i64> {
    let breakup = order
        .tax_breakup
        .as_deref()
        .and_then(|j| serde_json::from_str::<Vec<RateTax>>(j).ok());
    let mut nets = vec![0i64; lines.len()];
    let mut groups: BTreeMap<u16, (i64, Vec<usize>)> = BTreeMap::new();
    for (i, l) in lines.iter().enumerate() {
        let key = if breakup.is_some() {
            u16::try_from(l.tax_rate_bp.unwrap_or(0)).unwrap_or(0)
        } else {
            0
        };
        groups.entry(key).or_default().1.push(i);
    }
    for (rate, (amount, _)) in &mut groups {
        *amount = match &breakup {
            Some(b) => b.iter().filter(|r| r.rate_bp == *rate).fold(0, |sum, r| {
                sum + r.taxable_minor + if tax.inclusive { r.tax_minor } else { 0 }
            }),
            None => order.total_minor,
        };
    }
    for (amount, members) in groups.values() {
        let weights: Vec<i64> = members.iter().map(|i| lines[*i].amount_minor).collect();
        for (i, share) in members.iter().zip(allocate(*amount, &weights)) {
            nets[*i] = share;
        }
    }
    nets
}

fn round_div(n: i128, d: i128) -> i128 {
    (2 * n + d) / (2 * d)
}

/// One selected line, priced.
pub(crate) struct Planned<'a> {
    pub(crate) line: &'a RefundLineRow,
    pub(crate) qty: i64,
    /// The net amount refunded for the line, before tax.
    pub(crate) net_minor: i64,
    pub(crate) return_to_stock: bool,
}

/// A refund worked out from the intent and the invoice.
pub(crate) struct Plan<'a> {
    pub(crate) lines: Vec<Planned<'a>>,
    /// The total paid back, tax included.
    pub(crate) total_minor: i64,
    pub(crate) tax_minor: i64,
    /// The per-rate tax of the refunded lines (negative amounts), when the sale carries tax.
    pub(crate) breakup: Option<Vec<RateTax>>,
    /// The part that cuts the player's outstanding credit.
    pub(crate) credit_minor: i64,
    /// The rest, which goes back by `method`.
    pub(crate) money_minor: i64,
    pub(crate) method: Option<PaymentMethod>,
    pub(crate) reference: Option<String>,
    pub(crate) reason: ReasonCode,
    pub(crate) note: Option<String>,
}

fn note(intent: &Intent, r: &RefundCreate) -> Result<Option<String>, AppError> {
    if intent.reason != Some(ReasonCode::Other) {
        return Ok(None);
    }
    let note = r.note.as_deref().map(str::trim).unwrap_or_default();
    if note.is_empty() {
        return Err(invalid("note", "say why the order is refunded"));
    }
    if note.chars().count() > REFUND_NOTE_MAX_CHARS || note.chars().any(char::is_control) {
        return Err(invalid(
            "note",
            "the reason is plain text of up to 140 characters",
        ));
    }
    Ok(Some(note.to_owned()))
}

pub(crate) fn plan<'a>(
    intent: &Intent,
    r: &RefundCreate,
    t: &'a RefundTarget,
) -> Result<Plan<'a>, AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "refund.create takes no base_rev"));
    }
    let Some(order) = &t.order else {
        return Err(AppError::with_field(
            ErrorCode::InvoiceNotFound,
            "invoice_no",
            "There is no such invoice.",
        ));
    };
    if order.status != "settled" {
        return Err(AppError::with_field(
            ErrorCode::Stale,
            "invoice_no",
            "this order is not settled",
        ));
    }
    let reason = intent
        .reason
        .ok_or_else(|| invalid("reason", "choose a reason"))?;
    let note = note(intent, r)?;
    if r.receipt == Some(ReceiptChoice::Whatsapp) {
        return Err(invalid("receipt", "a credit note is printed or not"));
    }
    if r.lines.is_empty() || r.lines.len() > MAX_REFUND_LINES {
        return Err(invalid("lines", "choose the lines to refund"));
    }
    let nets = line_nets(order, &t.lines, &t.tax);
    let mut planned: Vec<Planned<'a>> = Vec::new();
    for (i, req) in r.lines.iter().enumerate() {
        let field = |name: &str| format!("lines[{i}].{name}");
        let Some(pos) = t.lines.iter().position(|l| l.id == req.line_id) else {
            return Err(invalid(
                &field("line_id"),
                "this line is not on the invoice",
            ));
        };
        let line = &t.lines[pos];
        if planned.iter().any(|p| p.line.id == line.id) {
            return Err(invalid(&field("line_id"), "each line is named once"));
        }
        if let Some(why) = plan_refusal(t).filter(|_| line.kind == "plan") {
            return Err(invalid(
                &field("line_id"),
                &format!("this plan cannot be refunded ({why})"),
            ));
        }
        let left_qty = line.qty - line.refunded_qty;
        if req.qty < 1 || req.qty > left_qty {
            return Err(invalid(
                &field("qty"),
                "choose how many units to refund, up to what is left",
            ));
        }
        if line.kind != "product" && req.qty != left_qty {
            return Err(invalid(&field("qty"), "this line is refunded whole"));
        }
        let return_to_stock = req.return_to_stock == Some(true);
        if return_to_stock && t.stock.get(&line.id).is_none_or(Vec::is_empty) {
            return Err(invalid(
                &field("return_to_stock"),
                "this line does not keep stock",
            ));
        }
        let left_minor = (nets[pos] - line.refunded_minor).max(0);
        let net_minor = if req.qty == left_qty {
            left_minor
        } else {
            let share = round_div(
                i128::from(nets[pos]) * i128::from(req.qty),
                i128::from(line.qty),
            );
            i64::try_from(share).unwrap_or(0).min(left_minor)
        };
        planned.push(Planned {
            line,
            qty: req.qty,
            net_minor,
            return_to_stock,
        });
    }
    let (total_minor, tax_minor, breakup) = if order.tax_breakup.is_some() {
        let gst = t.tax.tax_mode() == TaxMode::GstIn;
        let taxed: Vec<Line> = planned
            .iter()
            .filter(|p| p.net_minor > 0)
            .map(|p| Line {
                rate_bp: u16::try_from(p.line.tax_rate_bp.unwrap_or(0)).unwrap_or(0),
                amount_minor: -p.net_minor,
            })
            .collect();
        let bill = compute(&taxed, t.tax.inclusive, gst)
            .ok_or_else(|| invalid("lines", "the refund is too large"))?;
        (-bill.total_minor, -bill.tax_minor, Some(bill.breakup))
    } else {
        (planned.iter().map(|p| p.net_minor).sum(), 0, None)
    };
    if total_minor <= 0 {
        return Err(invalid(
            "lines",
            "these lines were free; there is nothing to refund",
        ));
    }
    let paid: i64 = t.paid.iter().map(|(_, v)| v).sum();
    if total_minor > paid - t.refunded.0 {
        return Err(AppError::with_field(
            ErrorCode::RefundExceedsPaid,
            "lines",
            "This is more than was paid on the invoice.",
        ));
    }
    let credit_paid = t
        .paid
        .iter()
        .find(|(m, _)| m == PaymentMethod::Credit.as_str())
        .map_or(0, |(_, v)| *v);
    let outstanding = t.credit.as_ref().map_or(0, |c| c.outstanding_minor);
    let credit_minor = total_minor
        .min((credit_paid - t.refunded.1).max(0))
        .min(outstanding);
    let money_minor = total_minor - credit_minor;
    let (method, reference) = if money_minor > 0 {
        let method = r
            .method
            .ok_or_else(|| invalid("method", "choose how the money goes back"))?;
        match method {
            PaymentMethod::Credit => {
                return Err(invalid("method", "a refund cannot go back on credit"));
            }
            PaymentMethod::Wallet if order.player_id.is_none() => {
                return Err(invalid("method", "a guest has no wallet"));
            }
            PaymentMethod::Cash if t.shift_id.is_none() => {
                return Err(AppError::with_field(
                    ErrorCode::ShiftNotOpen,
                    "method",
                    "Open a shift to hand back cash.",
                ));
            }
            _ => {}
        }
        check_reference(
            0,
            &PaymentRow {
                method,
                amount_minor: money_minor,
                reference: r.reference.clone(),
            },
        )
        .map_err(|mut e| {
            e.field = Some("reference".to_owned());
            e
        })?;
        let reference = r
            .reference
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        (Some(method), reference)
    } else {
        (None, None)
    };
    Ok(Plan {
        lines: planned,
        total_minor,
        tax_minor,
        breakup,
        credit_minor,
        money_minor,
        method,
        reference,
        reason,
        note,
    })
}

pub(super) fn validate(
    intent: &Intent,
    r: &RefundCreate,
    t: &RefundTarget,
) -> Result<(), AppError> {
    plan(intent, r, t).map(|_| ())
}

fn insert(key: RowKey) -> Written {
    Written {
        key,
        op: ChangeOp::Insert,
        before: None,
    }
}

fn update(key: RowKey) -> Written {
    Written {
        key,
        op: ChangeOp::Update,
        before: None,
    }
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    r: &RefundCreate,
    t: &RefundTarget,
    epoch: &Epoch,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let p = plan(intent, r, t)?;
    let (Some(order), Some(branch)) = (&t.order, &t.branch) else {
        return Err(internal("a refund validated without its invoice"));
    };
    let (business_day, fy) = crate::locale::stamp(tx, now_ms)?
        .ok_or_else(|| internal("the branch has not been bootstrapped"))?;
    let actor = intent.actor.id.as_str();
    let approver = intent.elevated_by.as_ref().map_or(actor, |e| e.id.as_str());
    let mut written = Vec::new();

    // The credit note takes the next number of the invoice series.
    let segment = number::segment(epoch.counter, &epoch.hub_instance_id);
    let (seq, opened) = take_sequence(tx, &fy, &segment)?;
    let credit_note_no = number::format(&branch.branch_code, &fy, &segment, seq);
    written.push(Written {
        key: RowKey::InvoiceSequences {
            fy: fy.clone(),
            segment,
        },
        op: if opened {
            ChangeOp::Insert
        } else {
            ChangeOp::Update
        },
        before: None,
    });

    let refund_id = ulid::Ulid::generate().to_string();
    let note_lines: Vec<CreditNoteLine> = p
        .lines
        .iter()
        .map(|l| CreditNoteLine {
            line_id: l.line.id.clone(),
            label: l.line.label.clone(),
            qty: l.qty,
            amount_minor: l.net_minor,
            return_to_stock: l.return_to_stock,
            hsn: l.line.hsn.clone(),
        })
        .collect();
    let lines_json = serde_json::to_string(&note_lines)
        .map_err(|_| internal("the credit note lines cannot be written"))?;
    let breakup_json = p
        .breakup
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| internal("the tax breakup cannot be written"))?;
    insert_refund(
        tx,
        &NewRefund {
            id: &refund_id,
            order_id: &order.id,
            credit_note_no: &credit_note_no,
            reason: p.reason.as_str(),
            note: p.note.as_deref(),
            method: p.method.map(PaymentMethod::as_str),
            reference: p.reference.as_deref(),
            amount_minor: p.total_minor,
            credit_minor: p.credit_minor,
            tax_minor: p.tax_minor,
            tax_breakup: breakup_json.as_deref(),
            lines: &lines_json,
            staff_id: actor,
            approved_by: approver,
            business_day: &business_day,
            fy: &fy,
        },
    )?;
    written.push(insert(RowKey::Refunds {
        id: refund_id.clone(),
    }));

    // What was refunded of each line, then the stock and the plan.
    for l in &p.lines {
        add_line_refunded(tx, &l.line.id, l.qty, l.net_minor)?;
        written.push(update(RowKey::OrderLines {
            id: l.line.id.clone(),
        }));
        if l.return_to_stock {
            let tag = StockFor {
                order_id: &order.id,
                placement_id: None,
                today: &business_day,
            };
            for (product_id, per_unit) in t.stock.get(&l.line.id).into_iter().flatten() {
                written.extend(super::stock::return_to_stock(
                    tx,
                    product_id,
                    per_unit * l.qty,
                    &tag,
                    actor,
                    now_ms,
                )?);
            }
        }
        if let Some(pack) = t.pack.as_ref().filter(|_| l.line.kind == "plan") {
            deactivate_pack(tx, &pack.id, now_ms)?;
            written.push(update(RowKey::PlayerPacks {
                id: pack.id.clone(),
            }));
        }
    }

    // The payments, negative, in the order they cut: credit first, then the chosen method.
    let mut position = 0;
    let mut pay = |method: PaymentMethod,
                   amount: i64,
                   reference: Option<&str>,
                   written: &mut Vec<Written>|
     -> Result<(), AppError> {
        let id = ulid::Ulid::generate().to_string();
        insert_refund_payment(
            tx,
            &NewRefundPayment {
                id: &id,
                order_id: &order.id,
                refund_id: &refund_id,
                method: method.as_str(),
                amount_minor: -amount,
                reference,
                staff_id: actor,
                position,
                business_day: &business_day,
                fy: &fy,
            },
        )?;
        position += 1;
        written.push(insert(RowKey::Payments { id }));
        Ok(())
    };
    let player = order.player_id.as_deref();
    if p.credit_minor > 0 {
        pay(PaymentMethod::Credit, p.credit_minor, None, &mut written)?;
        let player = player.ok_or_else(|| internal("a credit refund without a player"))?;
        let txn_id = ulid::Ulid::generate().to_string();
        insert_wallet_txn(
            tx,
            &NewWalletTxn {
                id: &txn_id,
                player_id: player,
                delta_minor: -p.credit_minor,
                reason: "credit",
                method: Some("refund"),
                staff_id: actor,
                order_id: &order.id,
                business_day: &business_day,
                fy: &fy,
            },
        )?;
        adjust_outstanding(tx, player, -p.credit_minor)?;
        written.push(insert(RowKey::WalletTxns { id: txn_id }));
        written.push(update(RowKey::CreditBalances {
            player_id: player.to_owned(),
        }));
    }
    if let Some(method) = p.method {
        pay(method, p.money_minor, p.reference.as_deref(), &mut written)?;
        match method {
            PaymentMethod::Cash => {
                let shift_id = t
                    .shift_id
                    .as_deref()
                    .ok_or_else(|| internal("a cash refund without an open shift"))?;
                let event_id = ulid::Ulid::generate().to_string();
                insert_cash_event(
                    tx,
                    &NewCashEvent {
                        id: &event_id,
                        shift_id,
                        kind: CASH_REFUND,
                        amount_minor: -p.money_minor,
                        staff_id: actor,
                        order_id: Some(&order.id),
                        reverses: None,
                    },
                )?;
                written.push(insert(RowKey::CashEvents { id: event_id }));
            }
            PaymentMethod::Wallet => {
                let player = player.ok_or_else(|| internal("a wallet refund without a player"))?;
                let txn_id = ulid::Ulid::generate().to_string();
                insert_wallet_txn(
                    tx,
                    &NewWalletTxn {
                        id: &txn_id,
                        player_id: player,
                        delta_minor: p.money_minor,
                        reason: "bill",
                        method: Some("refund"),
                        staff_id: actor,
                        order_id: &order.id,
                        business_day: &business_day,
                        fy: &fy,
                    },
                )?;
                adjust_wallet(tx, player, p.money_minor)?;
                written.push(insert(RowKey::WalletTxns { id: txn_id }));
                written.push(update(RowKey::Wallets {
                    player_id: player.to_owned(),
                }));
            }
            PaymentMethod::Upi | PaymentMethod::Card | PaymentMethod::Credit => {}
        }
    }
    if r.receipt == Some(ReceiptChoice::Print) {
        // The print task builds the credit note from the committed rows after this commits.
        let payload =
            serde_json::json!({ "reprint": false, "by": actor, "credit_note": refund_id });
        insert_outbox(
            tx,
            &ulid::Ulid::generate().to_string(),
            KIND_PRINT,
            &order.id,
            &payload.to_string(),
            now_ms,
        )?;
        written.push(update(RowKey::Orders {
            id: order.id.clone(),
        }));
    }
    if p.total_minor > super::owner_alert::thresholds(tx)?.large_refund_minor {
        super::owner_alert::queue(
            tx,
            &super::owner_alert::Alert {
                kind: OwnerAlertKind::LargeRefund,
                source_ref: &refund_id,
                order_id: None,
                player_id: "",
                params: BTreeMap::from([("amount_minor", p.total_minor.to_string())]),
            },
            now_ms,
            &mut written,
        )?;
    }
    Ok(written)
}
