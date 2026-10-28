//! Story 5.28: the Z-report at shift close.

use super::prepaid::{sql, system};
use super::session::{int, pay, pc, refusal, rev_of, sessions, start, submit, table, text};
use super::stock::{intent, make, manager_with_pin, owner, staff, stock_in, up};
use super::*;
use crate::rbac::ROLE_COUNTER;
use arena_proto::PaymentMethod::{Card, Cash, Upi};
use arena_proto::{
    CloseDeposit, DeviceKind, ElevatedBy, ElevationKind, ElevationMode, ExpenseCategory,
    ExpenseCreate, ExpensePaidFrom, InvoiceIssue, OrderPlace, OrderVoid, PaymentRow, PosSale,
    PosSaleLine, PosSaleReverted, PrinterErrorKind, ProductUpsert, ReceiptPrintFailed,
    ReceiptPrinted, RefundCreate, RefundLineReq, ShiftClose, ShiftOpen, StockWaste, UNDO_WINDOW_MS,
    ZReport, ZReportReprint,
};

const LAN: &str = r#"{"kind":"lan","id_or_addr":"192.168.1.50:9100","name":"Counter"}"#;

fn open(float_minor: i64) -> IntentPayload {
    IntentPayload::ShiftOpen(ShiftOpen { float_minor })
}

fn close(counted_minor: i64) -> ShiftClose {
    ShiftClose {
        counted_minor,
        deposit: None,
        reason: None,
        note: None,
    }
}

async fn product(f: &Fixture, name: &str, price: i64, qty: i64) -> String {
    let id = make(
        f,
        ProductUpsert {
            price_minor: price,
            hsn: "2106".into(),
            ..up(name)
        },
    )
    .await;
    stock_in(f, &id, qty, price / 2).await;
    id
}

async fn sell(f: &Fixture, id: &str, qty: i64, payments: Vec<PaymentRow>) -> String {
    submit(
        f,
        owner(),
        IntentPayload::PosSale(PosSale {
            business: None,
            coupon: None,
            discount: None,
            lines: vec![PosSaleLine {
                product_id: id.into(),
                qty,
            }],
            payments,
            player_id: None,
            credit_override: None,
            stock_override: None,
            receipt: None,
        }),
    )
    .await
    .unwrap()
    .changes
    .iter()
    .find(|c| c.tbl == "orders")
    .unwrap()
    .row_id
    .clone()
}

async fn revert(f: &Fixture, order: &str) -> Result<Applied, AppError> {
    let mut i = intent(
        owner(),
        IntentPayload::PosSaleReverted(PosSaleReverted {
            order_id: order.into(),
        }),
    );
    i.base_rev = Some(rev_of(f, Table::Orders, order));
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

/// Closes the Undo window and numbers the order.
async fn number(f: &Fixture, order: &str) -> String {
    f.clock.advance(UNDO_WINDOW_MS + 1);
    let mut i = intent(
        system(),
        IntentPayload::InvoiceIssue(InvoiceIssue {
            order_id: order.into(),
        }),
    );
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await.unwrap();
    text(&order_row(f, order), "invoice_no")
}

fn order_row(f: &Fixture, id: &str) -> Row {
    table(f, Table::Orders)
        .into_iter()
        .find(|o| text(o, "id") == id)
        .unwrap()
}

fn only_shift(f: &Fixture) -> Row {
    let mut all = table(f, Table::Shifts);
    assert_eq!(all.len(), 1);
    all.remove(0)
}

fn z_of(f: &Fixture) -> ZReport {
    let shift = only_shift(f);
    reopen_reader(f)
        .z_report(&text(&shift, "id"), f.clock.now_ms())
        .unwrap()
}

fn reopen_reader(f: &Fixture) -> crate::SnapshotReader {
    crate::SnapshotReader::open(&f.path).unwrap()
}

/// `(ref_id, state, payload)` of the outbox rows, oldest first.
fn outbox(f: &Fixture) -> Vec<(String, String, String, String)> {
    let mut rows = Vec::new();
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

fn amount(list: &[arena_proto::ReportAmount], key: &str) -> (i64, i64) {
    list.iter()
        .find(|a| a.key == key)
        .map_or((0, 0), |a| (a.amount_minor, a.count))
}

fn cash_sum(f: &Fixture, kinds: &[&str]) -> i64 {
    table(f, Table::CashEvents)
        .iter()
        .filter(|e| kinds.contains(&text(e, "kind").as_str()))
        .map(|e| int(e, "amount_minor"))
        .sum()
}

/// The fixture shift of the story: cash, UPI and card sales, an undone sale, a cash refund, an
/// expense, a deposit, waste and a voided order. Returns the ids the checks need.
struct Fixture28 {
    refunded_invoice: String,
    upi_invoice: String,
}

async fn fixture_shift(f: &Fixture) -> Fixture28 {
    submit(f, owner(), open(100_000)).await.unwrap();
    let cola = product(f, "Cola", 4_000, 30).await;
    let cash = sell(f, &cola, 3, vec![pay(Cash, 12_000, None)]).await;
    let upi = sell(f, &cola, 2, vec![pay(Upi, 8_000, Some("1234"))]).await;
    let refunded_invoice = number(f, &cash).await;
    let upi_invoice = number(f, &upi).await;
    // Card sale left unnumbered: its row lists an empty invoice number.
    sell(f, &cola, 1, vec![pay(Card, 4_000, Some("T-9"))]).await;
    // A sale taken back inside its Undo window nets to zero.
    let undone = sell(f, &cola, 1, vec![pay(Upi, 4_000, Some("9999"))]).await;
    revert(f, &undone).await.unwrap();

    // Cash refund of one cola from the numbered cash sale.
    let line = table(f, Table::OrderLines)
        .into_iter()
        .find(|l| text(l, "order_id") == cash)
        .map(|l| text(&l, "id"))
        .unwrap();
    let mut refund = intent(
        owner(),
        IntentPayload::RefundCreate(RefundCreate {
            invoice_no: refunded_invoice.clone(),
            lines: vec![RefundLineReq {
                line_id: line,
                qty: 1,
                return_to_stock: None,
            }],
            method: Some(Cash),
            reference: None,
            note: None,
            receipt: None,
        }),
    );
    refund.reason = Some(ReasonCode::Mistake);
    refund.issued_at = f.clock.now_ms();
    f.writer.submit_intent(refund).await.unwrap();

    submit(
        f,
        owner(),
        IntentPayload::ExpenseCreate(ExpenseCreate {
            category: ExpenseCategory::Supplies,
            amount_minor: 5_000,
            paid_from: ExpensePaidFrom::Drawer,
            receipt_photo: None,
            note: Some("Tissue rolls".into()),
        }),
    )
    .await
    .unwrap();
    submit(
        f,
        owner(),
        IntentPayload::CashDeposit(arena_proto::CashDeposit {
            amount_minor: 20_000,
            reference: "SLIP-1".into(),
        }),
    )
    .await
    .unwrap();

    let mut waste = intent(
        owner(),
        IntentPayload::StockWaste(StockWaste {
            product_id: cola.clone(),
            qty: 2,
        }),
    );
    waste.reason = Some(ReasonCode::Spoiled);
    waste.issued_at = f.clock.now_ms();
    f.writer.submit_intent(waste).await.unwrap();

    // A voided order: an item placed on a running session's bill, then voided.
    let d = pc(f, DeviceKind::Pc, "pc", Some(10_000)).await;
    start(f, &d, None).await.unwrap();
    let session = text(sessions(f).last().unwrap(), "id");
    submit(
        f,
        owner(),
        IntentPayload::OrderPlace(OrderPlace {
            session_id: session.clone(),
            lines: vec![PosSaleLine {
                product_id: cola,
                qty: 1,
            }],
            stock_override: None,
        }),
    )
    .await
    .unwrap();
    let held = table(f, Table::Orders)
        .into_iter()
        .find(|o| matches!(&o["session_id"], RowValue::Text(t) if *t == session))
        .map(|o| text(&o, "id"))
        .unwrap();
    let mut void = intent(
        owner(),
        IntentPayload::OrderVoid(OrderVoid {
            order_id: held,
            note: None,
        }),
    );
    void.reason = Some(ReasonCode::Mistake);
    void.issued_at = f.clock.now_ms();
    f.writer.submit_intent(void).await.unwrap();
    Fixture28 {
        refunded_invoice,
        upi_invoice,
    }
}

#[tokio::test]
async fn the_close_stores_a_z_report_that_nets_undone_sales_and_matches_the_ledger() {
    let f = bootstrapped_closed().await;
    let ids = fixture_shift(&f).await;
    // float 1,000.00 + cash sales 120.00 - refund 40.00 - expense 50.00 - deposit 200.00.
    let expected = 100_000 + 12_000 - 4_000 - 5_000 - 20_000;
    assert_eq!(
        cash_sum(
            &f,
            &[
                "float_in",
                "sale",
                "sale_reverted",
                "refund",
                "deposit",
                "expense"
            ]
        ),
        expected
    );
    submit(
        &f,
        owner(),
        IntentPayload::ShiftClose(close(expected - 500)),
    )
    .await
    .unwrap();

    let shift = only_shift(&f);
    let stored = match &shift["z_report"] {
        RowValue::Text(t) => t.clone(),
        other => panic!("{other:?}"),
    };
    let z: ZReport = serde_json::from_str(&stored).unwrap();
    assert_eq!(z, z_of(&f), "the read serves the stored report");
    assert!(!z.rebuilt);

    // Sales: 3 + 2 + 1 cola; the undone sale is gone.
    assert_eq!((z.orders, z.sales_minor), (3, 24_000));
    assert_eq!(
        z.by_type.iter().map(|t| t.amount_minor).sum::<i64>(),
        z.sales_minor,
        "the types add up to the total"
    );
    assert_eq!(amount(&z.by_type, "products"), (24_000, 6));
    // Payments by method, with counts; cash equals the cash sale events.
    assert_eq!(amount(&z.payments, "cash"), (12_000, 1));
    assert_eq!(amount(&z.payments, "upi"), (8_000, 1));
    assert_eq!(amount(&z.payments, "card"), (4_000, 1));
    assert_eq!(
        amount(&z.payments, "cash").0,
        cash_sum(&f, &["sale", "sale_reverted"])
    );
    // UPI and card references.
    assert_eq!(z.references.len(), 2);
    assert_eq!(z.references[0].method, "upi");
    assert_eq!(z.references[0].rows.len(), 1);
    let upi = &z.references[0].rows[0];
    assert_eq!(
        (
            upi.invoice_no.as_str(),
            upi.amount_minor,
            upi.reference.as_str()
        ),
        (ids.upi_invoice.as_str(), 8_000, "1234")
    );
    assert_eq!(z.references[1].method, "card");
    let card = &z.references[1].rows[0];
    assert_eq!(
        (
            card.invoice_no.as_str(),
            card.amount_minor,
            card.reference.as_str()
        ),
        ("", 4_000, "T-9")
    );
    // Refunds are negative lines.
    assert_eq!(z.refunds.len(), 1);
    assert_eq!(z.refunds[0].amount_minor, -4_000);
    assert_eq!(z.refunds[0].method, "cash");
    assert_eq!(z.refunds_minor, -4_000);
    assert!(z.refunds[0].credit_note_no.contains('/'));
    assert!(!ids.refunded_invoice.is_empty());
    // Waste, expenses, deposits, voids.
    assert_eq!(z.waste.len(), 1);
    assert_eq!((z.waste[0].reason.as_str(), z.waste[0].qty), ("spoiled", 2));
    assert_eq!(z.waste[0].value_minor, 4_000);
    assert_eq!(z.expenses.len(), 1);
    assert_eq!(
        (
            z.expenses[0].category.as_str(),
            z.expenses[0].paid_from.as_str()
        ),
        ("supplies", "drawer")
    );
    assert_eq!(
        (z.expenses[0].amount_minor, z.expenses[0].note.as_str()),
        (5_000, "Tissue rolls")
    );
    assert_eq!(z.deposits.len(), 1);
    assert_eq!(
        (z.deposits[0].reference.as_str(), z.deposits[0].amount_minor),
        ("SLIP-1", 20_000)
    );
    assert_eq!(z.voids.len(), 1);
    assert_eq!(
        (z.voids[0].reason.as_str(), z.voids[0].count),
        ("mistake", 1)
    );
    assert_eq!(z.drawer_opens, 0);
    // Expected vs counted with the variance.
    assert_eq!(z.float_minor, 100_000);
    assert_eq!(z.expected_minor, expected);
    assert_eq!(z.counted_minor, expected - 500);
    assert_eq!(z.variance_minor, -500);
    assert_eq!(z.variance_reason, None);
    assert_eq!(z.shift_id, text(&shift, "id"));
    assert_eq!(z.closed_at, int(&shift, "closed_at"));

    // A report built from the rows now is the stored one.
    sql(&f, "UPDATE shifts SET z_report = NULL".to_owned()).await;
    let rebuilt = z_of(&f);
    assert!(rebuilt.rebuilt, "a shift with no stored report is marked");
    assert_eq!(
        ZReport {
            rebuilt: false,
            ..rebuilt
        },
        z
    );
}

#[tokio::test]
async fn an_undo_after_the_close_leaves_the_stored_report_final() {
    let f = bootstrapped_closed().await;
    submit(&f, owner(), open(10_000)).await.unwrap();
    let cola = product(&f, "Cola", 4_000, 5).await;
    let order = sell(&f, &cola, 1, vec![pay(Cash, 4_000, None)]).await;
    submit(&f, owner(), IntentPayload::ShiftClose(close(14_000)))
        .await
        .unwrap();
    assert_eq!(z_of(&f).sales_minor, 4_000);

    revert(&f, &order).await.unwrap();
    let z = z_of(&f);
    assert_eq!((z.orders, z.sales_minor), (1, 4_000));
    assert_eq!(amount(&z.payments, "cash"), (4_000, 1));
    assert!(!z.rebuilt);
    // The live rows moved: a report built now would show no sale.
    sql(&f, "UPDATE shifts SET z_report = NULL".to_owned()).await;
    assert_eq!(z_of(&f).sales_minor, 0);
}

#[tokio::test]
async fn a_variance_reason_and_approver_and_a_deposit_at_close_are_in_the_report() {
    let f = bootstrapped_closed().await;
    let counter = staff(&f, "asha", ROLE_COUNTER).await;
    submit(&f, counter.clone(), open(100_000)).await.unwrap();
    let approver = manager_with_pin(&f).await;
    let mut i = intent(
        counter,
        IntentPayload::ShiftClose(ShiftClose {
            counted_minor: 80_000,
            deposit: Some(CloseDeposit {
                amount_minor: 70_000,
                reference: "SLIP-9".into(),
            }),
            reason: Some(arena_proto::VarianceReason::UnrecordedExpense),
            note: None,
        }),
    );
    i.elevated_by = Some(ElevatedBy {
        kind: ElevationKind::Pin,
        id: approver,
        mode: ElevationMode::Hub,
        verifier_version: 1,
    });
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await.unwrap();

    let z = z_of(&f);
    assert_eq!(
        (z.expected_minor, z.counted_minor, z.variance_minor),
        (100_000, 80_000, -20_000)
    );
    assert_eq!(z.variance_reason.as_deref(), Some("unrecorded_expense"));
    assert_eq!(z.approver.as_deref(), Some("mgr"));
    assert_eq!(z.closed_by, "asha");
    assert_eq!(z.deposits.len(), 1);
    assert_eq!(z.deposits[0].amount_minor, 70_000);
}

#[tokio::test]
async fn a_printer_chosen_queues_one_z_report_print_and_none_queues_nothing() {
    let f = bootstrapped_closed().await;
    submit(&f, owner(), open(0)).await.unwrap();
    submit(&f, owner(), IntentPayload::ShiftClose(close(0)))
        .await
        .unwrap();
    assert!(
        outbox(&f).is_empty(),
        "no printer: nothing queued, no alert"
    );
    assert!(table(&f, Table::Alerts).is_empty());

    sql(
        &f,
        format!("UPDATE settings SET value = '{LAN}' WHERE key = 'printer.selected'"),
    )
    .await;
    submit(&f, owner(), open(0)).await.unwrap();
    let applied = submit(&f, owner(), IntentPayload::ShiftClose(close(0)))
        .await
        .unwrap();
    assert_eq!(
        applied.changes.len(),
        1,
        "the close still writes only the shift"
    );
    let second = table(&f, Table::Shifts)
        .into_iter()
        .max_by_key(|s| int(s, "opened_at").max(int(s, "created_at")))
        .unwrap();
    let rows = outbox(&f);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, text(&second, "id"), "ref_id is the shift id");
    assert_eq!(rows[0].2, "queued");
    assert!(rows[0].3.contains("\"z_report\":true") && rows[0].3.contains("\"reprint\":false"));
}

#[tokio::test]
async fn print_results_close_the_row_touch_the_shift_and_a_failure_leaves_the_close() {
    let f = bootstrapped_closed().await;
    sql(
        &f,
        format!("UPDATE settings SET value = '{LAN}' WHERE key = 'printer.selected'"),
    )
    .await;
    submit(&f, owner(), open(0)).await.unwrap();
    submit(&f, owner(), IntentPayload::ShiftClose(close(0)))
        .await
        .unwrap();
    let shift = only_shift(&f);
    let id = text(&shift, "id");
    let row = outbox(&f).remove(0).0;
    let rev = int(&shift, "rev");

    let mut i = intent(
        system(),
        IntentPayload::ReceiptPrinted(ReceiptPrinted {
            outbox_id: row.clone(),
        }),
    );
    i.issued_at = f.clock.now_ms();
    let applied = f.writer.submit_intent(i).await.unwrap();
    assert_eq!(
        applied
            .changes
            .iter()
            .map(|c| c.tbl.as_str())
            .collect::<Vec<_>>(),
        ["shifts"],
        "a Z-report result touches the shift, not an order"
    );
    assert_eq!(outbox(&f)[0].2, "done");
    assert_eq!(int(&only_shift(&f), "rev"), rev + 1);

    // Reprint queues a new row (marked) and touches the shift.
    let mut again = intent(
        owner(),
        IntentPayload::ShiftReprintZ(ZReportReprint {
            shift_id: id.clone(),
        }),
    );
    again.issued_at = f.clock.now_ms();
    f.writer.submit_intent(again).await.unwrap();
    let rows = outbox(&f);
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[1].1.as_str(), rows[1].2.as_str()),
        (id.as_str(), "queued")
    );
    assert!(rows[1].3.contains("\"reprint\":true") && rows[1].3.contains("\"z_report\":true"));

    // A failed print closes the row, raises the one alert and leaves the shift closed.
    let mut fail = intent(
        system(),
        IntentPayload::ReceiptPrintFailed(ReceiptPrintFailed {
            outbox_id: rows[1].0.clone(),
            error_kind: PrinterErrorKind::Refused,
            error: "The printer refused the connection".into(),
        }),
    );
    fail.issued_at = f.clock.now_ms();
    f.writer.submit_intent(fail).await.unwrap();
    assert_eq!(outbox(&f)[1].2, "failed");
    assert_eq!(
        table(&f, Table::Alerts)
            .iter()
            .map(|a| text(a, "kind"))
            .collect::<Vec<_>>(),
        ["printer_offline"]
    );
    assert_eq!(text(&only_shift(&f), "state"), "closed");
}

#[tokio::test]
async fn reprint_refuses_an_unknown_shift_and_an_open_one() {
    let f = bootstrapped_closed().await;
    let reprint = |shift: String| {
        let mut i = intent(
            owner(),
            IntentPayload::ShiftReprintZ(ZReportReprint { shift_id: shift }),
        );
        i.issued_at = f.clock.now_ms();
        i
    };
    let err = f.writer.submit_intent(reprint("NOPE".into())).await;
    assert_eq!(refusal(err).0, ErrorCode::NotFound);
    submit(&f, owner(), open(1_000)).await.unwrap();
    let id = text(&only_shift(&f), "id");
    let err = f.writer.submit_intent(reprint(id)).await;
    assert_eq!(refusal(err).0, ErrorCode::Stale);
    assert!(outbox(&f).is_empty());
}

#[tokio::test]
async fn the_reader_refuses_an_unknown_and_an_open_shift_and_lists_closed_ones() {
    let f = bootstrapped_closed().await;
    let reader = reopen_reader(&f);
    assert_eq!(
        reader.z_report("NOPE", f.clock.now_ms()).unwrap_err().code,
        ErrorCode::NotFound
    );
    submit(&f, owner(), open(1_000)).await.unwrap();
    let id = text(&only_shift(&f), "id");
    assert_eq!(
        reader.z_report(&id, f.clock.now_ms()).unwrap_err().code,
        ErrorCode::Stale
    );
    assert!(reader.shift_list().unwrap().shifts.is_empty());
    submit(&f, owner(), IntentPayload::ShiftClose(close(1_000)))
        .await
        .unwrap();
    let list = reader.shift_list().unwrap().shifts;
    assert_eq!(list.len(), 1);
    assert_eq!(
        (
            list[0].shift_id.as_str(),
            list[0].counted_minor,
            list[0].variance_minor
        ),
        (id.as_str(), 1_000, 0)
    );
    let bytes = reader
        .z_report_bytes(&id, false, f.clock.now_ms())
        .unwrap()
        .expect("a closed shift prints");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("Z-Report") && !text.contains("Reprint"));
    assert!(reader.z_report_bytes("NOPE", false, 0).unwrap().is_none());
}
