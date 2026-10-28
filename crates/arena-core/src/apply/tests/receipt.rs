//! Story 5.17: the receipt choice at settle, the side-effect outbox, Reprint and the result
//! intents.

use super::prepaid::system;
use super::session::{billed, pay, refusal, rev_of, settle_intent, submit, table, text};
use super::stock::{intent, make, owner, staff, stock_in, up};
use super::*;
use crate::rbac::ROLE_COUNTER;
use arena_proto::PaymentMethod::Cash;
use arena_proto::{
    PosSale, PosSaleLine, PosSaleReverted, PrinterErrorKind, ProductUpsert, ReceiptChoice,
    ReceiptPrintFailed, ReceiptPrinted, ReceiptReprint, UNDO_WINDOW_MS,
};

async fn cola(f: &Fixture) -> String {
    let id = make(
        f,
        ProductUpsert {
            price_minor: 4_000,
            track_stock: true,
            ..up("Cola")
        },
    )
    .await;
    stock_in(f, &id, 50, 2_000).await;
    id
}

async fn counter(f: &Fixture) -> Actor {
    staff(f, "asha", ROLE_COUNTER).await
}

async fn sale_with(
    f: &Fixture,
    by: &Actor,
    product: &str,
    receipt: Option<ReceiptChoice>,
) -> (String, Applied) {
    let applied = submit(
        f,
        by.clone(),
        IntentPayload::PosSale(PosSale {
            lines: vec![PosSaleLine {
                product_id: product.into(),
                qty: 1,
            }],
            payments: vec![pay(Cash, 4_000, None)],
            player_id: None,
            credit_override: None,
            stock_override: None,
            receipt,
        }),
    )
    .await
    .unwrap();
    let id = applied
        .changes
        .iter()
        .find(|c| c.tbl == "orders" && c.op == ChangeOp::Insert)
        .unwrap()
        .row_id
        .clone();
    (id, applied)
}

fn number_of(f: &Fixture, id: &str) -> Option<String> {
    let o = table(f, Table::Orders)
        .into_iter()
        .find(|o| text(o, "id") == id)
        .unwrap();
    match &o["invoice_no"] {
        RowValue::Text(t) => Some(t.clone()),
        _ => None,
    }
}

/// The outbox rows as `(ref_id, state, payload)`, oldest first.
fn outbox(f: &Fixture) -> Vec<(String, String, String, String)> {
    let mut rows: Vec<(String, String, String, String)> = Vec::new();
    reopen(f)
        .write_tx(|tx| {
            let mut stmt = tx
                .prepare(
                    "SELECT id, ref_id, state, payload FROM side_effect_outbox ORDER BY at, id",
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

fn effect_recorded(f: &Fixture, order_id: &str) -> bool {
    let id = order_id.to_owned();
    reopen(f)
        .write_tx(|tx| arena_store::effect_recorded(tx, None, &id))
        .unwrap()
}

fn effect_kinds(f: &Fixture, order_id: &str) -> Vec<String> {
    let id = order_id.to_owned();
    let mut kinds = Vec::new();
    reopen(f)
        .write_tx(|tx| {
            let mut stmt = tx
                .prepare("SELECT kind FROM irreversible_effects WHERE order_id = ?1 ORDER BY kind")
                .unwrap();
            kinds = stmt
                .query_map([id], |r| r.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            Ok(())
        })
        .unwrap();
    kinds
}

fn revert_intent(f: &Fixture, by: &Actor, order: &str) -> Intent {
    let mut revert = intent(
        by.clone(),
        IntentPayload::PosSaleReverted(PosSaleReverted {
            order_id: order.into(),
        }),
    );
    revert.base_rev = Some(rev_of(f, Table::Orders, order));
    revert.issued_at = f.clock.now_ms();
    revert
}

async fn result(f: &Fixture, payload: IntentPayload) -> Result<Applied, AppError> {
    let mut i = intent(system(), payload);
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

async fn reprint(f: &Fixture, by: &Actor, order: &str) -> Result<Applied, AppError> {
    let mut i = intent(
        by.clone(),
        IntentPayload::ReceiptReprint(ReceiptReprint {
            order_id: order.into(),
        }),
    );
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

fn alerts(f: &Fixture) -> Vec<String> {
    table(f, Table::Alerts)
        .iter()
        .map(|a| text(a, "kind"))
        .collect()
}

#[tokio::test]
async fn a_print_settle_numbers_the_sale_in_its_own_transaction_and_queues_one_print() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    let (o, applied) = sale_with(&f, &by, &id, Some(ReceiptChoice::Print)).await;

    let number = number_of(&f, &o).expect("a printed sale is numbered at once");
    assert!(number.starts_with("DEV01/26-27/E1"), "{number}");
    assert!(number.ends_with("/000001"));
    let tables: Vec<_> = applied.changes.iter().map(|c| c.tbl.as_str()).collect();
    assert!(tables.contains(&"invoice_sequences"), "{tables:?}");
    assert_eq!(
        effect_kinds(&f, &o),
        ["invoice_number_assigned", "receipt_printed"]
    );
    let rows = outbox(&f);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].1.as_str(), rows[0].2.as_str()),
        (o.as_str(), "queued")
    );
    assert!(rows[0].3.contains("\"reprint\":false"));

    // The bill is never undoable: the Undo is refused and the sale stays.
    let refused = f.writer.submit_intent(revert_intent(&f, &by, &o)).await;
    assert_eq!(refusal(refused).0, ErrorCode::UndoNotAllowed);
    assert_eq!(number_of(&f, &o), Some(number));
}

#[tokio::test]
async fn none_and_an_absent_choice_leave_the_number_to_the_undo_window() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    let (none, _) = sale_with(&f, &by, &id, Some(ReceiptChoice::None)).await;
    let (absent, _) = sale_with(&f, &by, &id, None).await;
    for o in [&none, &absent] {
        assert_eq!(number_of(&f, o), None);
        assert!(!effect_recorded(&f, o));
    }
    assert!(outbox(&f).is_empty(), "no print is queued without Print");
    f.writer
        .submit_intent(revert_intent(&f, &by, &none))
        .await
        .expect("a sale with no receipt is still undoable inside the window");
}

#[tokio::test]
async fn a_print_settle_numbers_older_sales_whose_window_closed_first() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    let (older, _) = sale_with(&f, &by, &id, None).await;
    f.clock.advance(UNDO_WINDOW_MS + 1);
    let (young, _) = sale_with(&f, &by, &id, None).await;
    f.clock.advance(1_000);
    let (printed, _) = sale_with(&f, &by, &id, Some(ReceiptChoice::Print)).await;

    let seq = |o: &str| number_of(&f, o).map(|n| n.rsplit('/').next().unwrap().to_owned());
    assert_eq!(
        seq(&older).as_deref(),
        Some("000001"),
        "the due sale goes first"
    );
    assert_eq!(seq(&printed).as_deref(), Some("000002"));
    assert_eq!(
        seq(&young),
        None,
        "a sale still inside its window keeps its Undo and is numbered later"
    );
}

#[tokio::test]
async fn a_session_bill_settles_with_print_the_same_way() {
    let f = bootstrapped().await;
    let (order, _, rev) = billed(&f, 10_000, 30).await;
    let rows = |f: &Fixture| {
        crate::SnapshotReader::open(&f.path)
            .unwrap()
            .sessions_today(f.clock.now_ms())
            .unwrap()
            .sessions
    };
    assert!(rows(&f)[0].receipt_order_id.is_none());
    let mut i = settle_intent(&order, Some(rev), vec![pay(Cash, 5_000, None)]);
    if let IntentPayload::SessionSettle(s) = &mut i.payload {
        s.receipt = Some(ReceiptChoice::Print);
    }
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await.unwrap();

    assert!(number_of(&f, &order).is_some());
    assert!(effect_recorded(&f, &order));
    let queued = outbox(&f);
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].1, order);
    // The Sessions screen offers Reprint for the numbered bill.
    let listed = rows(&f);
    assert_eq!(listed[0].receipt_order_id.as_deref(), Some(order.as_str()));
    assert_eq!(listed[0].invoice_no, number_of(&f, &order));
}

#[tokio::test]
async fn reprint_queues_the_same_receipt_marked_and_names_who_asked() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    let (o, _) = sale_with(&f, &by, &id, Some(ReceiptChoice::Print)).await;

    let applied = reprint(&f, &by, &o).await.unwrap();
    assert_eq!(
        applied
            .changes
            .iter()
            .map(|c| c.tbl.as_str())
            .collect::<Vec<_>>(),
        ["orders"],
        "the order row carries the audit entry"
    );
    let rows = outbox(&f);
    assert_eq!(rows.len(), 2);
    assert!(rows[1].3.contains("\"reprint\":true"));
    assert!(rows[1].3.contains(&by.id));
    assert_eq!(rows[1].1, o);
}

#[tokio::test]
async fn reprint_needs_a_numbered_settled_order_and_the_pos_permission() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    let (unnumbered, _) = sale_with(&f, &by, &id, None).await;
    assert_eq!(
        refusal(reprint(&f, &by, &unnumbered).await),
        (ErrorCode::Stale, Some("order_id".into()))
    );
    assert_eq!(
        refusal(reprint(&f, &by, "nope").await),
        (ErrorCode::NotFound, Some("order_id".into()))
    );
    let (numbered, _) = sale_with(&f, &by, &id, Some(ReceiptChoice::Print)).await;
    let nobody = staff(&f, "kit", crate::rbac::ROLE_KITCHEN).await;
    assert_eq!(
        refusal(reprint(&f, &nobody, &numbered).await).0,
        ErrorCode::PermissionDenied
    );
    assert_eq!(outbox(&f).len(), 1, "only the settle's print is queued");
}

#[tokio::test]
async fn a_result_closes_its_row_once_and_only_the_hub_can_report_it() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    let (o, _) = sale_with(&f, &by, &id, Some(ReceiptChoice::Print)).await;
    let row = outbox(&f).remove(0).0;
    let printed = || {
        IntentPayload::ReceiptPrinted(ReceiptPrinted {
            outbox_id: row.clone(),
        })
    };

    let denied = f.writer.submit_intent(intent(owner(), printed())).await;
    assert_eq!(refusal(denied).0, ErrorCode::PermissionDenied);

    let applied = result(&f, printed()).await.unwrap();
    assert_eq!(applied.changes.len(), 1);
    assert_eq!(outbox(&f)[0].2, "done");
    assert_eq!(
        refusal(result(&f, printed()).await).0,
        ErrorCode::Stale,
        "a closed row stays closed"
    );
    assert_eq!(
        refusal(
            result(
                &f,
                IntentPayload::ReceiptPrinted(ReceiptPrinted {
                    outbox_id: "nope".into()
                })
            )
            .await
        )
        .0,
        ErrorCode::NotFound
    );
    assert!(alerts(&f).is_empty());
    assert!(number_of(&f, &o).is_some());
}

#[tokio::test]
async fn a_failed_print_keeps_the_sale_raises_one_offline_alert_and_records_the_error() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    let (first, _) = sale_with(&f, &by, &id, Some(ReceiptChoice::Print)).await;
    let (second, _) = sale_with(&f, &by, &id, Some(ReceiptChoice::Print)).await;
    let rows = outbox(&f);
    let failed = |row: &str, error: &str| {
        IntentPayload::ReceiptPrintFailed(ReceiptPrintFailed {
            outbox_id: row.into(),
            error_kind: PrinterErrorKind::Refused,
            error: error.into(),
        })
    };

    let applied = result(&f, failed(&rows[0].0, "The printer refused"))
        .await
        .unwrap();
    let tables: Vec<_> = applied.changes.iter().map(|c| c.tbl.as_str()).collect();
    assert_eq!(tables, ["orders", "alerts"]);
    assert_eq!(alerts(&f), ["printer_offline"]);
    result(&f, failed(&rows[1].0, "paper out")).await.unwrap();
    assert_eq!(
        alerts(&f),
        ["printer_offline"],
        "an alert that is open is not raised twice"
    );

    let after = outbox(&f);
    assert!(after.iter().all(|r| r.2 == "failed"));
    assert!(number_of(&f, &first).is_some() && number_of(&f, &second).is_some());
    assert_eq!(
        refusal(result(&f, failed(&rows[0].0, " ")).await).0,
        ErrorCode::Stale
    );
    let blank = sale_with(&f, &by, &id, Some(ReceiptChoice::Print)).await.0;
    let row = outbox(&f).pop().unwrap();
    assert_eq!(row.1, blank);
    assert_eq!(
        refusal(result(&f, failed(&row.0, "  ")).await),
        (ErrorCode::ValidationFailed, Some("error".into()))
    );
}

// ---- Story 5.18: the cash drawer kick ----

async fn drawer_setting(f: &Fixture, value: &str) {
    let row = reopen(f)
        .rows(Table::Settings)
        .unwrap()
        .into_iter()
        .find(|r| r["key"] == RowValue::Text(arena_proto::KEY_RECEIPT_DRAWER.into()))
        .expect("the setting is seeded");
    let mut i = intent(
        owner(),
        IntentPayload::SettingsSet(arena_proto::SettingsSet {
            key: arena_proto::KEY_RECEIPT_DRAWER.into(),
            value: value.into(),
        }),
    );
    i.base_rev = Some(match row["rev"] {
        RowValue::Integer(n) => n,
        ref other => panic!("{other:?}"),
    });
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await.unwrap();
}

/// The outbox rows of one kind as `(id, ref_id, state)`.
fn outbox_of(f: &Fixture, kind: &str) -> Vec<(String, String, String)> {
    let kind = kind.to_owned();
    let mut rows = Vec::new();
    reopen(f)
        .write_tx(|tx| {
            let mut stmt = tx
                .prepare(
                    "SELECT id, ref_id, state FROM side_effect_outbox WHERE kind = ?1
                     ORDER BY at, id",
                )
                .unwrap();
            rows = stmt
                .query_map([kind], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            Ok(())
        })
        .unwrap();
    rows
}

fn cash_kinds(f: &Fixture, order_id: &str) -> Vec<String> {
    table(f, Table::CashEvents)
        .iter()
        .filter(|e| text(e, "order_id") == order_id)
        .map(|e| text(e, "kind"))
        .collect()
}

#[tokio::test]
async fn a_cash_settle_with_the_drawer_on_queues_one_kick_and_closes_undo() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    drawer_setting(&f, "true").await;
    let (o, _) = sale_with(&f, &by, &id, None).await;

    assert_eq!(effect_kinds(&f, &o), ["drawer_kicked"]);
    let rows = outbox_of(&f, "drawer");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].1.as_str(), rows[0].2.as_str()),
        (o.as_str(), "queued")
    );
    assert!(
        outbox_of(&f, "print").is_empty(),
        "no receipt was asked for"
    );
    assert_eq!(number_of(&f, &o), None, "a kick does not number the sale");

    let refused = f.writer.submit_intent(revert_intent(&f, &by, &o)).await;
    assert_eq!(refusal(refused).0, ErrorCode::UndoNotAllowed);
}

#[tokio::test]
async fn print_and_the_drawer_queue_side_by_side() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    drawer_setting(&f, "true").await;
    let (o, _) = sale_with(&f, &by, &id, Some(ReceiptChoice::Print)).await;
    assert_eq!(
        effect_kinds(&f, &o),
        [
            "drawer_kicked",
            "invoice_number_assigned",
            "receipt_printed"
        ]
    );
    assert_eq!(outbox_of(&f, "drawer").len(), 1);
    assert_eq!(outbox_of(&f, "print").len(), 1);
}

#[tokio::test]
async fn the_drawer_off_or_no_cash_row_sends_no_kick() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;

    let (off, _) = sale_with(&f, &by, &id, None).await;
    assert!(outbox_of(&f, "drawer").is_empty());
    assert!(!effect_recorded(&f, &off));

    drawer_setting(&f, "true").await;
    let applied = submit(
        &f,
        by.clone(),
        IntentPayload::PosSale(PosSale {
            lines: vec![PosSaleLine {
                product_id: id.clone(),
                qty: 1,
            }],
            payments: vec![pay(arena_proto::PaymentMethod::Upi, 4_000, Some("1234"))],
            player_id: None,
            credit_override: None,
            stock_override: None,
            receipt: None,
        }),
    )
    .await
    .unwrap();
    let upi = applied
        .changes
        .iter()
        .find(|c| c.tbl == "orders" && c.op == ChangeOp::Insert)
        .unwrap()
        .row_id
        .clone();
    assert!(
        outbox_of(&f, "drawer").is_empty(),
        "UPI never opens the drawer"
    );
    assert!(!effect_recorded(&f, &upi), "and its Undo stays open");
}

#[tokio::test]
async fn drawer_kicked_records_a_drawer_open_event_once_and_only_the_hub_can_report_it() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    drawer_setting(&f, "true").await;
    let (o, _) = sale_with(&f, &by, &id, None).await;
    let row = outbox_of(&f, "drawer").remove(0).0;
    let kicked = || {
        IntentPayload::DrawerKicked(arena_proto::DrawerKicked {
            outbox_id: row.clone(),
        })
    };

    let denied = f.writer.submit_intent(intent(owner(), kicked())).await;
    assert_eq!(refusal(denied).0, ErrorCode::PermissionDenied);
    // A print result cannot close a drawer row, nor the other way round.
    let wrong = result(
        &f,
        IntentPayload::ReceiptPrinted(ReceiptPrinted {
            outbox_id: row.clone(),
        }),
    )
    .await;
    assert_eq!(refusal(wrong).0, ErrorCode::ValidationFailed);

    let applied = result(&f, kicked()).await.unwrap();
    let tables: Vec<_> = applied.changes.iter().map(|c| c.tbl.as_str()).collect();
    assert_eq!(tables, ["orders", "cash_events"]);
    assert_eq!(outbox_of(&f, "drawer")[0].2, "done");
    assert_eq!(cash_kinds(&f, &o), ["sale", "drawer_open"]);
    let open = table(&f, Table::CashEvents)
        .into_iter()
        .find(|e| text(e, "kind") == "drawer_open")
        .unwrap();
    assert_eq!(open["amount_minor"], RowValue::Integer(0));
    assert_eq!(text(&open, "staff_id"), by.id);
    assert_eq!(
        text(&open, "shift_id"),
        text(
            &table(&f, Table::CashEvents)
                .into_iter()
                .find(|e| text(e, "kind") == "sale")
                .unwrap(),
            "shift_id"
        )
    );
    assert_eq!(refusal(result(&f, kicked()).await).0, ErrorCode::Stale);
}

#[tokio::test]
async fn a_failed_kick_keeps_the_sale_raises_the_offline_alert_and_writes_no_cash_event() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    drawer_setting(&f, "true").await;
    let (o, _) = sale_with(&f, &by, &id, None).await;
    let row = outbox_of(&f, "drawer").remove(0).0;
    let failed = |error: &str| {
        IntentPayload::DrawerKickFailed(arena_proto::DrawerKickFailed {
            outbox_id: row.clone(),
            error_kind: PrinterErrorKind::Refused,
            error: error.into(),
        })
    };
    assert_eq!(
        refusal(result(&f, failed("  ")).await),
        (ErrorCode::ValidationFailed, Some("error".into()))
    );
    let applied = result(&f, failed("The printer refused")).await.unwrap();
    let tables: Vec<_> = applied.changes.iter().map(|c| c.tbl.as_str()).collect();
    assert_eq!(tables, ["orders", "alerts"]);
    assert_eq!(alerts(&f), ["printer_offline"]);
    assert_eq!(outbox_of(&f, "drawer")[0].2, "failed");
    assert_eq!(cash_kinds(&f, &o), ["sale"]);
    assert_eq!(
        refusal(result(&f, failed("again")).await).0,
        ErrorCode::Stale
    );
}
