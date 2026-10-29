//! Story 7.5: a ticket queues a KOT after commit, and `kot.reprint`, `kot.printed` and
//! `kot.print_failed` close the loop without ever touching the ticket's stage or the order.

use super::kitchen_ticket::{advance, basic, flags, tickets};
use super::order_line::{cancel, code, line, order_of, place, place_of, running};
use super::prepaid::{sql, system};
use super::session::{refusal, rev_of, submit, table, text};
use super::stock::owner;
use super::*;
use arena_proto::{KotPrintFailed, KotPrinted, KotReprint, PrinterErrorKind};

const LAN: &str = r#"{"kind":"lan","id_or_addr":"192.168.1.50:9100","name":"Counter"}"#;

async fn setting(f: &Fixture, key: &str, value: &str) {
    sql(
        f,
        format!("UPDATE settings SET value = '{value}' WHERE key = '{key}'"),
    )
    .await;
}

/// The KOT rows on the outbox: id, ticket, state, payload.
fn kots(f: &Fixture) -> Vec<(String, String, String, String)> {
    let mut rows = Vec::new();
    reopen(f)
        .write_tx(|tx| {
            let mut stmt = tx
                .prepare(
                    "SELECT id, ref_id, state, payload FROM side_effect_outbox
                     WHERE kind = 'kot' ORDER BY at, id",
                )
                .unwrap();
            rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            Ok(())
        })
        .unwrap();
    rows
}

async fn one_ticket(f: &Fixture) -> (String, String) {
    let (b, session) = basic(f).await;
    place(f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    (text(&tickets(f)[0], "id"), session)
}

async fn result(f: &Fixture, payload: IntentPayload) -> Result<Applied, AppError> {
    submit(f, system(), payload).await
}

fn failed(row: &str, error: &str) -> IntentPayload {
    IntentPayload::KotPrintFailed(KotPrintFailed {
        outbox_id: row.into(),
        error_kind: PrinterErrorKind::Refused,
        error: error.into(),
    })
}

async fn reprint(f: &Fixture, ticket: &str) -> Result<Applied, AppError> {
    submit(
        f,
        owner(),
        IntentPayload::KotReprint(KotReprint {
            ticket_id: ticket.into(),
        }),
    )
    .await
}

fn alert_kinds(f: &Fixture) -> Vec<String> {
    table(f, Table::Alerts)
        .iter()
        .map(|a| text(a, "kind"))
        .collect()
}

#[tokio::test]
async fn a_ticket_queues_one_kot_when_a_printer_is_set_up_and_nothing_otherwise() {
    let f = bootstrapped().await;
    let (b, session) = basic(&f).await;

    // The receipt printer is the default, but none is chosen yet: nothing is queued.
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    assert_eq!(tickets(&f).len(), 1);
    assert!(kots(&f).is_empty());

    setting(&f, "printer.selected", LAN).await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    let rows = kots(&f);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, text(&tickets(&f)[1], "id"));
    assert_eq!(rows[0].2, "queued");
    assert_eq!(rows[0].3, r#"{"by":"","reprint":false}"#);

    // None: the ticket flows to the Hub only.
    setting(&f, "kitchen.kot_mode", "none").await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    assert_eq!(tickets(&f).len(), 3);
    assert_eq!(kots(&f).len(), 1);

    // Another printer takes the slip once it is chosen.
    setting(&f, "kitchen.kot_mode", "another").await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    assert_eq!(kots(&f).len(), 1);
    setting(&f, "kitchen.kot_printer", LAN).await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    assert_eq!(kots(&f).len(), 2);
}

#[tokio::test]
async fn a_free_branch_opens_no_ticket_and_queues_no_kot() {
    let f = bootstrapped().await;
    setting(&f, "printer.selected", LAN).await;
    let b = super::order_line::burger(&f, 5).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    assert!(tickets(&f).is_empty());
    assert!(kots(&f).is_empty());
}

#[tokio::test]
async fn a_cancelled_ticket_prints_no_slip() {
    let f = bootstrapped().await;
    setting(&f, "printer.selected", LAN).await;
    let (b, session) = basic(&f).await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    assert_eq!(kots(&f)[0].2, "queued");
    cancel(
        &f,
        owner(),
        &text(&line(&f, &session, "Burger"), "id"),
        false,
    )
    .await
    .unwrap();
    assert_eq!(kots(&f)[0].2, "done", "the cancel closed the queued slip");
    assert_eq!(text(&tickets(&f)[0], "stage"), "cancelled");
}

#[tokio::test]
async fn a_printed_first_kot_writes_nothing_and_leaves_the_ticket_revision_alone() {
    let f = bootstrapped().await;
    setting(&f, "printer.selected", LAN).await;
    let (ticket, _) = one_ticket(&f).await;
    let rev = rev_of(&f, Table::KitchenTickets, &ticket);
    let row = kots(&f)[0].0.clone();

    let applied = result(
        &f,
        IntentPayload::KotPrinted(KotPrinted {
            outbox_id: row.clone(),
        }),
    )
    .await
    .unwrap();
    assert!(applied.changes.is_empty());
    assert_eq!(kots(&f)[0].2, "done");
    assert_eq!(rev_of(&f, Table::KitchenTickets, &ticket), rev);
    // The Undo of a stage change is still open after the slip printed.
    advance(&f, &ticket, "preparing").await.unwrap();
    assert_eq!(
        code(result(&f, IntentPayload::KotPrinted(KotPrinted { outbox_id: row })).await),
        ErrorCode::Stale
    );
}

#[tokio::test]
async fn a_failed_kot_raises_one_offline_alert_and_leaves_ticket_and_order_unaffected() {
    let f = bootstrapped().await;
    setting(&f, "printer.selected", LAN).await;
    let (ticket, session) = one_ticket(&f).await;
    let order = text(&order_of(&f, &session).unwrap(), "id");
    let order_rev = rev_of(&f, Table::Orders, &order);
    let row = kots(&f)[0].0.clone();

    let applied = result(&f, failed(&row, "paper out")).await.unwrap();
    let tables: Vec<_> = applied.changes.iter().map(|c| c.tbl.as_str()).collect();
    assert_eq!(tables, ["kitchen_tickets", "alerts"]);
    assert_eq!(alert_kinds(&f), ["printer_offline"]);
    assert_eq!(kots(&f)[0].2, "failed");
    assert_eq!(text(&tickets(&f)[0], "stage"), "new");
    assert_eq!(rev_of(&f, Table::Orders, &order), order_rev);

    // The ticket still moves, and a second failure adds no second alert.
    advance(&f, &ticket, "preparing").await.unwrap();
    reprint(&f, &ticket).await.unwrap();
    let again = kots(&f)[1].0.clone();
    assert_eq!(
        code(result(&f, failed(&again, " ")).await),
        ErrorCode::ValidationFailed
    );
    result(&f, failed(&again, "still offline")).await.unwrap();
    assert_eq!(alert_kinds(&f), ["printer_offline"]);
}

#[tokio::test]
async fn kot_reprint_queues_a_marked_slip_and_the_audit_names_who_asked() {
    let f = bootstrapped().await;
    setting(&f, "printer.selected", LAN).await;
    let (ticket, _) = one_ticket(&f).await;

    let applied = reprint(&f, &ticket).await.unwrap();
    assert_eq!(
        applied
            .changes
            .iter()
            .map(|c| c.tbl.as_str())
            .collect::<Vec<_>>(),
        ["kitchen_tickets"]
    );
    let rows = kots(&f);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].1, ticket);
    assert_eq!(rows[1].2, "queued");
    assert!(rows[1].3.contains(r#""reprint":true"#));
    assert!(rows[1].3.contains(&owner().id));

    // The reprint's success refreshes the ticket row, so the Hub drops "KOT not printed".
    let done = result(
        &f,
        IntentPayload::KotPrinted(KotPrinted {
            outbox_id: rows[1].0.clone(),
        }),
    )
    .await
    .unwrap();
    assert_eq!(done.changes.len(), 1);
}

#[tokio::test]
async fn kot_reprint_is_refused_for_a_cancelled_ticket_a_missing_ticket_and_no_printer() {
    let f = bootstrapped().await;
    let (ticket, session) = one_ticket(&f).await;
    assert_eq!(
        refusal(reprint(&f, &ticket).await).0,
        ErrorCode::ValidationFailed,
        "no printer is set up"
    );
    setting(&f, "printer.selected", LAN).await;
    assert_eq!(code(reprint(&f, "nope").await), ErrorCode::NotFound);
    setting(&f, "kitchen.kot_mode", "none").await;
    assert_eq!(
        code(reprint(&f, &ticket).await),
        ErrorCode::ValidationFailed
    );
    setting(&f, "kitchen.kot_mode", "receipt").await;
    cancel(
        &f,
        owner(),
        &text(&line(&f, &session, "Burger"), "id"),
        false,
    )
    .await
    .unwrap();
    assert_eq!(code(reprint(&f, &ticket).await), ErrorCode::Stale);
    assert!(kots(&f).is_empty(), "a refused reprint queues nothing");
}

#[tokio::test]
async fn after_a_lapse_a_reprint_of_an_existing_ticket_works_but_a_missing_one_is_locked() {
    let f = bootstrapped().await;
    setting(&f, "printer.selected", LAN).await;
    let (ticket, _) = one_ticket(&f).await;
    flags(&f, "").await;
    assert_eq!(code(reprint(&f, "nope").await), ErrorCode::TierLocked);
    reprint(&f, &ticket).await.unwrap();
    assert_eq!(kots(&f).len(), 2);
}

#[tokio::test]
async fn kot_results_are_system_only() {
    let f = bootstrapped().await;
    setting(&f, "printer.selected", LAN).await;
    let (_, _) = one_ticket(&f).await;
    let row = kots(&f)[0].0.clone();
    let as_owner = submit(
        &f,
        owner(),
        IntentPayload::KotPrinted(KotPrinted { outbox_id: row }),
    )
    .await;
    assert_eq!(code(as_owner), ErrorCode::PermissionDenied);
}
