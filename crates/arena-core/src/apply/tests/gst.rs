//! Story 5.29: the GSTR-1 export over invoices and credit notes the Hub really wrote.

use super::prepaid::{sql, system};
use super::session::{int, pay, submit, table, text};
use super::stock::{intent, make, owner, stock_in, up};
use super::*;
use arena_proto::PaymentMethod::Cash;
use arena_proto::{
    BusinessBuyer, GST_B2B, GST_B2CS, GST_CDNR, GST_HSN, InvoiceIssue, PosSale, PosSaleLine,
    ProductUpsert, RefundCreate, RefundLineReq, UNDO_WINDOW_MS,
};

const GST_ON: &str = r#"{"country":"IN","gst":true,"gstin":"27AAPFU0939F1ZV","inclusive":true,"product_bp":500,"play_bp":1800,"sac":"9996","label":"Tax","rate_bp":0}"#;
const BUYER_GSTIN: &str = "29AABCT1332L1ZA";

async fn product(f: &Fixture, name: &str, price: i64, rate_bp: u16, hsn: &str) -> String {
    let id = make(
        f,
        ProductUpsert {
            price_minor: price,
            tax_rate_bp: Some(rate_bp),
            hsn: hsn.into(),
            ..up(name)
        },
    )
    .await;
    stock_in(f, &id, 50, price / 2).await;
    id
}

async fn sell(
    f: &Fixture,
    lines: &[(&str, i64)],
    cash: i64,
    business: Option<BusinessBuyer>,
) -> String {
    submit(
        f,
        owner(),
        IntentPayload::PosSale(PosSale {
            business,
            coupon: None,
            discount: None,
            lines: lines
                .iter()
                .map(|(id, qty)| PosSaleLine {
                    product_id: (*id).into(),
                    qty: *qty,
                })
                .collect(),
            payments: vec![pay(Cash, cash, None)],
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

fn line_id(f: &Fixture, order: &str, product: &str) -> String {
    table(f, Table::OrderLines)
        .iter()
        .find(|l| text(l, "order_id") == order && text(l, "product_id") == product)
        .map(|l| text(l, "id"))
        .unwrap()
}

async fn refund_one(f: &Fixture, invoice_no: &str, line: String) {
    let mut i = intent(
        owner(),
        IntentPayload::RefundCreate(RefundCreate {
            invoice_no: invoice_no.into(),
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
    i.reason = Some(ReasonCode::Mistake);
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await.unwrap();
}

fn reader(f: &Fixture) -> crate::SnapshotReader {
    crate::SnapshotReader::open(&f.path).unwrap()
}

fn sum(rows: &[Row], col: &str) -> i64 {
    rows.iter().map(|r| int(r, col)).sum()
}

#[tokio::test]
async fn a_month_of_real_invoices_and_credit_notes_reconciles_to_the_stored_totals() {
    let f = bootstrapped().await;
    sql(
        &f,
        format!("UPDATE settings SET value = '{GST_ON}' WHERE key = 'branch.tax'"),
    )
    .await;
    let snack = product(&f, "Snack", 3_100, 500, "2106").await;
    let drink = product(&f, "Drink", 1_180, 1800, "2202").await;

    let consumer = sell(&f, &[(&snack, 2), (&drink, 1)], 7_380, None).await;
    let business = sell(
        &f,
        &[(&drink, 2)],
        2_360,
        Some(BusinessBuyer {
            gstin: BUYER_GSTIN.into(),
            legal_name: "Acme Traders".into(),
        }),
    )
    .await;
    let (no_consumer, no_business) = (invoice(&f, &consumer).await, invoice(&f, &business).await);
    refund_one(&f, &no_consumer, line_id(&f, &consumer, &snack)).await;
    refund_one(&f, &no_business, line_id(&f, &business, &drink)).await;

    let day = text(&order_row(&f, &consumer), "business_day");
    let month = day[..7].to_owned();
    let m = reader(&f).gst_month(&month).unwrap();
    assert_eq!((m.invoices, m.credit_notes, m.untaxed), (2, 2, 0));
    let section = |key: &str| m.sections.iter().find(|s| s.key == key).unwrap().clone();
    let (b2b, b2cs, cdnr, hsn) = (
        section(GST_B2B),
        section(GST_B2CS),
        section(GST_CDNR),
        section(GST_HSN),
    );
    assert_eq!((b2b.documents, b2cs.documents, cdnr.documents), (1, 2, 1));

    // The Hub's invoices minus its credit notes, from the stored rows.
    let orders = table(&f, Table::Orders);
    let refunds = table(&f, Table::Refunds);
    let taxable = sum(&orders, "subtotal_minor")
        - (sum(&refunds, "amount_minor") - sum(&refunds, "tax_minor"));
    let tax = sum(&orders, "tax_minor") - sum(&refunds, "tax_minor");
    assert_eq!(hsn.taxable_minor, taxable);
    assert_eq!(
        hsn.central_minor + hsn.state_minor + hsn.integrated_minor,
        tax
    );
    // Buyer in state 29 and branch in state 27: the business invoice is inter-state.
    assert!(b2b.integrated_minor > 0 && b2b.central_minor == 0);
    assert_eq!(b2cs.integrated_minor, 0);
    assert_eq!(
        b2b.taxable_minor + b2cs.taxable_minor - cdnr.taxable_minor,
        hsn.taxable_minor
    );

    let (name, zip, _) = reader(&f).gst_export(&month).unwrap();
    assert!(
        name.starts_with("GSTR1_") && name.ends_with(&format!("_{month}.zip")),
        "{name}"
    );
    assert!(zip.starts_with(b"PK\x03\x04"));

    // A month with nothing in it is empty; a bad month and a simple-tax branch are refused.
    assert!(reader(&f).gst_month("2001-01").unwrap().empty);
    let (code, field) = {
        let e = reader(&f).gst_month("2026-13").unwrap_err();
        (e.code, e.field)
    };
    assert_eq!(
        (code, field.as_deref()),
        (ErrorCode::ValidationFailed, Some("month"))
    );
    sql(
        &f,
        format!(
            "UPDATE settings SET value = '{}' WHERE key = 'branch.tax'",
            arena_proto::DEFAULT_TAX
        ),
    )
    .await;
    let e = reader(&f).gst_month(&month).unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::ValidationFailed, Some("tax"))
    );
}
