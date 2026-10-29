//! Story 5.24: refund a settled order against a credit note.

use super::prepaid::{group_of, plan_for, sql, system};
use super::session::{
    credit_player, end, int, pay, pc, player, refusal, sell_intent, submit, table, text, topped_up,
};
use super::stock::{held, intent, make, manager_with_pin, owner, staff, stock_in, up};
use super::*;
use crate::rbac::ROLE_COUNTER;
use arena_proto::PaymentMethod::{Card, Cash, Credit, Upi, Wallet};
use arena_proto::{
    DeviceKind, ElevatedBy, ElevationKind, ElevationMode, InvoiceIssue, PaymentMethod, PosSale,
    PosSaleLine, ProductUpsert, RefundCreate, RefundLineReq, UNDO_WINDOW_MS,
};

async fn product(f: &Fixture, name: &str, price: i64, rate_bp: Option<u16>, qty: i64) -> String {
    let id = make(
        f,
        ProductUpsert {
            price_minor: price,
            tax_rate_bp: rate_bp,
            hsn: "2106".into(),
            ..up(name)
        },
    )
    .await;
    stock_in(f, &id, qty, price / 2).await;
    id
}

async fn sell(
    f: &Fixture,
    lines: &[(&str, i64)],
    payments: Vec<arena_proto::PaymentRow>,
    player_id: Option<&str>,
) -> String {
    submit(
        f,
        owner(),
        IntentPayload::PosSale(PosSale {
            kiosk: None,
            business: None,
            coupon: None,
            discount: None,
            lines: lines
                .iter()
                .map(|(id, qty)| PosSaleLine {
                    product_id: (*id).into(),
                    qty: *qty,
                    kitchen_notes: Vec::new(),
                })
                .collect(),
            payments,
            player_id: player_id.map(Into::into),
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

/// Closes the Undo window and numbers the order; returns its invoice number.
async fn invoice(f: &Fixture, order: &str) -> String {
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

fn lines_of(f: &Fixture, order: &str) -> Vec<Row> {
    let mut rows: Vec<Row> = table(f, Table::OrderLines)
        .into_iter()
        .filter(|l| text(l, "order_id") == order)
        .collect();
    rows.sort_by_key(|l| int(l, "position"));
    rows
}

struct Req<'a> {
    invoice: &'a str,
    lines: Vec<(String, i64, bool)>,
    method: Option<(PaymentMethod, Option<&'a str>)>,
    reason: Option<ReasonCode>,
    approver: Option<&'a str>,
    note: Option<&'a str>,
}

fn req<'a>(invoice: &'a str, lines: Vec<(String, i64, bool)>) -> Req<'a> {
    Req {
        invoice,
        lines,
        method: Some((Cash, None)),
        reason: Some(ReasonCode::Mistake),
        approver: None,
        note: None,
    }
}

fn refund_intent(actor: Actor, r: &Req<'_>, now: i64) -> Intent {
    let mut i = intent(
        actor,
        IntentPayload::RefundCreate(RefundCreate {
            invoice_no: r.invoice.into(),
            lines: r
                .lines
                .iter()
                .map(|(id, qty, ret)| RefundLineReq {
                    line_id: id.clone(),
                    qty: *qty,
                    return_to_stock: ret.then_some(true),
                })
                .collect(),
            method: r.method.map(|m| m.0),
            reference: r.method.and_then(|m| m.1).map(str::to_owned),
            note: r.note.map(str::to_owned),
            receipt: None,
        }),
    );
    i.reason = r.reason;
    i.elevated_by = r.approver.map(|id| ElevatedBy {
        kind: ElevationKind::Pin,
        id: id.into(),
        mode: ElevationMode::Hub,
        verifier_version: 1,
    });
    i.issued_at = now;
    i
}

async fn refund(f: &Fixture, actor: Actor, r: &Req<'_>) -> Result<Applied, AppError> {
    f.writer
        .submit_intent(refund_intent(actor, r, f.clock.now_ms()))
        .await
}

fn refunds(f: &Fixture) -> Vec<Row> {
    let mut rows = table(f, Table::Refunds);
    rows.sort_by_key(|r| text(r, "credit_note_no"));
    rows
}

fn payments_of(f: &Fixture, order: &str) -> Vec<(String, i64)> {
    table(f, Table::Payments)
        .iter()
        .filter(|p| text(p, "order_id") == order)
        .map(|p| (text(p, "method"), int(p, "amount_minor")))
        .collect()
}

fn cash_events(f: &Fixture, kind: &str) -> Vec<i64> {
    table(f, Table::CashEvents)
        .iter()
        .filter(|e| text(e, "kind") == kind)
        .map(|e| int(e, "amount_minor"))
        .collect()
}

fn moves_of(f: &Fixture, id: &str, reason: &str) -> usize {
    reopen(f)
        .moves(id)
        .unwrap()
        .into_iter()
        .filter(|m| m.reason == reason)
        .count()
}

async fn set_tax(f: &Fixture, json: &str) {
    sql(
        f,
        format!("UPDATE settings SET value = '{json}' WHERE key = 'branch.tax'"),
    )
    .await;
}

#[tokio::test]
async fn a_cash_refund_pays_back_the_lines_returns_stock_and_issues_a_credit_note() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None, 10).await;
    let order = sell(&f, &[(&cola, 3)], vec![pay(Cash, 12_000, None)], None).await;
    let no = invoice(&f, &order).await;
    assert_eq!(held(&f, &cola).0, 7);
    let line = text(&lines_of(&f, &order)[0], "id");

    let a = refund(&f, owner(), &req(&no, vec![(line.clone(), 2, true)]))
        .await
        .unwrap();
    let tables: Vec<&str> = a.changes.iter().map(|c| c.tbl.as_str()).collect();
    for t in [
        "refunds",
        "payments",
        "cash_events",
        "order_lines",
        "invoice_sequences",
        "stock_moves",
    ] {
        assert!(tables.contains(&t), "{t} in {tables:?}");
    }
    let r = &refunds(&f)[0];
    assert_eq!(text(r, "order_id"), order);
    assert_eq!(int(r, "amount_minor"), 8_000);
    assert_eq!(int(r, "credit_minor"), 0);
    assert_eq!(text(r, "reason"), "mistake");
    assert_eq!(text(r, "method"), "cash");
    assert_eq!(text(r, "approved_by"), "o");
    let cn = text(r, "credit_note_no");
    assert_ne!(cn, no, "the credit note is the next number of the series");
    assert!(cn.ends_with("/000002"), "{cn}");
    assert_eq!(
        payments_of(&f, &order),
        vec![("cash".to_owned(), 12_000), ("cash".to_owned(), -8_000)]
    );
    assert_eq!(cash_events(&f, "refund"), vec![-8_000]);
    let l = &lines_of(&f, &order)[0];
    assert_eq!(
        (int(l, "refunded_qty"), int(l, "refunded_minor")),
        (2, 8_000)
    );
    assert_eq!(held(&f, &cola).0, 9, "two units went back to stock");
    assert_eq!(moves_of(&f, &cola, "return"), 1);

    // The rest, without stock: the last unit takes what is left of the line.
    refund(&f, owner(), &req(&no, vec![(line, 1, false)]))
        .await
        .unwrap();
    assert_eq!(held(&f, &cola).0, 9);
    assert_eq!(refunds(&f).len(), 2);
    let l = &lines_of(&f, &order)[0];
    assert_eq!(
        (int(l, "refunded_qty"), int(l, "refunded_minor")),
        (3, 12_000)
    );
}

#[tokio::test]
async fn an_unknown_invoice_and_a_sale_inside_its_undo_window_have_no_invoice() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None, 10).await;
    let order = sell(&f, &[(&cola, 1)], vec![pay(Cash, 4_000, None)], None).await;
    let line = text(&lines_of(&f, &order)[0], "id");
    for no in ["NOPE/26-27/E1a/000001", ""] {
        let (code, _) =
            refusal(refund(&f, owner(), &req(no, vec![(line.clone(), 1, false)])).await);
        assert_eq!(code, ErrorCode::InvoiceNotFound);
    }
    assert!(refunds(&f).is_empty());
}

#[tokio::test]
async fn a_refund_never_exceeds_what_was_paid_less_earlier_refunds() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None, 10).await;
    let order = sell(&f, &[(&cola, 2)], vec![pay(Cash, 8_000, None)], None).await;
    let no = invoice(&f, &order).await;
    let line = text(&lines_of(&f, &order)[0], "id");
    // Money paid back through another route leaves less than the line is worth.
    sql(
        &f,
        format!("UPDATE payments SET amount_minor = 5000 WHERE order_id = '{order}'"),
    )
    .await;
    let before = (refunds(&f).len(), table(&f, Table::Payments).len());
    let (code, _) = refusal(refund(&f, owner(), &req(&no, vec![(line.clone(), 2, false)])).await);
    assert_eq!(code, ErrorCode::RefundExceedsPaid);
    assert_eq!(
        before,
        (refunds(&f).len(), table(&f, Table::Payments).len())
    );
    // A refund within what is left goes through, and then no more units are left to refund.
    refund(&f, owner(), &req(&no, vec![(line.clone(), 1, false)]))
        .await
        .unwrap();
    let (code, _) = refusal(refund(&f, owner(), &req(&no, vec![(line.clone(), 2, false)])).await);
    assert_eq!(code, ErrorCode::ValidationFailed);
    refund(&f, owner(), &req(&no, vec![(line, 1, false)]))
        .await
        .unwrap_err();
}

#[tokio::test]
async fn the_lines_reason_and_method_are_checked() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None, 10).await;
    let plain = make(
        &f,
        ProductUpsert {
            price_minor: 1_000,
            track_stock: false,
            ..up("Tea")
        },
    )
    .await;
    let order = sell(
        &f,
        &[(&cola, 2), (&plain, 1)],
        vec![pay(Cash, 9_000, None)],
        None,
    )
    .await;
    let no = invoice(&f, &order).await;
    let ls = lines_of(&f, &order);
    let (cola_line, tea_line) = (text(&ls[0], "id"), text(&ls[1], "id"));
    let bad = |lines: Vec<(String, i64, bool)>| req(&no, lines);
    let field = |r: Result<Applied, AppError>| {
        let e = r.unwrap_err();
        (e.code, e.field)
    };
    let validation = |f: &str| (ErrorCode::ValidationFailed, Some(f.to_owned()));

    assert_eq!(
        field(refund(&f, owner(), &bad(vec![])).await),
        validation("lines")
    );
    assert_eq!(
        field(refund(&f, owner(), &bad(vec![("nope".into(), 1, false)])).await),
        validation("lines[0].line_id")
    );
    assert_eq!(
        field(refund(&f, owner(), &bad(vec![(cola_line.clone(), 0, false)])).await),
        validation("lines[0].qty")
    );
    assert_eq!(
        field(refund(&f, owner(), &bad(vec![(cola_line.clone(), 3, false)])).await),
        validation("lines[0].qty")
    );
    assert_eq!(
        field(
            refund(
                &f,
                owner(),
                &bad(vec![
                    (cola_line.clone(), 1, false),
                    (cola_line.clone(), 1, false)
                ])
            )
            .await
        ),
        validation("lines[1].line_id")
    );
    assert_eq!(
        field(refund(&f, owner(), &bad(vec![(tea_line.clone(), 1, true)])).await),
        validation("lines[0].return_to_stock"),
        "an untracked product keeps no stock"
    );
    let mut r = bad(vec![(cola_line.clone(), 1, false)]);
    r.reason = None;
    assert_eq!(
        field(refund(&f, owner(), &r).await).0,
        ErrorCode::ReasonRequired
    );
    r.reason = Some(ReasonCode::Other);
    assert_eq!(field(refund(&f, owner(), &r).await), validation("note"));
    r.note = Some("Customer left");
    refund(&f, owner(), &r).await.unwrap();
    assert_eq!(text(&refunds(&f)[0], "note"), "Customer left");
    let mut r = bad(vec![(tea_line.clone(), 1, false)]);
    r.method = None;
    assert_eq!(field(refund(&f, owner(), &r).await), validation("method"));
    r.method = Some((Credit, None));
    assert_eq!(field(refund(&f, owner(), &r).await), validation("method"));
    r.method = Some((Wallet, None));
    assert_eq!(
        field(refund(&f, owner(), &r).await),
        validation("method"),
        "a guest has no wallet"
    );
    r.method = Some((Upi, Some("12")));
    assert_eq!(
        field(refund(&f, owner(), &r).await).1.as_deref(),
        Some("reference")
    );
    r.method = Some((Card, None));
    assert_eq!(
        field(refund(&f, owner(), &r).await).1.as_deref(),
        Some("reference")
    );
    r.method = Some((Cash, Some("x")));
    assert_eq!(
        field(refund(&f, owner(), &r).await).1.as_deref(),
        Some("reference")
    );
    r.method = Some((Upi, Some("4321")));
    refund(&f, owner(), &r).await.unwrap();
    assert_eq!(
        cash_events(&f, "refund"),
        vec![-4_000],
        "the UPI refund moved no cash"
    );
    let upi = refunds(&f).pop().unwrap();
    assert_eq!(text(&upi, "method"), "upi");
    assert_eq!(text(&upi, "reference"), "4321");
}

#[tokio::test]
async fn a_cash_refund_needs_an_open_shift_and_a_counter_needs_a_manager_pin() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None, 10).await;
    let order = sell(&f, &[(&cola, 2)], vec![pay(Cash, 8_000, None)], None).await;
    let no = invoice(&f, &order).await;
    let line = text(&lines_of(&f, &order)[0], "id");
    let counter = staff(&f, "asha", ROLE_COUNTER).await;
    let r = req(&no, vec![(line.clone(), 1, false)]);
    assert_eq!(
        refusal(refund(&f, counter.clone(), &r).await).0,
        ErrorCode::PermissionDenied
    );
    let manager = manager_with_pin(&f).await;
    let mut with_pin = req(&no, vec![(line.clone(), 1, false)]);
    with_pin.approver = Some(&manager);
    refund(&f, counter.clone(), &with_pin).await.unwrap();
    let row = &refunds(&f)[0];
    assert_eq!(text(row, "approved_by"), manager);
    assert_eq!(text(row, "staff_id"), counter.id);

    sql(
        &f,
        "UPDATE shifts SET state = 'closed', closed_at = 1".to_owned(),
    )
    .await;
    assert_eq!(
        refusal(refund(&f, owner(), &req(&no, vec![(line.clone(), 1, false)])).await).0,
        ErrorCode::ShiftNotOpen
    );
    let mut upi = req(&no, vec![(line, 1, false)]);
    upi.method = Some((Upi, Some("1234")));
    refund(&f, owner(), &upi).await.unwrap();
}

#[tokio::test]
async fn a_wallet_refund_goes_back_to_the_player() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None, 10).await;
    let p = player(&f, "rohan", "9876500001").await;
    topped_up(&f, &p, 10_000).await;
    let order = sell(&f, &[(&cola, 2)], vec![pay(Wallet, 8_000, None)], Some(&p)).await;
    let no = invoice(&f, &order).await;
    let line = text(&lines_of(&f, &order)[0], "id");
    let wallet = |f: &Fixture| {
        int(
            table(f, Table::Wallets)
                .iter()
                .find(|w| text(w, "player_id") == p)
                .unwrap(),
            "balance_minor",
        )
    };
    assert_eq!(wallet(&f), 2_000);
    let mut r = req(&no, vec![(line, 1, false)]);
    r.method = Some((Wallet, None));
    refund(&f, owner(), &r).await.unwrap();
    assert_eq!(wallet(&f), 6_000);
    let txn = table(&f, Table::WalletTxns)
        .into_iter()
        .find(|t| matches!(&t["method"], RowValue::Text(m) if m == "refund"))
        .unwrap();
    assert_eq!(
        (int(&txn, "delta_minor"), text(&txn, "order_id")),
        (4_000, order)
    );
    assert_eq!(cash_events(&f, "refund"), Vec::<i64>::new());
}

#[tokio::test]
async fn the_credit_portion_cuts_outstanding_credit_before_any_money_goes_back() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None, 10).await;
    let p = credit_player(&f, "kiran", "9876500002", 100_000).await;
    let order = sell(
        &f,
        &[(&cola, 3)],
        vec![pay(Credit, 8_000, None), pay(Cash, 4_000, None)],
        Some(&p),
    )
    .await;
    let no = invoice(&f, &order).await;
    let line = text(&lines_of(&f, &order)[0], "id");
    let outstanding = |f: &Fixture| {
        int(
            table(f, Table::CreditBalances)
                .iter()
                .find(|c| text(c, "player_id") == p)
                .unwrap(),
            "outstanding_minor",
        )
    };
    assert_eq!(outstanding(&f), 8_000);

    // One unit is fully absorbed by the credit: no method is needed and no cash moves.
    let mut r = req(&no, vec![(line.clone(), 1, false)]);
    r.method = None;
    refund(&f, owner(), &r).await.unwrap();
    assert_eq!(outstanding(&f), 4_000);
    let first = &refunds(&f)[0];
    assert_eq!(
        (int(first, "amount_minor"), int(first, "credit_minor")),
        (4_000, 4_000)
    );
    assert_eq!(first["method"], RowValue::Null);
    assert!(cash_events(&f, "refund").is_empty());

    // Two more units: 4,000 more credit, then 4,000 back by cash.
    let r = req(&no, vec![(line, 2, false)]);
    refund(&f, owner(), &r).await.unwrap();
    assert_eq!(outstanding(&f), 0);
    let second = &refunds(&f)[1];
    assert_eq!(
        (int(second, "amount_minor"), int(second, "credit_minor")),
        (8_000, 4_000)
    );
    assert_eq!(cash_events(&f, "refund"), vec![-4_000]);
    let pays = payments_of(&f, &order);
    assert!(pays.contains(&("credit".to_owned(), -4_000)), "{pays:?}");
    assert!(pays.contains(&("cash".to_owned(), -4_000)), "{pays:?}");
}

#[tokio::test]
async fn tax_is_recomputed_once_per_rate_and_a_coupon_style_discount_is_refunded_pro_rata() {
    let f = bootstrapped().await;
    set_tax(
        &f,
        r#"{"country":"IN","gst":true,"gstin":"29ABCDE1234F1Z5","inclusive":true,"product_bp":500,"play_bp":1800,"sac":"9996","label":"Tax","rate_bp":0}"#,
    )
    .await;
    let snack = product(&f, "Snack", 10_500, Some(500), 10).await;
    let meal = product(&f, "Meal", 11_800, Some(1800), 10).await;
    let order = sell(
        &f,
        &[(&snack, 2), (&meal, 1)],
        vec![pay(Cash, 32_800, None)],
        None,
    )
    .await;
    let no = invoice(&f, &order).await;
    let ls = lines_of(&f, &order);
    let (snack_line, meal_line) = (text(&ls[0], "id"), text(&ls[1], "id"));
    refund(&f, owner(), &req(&no, vec![(snack_line, 1, false)]))
        .await
        .unwrap();
    let r = &refunds(&f)[0];
    assert_eq!((int(r, "amount_minor"), int(r, "tax_minor")), (10_500, 500));
    let breakup: Vec<crate::invoice::tax::RateTax> =
        serde_json::from_str(&text(r, "tax_breakup")).unwrap();
    assert_eq!(breakup.len(), 1);
    assert_eq!(
        (
            breakup[0].rate_bp,
            breakup[0].taxable_minor,
            breakup[0].tax_minor
        ),
        (500, -10_000, -500)
    );
    assert_eq!(
        (breakup[0].cgst_minor, breakup[0].sgst_minor),
        (Some(-250), Some(-250))
    );
    refund(&f, owner(), &req(&no, vec![(meal_line, 1, false)]))
        .await
        .unwrap();
    let r = &refunds(&f)[1];
    assert_eq!(
        (int(r, "amount_minor"), int(r, "tax_minor")),
        (11_800, 1_800)
    );
}

#[tokio::test]
async fn a_plan_is_refunded_whole_while_unused_and_then_deactivated() {
    let f = bootstrapped().await;
    let p = player(&f, "mira", "9876500003").await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(10_000)).await;
    let groups = vec![group_of(&f, &d)];
    let plan = plan_for(&f, &groups, 30_000).await;
    end(&f, sell_intent(&p, &plan, vec![pay(Cash, 30_000, None)]))
        .await
        .unwrap();
    let order = text(table(&f, Table::Orders).last().unwrap(), "id");
    let pack = text(table(&f, Table::PlayerPacks).last().unwrap(), "id");
    let no = invoice(&f, &order).await;
    let line = text(&lines_of(&f, &order)[0], "id");

    // Partly used: the line cannot be refunded.
    sql(
        &f,
        format!("UPDATE player_packs SET minutes_left = 200 WHERE id = '{pack}'"),
    )
    .await;
    let e = refund(&f, owner(), &req(&no, vec![(line.clone(), 1, false)]))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::ValidationFailed);
    assert!(e.message.contains("plan_used"), "{}", e.message);
    assert!(refunds(&f).is_empty());

    sql(
        &f,
        format!("UPDATE player_packs SET minutes_left = 300 WHERE id = '{pack}'"),
    )
    .await;
    refund(&f, owner(), &req(&no, vec![(line, 1, false)]))
        .await
        .unwrap();
    let row = table(&f, Table::PlayerPacks)
        .into_iter()
        .find(|r| text(r, "id") == pack)
        .unwrap();
    assert_eq!(int(&row, "minutes_left"), 0);
    assert!(int(&row, "expires_at") <= f.clock.now_ms());
    assert_eq!(int(&refunds(&f)[0], "amount_minor"), 30_000);
}

#[tokio::test]
async fn a_refunded_sale_can_no_longer_be_undone_and_a_refund_has_no_undo() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None, 10).await;
    let order = sell(&f, &[(&cola, 1)], vec![pay(Cash, 4_000, None)], None).await;
    let no = invoice(&f, &order).await;
    let line = text(&lines_of(&f, &order)[0], "id");
    refund(&f, owner(), &req(&no, vec![(line, 1, false)]))
        .await
        .unwrap();
    let mut undo = intent(
        owner(),
        IntentPayload::PosSaleReverted(arena_proto::PosSaleReverted {
            order_id: order.clone(),
        }),
    );
    undo.base_rev = Some(int(&order_row(&f, &order), "rev"));
    undo.issued_at = f.clock.now_ms();
    assert!(f.writer.submit_intent(undo).await.is_err());
    assert_eq!(refunds(&f).len(), 1);
}

#[tokio::test]
async fn a_print_refund_queues_the_credit_note_which_prints_negative_with_the_invoice_it_is_against()
 {
    let f = bootstrapped().await;
    set_tax(
        &f,
        r#"{"country":"IN","gst":true,"gstin":"29ABCDE1234F1Z5","inclusive":true,"product_bp":500,"play_bp":1800,"sac":"9996","label":"Tax","rate_bp":0}"#,
    )
    .await;
    let snack = product(&f, "Snack", 10_500, Some(500), 10).await;
    let order = sell(&f, &[(&snack, 2)], vec![pay(Cash, 21_000, None)], None).await;
    let no = invoice(&f, &order).await;
    let line = text(&lines_of(&f, &order)[0], "id");
    let mut i = refund_intent(
        owner(),
        &req(&no, vec![(line.clone(), 1, false)]),
        f.clock.now_ms(),
    );
    if let IntentPayload::RefundCreate(r) = &mut i.payload {
        r.receipt = Some(arena_proto::ReceiptChoice::Print);
    }
    f.writer.submit_intent(i).await.unwrap();

    let rows = reopen(&f).queued_prints(10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].ref_id, order);
    let refund_id = text(&refunds(&f)[0], "id");
    let payload: serde_json::Value = serde_json::from_str(&rows[0].payload).unwrap();
    assert_eq!(payload["credit_note"], refund_id.as_str());

    let source = reopen(&f).credit_note_source(&refund_id).unwrap().unwrap();
    assert_eq!(source.invoice_no, no);
    let settings: Vec<arena_proto::SettingEntry> = reopen(&f)
        .settings()
        .unwrap()
        .into_iter()
        .map(|s| arena_proto::SettingEntry {
            key: s.key,
            value: s.value,
            rev: s.rev,
        })
        .collect();
    let (sale, head) = crate::printer::job::credit_note_receipt(&settings, &source);
    assert_eq!(head.invoice_no, no);
    assert_eq!(head.credit_note_no, text(&refunds(&f)[0], "credit_note_no"));
    assert_eq!(
        (sale.total_minor, sale.items[0].amount_minor),
        (-10_500, -10_500)
    );
    assert_eq!(sale.tax[0].tax_minor, -500);
    assert_eq!(sale.items[0].hsn.as_deref(), Some("2106"));
    let bytes = crate::printer::job::render_credit_note_order(&settings, "Arena", None, &source);
    let printed = String::from_utf8_lossy(&bytes);
    for want in [
        "Credit Note",
        head.credit_note_no.as_str(),
        no.as_str(),
        "-105.00",
        "-2.50",
    ] {
        assert!(printed.contains(want), "{want} in {printed:?}");
    }
    // Choosing nothing queues nothing; WhatsApp is not a credit note choice.
    let none = req(&no, vec![(line.clone(), 1, false)]);
    refund(&f, owner(), &none).await.unwrap();
    assert_eq!(reopen(&f).queued_prints(10).unwrap().len(), 1);
    let mut i = refund_intent(owner(), &none, f.clock.now_ms());
    if let IntentPayload::RefundCreate(r) = &mut i.payload {
        r.receipt = Some(arena_proto::ReceiptChoice::Whatsapp);
    }
    assert_eq!(
        f.writer
            .submit_intent(i)
            .await
            .unwrap_err()
            .field
            .as_deref(),
        Some("receipt")
    );
}

#[tokio::test]
async fn a_manual_discount_is_refunded_pro_rata_and_every_line_back_is_exactly_what_was_paid() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None, 10).await;
    let chips = product(&f, "Chips", 6_000, None, 10).await;
    let order = submit(
        &f,
        owner(),
        IntentPayload::PosSale(PosSale {
            kiosk: None,
            business: None,
            coupon: None,
            discount: Some(arena_proto::ManualDiscount {
                percent_bp: Some(1_000),
                amount_minor: None,
                reason: arena_proto::DiscountReason::RegularCustomer,
                note: None,
            }),
            lines: vec![
                PosSaleLine {
                    product_id: cola.clone(),
                    qty: 1,
                    kitchen_notes: Vec::new(),
                },
                PosSaleLine {
                    product_id: chips.clone(),
                    qty: 1,
                    kitchen_notes: Vec::new(),
                },
            ],
            payments: vec![pay(Cash, 9_000, None)],
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
    .clone();
    let no = invoice(&f, &order).await;
    let ls = lines_of(&f, &order);
    refund(&f, owner(), &req(&no, vec![(text(&ls[0], "id"), 1, false)]))
        .await
        .unwrap();
    assert_eq!(
        int(&refunds(&f)[0], "amount_minor"),
        3_600,
        "the cola's share of 9,000"
    );
    refund(&f, owner(), &req(&no, vec![(text(&ls[1], "id"), 1, false)]))
        .await
        .unwrap();
    assert_eq!(int(&refunds(&f)[1], "amount_minor"), 5_400);
    let given: i64 = refunds(&f).iter().map(|r| int(r, "amount_minor")).sum();
    assert_eq!(given, 9_000, "all lines back is exactly what was paid");
}
