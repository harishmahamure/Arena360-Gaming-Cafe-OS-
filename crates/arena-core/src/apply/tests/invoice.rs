//! Story 5.14: `invoice.issue` numbers a settled sale in the epoch-segmented series.

use super::prepaid::{sql, system};
use super::session::{int, pay, player, refusal, rev_of, submit, table, text, topped_up};
use super::stock::{intent, make, owner, staff, stock_in, up};
use super::*;
use crate::invoice::number;
use crate::rbac::ROLE_COUNTER;
use arena_proto::PaymentMethod::Cash;
use arena_proto::{
    InvoiceIssue, PosSale, PosSaleLine, PosSaleReverted, ProductUpsert, UNDO_WINDOW_MS,
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

async fn sale(f: &Fixture, by: &Actor, product: &str) -> String {
    submit(
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

fn issue_intent(f: &Fixture, actor: Actor, order: &str) -> Intent {
    let mut i = intent(
        actor,
        IntentPayload::InvoiceIssue(InvoiceIssue {
            order_id: order.into(),
        }),
    );
    i.issued_at = f.clock.now_ms();
    i
}

async fn issue(f: &Fixture, order: &str) -> Result<Applied, AppError> {
    f.writer
        .submit_intent(issue_intent(f, system(), order))
        .await
}

/// Lets the Undo window of everything settled so far close.
fn close_window(f: &Fixture) {
    f.clock.advance(UNDO_WINDOW_MS + 1);
}

fn number_of(f: &Fixture, id: &str) -> Option<String> {
    let o = table(f, Table::Orders)
        .into_iter()
        .find(|o| text(o, "id") == id)
        .unwrap();
    match &o["invoice_no"] {
        RowValue::Text(t) => Some(t.clone()),
        RowValue::Null => None,
        other => panic!("invoice_no: {other:?}"),
    }
}

fn seq_of(f: &Fixture, id: &str) -> String {
    number_of(f, id)
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_owned()
}

fn sequences(f: &Fixture) -> Vec<(String, String, i64)> {
    let mut rows: Vec<_> = table(f, Table::InvoiceSequences)
        .iter()
        .map(|r| (text(r, "fy"), text(r, "segment"), int(r, "next")))
        .collect();
    rows.sort();
    rows
}

fn effect_recorded(f: &Fixture, order_id: &str) -> bool {
    let id = order_id.to_owned();
    reopen(f)
        .write_tx(|tx| arena_store::effect_recorded(tx, None, &id))
        .unwrap()
}

#[tokio::test]
async fn a_settled_sale_gets_the_next_number_once_its_undo_window_has_closed() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let o = sale(&f, &counter(&f).await, &id).await;
    assert_eq!(number_of(&f, &o), None, "a settle issues no number itself");
    close_window(&f);

    let applied = issue(&f, &o).await.unwrap();
    let tables: Vec<_> = applied.changes.iter().map(|c| c.tbl.as_str()).collect();
    assert_eq!(tables, ["invoice_sequences", "orders", "settings"]);
    let segment = number::segment(1, HUB);
    assert_eq!(
        number_of(&f, &o),
        Some(format!("DEV01/26-27/{segment}/000001"))
    );
    assert_eq!(
        sequences(&f),
        [("FY2026-27".to_owned(), segment, 2)],
        "the sequence advanced by exactly one"
    );
    assert!(
        effect_recorded(&f, &o),
        "the number is an irreversible effect"
    );
    let lock = setting(&reopen(&f), "branch.code_locked_at");
    assert_ne!(
        text(&lock, "value"),
        "",
        "the first invoice locks the branch code"
    );
}

#[tokio::test]
async fn numbers_are_consecutive_in_the_order_they_are_issued() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    let (a, b, c) = (
        sale(&f, &by, &id).await,
        sale(&f, &by, &id).await,
        sale(&f, &by, &id).await,
    );
    close_window(&f);
    for o in [&a, &b, &c] {
        issue(&f, o).await.unwrap();
    }
    assert_eq!(
        [seq_of(&f, &a), seq_of(&f, &b), seq_of(&f, &c)],
        ["000001", "000002", "000003"]
    );
    assert_eq!(sequences(&f)[0].2, 4);
}

#[tokio::test]
async fn an_order_inside_its_undo_window_or_already_numbered_is_stale() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let o = sale(&f, &counter(&f).await, &id).await;
    f.clock.advance(UNDO_WINDOW_MS);
    assert_eq!(
        refusal(issue(&f, &o).await),
        (ErrorCode::Stale, Some("order_id".into())),
        "the window is still open at its last millisecond"
    );
    assert_eq!(number_of(&f, &o), None);
    assert!(sequences(&f).is_empty(), "a refused issue opens no segment");

    f.clock.advance(1);
    issue(&f, &o).await.unwrap();
    assert_eq!(
        refusal(issue(&f, &o).await),
        (ErrorCode::Stale, Some("order_id".into()))
    );
    assert_eq!(sequences(&f)[0].2, 2, "a number is never issued twice");
}

#[tokio::test]
async fn a_sale_undone_inside_the_window_never_takes_a_number() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    let kept = sale(&f, &by, &id).await;
    let undone = sale(&f, &by, &id).await;
    let mut revert = intent(
        by.clone(),
        IntentPayload::PosSaleReverted(PosSaleReverted {
            order_id: undone.clone(),
        }),
    );
    revert.base_rev = Some(rev_of(&f, Table::Orders, &undone));
    revert.issued_at = f.clock.now_ms();
    f.writer.submit_intent(revert).await.unwrap();

    close_window(&f);
    assert_eq!(
        refusal(issue(&f, &undone).await).0,
        ErrorCode::NotFound,
        "an undone sale has nothing to number"
    );
    issue(&f, &kept).await.unwrap();
    assert_eq!(seq_of(&f, &kept), "000001");
    assert_eq!(sequences(&f)[0].2, 2, "the series has no gap");
}

#[tokio::test]
async fn orders_that_are_not_sales_take_no_number() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let p = player(&f, "ann", "9876500001").await;
    let topup = topped_up(&f, &p, 10_000).await;
    close_window(&f);
    assert_eq!(
        refusal(issue(&f, &topup).await),
        (ErrorCode::ValidationFailed, Some("order_id".into())),
        "a wallet top-up is a payment, not a sale"
    );

    let free = sale(&f, &counter(&f).await, &id).await;
    sql(
        &f,
        format!("UPDATE orders SET total_minor = 0 WHERE id = '{free}'"),
    )
    .await;
    close_window(&f);
    assert_eq!(
        refusal(issue(&f, &free).await),
        (ErrorCode::ValidationFailed, Some("order_id".into())),
        "a zero total takes no number"
    );

    sql(
        &f,
        format!("UPDATE orders SET total_minor = 5, status = 'open' WHERE id = '{free}'"),
    )
    .await;
    assert_eq!(
        refusal(issue(&f, &free).await),
        (ErrorCode::Stale, Some("order_id".into())),
        "an unsettled order has no number yet"
    );
    assert_eq!(
        refusal(issue(&f, "NOPE").await).0,
        ErrorCode::NotFound,
        "no such order"
    );
    assert!(sequences(&f).is_empty());
}

#[tokio::test]
async fn only_the_system_issues_numbers_and_without_a_base_rev() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    let o = sale(&f, &by, &id).await;
    close_window(&f);
    for actor in [owner(), by] {
        let i = issue_intent(&f, actor, &o);
        assert_eq!(
            refusal(f.writer.submit_intent(i).await).0,
            ErrorCode::PermissionDenied
        );
    }
    let mut i = issue_intent(&f, system(), &o);
    i.base_rev = Some(1);
    assert_eq!(
        refusal(f.writer.submit_intent(i).await),
        (ErrorCode::ValidationFailed, Some("base_rev".into()))
    );
    assert_eq!(number_of(&f, &o), None);
}

#[tokio::test]
async fn a_new_epoch_or_a_new_financial_year_starts_a_new_segment_at_one() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let by = counter(&f).await;
    let (a, b, c, d) = (
        sale(&f, &by, &id).await,
        sale(&f, &by, &id).await,
        sale(&f, &by, &id).await,
        sale(&f, &by, &id).await,
    );
    close_window(&f);
    issue(&f, &a).await.unwrap();
    issue(&f, &b).await.unwrap();

    // The next epoch: a new lease counter on the same Hub instance.
    let row = LeaseRow {
        expires_at: i64::MAX,
        ..lease_row(2, HUB)
    };
    reopen(&f).write_tx(|tx| write_lease(tx, &row)).unwrap();
    f.status
        .send_modify(|s| s.lease = Some(LeaseInfo::from(&row)));
    issue(&f, &c).await.unwrap();

    // The next financial year, stamped on the order at settle.
    sql(
        &f,
        format!("UPDATE orders SET fy = 'FY2027-28' WHERE id = '{d}'"),
    )
    .await;
    issue(&f, &d).await.unwrap();

    let (e1, e2) = (number::segment(1, HUB), number::segment(2, HUB));
    assert_eq!(
        number_of(&f, &b).unwrap(),
        format!("DEV01/26-27/{e1}/000002")
    );
    assert_eq!(
        number_of(&f, &c).unwrap(),
        format!("DEV01/26-27/{e2}/000001")
    );
    assert_eq!(
        number_of(&f, &d).unwrap(),
        format!("DEV01/27-28/{e2}/000001")
    );
    assert_eq!(
        sequences(&f),
        [
            ("FY2026-27".to_owned(), e1, 3),
            ("FY2026-27".to_owned(), e2.clone(), 2),
            ("FY2027-28".to_owned(), e2, 2),
        ],
        "an old segment is never written to again"
    );
}

#[tokio::test]
async fn a_failed_issue_rolls_the_sequence_back() {
    let f = bootstrapped().await;
    let id = cola(&f).await;
    let o = sale(&f, &counter(&f).await, &id).await;
    close_window(&f);
    let before = reopen(&f).changes().unwrap().len();
    // With the lock row gone the transaction fails after the number was taken.
    sql(
        &f,
        "DELETE FROM settings WHERE key = 'branch.code_locked_at'".into(),
    )
    .await;
    assert_eq!(refusal(issue(&f, &o).await).0, ErrorCode::Internal);
    assert_eq!(number_of(&f, &o), None);
    assert!(sequences(&f).is_empty());
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
}
