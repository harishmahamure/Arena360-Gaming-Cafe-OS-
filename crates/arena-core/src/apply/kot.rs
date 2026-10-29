//! KOT printing through the side-effect outbox (Story 7.5).
//!
//! * A ticket opened on a branch that has a KOT printer ([`queue_on_open`]) queues a `kot` row in
//!   the transaction that opens it, after the commit the print task sends it. With the KOT printer
//!   None, or no printer chosen, nothing is queued and the ticket flows only to the Hub and the
//!   kitchen displays. A ticket that is cancelled, or removed with its placement, before its KOT
//!   printed never prints one (the store closes the row).
//! * `kot.reprint` queues the same slip again, marked "REPRINT". The ticket row is touched so the
//!   audit names who asked.
//! * `kot.printed` and `kot.print_failed` (system) close the row. A success of a first print writes
//!   nothing, so the ticket's `rev` (and the Undo of a stage change) is untouched; a failure raises
//!   the "Printer offline" alert and touches the ticket so the Hub shows "KOT not printed" with
//!   Reprint. The ticket and the order are unaffected either way.

use arena_proto::{
    AlertKind, AppError, ErrorCode, Intent, IntentPayload, KEY_KITCHEN_KOT_MODE,
    KEY_KITCHEN_KOT_PRINTER, KEY_PRINTER_SELECTED, STAGE_CANCELLED,
};
use arena_store::{
    ChangeOp, KIND_KOT, NewAlert, OutboxRow, RowKey, STATE_DONE, STATE_FAILED, STATE_QUEUED,
    STATE_SENDING, StoreTx, TicketRow, find_open_alert, find_outbox, find_setting, find_ticket,
    finish_outbox, insert_alert, insert_outbox,
};

use super::mutate::Written;
use super::receipt_print::valid_error;
use super::session::invalid;

#[derive(Default)]
pub(crate) struct KotTarget {
    ticket: Option<TicketRow>,
    row: Option<OutboxRow>,
    /// Whether the branch has the `kitchen` flag.
    kitchen: bool,
    /// Whether a KOT printer is set up: the receipt printer, or another one, per the KOT mode.
    printer: bool,
    printer_alert_open: bool,
}

fn setting(tx: &StoreTx<'_>, key: &str) -> Result<String, AppError> {
    Ok(find_setting(tx, key)?.map(|s| s.value).unwrap_or_default())
}

/// Whether a KOT printer is set up: not None, and a printer chosen for it.
fn has_printer(tx: &StoreTx<'_>) -> Result<bool, AppError> {
    let key = match setting(tx, KEY_KITCHEN_KOT_MODE)?.as_str() {
        "none" => return Ok(false),
        "another" => KEY_KITCHEN_KOT_PRINTER,
        _ => KEY_PRINTER_SELECTED,
    };
    Ok(!setting(tx, key)?.trim().is_empty())
}

fn payload(reprint: bool, by: &str) -> String {
    serde_json::json!({ "reprint": reprint, "by": by }).to_string()
}

/// Whether a `kot` row is a reprint.
pub(crate) fn is_reprint(row: &OutboxRow) -> bool {
    serde_json::from_str::<serde_json::Value>(&row.payload)
        .ok()
        .and_then(|v| v.get("reprint").and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
}

/// Queues the KOT of a ticket that was just opened (the print task sends it after commit).
pub(super) fn queue_on_open(
    tx: &StoreTx<'_>,
    ticket_id: &str,
    now_ms: i64,
) -> Result<(), AppError> {
    if !has_printer(tx)? {
        return Ok(());
    }
    insert_outbox(
        tx,
        &ulid::Ulid::generate().to_string(),
        KIND_KOT,
        ticket_id,
        &payload(false, ""),
        now_ms,
    )
}

pub(super) fn load(tx: &StoreTx<'_>, p: &IntentPayload) -> Result<KotTarget, AppError> {
    Ok(match p {
        IntentPayload::KotReprint(r) => KotTarget {
            ticket: find_ticket(tx, &r.ticket_id)?,
            kitchen: crate::entitlement::has_flag(tx, arena_proto::FLAG_KITCHEN)?,
            printer: has_printer(tx)?,
            ..KotTarget::default()
        },
        IntentPayload::KotPrinted(r) => result_target(tx, &r.outbox_id, false)?,
        IntentPayload::KotPrintFailed(r) => result_target(tx, &r.outbox_id, true)?,
        _ => KotTarget::default(),
    })
}

fn result_target(tx: &StoreTx<'_>, outbox_id: &str, failed: bool) -> Result<KotTarget, AppError> {
    let row = find_outbox(tx, outbox_id)?;
    let ticket = match &row {
        Some(r) => find_ticket(tx, &r.ref_id)?,
        None => None,
    };
    Ok(KotTarget {
        ticket,
        row,
        printer_alert_open: failed
            && find_open_alert(tx, AlertKind::PrinterOffline.as_str(), None)?.is_some(),
        ..KotTarget::default()
    })
}

fn open_row(t: &KotTarget) -> Result<&OutboxRow, AppError> {
    let Some(row) = &t.row else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "outbox_id",
            "no such outbox row",
        ));
    };
    if row.kind != KIND_KOT {
        return Err(invalid(
            "outbox_id",
            "this outbox row is not of the kind this result names",
        ));
    }
    if row.state != STATE_QUEUED && row.state != STATE_SENDING {
        return Err(AppError::with_field(
            ErrorCode::Stale,
            "outbox_id",
            "this KOT has already been recorded",
        ));
    }
    Ok(row)
}

pub(super) fn validate(intent: &Intent, t: &KotTarget) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "KOT intents take no base_rev"));
    }
    match &intent.payload {
        IntentPayload::KotReprint(_) => {
            let Some(ticket) = &t.ticket else {
                return Err(super::kitchen_ticket::missing(t.kitchen));
            };
            if ticket.stage == STAGE_CANCELLED {
                return Err(AppError::with_field(
                    ErrorCode::Stale,
                    "ticket_id",
                    "this ticket was cancelled, so there is no KOT to reprint",
                ));
            }
            if !t.printer {
                return Err(invalid(
                    "ticket_id",
                    "no KOT printer is set up (Settings → Kitchen)",
                ));
            }
            Ok(())
        }
        IntentPayload::KotPrinted(_) => open_row(t).map(|_| ()),
        IntentPayload::KotPrintFailed(p) => {
            open_row(t)?;
            valid_error(&p.error, "KOT print")
        }
        _ => Err(AppError::new(
            ErrorCode::Internal,
            "the loaded target does not match the payload",
        )),
    }
}

fn touch(ticket: &TicketRow) -> Written {
    Written {
        key: RowKey::KitchenTickets {
            id: ticket.id.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    }
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    t: &KotTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    match &intent.payload {
        IntentPayload::KotReprint(r) => {
            insert_outbox(
                tx,
                &ulid::Ulid::generate().to_string(),
                KIND_KOT,
                &r.ticket_id,
                &payload(true, &intent.actor.id),
                now_ms,
            )?;
            Ok(t.ticket.iter().map(touch).collect())
        }
        IntentPayload::KotPrinted(p) => {
            let row = open_row(t)?;
            finish_outbox(tx, &p.outbox_id, STATE_DONE, now_ms, None)?;
            // A reprint was asked for at the screen, so it refreshes the ticket row.
            Ok(t.ticket
                .iter()
                .filter(|_| is_reprint(row))
                .map(touch)
                .collect())
        }
        IntentPayload::KotPrintFailed(p) => {
            open_row(t)?;
            finish_outbox(tx, &p.outbox_id, STATE_FAILED, now_ms, Some(p.error.trim()))?;
            let mut written: Vec<Written> = t.ticket.iter().map(touch).collect();
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
                        ticket_id: None,
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
            "a KOT intent reached mutate without its target",
        )),
    }
}
