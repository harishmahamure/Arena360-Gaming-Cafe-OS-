//! GST tax invoices for subscription charges (Story 10.7). An invoice is issued in the same
//! transaction as the billing event that caused it ([`NewInvoice`] goes to
//! `ControlDb::apply_billing_event`): the number is `A360/<FY>/<seq:06>`, the sequence is
//! `MAX(seq) + 1` for the financial year inside that transaction, so it has no gaps, and the
//! unique `billing_event_id` makes a repeated delivery issue nothing. Once issued an invoice never
//! changes; only the render and notify marks are set afterwards.

use super::{
    ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_int, col_opt_text, col_text, text,
};

/// An invoice to issue. The number and sequence are assigned when it is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewInvoice {
    pub id: String,
    /// The financial year, `2026-27`.
    pub fy: String,
    pub tenant_id: String,
    pub branch_id: String,
    pub subscription_id: String,
    /// Epoch ms.
    pub issued_at: i64,
    pub period_start: i64,
    pub period_end: i64,
    pub band: String,
    pub currency: String,
    /// The invoice lines as JSON.
    pub lines_json: String,
    pub subtotal_minor: i64,
    pub cgst_minor: i64,
    pub sgst_minor: i64,
    pub igst_minor: i64,
    pub total_minor: i64,
    pub sac: String,
    pub buyer_name: String,
    pub buyer_gstin: Option<String>,
    pub buyer_state_code: Option<String>,
    pub seller_name: String,
    pub seller_address: String,
    pub seller_gstin: String,
    pub note: Option<String>,
    /// The Razorpay payment this invoice charges; set on an invoice so a refund of that payment finds it.
    pub payment_id: Option<String>,
    /// Set on a credit note: the number of the invoice it reverses. The note then carries the
    /// invoice's own lines and amounts and takes the next `CN` number.
    pub credit_for: Option<String>,
}

/// An issued invoice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invoice {
    pub id: String,
    pub number: String,
    pub fy: String,
    pub kind: String,
    pub seq: i64,
    pub tenant_id: String,
    pub branch_id: String,
    pub subscription_id: String,
    /// The `billing_events.id` of the charge.
    pub billing_event_id: String,
    pub issued_at: i64,
    pub period_start: i64,
    pub period_end: i64,
    pub band: String,
    pub currency: String,
    pub lines_json: String,
    pub subtotal_minor: i64,
    pub cgst_minor: i64,
    pub sgst_minor: i64,
    pub igst_minor: i64,
    pub total_minor: i64,
    pub sac: String,
    pub buyer_name: String,
    pub buyer_gstin: Option<String>,
    pub buyer_state_code: Option<String>,
    pub seller_name: String,
    pub seller_address: String,
    pub seller_gstin: String,
    pub note: Option<String>,
    pub pdf_key: Option<String>,
    pub pdf_at: Option<i64>,
    pub notified_at: Option<i64>,
    /// The number of the invoice a credit note reverses.
    pub original_number: Option<String>,
    /// The Razorpay payment the invoice charges.
    pub payment_id: Option<String>,
}

const COLUMNS: &str = "i.id, i.number, i.fy, i.kind, i.seq, i.tenant_id, i.branch_id, \
     i.subscription_id, i.billing_event_id, i.issued_at, i.period_start, i.period_end, i.band, \
     i.currency, i.lines_json, i.subtotal_minor, i.cgst_minor, i.sgst_minor, i.igst_minor, \
     i.total_minor, i.sac, i.buyer_name, i.buyer_gstin, i.buyer_state_code, i.seller_name, \
     i.seller_address, i.seller_gstin, i.note, i.pdf_key, i.pdf_at, i.notified_at, \
     i.original_number, i.payment_id";

fn invoice_from(r: &[SqlValue]) -> Result<Invoice, ControlError> {
    Ok(Invoice {
        id: col_text(r, 0)?,
        number: col_text(r, 1)?,
        fy: col_text(r, 2)?,
        kind: col_text(r, 3)?,
        seq: col_int(r, 4)?,
        tenant_id: col_text(r, 5)?,
        branch_id: col_text(r, 6)?,
        subscription_id: col_text(r, 7)?,
        billing_event_id: col_text(r, 8)?,
        issued_at: col_int(r, 9)?,
        period_start: col_int(r, 10)?,
        period_end: col_int(r, 11)?,
        band: col_text(r, 12)?,
        currency: col_text(r, 13)?,
        lines_json: col_text(r, 14)?,
        subtotal_minor: col_int(r, 15)?,
        cgst_minor: col_int(r, 16)?,
        sgst_minor: col_int(r, 17)?,
        igst_minor: col_int(r, 18)?,
        total_minor: col_int(r, 19)?,
        sac: col_text(r, 20)?,
        buyer_name: col_text(r, 21)?,
        buyer_gstin: col_opt_text(r, 22)?,
        buyer_state_code: col_opt_text(r, 23)?,
        seller_name: col_text(r, 24)?,
        seller_address: col_text(r, 25)?,
        seller_gstin: col_text(r, 26)?,
        note: col_opt_text(r, 27)?,
        pdf_key: col_opt_text(r, 28)?,
        pdf_at: col_opt_int(r, 29)?,
        notified_at: col_opt_int(r, 30)?,
        original_number: col_opt_text(r, 31)?,
        payment_id: col_opt_text(r, 32)?,
    })
}

fn opt_text(v: Option<&str>) -> SqlValue {
    v.map_or(SqlValue::Null, text)
}

/// The SQL that issues an invoice or a credit note: the next number of the financial year, taken in
/// the transaction that records the event. `$infix` is what follows the year in the number.
macro_rules! issue_sql {
    ($infix:literal) => {
        concat!(
            "INSERT INTO billing_invoices (id, number, fy, kind, seq, tenant_id, branch_id, \
             subscription_id, billing_event_id, issued_at, period_start, period_end, band, \
             currency, lines_json, subtotal_minor, cgst_minor, sgst_minor, igst_minor, \
             total_minor, sac, buyer_name, buyer_gstin, buyer_state_code, seller_name, \
             seller_address, seller_gstin, note, original_number, payment_id) \
             SELECT ?, 'A360/' || ? || '",
            $infix,
            "' || printf('%06d', n.seq), ?, ?, n.seq, \
             ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ? \
             FROM (SELECT COALESCE(MAX(seq), 0) + 1 AS seq FROM billing_invoices \
             WHERE fy = ? AND kind = ?) n"
        )
    };
}

/// The statement that issues `inv` for the billing event row `event_row_id`: the next number of
/// its financial year, taken in the same transaction that records the event. An invoice is
/// `A360/<FY>/<seq:06>`; a credit note (`credit_for` set) is `A360/<FY>/CN<seq:06>` with its own
/// sequence.
pub(super) fn issue_stmt(inv: &NewInvoice, event_row_id: &str) -> Stmt {
    let (sql, kind) = if inv.credit_for.is_some() {
        (issue_sql!("/CN"), "credit_note")
    } else {
        (issue_sql!("/"), "invoice")
    };
    Stmt::new(
        sql,
        vec![
            text(&inv.id),
            text(&inv.fy),
            text(&inv.fy),
            text(kind),
            text(&inv.tenant_id),
            text(&inv.branch_id),
            text(&inv.subscription_id),
            text(event_row_id),
            SqlValue::Integer(inv.issued_at),
            SqlValue::Integer(inv.period_start),
            SqlValue::Integer(inv.period_end),
            text(&inv.band),
            text(&inv.currency),
            text(&inv.lines_json),
            SqlValue::Integer(inv.subtotal_minor),
            SqlValue::Integer(inv.cgst_minor),
            SqlValue::Integer(inv.sgst_minor),
            SqlValue::Integer(inv.igst_minor),
            SqlValue::Integer(inv.total_minor),
            text(&inv.sac),
            text(&inv.buyer_name),
            opt_text(inv.buyer_gstin.as_deref()),
            opt_text(inv.buyer_state_code.as_deref()),
            text(&inv.seller_name),
            text(&inv.seller_address),
            text(&inv.seller_gstin),
            opt_text(inv.note.as_deref()),
            opt_text(inv.credit_for.as_deref()),
            opt_text(inv.payment_id.as_deref()),
            text(&inv.fy),
            text(kind),
        ],
    )
    .expect(1)
}

impl ControlDb {
    /// The invoice with this number (`A360/2026-27/000001`).
    pub async fn invoice(&self, number: &str) -> Result<Option<Invoice>, ControlError> {
        let sql = format!("SELECT {COLUMNS} FROM billing_invoices i WHERE i.number = ?");
        let rows = self.query(&sql, vec![text(number)]).await?;
        rows.first().map(|r| invoice_from(r)).transpose()
    }

    /// The invoices issued for the Razorpay event `razorpay_event_id` (none or one).
    pub async fn invoices_for_event(
        &self,
        razorpay_event_id: &str,
    ) -> Result<Vec<Invoice>, ControlError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM billing_invoices i \
             JOIN billing_events e ON e.id = i.billing_event_id \
             WHERE e.razorpay_event_id = ? ORDER BY i.fy, i.seq"
        );
        let rows = self.query(&sql, vec![text(razorpay_event_id)]).await?;
        rows.iter().map(|r| invoice_from(r)).collect()
    }

    /// Invoices still to finish, oldest first: no PDF yet, or the owner not told yet.
    pub async fn invoices_to_render(&self, limit: u32) -> Result<Vec<Invoice>, ControlError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM billing_invoices i \
             WHERE i.pdf_key IS NULL OR i.notified_at IS NULL \
             ORDER BY i.issued_at, i.id LIMIT ?"
        );
        let rows = self
            .query(&sql, vec![SqlValue::Integer(i64::from(limit))])
            .await?;
        rows.iter().map(|r| invoice_from(r)).collect()
    }

    /// Records where the PDF of invoice `id` was written. `false`: it already has one.
    pub async fn set_invoice_pdf(
        &self,
        id: &str,
        key: &str,
        now: i64,
    ) -> Result<bool, ControlError> {
        self.transact(vec![
            Stmt::new(
                "UPDATE billing_invoices SET pdf_key = ?, pdf_at = ? \
                 WHERE id = ? AND pdf_key IS NULL",
                vec![text(key), SqlValue::Integer(now), text(id)],
            )
            .expect(1),
        ])
        .await
    }

    /// Marks invoice `id` as announced to the owner. `false`: it already was.
    pub async fn mark_invoice_notified(&self, id: &str, now: i64) -> Result<bool, ControlError> {
        self.transact(vec![
            Stmt::new(
                "UPDATE billing_invoices SET notified_at = ? WHERE id = ? AND notified_at IS NULL",
                vec![SqlValue::Integer(now), text(id)],
            )
            .expect(1),
        ])
        .await
    }
}

impl ControlDb {
    /// The invoice with this id.
    pub async fn invoice_by_id(&self, id: &str) -> Result<Option<Invoice>, ControlError> {
        let sql = format!("SELECT {COLUMNS} FROM billing_invoices i WHERE i.id = ?");
        let rows = self.query(&sql, vec![text(id)]).await?;
        rows.first().map(|r| invoice_from(r)).transpose()
    }

    /// The invoices and credit notes of a tenant, newest first, for one branch and one financial
    /// year when given (Story 10.8). `limit` caps the list.
    pub async fn invoices_of_tenant(
        &self,
        tenant_id: &str,
        branch_id: Option<&str>,
        fy: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Invoice>, ControlError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM billing_invoices i WHERE i.tenant_id = ? \
             AND (? IS NULL OR i.branch_id = ?) AND (? IS NULL OR i.fy = ?) \
             ORDER BY i.issued_at DESC, i.id DESC LIMIT ?"
        );
        let rows = self
            .query(
                &sql,
                vec![
                    text(tenant_id),
                    opt_text(branch_id),
                    opt_text(branch_id),
                    opt_text(fy),
                    opt_text(fy),
                    SqlValue::Integer(i64::from(limit)),
                ],
            )
            .await?;
        rows.iter().map(|r| invoice_from(r)).collect()
    }

    /// The financial years a tenant has invoices in, newest first.
    pub async fn invoice_years_of_tenant(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<String>, ControlError> {
        let rows = self
            .query(
                "SELECT DISTINCT fy FROM billing_invoices WHERE tenant_id = ? ORDER BY fy DESC",
                vec![text(tenant_id)],
            )
            .await?;
        rows.iter().map(|r| col_text(r, 0)).collect()
    }
}

impl ControlDb {
    /// The invoices (not credit notes) of a subscription, oldest first.
    pub async fn invoices_of_subscription(
        &self,
        subscription_id: &str,
    ) -> Result<Vec<Invoice>, ControlError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM billing_invoices i WHERE i.subscription_id = ? \
             AND i.kind = 'invoice' ORDER BY i.issued_at, i.id"
        );
        let rows = self.query(&sql, vec![text(subscription_id)]).await?;
        rows.iter().map(|r| invoice_from(r)).collect()
    }

    /// The invoice (not credit note) that charged Razorpay payment `payment_id`.
    pub async fn invoice_of_payment(
        &self,
        payment_id: &str,
    ) -> Result<Option<Invoice>, ControlError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM billing_invoices i WHERE i.payment_id = ? \
             AND i.kind = 'invoice' ORDER BY i.issued_at, i.id LIMIT 1"
        );
        let rows = self.query(&sql, vec![text(payment_id)]).await?;
        rows.first().map(|r| invoice_from(r)).transpose()
    }
}
