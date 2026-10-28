//! The rows a month's GSTR-1 export is built from (Story 5.29): the numbered invoices of the
//! month's business days, the credit notes of those days, and the lines of both so a document's
//! tax can be shared over HSN codes. Everything is read as stored; no tax is recomputed. The fold
//! into the CSV sections lives in `arena-core`.

use arena_proto::AppError;
use rusqlite::{Connection, params};

use crate::db_error;

/// A numbered, settled sales invoice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GstInvoiceRow {
    pub order_id: String,
    pub invoice_no: String,
    pub business_day: String,
    pub total_minor: i64,
    /// The per-rate tax of the sale; only a counter sale stores one.
    pub tax_breakup: Option<String>,
    pub buyer_gstin: Option<String>,
    pub buyer_name: Option<String>,
}

/// A credit note, with the buyer and invoice number of the order it refunds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GstCreditRow {
    pub order_id: String,
    pub credit_note_no: String,
    pub invoice_no: String,
    pub business_day: String,
    pub amount_minor: i64,
    pub tax_breakup: Option<String>,
    /// The refunded lines, as stored on the credit note (a JSON array).
    pub lines: String,
    pub buyer_gstin: Option<String>,
    pub buyer_name: Option<String>,
}

/// One line of an invoiced order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GstLineRow {
    pub id: String,
    pub order_id: String,
    /// `time`, `product` or `plan`.
    pub kind: String,
    pub label: String,
    pub qty: i64,
    pub amount_minor: i64,
    pub tax_rate_bp: Option<i64>,
    pub hsn: Option<String>,
}

/// What one month holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GstRows {
    pub invoices: Vec<GstInvoiceRow>,
    pub credits: Vec<GstCreditRow>,
    /// The lines of every order an invoice or a credit note of the month belongs to.
    pub lines: Vec<GstLineRow>,
    /// Settled sales of the month that have no invoice number yet (inside their Undo window).
    pub unnumbered: i64,
}

const SALES_KINDS: &str = "('session', 'plan_sale', 'pos_sale')";

fn invoices(conn: &Connection, from: &str, to: &str) -> Result<Vec<GstInvoiceRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT id, invoice_no, business_day, total_minor, tax_breakup, buyer_gstin,
                    buyer_name
             FROM orders
             WHERE status = 'settled' AND kind IN {SALES_KINDS} AND invoice_no IS NOT NULL
               AND business_day BETWEEN ?1 AND ?2
             ORDER BY business_day, settled_at, invoice_no"
        ))
        .map_err(db_error)?;
    stmt.query_map(params![from, to], |r| {
        Ok(GstInvoiceRow {
            order_id: r.get(0)?,
            invoice_no: r.get(1)?,
            business_day: r.get(2)?,
            total_minor: r.get(3)?,
            tax_breakup: r.get(4)?,
            buyer_gstin: r.get(5)?,
            buyer_name: r.get(6)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

fn credits(conn: &Connection, from: &str, to: &str) -> Result<Vec<GstCreditRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT f.order_id, f.credit_note_no, COALESCE(o.invoice_no, ''), f.business_day,
                    f.amount_minor, f.tax_breakup, f.lines, o.buyer_gstin, o.buyer_name
             FROM refunds f JOIN orders o ON o.id = f.order_id
             WHERE f.business_day BETWEEN ?1 AND ?2
             ORDER BY f.business_day, f.created_at, f.credit_note_no",
        )
        .map_err(db_error)?;
    stmt.query_map(params![from, to], |r| {
        Ok(GstCreditRow {
            order_id: r.get(0)?,
            credit_note_no: r.get(1)?,
            invoice_no: r.get(2)?,
            business_day: r.get(3)?,
            amount_minor: r.get(4)?,
            tax_breakup: r.get(5)?,
            lines: r.get(6)?,
            buyer_gstin: r.get(7)?,
            buyer_name: r.get(8)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

fn lines(conn: &Connection, from: &str, to: &str) -> Result<Vec<GstLineRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT l.id, l.order_id,
                    CASE WHEN l.product_id IS NOT NULL THEN 'product'
                         WHEN l.plan_id IS NOT NULL THEN 'plan' ELSE 'time' END,
                    COALESCE(l.label, ''), COALESCE(l.qty, 1), l.amount_minor, l.tax_rate_bp,
                    pr.hsn
             FROM order_lines l
             LEFT JOIN products pr ON pr.product_id = l.product_id
             WHERE l.order_id IN (
                 SELECT id FROM orders
                 WHERE status = 'settled' AND kind IN {SALES_KINDS} AND invoice_no IS NOT NULL
                   AND business_day BETWEEN ?1 AND ?2
                 UNION
                 SELECT order_id FROM refunds WHERE business_day BETWEEN ?1 AND ?2)
             ORDER BY l.order_id, l.position, l.id"
        ))
        .map_err(db_error)?;
    stmt.query_map(params![from, to], |r| {
        Ok(GstLineRow {
            id: r.get(0)?,
            order_id: r.get(1)?,
            kind: r.get(2)?,
            label: r.get(3)?,
            qty: r.get(4)?,
            amount_minor: r.get(5)?,
            tax_rate_bp: r.get(6)?,
            hsn: r.get::<_, Option<String>>(7)?.filter(|h| !h.is_empty()),
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

fn unnumbered(conn: &Connection, from: &str, to: &str) -> Result<i64, AppError> {
    conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM orders
             WHERE status = 'settled' AND kind IN {SALES_KINDS} AND invoice_no IS NULL
               AND total_minor > 0 AND business_day BETWEEN ?1 AND ?2"
        ),
        params![from, to],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// The rows of the business days `from` to `to` inclusive (`YYYY-MM-DD`).
pub fn load_gst_rows(conn: &Connection, from: &str, to: &str) -> Result<GstRows, AppError> {
    Ok(GstRows {
        invoices: invoices(conn, from, to)?,
        credits: credits(conn, from, to)?,
        lines: lines(conn, from, to)?,
        unnumbered: unnumbered(conn, from, to)?,
    })
}
