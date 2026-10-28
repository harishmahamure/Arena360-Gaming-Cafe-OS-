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
            business: None,
            coupon: None,
            discount: None,
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
            business: None,
            coupon: None,
            discount: None,
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

// ---- Story 5.19: WhatsApp receipt queued to the notification outbox ----

const PHONE: &str = "+91 98765-43210";

async fn set_key(f: &Fixture, key: &str, value: &str) {
    let row = reopen(f)
        .rows(Table::Settings)
        .unwrap()
        .into_iter()
        .find(|r| r["key"] == RowValue::Text(key.into()))
        .expect("the setting is seeded");
    let mut i = intent(
        owner(),
        IntentPayload::SettingsSet(arena_proto::SettingsSet {
            key: key.into(),
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

/// A paid branch with the WhatsApp receipt switched on.
async fn whatsapp_branch(f: &Fixture) {
    f.writer
        .apply_licence(arena_proto::LicenceApply {
            standing: arena_proto::LicenceStanding::Verified,
            clock_wrong: false,
            tier: arena_proto::Tier::Basic,
            flags: String::new(),
            device_cap: 60,
            user_cap: 10,
        })
        .await
        .unwrap();
    set_key(f, arena_proto::KEY_NOTIFY_WHATSAPP_RECEIPT, "true").await;
}

async fn profiled(f: &Fixture, username: &str, consent: bool) -> String {
    // One profile per phone: the last digits follow the username.
    let phone = format!("{PHONE}{}", username.len() + usize::from(consent));
    let mut form = super::players::form(username, &phone);
    form.consent = consent;
    super::players::add(f, owner(), &form).await.unwrap().0
}

async fn whatsapp_sale(
    f: &Fixture,
    by: &Actor,
    product: &str,
    player: Option<&str>,
) -> Result<(String, Applied), AppError> {
    let applied = submit(
        f,
        by.clone(),
        IntentPayload::PosSale(PosSale {
            business: None,
            coupon: None,
            discount: None,
            lines: vec![PosSaleLine {
                product_id: product.into(),
                qty: 1,
            }],
            payments: vec![pay(Cash, 4_000, None)],
            player_id: player.map(str::to_owned),
            credit_override: None,
            stock_override: None,
            receipt: Some(ReceiptChoice::Whatsapp),
        }),
    )
    .await?;
    let id = applied
        .changes
        .iter()
        .find(|c| c.tbl == "orders" && c.op == ChangeOp::Insert)
        .unwrap()
        .row_id
        .clone();
    Ok((id, applied))
}

type NotifyView = (String, String, String, String, String, i64, String);

/// `(order_id, player_id, kind, purpose, policy_version, not_before, state)` of every row.
fn notifies(f: &Fixture) -> Vec<NotifyView> {
    table(f, Table::NotifyOutbox)
        .iter()
        .map(|r| {
            (
                text(r, "order_id"),
                text(r, "player_id"),
                text(r, "kind"),
                text(r, "purpose"),
                text(r, "consent_policy_version"),
                super::session::int(r, "not_before"),
                text(r, "state"),
            )
        })
        .collect()
}

#[tokio::test]
async fn a_whatsapp_settle_queues_one_row_for_the_player_with_no_phone_in_the_log() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    whatsapp_branch(&f).await;
    let player = profiled(&f, "asha", true).await;
    let (o, applied) = whatsapp_sale(&f, &by, &id, Some(&player)).await.unwrap();

    let at = f.clock.now_ms();
    assert_eq!(
        notifies(&f),
        [(
            o.clone(),
            player.clone(),
            "receipt".into(),
            "transactional".into(),
            "1".into(),
            at + UNDO_WINDOW_MS,
            "queued".into()
        )]
    );
    assert!(
        applied
            .changes
            .iter()
            .any(|c| c.tbl == "notify_outbox" && c.op == ChangeOp::Insert)
    );
    for c in reopen(&f).changes().unwrap() {
        let blob = format!("{c:?}");
        assert!(
            !blob.contains("98765") && !blob.contains("9876543210"),
            "no phone number in the change log: {blob}"
        );
    }
    assert_eq!(number_of(&f, &o), None, "WhatsApp does not number the sale");
    assert!(
        outbox_of(&f, "print").is_empty(),
        "nothing goes to the printer"
    );
    assert!(effect_kinds(&f, &o).is_empty(), "the Undo stays open");
}

#[tokio::test]
async fn undoing_a_whatsapp_settle_skips_its_row_for_good() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    whatsapp_branch(&f).await;
    let player = profiled(&f, "asha", true).await;
    let (o, _) = whatsapp_sale(&f, &by, &id, Some(&player)).await.unwrap();

    let undone = f
        .writer
        .submit_intent(revert_intent(&f, &by, &o))
        .await
        .unwrap();
    assert!(
        undone
            .changes
            .iter()
            .any(|c| c.tbl == "notify_outbox" && c.op == ChangeOp::Update)
    );
    let rows = notifies(&f);
    assert_eq!(rows.len(), 1, "the skipped row stays as the record");
    assert_eq!(rows[0].6, "skipped");
}

#[tokio::test]
async fn a_session_bill_settles_to_whatsapp_and_its_undo_skips_the_row() {
    let f = bootstrapped().await;
    whatsapp_branch(&f).await;
    let player = profiled(&f, "asha", true).await;
    let (order, rev) = super::session::player_bill(&f, &player, 10_000, 30).await;
    let mut i = settle_intent(&order, Some(rev), vec![pay(Cash, 5_000, None)]);
    if let IntentPayload::SessionSettle(s) = &mut i.payload {
        s.receipt = Some(ReceiptChoice::Whatsapp);
    }
    super::session::end(&f, i).await.unwrap();
    let rows = notifies(&f);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].0.as_str(), rows[0].6.as_str()),
        (order.as_str(), "queued")
    );

    let rev = rev_of(&f, Table::Orders, &order);
    super::session::end(&f, super::session::settle_revert_intent(&order, Some(rev)))
        .await
        .unwrap();
    assert_eq!(notifies(&f)[0].6, "skipped");
}

#[tokio::test]
async fn whatsapp_is_refused_for_a_guest_a_player_without_consent_and_a_free_branch() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    let consenting = profiled(&f, "asha", true).await;
    let silent = profiled(&f, "bala", false).await;

    // Free branch: the tier is told first, even for a consenting player.
    let e = whatsapp_sale(&f, &by, &id, Some(&consenting))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::FeatureNotEntitled);

    whatsapp_branch(&f).await;
    let sales = table(&f, Table::Orders).len();
    let moves = table(&f, Table::StockMoves).len();
    let e = whatsapp_sale(&f, &by, &id, None).await.unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::ConsentRequired, Some("receipt"))
    );
    let e = whatsapp_sale(&f, &by, &id, Some(&silent))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::ConsentRequired);

    set_key(&f, arena_proto::KEY_NOTIFY_WHATSAPP_RECEIPT, "false").await;
    let e = whatsapp_sale(&f, &by, &id, Some(&consenting))
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::ValidationFailed, Some("receipt"))
    );

    assert!(notifies(&f).is_empty());
    assert_eq!(
        table(&f, Table::Orders).len(),
        sales,
        "a refused settle writes no sale"
    );
    assert_eq!(
        table(&f, Table::StockMoves).len(),
        moves,
        "and takes no stock"
    );
}

#[tokio::test]
async fn a_withdrawn_consent_closes_whatsapp_for_the_next_settle() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    whatsapp_branch(&f).await;
    let player = profiled(&f, "asha", true).await;
    whatsapp_sale(&f, &by, &id, Some(&player)).await.unwrap();

    // The latest consent row decides: withdraw it.
    let at = f.clock.now_ms() + 1;
    reopen(&f)
        .write_tx(|tx| {
            arena_store::insert_consent(
                tx,
                "withdrawn",
                &player,
                arena_proto::CONSENT_TRANSACTIONAL,
                "2",
                false,
                at,
            )
        })
        .unwrap();
    let e = whatsapp_sale(&f, &by, &id, Some(&player))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::ConsentRequired);
}

#[tokio::test]
async fn a_plan_sale_settles_to_whatsapp_and_its_undo_skips_the_row() {
    let f = bootstrapped().await;
    whatsapp_branch(&f).await;
    let player = profiled(&f, "asha", true).await;
    let plan = super::session::plan_of(&f, "pack", 30_000, true).await;
    let mut i = super::session::sell_intent(&player, &plan, vec![pay(Cash, 30_000, None)]);
    if let IntentPayload::PlanSell(s) = &mut i.payload {
        s.receipt = Some(ReceiptChoice::Whatsapp);
    }
    super::session::end(&f, i).await.unwrap();
    let order = text(&super::session::sale_order(&f), "id");
    let rows = notifies(&f);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].0.as_str(), rows[0].1.as_str(), rows[0].6.as_str()),
        (order.as_str(), player.as_str(), "queued")
    );

    let rev = rev_of(&f, Table::Orders, &order);
    super::session::end(&f, super::session::sell_revert_intent(&order, Some(rev)))
        .await
        .unwrap();
    assert_eq!(notifies(&f)[0].6, "skipped");
}

// Story 5.20: business (B2B) invoices.

const GST_ON: &str = r#"{"country":"IN","gst":true,"gstin":"27AAPFU0939F1ZV","inclusive":true,"product_bp":500,"play_bp":1800,"sac":"","label":"Tax","rate_bp":0}"#;
const BUYER_GSTIN: &str = "29AABCT1332L1ZA";

async fn gst_branch(f: &Fixture) {
    super::prepaid::sql(
        f,
        format!("UPDATE settings SET value = '{GST_ON}' WHERE key = 'branch.tax'"),
    )
    .await;
}

fn buyer(gstin: &str, name: &str) -> arena_proto::BusinessBuyer {
    arena_proto::BusinessBuyer {
        gstin: gstin.into(),
        legal_name: name.into(),
    }
}

async fn business_sale(
    f: &Fixture,
    by: &Actor,
    product: &str,
    business: Option<arena_proto::BusinessBuyer>,
    receipt: Option<ReceiptChoice>,
) -> Result<String, AppError> {
    let applied = submit(
        f,
        by.clone(),
        IntentPayload::PosSale(PosSale {
            business,
            coupon: None,
            discount: None,
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
    .await?;
    Ok(applied
        .changes
        .iter()
        .find(|c| c.tbl == "orders" && c.op == ChangeOp::Insert)
        .unwrap()
        .row_id
        .clone())
}

/// `(buyer_gstin, buyer_name)` of an order, `None` for a consumer sale.
fn buyer_of(f: &Fixture, id: &str) -> Option<(String, String)> {
    let o = table(f, Table::Orders)
        .into_iter()
        .find(|o| text(o, "id") == id)
        .unwrap();
    match (&o["buyer_gstin"], &o["buyer_name"]) {
        (RowValue::Text(g), RowValue::Text(n)) => Some((g.clone(), n.clone())),
        (RowValue::Null, RowValue::Null) => None,
        other => panic!("buyer columns set together or not at all: {other:?}"),
    }
}

#[tokio::test]
async fn a_gst_settle_stores_the_business_on_the_order_with_every_receipt_choice() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    gst_branch(&f).await;
    for choice in [None, Some(ReceiptChoice::None), Some(ReceiptChoice::Print)] {
        let o = business_sale(
            &f,
            &by,
            &id,
            Some(buyer(BUYER_GSTIN, "Tata Consultancy Services")),
            choice,
        )
        .await
        .unwrap();
        assert_eq!(
            buyer_of(&f, &o),
            Some((BUYER_GSTIN.into(), "Tata Consultancy Services".into())),
            "{choice:?}"
        );
    }
    let b2c = business_sale(&f, &by, &id, None, None).await.unwrap();
    assert_eq!(buyer_of(&f, &b2c), None, "a sale without a buyer is B2C");
}

#[tokio::test]
async fn a_printed_business_invoice_carries_the_buyer_on_the_receipt_source() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    gst_branch(&f).await;
    let o = business_sale(
        &f,
        &by,
        &id,
        Some(buyer(BUYER_GSTIN, "Acme & Sons")),
        Some(ReceiptChoice::Print),
    )
    .await
    .unwrap();
    assert!(number_of(&f, &o).is_some(), "a printed sale is numbered");
    let source = reopen(&f)
        .write_tx(|tx| arena_store::load_receipt_source(tx, &o))
        .unwrap()
        .unwrap();
    assert_eq!(
        source.head.buyer,
        Some((BUYER_GSTIN.into(), "Acme & Sons".into()))
    );
}

#[tokio::test]
async fn an_invalid_business_refuses_the_settle_and_writes_nothing() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    gst_branch(&f).await;
    let before = (
        table(&f, Table::Orders).len(),
        reopen(&f).changes().unwrap().len(),
    );
    for (bad, field, message) in [
        (
            buyer("29AABCT1332L1ZX", "Acme"),
            "business.gstin",
            "GSTIN_INVALID",
        ),
        (buyer("", "Acme"), "business.gstin", "GSTIN_INVALID"),
        (buyer(BUYER_GSTIN, ""), "business.legal_name", "legal name"),
        (
            buyer(BUYER_GSTIN, " Acme"),
            "business.legal_name",
            "legal name",
        ),
    ] {
        let e = business_sale(&f, &by, &id, Some(bad), None)
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::ValidationFailed);
        assert_eq!(e.field.as_deref(), Some(field));
        assert!(e.message.contains(message), "{}", e.message);
    }
    assert_eq!(
        (
            table(&f, Table::Orders).len(),
            reopen(&f).changes().unwrap().len()
        ),
        before,
        "a refused settle rolls back whole"
    );
}

#[tokio::test]
async fn a_branch_that_is_not_on_gst_takes_no_business_invoice() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    let e = business_sale(&f, &by, &id, Some(buyer(BUYER_GSTIN, "Acme")), None)
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::ValidationFailed, Some("business"))
    );
}

#[tokio::test]
async fn a_session_bill_and_a_plan_sale_take_a_business_too() {
    let f = bootstrapped().await;
    gst_branch(&f).await;
    let player = profiled(&f, "asha", true).await;

    let (order, rev) = super::session::player_bill(&f, &player, 10_000, 30).await;
    let mut i = settle_intent(&order, Some(rev), vec![pay(Cash, 5_000, None)]);
    if let IntentPayload::SessionSettle(s) = &mut i.payload {
        s.business = Some(buyer(BUYER_GSTIN, "Acme"));
    }
    super::session::end(&f, i).await.unwrap();
    assert_eq!(
        buyer_of(&f, &order),
        Some((BUYER_GSTIN.into(), "Acme".into()))
    );

    let plan = super::session::plan_of(&f, "pack", 30_000, true).await;
    let mut i = super::session::sell_intent(&player, &plan, vec![pay(Cash, 30_000, None)]);
    if let IntentPayload::PlanSell(s) = &mut i.payload {
        s.business = Some(buyer("29AABCT1332L1ZX", "Acme"));
    }
    let e = super::session::end(&f, i).await.unwrap_err();
    assert_eq!(e.field.as_deref(), Some("business.gstin"));
}
