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
//! A WhatsApp choice (Story 5.19) is handled by `receipt_notify`, which queues a `notify_outbox`
//! row instead of a print.
//!
//! Cash drawer kicks (Story 5.18) ride the same outbox. A settle with a cash row, while
//! `receipt.drawer` is on, records the irreversible effect "drawer kicked" and queues a `drawer`
//! row in its own transaction; the print task sends the pulse after commit and `drawer.kicked`
//! (which writes a `drawer_open` cash event) or `drawer.kick_failed` closes the row.
//!
//! The Z-report of a closed shift (Story 5.28) rides the same outbox: `shift.close` queues a `print`
//! row whose `ref_id` is the shift id and whose payload carries `"z_report": true` when a printer is
//! chosen, and `shift.reprint_z` queues another. Their results touch the `shifts` row, not an order.
//!
//! A result or a reprint touches the order row so the change log, and so the audit, names who and
//! what. The order is already irreversible by then, so the revision bump costs nothing.

use arena_proto::{
    AlertKind, AppError, CASH_DRAWER_OPEN, EffectKind, ErrorCode, Intent, IntentPayload,
    KEY_RECEIPT_DRAWER, RECEIPT_ERROR_MAX_CHARS, ReceiptChoice, UNDO_WINDOW_MS,
};
use arena_store::{
    ChangeOp, INVOICED_KINDS, InvoiceOrder, KIND_DRAWER, KIND_PRINT, NewAlert, NewCashEvent,
    OutboxRow, RowKey, STATE_DONE, STATE_FAILED, STATE_QUEUED, ShiftDetail, StoreTx,
    find_branch_summary, find_invoice_order, find_open_alert, find_outbox, find_setting,
    find_shift, finish_outbox, insert_alert, insert_cash_event, insert_effect, insert_outbox,
    list_unnumbered, shift_of_order_sale,
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
    /// The shift a drawer kick's cash event is written against.
    shift_id: Option<String>,
    /// The shift a Z-report reprint names (Story 5.28).
    z_shift: Option<ShiftDetail>,
}

pub(super) fn load(tx: &StoreTx<'_>, payload: &IntentPayload) -> Result<ReceiptTarget, AppError> {
    Ok(match payload {
        IntentPayload::ReceiptReprint(r) => ReceiptTarget {
            order: find_invoice_order(tx, &r.order_id)?,
            ..ReceiptTarget::default()
        },
        IntentPayload::ShiftReprintZ(z) => ReceiptTarget {
            z_shift: find_shift(tx, &z.shift_id)?,
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
        IntentPayload::DrawerKicked(p) => {
            let row = find_outbox(tx, &p.outbox_id)?;
            let shift_id = match &row {
                Some(r) => shift_of_order_sale(tx, &r.ref_id)?,
                None => None,
            };
            ReceiptTarget {
                row,
                shift_id,
                ..ReceiptTarget::default()
            }
        }
        IntentPayload::DrawerKickFailed(p) => ReceiptTarget {
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

fn queued_row<'a>(
    t: &'a ReceiptTarget,
    field: &str,
    kind: &str,
) -> Result<&'a OutboxRow, AppError> {
    let Some(row) = &t.row else {
        return Err(not_found(field, "no such outbox row"));
    };
    if row.kind != kind {
        return Err(invalid(
            field,
            "this outbox row is not of the kind this result names",
        ));
    }
    if row.state != STATE_QUEUED {
        return Err(stale(
            field,
            if kind == KIND_DRAWER {
                "this drawer kick has already been recorded"
            } else {
                "this print has already been recorded"
            },
        ));
    }
    Ok(row)
}

fn valid_error(error: &str, what: &str) -> Result<(), AppError> {
    let error = error.trim();
    if error.is_empty() || error.chars().count() > RECEIPT_ERROR_MAX_CHARS {
        return Err(invalid(
            "error",
            &format!("a {what} error is 1 to 200 characters long"),
        ));
    }
    Ok(())
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
        IntentPayload::ShiftReprintZ(_) => {
            let Some(shift) = &t.z_shift else {
                return Err(not_found("shift_id", "no such shift"));
            };
            if shift.open {
                return Err(stale("shift_id", "this shift is still open"));
            }
            Ok(())
        }
        IntentPayload::ReceiptPrinted(_) => queued_row(t, "outbox_id", KIND_PRINT).map(|_| ()),
        IntentPayload::ReceiptPrintFailed(p) => {
            queued_row(t, "outbox_id", KIND_PRINT)?;
            valid_error(&p.error, "print")
        }
        IntentPayload::DrawerKicked(_) => {
            queued_row(t, "outbox_id", KIND_DRAWER)?;
            if t.shift_id.is_none() {
                return Err(stale(
                    "outbox_id",
                    "no shift holds the cash of this drawer kick",
                ));
            }
            Ok(())
        }
        IntentPayload::DrawerKickFailed(p) => {
            queued_row(t, "outbox_id", KIND_DRAWER)?;
            valid_error(&p.error, "drawer kick")
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

/// Whether a `print` row carries a Z-report (its `ref_id` is a shift id, Story 5.28).
pub(crate) fn is_z_row(row: &OutboxRow) -> bool {
    serde_json::from_str::<serde_json::Value>(&row.payload)
        .ok()
        .and_then(|v| v.get("z_report").and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
}

/// Touches the row a result or reprint is about: the order of a receipt, the shift of a Z-report.
fn touch_ref(row: &OutboxRow) -> Written {
    if is_z_row(row) {
        Written {
            key: RowKey::Shifts {
                id: row.ref_id.clone(),
            },
            op: ChangeOp::Update,
            before: None,
        }
    } else {
        touch_order(&row.ref_id)
    }
}

/// The staff who took the cash of a drawer row (its payload's `by`).
fn kicked_by(row: &OutboxRow) -> String {
    serde_json::from_str::<serde_json::Value>(&row.payload)
        .ok()
        .and_then(|v| v.get("by").and_then(|b| b.as_str().map(str::to_owned)))
        .unwrap_or_default()
}

/// Closes a queued row as failed and raises the one "Printer offline" alert while none is open.
/// The sale stays settled.
fn fail_row(
    tx: &StoreTx<'_>,
    t: &ReceiptTarget,
    row: &OutboxRow,
    error: &str,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    finish_outbox(tx, &row.id, STATE_FAILED, now_ms, Some(error.trim()))?;
    let mut written = vec![touch_ref(row)];
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

/// The JSON a Z-report `print` row carries (Story 5.28): marked `z_report`, its `ref_id` is the shift.
pub(super) fn z_payload(reprint: bool, by: &str) -> String {
    serde_json::json!({ "z_report": true, "reprint": reprint, "by": by }).to_string()
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
        IntentPayload::ShiftReprintZ(z) => {
            insert_outbox(
                tx,
                &ulid::Ulid::generate().to_string(),
                KIND_PRINT,
                &z.shift_id,
                &z_payload(true, &intent.actor.id),
                now_ms,
            )?;
            Ok(vec![Written {
                key: RowKey::Shifts {
                    id: z.shift_id.clone(),
                },
                op: ChangeOp::Update,
                before: None,
            }])
        }
        IntentPayload::ReceiptPrinted(p) => {
            let row = queued_row(t, "outbox_id", KIND_PRINT)?;
            finish_outbox(tx, &p.outbox_id, STATE_DONE, now_ms, None)?;
            Ok(vec![touch_ref(row)])
        }
        IntentPayload::ReceiptPrintFailed(p) => {
            let row = queued_row(t, "outbox_id", KIND_PRINT)?;
            fail_row(tx, t, row, &p.error, now_ms)
        }
        IntentPayload::DrawerKicked(p) => {
            let row = queued_row(t, "outbox_id", KIND_DRAWER)?;
            let shift_id = t
                .shift_id
                .as_deref()
                .ok_or_else(|| stale("outbox_id", "no shift holds this drawer kick"))?;
            finish_outbox(tx, &p.outbox_id, STATE_DONE, now_ms, None)?;
            let event_id = ulid::Ulid::generate().to_string();
            insert_cash_event(
                tx,
                &NewCashEvent {
                    id: &event_id,
                    shift_id,
                    kind: CASH_DRAWER_OPEN,
                    amount_minor: 0,
                    staff_id: &kicked_by(row),
                    order_id: Some(&row.ref_id),
                    reverses: None,
                },
            )?;
            Ok(vec![
                touch_order(&row.ref_id),
                Written {
                    key: RowKey::CashEvents { id: event_id },
                    op: ChangeOp::Insert,
                    before: None,
                },
            ])
        }
        IntentPayload::DrawerKickFailed(p) => {
            let row = queued_row(t, "outbox_id", KIND_DRAWER)?;
            fail_row(tx, t, row, &p.error, now_ms)
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
    if !is_settle(intent) {
        return Ok(());
    }
    let internal = |m: &str| AppError::new(ErrorCode::Internal, m.to_owned());
    let order_id =
        settled_order_id(intent, written).ok_or_else(|| internal("a settle wrote no order"))?;
    let total = find_invoice_order(tx, &order_id)?.map_or(0, |o| o.total_minor);
    super::business_invoice::record_on_settle(tx, intent, &order_id, total)?;
    queue_drawer(tx, intent, &order_id, now_ms)?;
    match choice(intent) {
        Some(ReceiptChoice::Print) => queue_print(tx, intent, epoch, &order_id, now_ms, written)?,
        Some(ReceiptChoice::Whatsapp) => {
            super::receipt_notify::queue_on_settle(tx, intent, &order_id, total, now_ms, written)?;
        }
        Some(ReceiptChoice::None) | None => {}
    }
    Ok(())
}

/// Whether the intent settles a sale (a session bill, a counter sale or a plan sale).
fn is_settle(intent: &Intent) -> bool {
    matches!(
        &intent.payload,
        IntentPayload::SessionSettle(_) | IntentPayload::PosSale(_) | IntentPayload::PlanSell(_)
    )
}

/// The drawer step of a settle: with `receipt.drawer` on and a cash row among the payments, the
/// kick is registered as an irreversible effect (so Undo is refused and a correction is a
/// Refund) and queued on the outbox. Nothing here touches the printer.
fn queue_drawer(
    tx: &StoreTx<'_>,
    intent: &Intent,
    order_id: &str,
    now_ms: i64,
) -> Result<(), AppError> {
    let on = find_setting(tx, KEY_RECEIPT_DRAWER)?.is_some_and(|s| s.value == "true");
    if !on {
        return Ok(());
    }
    if shift_of_order_sale(tx, order_id)?.is_none() {
        return Ok(());
    }
    insert_effect(
        tx,
        &ulid::Ulid::generate().to_string(),
        EffectKind::DrawerKicked,
        Some(&intent.intent_id),
        Some(order_id),
        now_ms,
    )?;
    insert_outbox(
        tx,
        &ulid::Ulid::generate().to_string(),
        KIND_DRAWER,
        order_id,
        &payload(false, &intent.actor.id),
        now_ms,
    )
}

fn queue_print(
    tx: &StoreTx<'_>,
    intent: &Intent,
    epoch: &Epoch,
    order_id: &str,
    now_ms: i64,
    written: &mut Vec<Written>,
) -> Result<(), AppError> {
    let internal = |m: &str| AppError::new(ErrorCode::Internal, m.to_owned());
    let order_id = order_id.to_owned();
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
