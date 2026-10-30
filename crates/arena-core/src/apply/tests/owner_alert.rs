//! Owner alerts raised by the Hub (Story 8.34): the five sources queue one `owner_alert` row in the
//! transaction of the event, a Free branch queues none, the Undo of a discounted sale skips its
//! row, and the drain turns a due row into the request the cloud raises the alert from.

use std::collections::BTreeMap;

use super::prepaid::sql;
use super::receipt::{set_key, whatsapp_branch};
use super::session::{
    credit_player, end, int, pay, player_bill, settle_on_credit, submit, table, text,
};
use super::stock::{intent, make, owner, set_threshold, stock_in, up};
use super::*;
use crate::notify_drain::{DrainAction, plan};
use arena_proto::PaymentMethod::Cash;
use arena_proto::{
    DiscountReason, InvoiceIssue, KEY_OWNER_ALERT_THRESHOLDS, ManualDiscount, PosSale, PosSaleLine,
    PosSaleReverted, ProductUpsert, RefundCreate, RefundLineReq, ShiftClose, ShiftOpen,
    UNDO_WINDOW_MS, VarianceReason,
};

const KEK: [u8; crate::players::KEK_LEN] = super::players::KEK;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

fn alerts(f: &Fixture) -> Vec<Row> {
    table(f, Table::NotifyOutbox)
        .into_iter()
        .filter(|r| text(r, "purpose") == "owner_alert")
        .collect()
}

fn params_of(row: &Row) -> BTreeMap<String, String> {
    serde_json::from_str(&text(row, "params")).unwrap()
}

fn null(row: &Row, col: &str) -> bool {
    row[col] == RowValue::Null
}

async fn thresholds(f: &Fixture, json: &str) {
    set_key(f, KEY_OWNER_ALERT_THRESHOLDS, json).await;
}

fn drain(f: &Fixture, now: i64) -> Vec<crate::notify_drain::DrainItem> {
    plan(&reopen(f), &KEK, now, 20).unwrap()
}

async fn shift(f: &Fixture, counted: i64, reason: Option<VarianceReason>) -> String {
    submit(
        f,
        owner(),
        IntentPayload::ShiftOpen(ShiftOpen {
            float_minor: 100_000,
        }),
    )
    .await
    .unwrap();
    let applied = submit(
        f,
        owner(),
        IntentPayload::ShiftClose(ShiftClose {
            counted_minor: counted,
            deposit: None,
            reason,
            note: None,
        }),
    )
    .await
    .unwrap();
    applied
        .changes
        .iter()
        .find(|c| c.tbl == "shifts")
        .unwrap()
        .row_id
        .clone()
}

#[tokio::test]
async fn a_close_over_the_branch_limit_queues_one_cash_variance_row_after_the_undo_window() {
    let f = bootstrapped_closed().await;
    whatsapp_branch(&f).await;
    let at = f.clock.now_ms();
    // 200.00 short, over the 100.00 default.
    let id = shift(&f, 80_000, Some(VarianceReason::CountingError)).await;
    let rows = alerts(&f);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(text(row, "kind"), "cash_variance");
    assert_eq!(text(row, "source_ref"), id);
    assert_eq!(text(row, "state"), "queued");
    assert_eq!(text(row, "consent_policy_version"), "owner");
    assert_eq!(int(row, "not_before"), at + UNDO_WINDOW_MS);
    assert!(null(row, "player_id") && null(row, "order_id"));
    assert_eq!(
        params_of(row),
        BTreeMap::from([
            ("amount_minor".to_owned(), "20000".to_owned()),
            ("reason".to_owned(), "counting error".to_owned()),
            ("shift".to_owned(), "o".to_owned()),
        ])
    );
}

#[tokio::test]
async fn a_close_inside_the_limit_queues_nothing_and_the_owners_own_limit_applies() {
    let f = bootstrapped_closed().await;
    whatsapp_branch(&f).await;
    // Exactly at the 100.00 default: not over.
    shift(&f, 90_000, None).await;
    assert!(alerts(&f).is_empty());

    // The owner lowers the limit to 20.00 (the branch's own limit stays 100.00): 50.00 short
    // needs no manager reason but is still an alert, without a reason parameter.
    thresholds(
        &f,
        r#"{"cash_variance_minor":2000,"large_refund_minor":50000,"large_discount_minor":20000,"large_discount_percent":20}"#,
    )
    .await;
    shift(&f, 95_000, None).await;
    let rows = alerts(&f);
    assert_eq!(rows.len(), 1);
    let p = params_of(&rows[0]);
    assert_eq!(p["amount_minor"], "5000");
    assert!(!p.contains_key("reason"));
}

#[tokio::test]
async fn a_free_branch_queues_no_alert_from_any_source() {
    let f = bootstrapped_closed().await;
    shift(&f, 50_000, Some(VarianceReason::CountingError)).await;
    assert!(alerts(&f).is_empty());
    assert!(table(&f, Table::NotifyOutbox).is_empty());
}

async fn cola(f: &Fixture, qty: i64) -> String {
    let id = make(
        f,
        ProductUpsert {
            price_minor: 10_000,
            ..up("Cola")
        },
    )
    .await;
    stock_in(f, &id, qty, 5_000).await;
    id
}

#[tokio::test]
async fn a_low_stock_crossing_queues_one_row_and_a_recovered_stock_is_skipped() {
    let f = bootstrapped().await;
    whatsapp_branch(&f).await;
    let id = cola(&f, 4).await;
    set_threshold(&f, owner(), &id, Some(5)).await.unwrap();
    let rows = alerts(&f);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(text(row, "kind"), "low_stock");
    assert_eq!(
        params_of(row),
        BTreeMap::from([
            ("product".to_owned(), "Cola".to_owned()),
            ("qty".to_owned(), "4".to_owned()),
        ])
    );
    let hub_alert = reopen(&f).alerts().unwrap().remove(0);
    assert_eq!(text(row, "source_ref"), hub_alert.id);

    // Inside the Undo window nothing is due. Afterwards the request carries the parameters, with
    // no recipient and no player.
    assert!(drain(&f, f.clock.now_ms()).is_empty());
    let due = f.clock.now_ms() + UNDO_WINDOW_MS;
    let items = drain(&f, due);
    let DrainAction::Send(req) = &items[0].action else {
        panic!("a send, got {:?}", items[0].action);
    };
    assert_eq!(
        (req.kind.as_str(), req.purpose.as_str()),
        ("low_stock", "owner_alert")
    );
    assert_eq!(req.consent_policy_version, "owner");
    assert_eq!(req.recipient, "");
    assert_eq!(req.outbox_id, text(row, "id"));
    assert_eq!(req.params["product"], "Cola");
    assert!(!req.params.contains_key("cafe") && !req.params.contains_key("name"));

    // The stock is refilled before the drain: the Hub alert is acknowledged and the row skipped.
    sql(
        &f,
        format!(
            "UPDATE alerts SET acked_at = 1, acked_by = 'o' WHERE id = '{}'",
            hub_alert.id
        ),
    )
    .await;
    assert_eq!(
        drain(&f, due)[0].action,
        DrainAction::Skip("stock_recovered".into())
    );
}

#[tokio::test]
async fn a_player_whose_credit_reaches_its_overdue_days_raises_one_credit_overdue_row() {
    let f = bootstrapped().await;
    whatsapp_branch(&f).await;
    let p = credit_player(&f, "asha", "+919876543210", 100_000).await;
    let (order, rev) = player_bill(&f, &p, 6_000, 60).await;
    end(&f, settle_on_credit(&order, rev, 6_000, false))
        .await
        .unwrap();
    f.clock.advance(14 * DAY_MS);
    assert!(f.writer.credit_auto_pause(None).await.unwrap().is_none());
    assert!(alerts(&f).is_empty());

    f.clock.advance(2 * DAY_MS);
    let applied = f.writer.credit_auto_pause(None).await.unwrap().unwrap();
    assert!(applied.changes.iter().any(|c| c.tbl == "notify_outbox"));
    let rows = alerts(&f);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(text(row, "kind"), "credit_overdue");
    assert_eq!(text(row, "player_id"), p);
    assert!(text(row, "source_ref").starts_with(&format!("{p}:")));
    let params = params_of(row);
    assert_eq!(params["amount_minor"], "6000");
    assert_eq!(params["days"], "16");
    assert_eq!(params["player_id"], p);

    // The pause is already on: the next check changes nothing and queues nothing.
    assert!(f.writer.credit_auto_pause(None).await.unwrap().is_none());
    assert_eq!(alerts(&f).len(), 1);

    // The drain still names no player's phone or name: the request has the id only.
    let items = drain(&f, f.clock.now_ms() + UNDO_WINDOW_MS);
    let DrainAction::Send(req) = &items[0].action else {
        panic!("a send, got {:?}", items[0].action);
    };
    assert_eq!(req.recipient, "");
    assert!(!req.params.contains_key("name"));
}

async fn sold(
    f: &Fixture,
    id: &str,
    qty: i64,
    paid: i64,
    discount: Option<ManualDiscount>,
) -> String {
    submit(
        f,
        owner(),
        IntentPayload::PosSale(PosSale {
            kiosk: None,
            business: None,
            coupon: None,
            discount,
            lines: vec![PosSaleLine {
                product_id: id.into(),
                qty,
                kitchen_notes: Vec::new(),
            }],
            payments: vec![pay(Cash, paid, None)],
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

fn off(minor: i64) -> ManualDiscount {
    ManualDiscount {
        percent_bp: None,
        amount_minor: Some(minor),
        reason: DiscountReason::ServiceIssue,
        note: None,
    }
}

/// Numbers the sale after its Undo window and returns its invoice number and first line.
async fn invoiced(f: &Fixture, order: &str) -> (String, String) {
    f.clock.advance(UNDO_WINDOW_MS + 1);
    let mut issue = intent(
        super::prepaid::system(),
        IntentPayload::InvoiceIssue(InvoiceIssue {
            order_id: order.into(),
        }),
    );
    issue.issued_at = f.clock.now_ms();
    f.writer.submit_intent(issue).await.unwrap();
    let invoice_no = text(
        &table(f, Table::Orders)
            .into_iter()
            .find(|o| text(o, "id") == order)
            .unwrap(),
        "invoice_no",
    );
    let line = text(
        &table(f, Table::OrderLines)
            .into_iter()
            .find(|l| text(l, "order_id") == order)
            .unwrap(),
        "id",
    );
    (invoice_no, line)
}

async fn refund(f: &Fixture, invoice_no: &str, line: &str, qty: i64) -> Applied {
    let mut i = intent(
        owner(),
        IntentPayload::RefundCreate(RefundCreate {
            invoice_no: invoice_no.into(),
            lines: vec![RefundLineReq {
                line_id: line.into(),
                qty,
                return_to_stock: None,
            }],
            method: Some(Cash),
            reference: None,
            note: None,
            receipt: None,
        }),
    );
    i.reason = Some(ReasonCode::Mistake);
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await.unwrap()
}

#[tokio::test]
async fn a_large_refund_queues_one_row_and_a_small_one_none() {
    let f = bootstrapped().await;
    whatsapp_branch(&f).await;
    thresholds(
        &f,
        r#"{"large_refund_minor":15000,"large_discount_minor":20000,"large_discount_percent":20}"#,
    )
    .await;
    let id = cola(&f, 10).await;
    let order = sold(&f, &id, 3, 30_000, None).await;
    let (invoice_no, line) = invoiced(&f, &order).await;

    // 100.00 back is under the 150.00 limit; 200.00 is over it.
    let small = refund(&f, &invoice_no, &line, 1).await;
    assert!(alerts(&f).is_empty());
    assert!(!small.changes.iter().any(|c| c.tbl == "notify_outbox"));
    let large = refund(&f, &invoice_no, &line, 2).await;
    assert!(large.changes.iter().any(|c| c.tbl == "notify_outbox"));
    let rows = alerts(&f);
    assert_eq!(rows.len(), 1);
    let credit_note = table(&f, Table::Refunds)
        .into_iter()
        .max_by_key(|r| int(r, "amount_minor"))
        .unwrap();
    assert_eq!(text(&rows[0], "kind"), "large_refund");
    assert_eq!(text(&rows[0], "source_ref"), text(&credit_note, "id"));
    assert_eq!(params_of(&rows[0])["amount_minor"], "20000");
    assert!(null(&rows[0], "player_id") && null(&rows[0], "order_id"));
}

#[tokio::test]
async fn a_discount_over_the_amount_or_the_percent_limit_queues_a_row_tied_to_its_order() {
    let f = bootstrapped().await;
    whatsapp_branch(&f).await;
    let id = cola(&f, 20).await;

    // Over the percent limit only: 50.00 off a 200.00 bill is 25%, the amount limit is 1000.00.
    thresholds(
        &f,
        r#"{"large_refund_minor":50000,"large_discount_minor":100000,"large_discount_percent":20}"#,
    )
    .await;
    let order = sold(&f, &id, 2, 15_000, Some(off(5_000))).await;
    let rows = alerts(&f);
    assert_eq!(rows.len(), 1);
    assert_eq!(text(&rows[0], "kind"), "large_discount");
    assert_eq!(text(&rows[0], "source_ref"), order);
    assert_eq!(text(&rows[0], "order_id"), order);
    assert!(null(&rows[0], "player_id"));
    assert_eq!(params_of(&rows[0])["amount_minor"], "5000");

    // Exactly 20% is not over it.
    sold(&f, &id, 2, 16_000, Some(off(4_000))).await;
    assert_eq!(alerts(&f).len(), 1);

    // Over the amount limit only: 40.00 off is 20% of the bill but over a 30.00 limit.
    thresholds(
        &f,
        r#"{"large_refund_minor":50000,"large_discount_minor":3000,"large_discount_percent":50}"#,
    )
    .await;
    sold(&f, &id, 2, 16_000, Some(off(4_000))).await;
    assert_eq!(alerts(&f).len(), 2);
}

#[tokio::test]
async fn undoing_a_sale_with_a_large_discount_skips_its_alert() {
    let f = bootstrapped().await;
    whatsapp_branch(&f).await;
    thresholds(
        &f,
        r#"{"large_refund_minor":50000,"large_discount_minor":1000,"large_discount_percent":20}"#,
    )
    .await;
    let id = cola(&f, 20).await;
    let order = sold(&f, &id, 2, 15_000, Some(off(5_000))).await;
    assert_eq!(text(&alerts(&f)[0], "state"), "queued");

    f.clock.advance(3_000);
    let rev = int(
        &table(&f, Table::Orders)
            .into_iter()
            .find(|o| text(o, "id") == order)
            .unwrap(),
        "rev",
    );
    let mut undo = intent(
        owner(),
        IntentPayload::PosSaleReverted(PosSaleReverted {
            order_id: order.clone(),
        }),
    );
    undo.base_rev = Some(rev);
    undo.issued_at = f.clock.now_ms();
    f.writer.submit_intent(undo).await.unwrap();
    assert_eq!(text(&alerts(&f)[0], "state"), "skipped");
    assert!(drain(&f, f.clock.now_ms() + UNDO_WINDOW_MS).is_empty());
}

#[tokio::test]
async fn a_due_money_alert_names_its_amount_in_the_branch_format_and_no_cafe() {
    let f = bootstrapped_closed().await;
    whatsapp_branch(&f).await;
    shift(&f, 80_000, Some(VarianceReason::CountingError)).await;
    let items = crate::SnapshotReader::open(&f.path)
        .unwrap()
        .notify_plan(&KEK, f.clock.now_ms() + UNDO_WINDOW_MS)
        .unwrap();
    assert_eq!(items.len(), 1);
    let DrainAction::Send(req) = &items[0].action else {
        panic!("a send, got {:?}", items[0].action);
    };
    assert_eq!(req.kind, "cash_variance");
    assert!(!req.params.contains_key("amount_minor"));
    assert!(req.params["amount"].contains("200.00"), "{:?}", req.params);
    assert_eq!(req.params["shift"], "o");
    assert_eq!(req.params["reason"], "counting error");
    assert!(!req.params.contains_key("cafe"));
}

fn set_thresholds(value: &str, rev: i64) -> Intent {
    let mut i = intent(
        owner(),
        IntentPayload::SettingsSet(arena_proto::SettingsSet {
            key: KEY_OWNER_ALERT_THRESHOLDS.into(),
            value: value.into(),
        }),
    );
    i.base_rev = Some(rev);
    i
}

#[tokio::test]
async fn the_thresholds_are_seeded_and_set_by_an_owner_command_within_range() {
    let f = bootstrapped().await;
    let stored = |f: &Fixture| {
        reopen(f)
            .rows(Table::Settings)
            .unwrap()
            .into_iter()
            .find(|r| text(r, "key") == KEY_OWNER_ALERT_THRESHOLDS)
            .unwrap()
    };
    let row = stored(&f);
    assert_eq!(
        text(&row, "value"),
        arena_proto::DEFAULT_OWNER_ALERT_THRESHOLDS
    );

    // An out-of-range value is refused and changes nothing.
    for bad in [
        r#"{"large_refund_minor":-1,"large_discount_minor":1,"large_discount_percent":20}"#,
        r#"{"large_refund_minor":1,"large_discount_minor":1,"large_discount_percent":0}"#,
        r#"{"large_refund_minor":1,"large_discount_minor":1,"large_discount_percent":101}"#,
        "not json",
    ] {
        let err = f
            .writer
            .submit_intent(set_thresholds(bad, int(&row, "rev")))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ValidationFailed, "{bad}");
    }
    assert_eq!(int(&stored(&f), "rev"), int(&row, "rev"));

    // The command the cloud forwards for every branch becomes the owner's settings.set.
    let json = r#"{"cash_variance_minor":2500,"large_refund_minor":1,"large_discount_minor":2,"large_discount_percent":3}"#;
    let done = f
        .writer
        .apply_command(arena_proto::CommandWire {
            id: 1,
            idempotency_key: ulid::Ulid::generate().to_string(),
            kind: "settings.set".into(),
            schema_version: 1,
            payload: serde_json::json!({"key": KEY_OWNER_ALERT_THRESHOLDS, "value": json}),
            issued_by: "owner-1".into(),
            elevated_by: None,
        })
        .await
        .unwrap();
    assert!(done.applied && done.error.is_none(), "{done:?}");
    assert_eq!(text(&stored(&f), "value"), json);
}
