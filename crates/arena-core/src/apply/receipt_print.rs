//! Receipts through the side-effect outbox (Story 5.17).
//!
//! * A settle whose receipt choice is `print` ([`queue_on_settle`]) issues the invoice number
//!   inside its own transaction, so the bill can never be undone, records the irreversible effect
//!   "receipt printed" and queues a `print` row on `side_effect_outbox`. Nothing here touches a
//!   printer: the Hub's print task sends the row after commit.
//! * `receipt.reprint` queues the same receipt again, marked "Reprint".
//! * `receipt.printed` and `receipt.print_failed` (system) close the row; a failure also raises
//!   the "Printer offline" alert. The sale stays settled either way.
//!
//! A result or a reprint touches the order row so the change log, and so the audit, names who and
//! what. The order is already irreversible by then, so the revision bump costs nothing.

use arena_proto::{
    AlertKind, AppError, EffectKind, ErrorCode, Intent, IntentPayload, RECEIPT_ERROR_MAX_CHARS,
    ReceiptChoice, UNDO_WINDOW_MS,
};
use arena_store::{
    ChangeOp, INVOICED_KINDS, InvoiceOrder, KIND_PRINT, NewAlert, OutboxRow, RowKey, STATE_DONE,
    STATE_FAILED, STATE_QUEUED, StoreTx, find_branch_summary, find_invoice_order, find_open_alert,
    find_outbox, finish_outbox, insert_alert, insert_effect, insert_outbox, list_unnumbered,
};

use super::invoice_issue::issue_number;
use super::mutate::Written;
use super::session::invalid;
use crate::status::Epoch;

/// How many older unnumbered sales one print settle numbers first.
const OLDER_BATCH: i64 = 100;

#[derive(Default)]
pub(crate) struct ReceiptTarget {
    order: Option<InvoiceOrder>,
    row: Option<OutboxRow>,
    printer_alert_open: bool,
}

pub(super) fn load(tx: &StoreTx<'_>, payload: &IntentPayload) -> Result<ReceiptTarget, AppError> {
    Ok(match payload {
        IntentPayload::ReceiptReprint(r) => ReceiptTarget {
            order: find_invoice_order(tx, &r.order_id)?,
            ..ReceiptTarget::default()
        },
        IntentPayload::ReceiptPrinted(p) => ReceiptTarget {
            row: find_outbox(tx, &p.outbox_id)?,
            ..ReceiptTarget::default()
        },
        IntentPayload::ReceiptPrintFailed(p) => ReceiptTarget {
            row: find_outbox(tx, &p.outbox_id)?,
            printer_alert_open: find_open_alert(tx, AlertKind::PrinterOffline.as_str(), None)?
                .is_some(),
            ..ReceiptTarget::default()
        },
        _ => ReceiptTarget::default(),
    })
}

fn stale(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::Stale, field, message)
}

fn not_found(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::NotFound, field, message)
}

fn queued_row<'a>(t: &'a ReceiptTarget, field: &str) -> Result<&'a OutboxRow, AppError> {
    let Some(row) = &t.row else {
        return Err(not_found(field, "no such outbox row"));
    };
    if row.kind != KIND_PRINT {
        return Err(invalid(field, "this outbox row is not a print"));
    }
    if row.state != STATE_QUEUED {
        return Err(stale(field, "this print has already been recorded"));
    }
    Ok(row)
}

pub(super) fn validate(intent: &Intent, t: &ReceiptTarget) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "receipt intents take no base_rev"));
    }
    match &intent.payload {
        IntentPayload::ReceiptReprint(_) => {
            let Some(order) = &t.order else {
                return Err(not_found("order_id", "no such order"));
            };
            if order.status != "settled" {
                return Err(stale("order_id", "this order is not settled"));
            }
            if order.invoice_no.is_none() {
                return Err(stale(
                    "order_id",
                    "this order has no invoice number, so there is no receipt to reprint",
                ));
            }
            Ok(())
        }
        IntentPayload::ReceiptPrinted(_) => queued_row(t, "outbox_id").map(|_| ()),
        IntentPayload::ReceiptPrintFailed(p) => {
            queued_row(t, "outbox_id")?;
            let error = p.error.trim();
            if error.is_empty() || error.chars().count() > RECEIPT_ERROR_MAX_CHARS {
                return Err(invalid(
                    "error",
                    "a print error is 1 to 200 characters long",
                ));
            }
            Ok(())
        }
        _ => Err(AppError::new(
            ErrorCode::Internal,
            "the loaded target does not match the payload",
        )),
    }
}

fn touch_order(order_id: &str) -> Written {
    Written {
        key: RowKey::Orders {
            id: order_id.to_owned(),
        },
        op: ChangeOp::Update,
        before: None,
    }
}

/// The JSON a `print` row carries: whether it is a reprint and who asked.
fn payload(reprint: bool, by: &str) -> String {
    serde_json::json!({ "reprint": reprint, "by": by }).to_string()
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    t: &ReceiptTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    match &intent.payload {
        IntentPayload::ReceiptReprint(r) => {
            insert_outbox(
                tx,
                &ulid::Ulid::generate().to_string(),
                KIND_PRINT,
                &r.order_id,
                &payload(true, &intent.actor.id),
                now_ms,
            )?;
            Ok(vec![touch_order(&r.order_id)])
        }
        IntentPayload::ReceiptPrinted(p) => {
            let row = queued_row(t, "outbox_id")?;
            finish_outbox(tx, &p.outbox_id, STATE_DONE, now_ms, None)?;
            Ok(vec![touch_order(&row.ref_id)])
        }
        IntentPayload::ReceiptPrintFailed(p) => {
            let row = queued_row(t, "outbox_id")?;
            finish_outbox(tx, &p.outbox_id, STATE_FAILED, now_ms, Some(p.error.trim()))?;
            let mut written = vec![touch_order(&row.ref_id)];
            if !t.printer_alert_open {
                let id = ulid::Ulid::generate().to_string();
                insert_alert(
                    tx,
                    &NewAlert {
                        id: &id,
                        kind: AlertKind::PrinterOffline.as_str(),
                        device_id: None,
                        raised_at: now_ms,
                        product_id: None,
                        qty: None,
                        threshold: None,
                    },
                )?;
                written.push(Written {
                    key: RowKey::Alerts { id },
                    op: ChangeOp::Insert,
                    before: None,
                });
            }
            Ok(written)
        }
        _ => Err(AppError::new(
            ErrorCode::Internal,
            "a receipt intent reached mutate without its target",
        )),
    }
}

/// Whether an order of this kind and total is a sale that takes an invoice number.
fn invoiced(order: &InvoiceOrder) -> bool {
    order.total_minor > 0 && INVOICED_KINDS.contains(&order.kind.as_str())
}

/// The receipt choice of a settle intent, `None` for any other intent.
fn choice(intent: &Intent) -> Option<ReceiptChoice> {
    match &intent.payload {
        IntentPayload::SessionSettle(s) => s.receipt,
        IntentPayload::PosSale(s) => s.receipt,
        IntentPayload::PlanSell(s) => s.receipt,
        _ => None,
    }
}

/// The order a settle intent settled: named by `session.settle`, inserted by a counter sale.
fn settled_order_id(intent: &Intent, written: &[Written]) -> Option<String> {
    if let IntentPayload::SessionSettle(s) = &intent.payload {
        return Some(s.order_id.clone());
    }
    written.iter().find_map(|w| match (&w.key, w.op) {
        (RowKey::Orders { id }, ChangeOp::Insert) => Some(id.clone()),
        _ => None,
    })
}

/// Drops a second write of a row an earlier entry already writes, so the order's revision moves
/// once per intent.
fn dedupe(written: &mut Vec<Written>) {
    let mut seen: Vec<RowKey> = Vec::with_capacity(written.len());
    written.retain(|w| {
        if w.op == ChangeOp::Update && seen.contains(&w.key) {
            return false;
        }
        seen.push(w.key.clone());
        true
    });
}

/// The settle step of a print choice: numbers the sale (and first any older sale whose Undo window
/// has closed, so the series stays in `settled_at` order), registers the irreversible effect and
/// queues the print. A settle with any other choice is left as it is.
pub(super) fn queue_on_settle(
    tx: &StoreTx<'_>,
    intent: &Intent,
    epoch: &Epoch,
    now_ms: i64,
    written: &mut Vec<Written>,
) -> Result<(), AppError> {
    if choice(intent) != Some(ReceiptChoice::Print) {
        return Ok(());
    }
    let internal = |m: &str| AppError::new(ErrorCode::Internal, m.to_owned());
    let order_id = settled_order_id(intent, written)
        .ok_or_else(|| internal("a print settle wrote no order"))?;
    let order = find_invoice_order(tx, &order_id)?
        .ok_or_else(|| internal("a print settle lost its order"))?;
    let branch =
        find_branch_summary(tx)?.ok_or_else(|| internal("the branch has not been bootstrapped"))?;
    for (older_id, settled_at) in list_unnumbered(tx, OLDER_BATCH)? {
        if older_id == order_id || now_ms - settled_at <= UNDO_WINDOW_MS {
            // Ordered by settle time: the rest are inside their window too.
            break;
        }
        if let Some(older) = find_invoice_order(tx, &older_id)? {
            issue_number(
                tx,
                &older.id,
                &older.fy,
                &branch.branch_code,
                epoch,
                None,
                now_ms,
                written,
            )?;
        }
    }
    if invoiced(&order) {
        issue_number(
            tx,
            &order.id,
            &order.fy,
            &branch.branch_code,
            epoch,
            Some(&intent.intent_id),
            now_ms,
            written,
        )?;
    }
    insert_effect(
        tx,
        &ulid::Ulid::generate().to_string(),
        EffectKind::ReceiptPrinted,
        Some(&intent.intent_id),
        Some(&order.id),
        now_ms,
    )?;
    insert_outbox(
        tx,
        &ulid::Ulid::generate().to_string(),
        KIND_PRINT,
        &order.id,
        &payload(false, &intent.actor.id),
        now_ms,
    )?;
    dedupe(written);
    Ok(())
}
