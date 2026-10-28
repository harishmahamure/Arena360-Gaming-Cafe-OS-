//! Refunds with credit notes (Story 5.24): the invoice a refund names, its lines with what has
//! been refunded of each, the `refunds` change-log table and the negative `payments` rows a
//! refund writes.

use arena_proto::AppError;
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

/// A settled, invoiced order, as `refund.create` reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefundOrder {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub total_minor: i64,
    pub fy: String,
    pub settled_at: Option<i64>,
    pub invoice_no: Option<String>,
    pub player_id: Option<String>,
    /// The per-rate tax of the sale, when it carries one (a counter sale does).
    pub tax_breakup: Option<String>,
}

const ORDER_COLUMNS: &str =
    "id, kind, status, total_minor, fy, settled_at, invoice_no, player_id, tax_breakup";

fn order_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<RefundOrder> {
    Ok(RefundOrder {
        id: r.get(0)?,
        kind: r.get(1)?,
        status: r.get(2)?,
        total_minor: r.get(3)?,
        fy: r.get(4)?,
        settled_at: r.get(5)?,
        invoice_no: r.get(6)?,
        player_id: r.get(7)?,
        tax_breakup: r.get(8)?,
    })
}

/// The order an invoice number was issued to.
pub fn find_order_by_invoice(
    conn: &Connection,
    invoice_no: &str,
) -> Result<Option<RefundOrder>, AppError> {
    conn.query_row(
        &format!("SELECT {ORDER_COLUMNS} FROM orders WHERE invoice_no = ?1"),
        [invoice_no],
        order_from,
    )
    .optional()
    .map_err(db_error)
}

/// One line of an invoice with its refund counters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefundLineRow {
    pub id: String,
    /// `time`, `product` or `plan`.
    pub kind: String,
    pub label: String,
    pub product_id: Option<String>,
    pub plan_id: Option<String>,
    /// The units sold (1 for a line without a quantity).
    pub qty: i64,
    pub tax_rate_bp: Option<i64>,
    pub amount_minor: i64,
    pub refunded_qty: i64,
    pub refunded_minor: i64,
    /// Whether the product keeps stock.
    pub tracked: bool,
    pub bundle: bool,
    pub hsn: Option<String>,
}

/// The lines of an order, in bill order.
pub fn list_refund_lines(
    conn: &Connection,
    order_id: &str,
) -> Result<Vec<RefundLineRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT l.id,
                    CASE WHEN l.product_id IS NOT NULL THEN 'product'
                         WHEN l.plan_id IS NOT NULL THEN 'plan' ELSE 'time' END,
                    COALESCE(l.label, d.label, 'Play time'), l.product_id, l.plan_id,
                    COALESCE(l.qty, 1), l.tax_rate_bp, l.amount_minor, l.refunded_qty,
                    l.refunded_minor, COALESCE(pr.track_stock, 0), COALESCE(pr.bundle, 0), pr.hsn
             FROM order_lines l
             LEFT JOIN devices d ON d.device_id = l.device_id
             LEFT JOIN products pr ON pr.product_id = l.product_id
             WHERE l.order_id = ?1 ORDER BY l.position, l.id",
        )
        .map_err(db_error)?;
    stmt.query_map([order_id], |r| {
        Ok(RefundLineRow {
            id: r.get(0)?,
            kind: r.get(1)?,
            label: r.get(2)?,
            product_id: r.get(3)?,
            plan_id: r.get(4)?,
            qty: r.get(5)?,
            tax_rate_bp: r.get(6)?,
            amount_minor: r.get(7)?,
            refunded_qty: r.get(8)?,
            refunded_minor: r.get(9)?,
            tracked: r.get::<_, i64>(10)? != 0,
            bundle: r.get::<_, i64>(11)? != 0,
            hsn: r.get::<_, Option<String>>(12)?.filter(|h| !h.is_empty()),
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// Raises the refunded counters of a line.
pub fn add_line_refunded(
    conn: &Connection,
    line_id: &str,
    qty: i64,
    minor: i64,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE order_lines SET refunded_qty = refunded_qty + ?2,
                                refunded_minor = refunded_minor + ?3 WHERE id = ?1",
        params![line_id, qty, minor],
    )
    .map_err(db_error)?;
    Ok(())
}

/// What was taken on an order, by method: the positive payment rows only.
pub fn paid_by_method(conn: &Connection, order_id: &str) -> Result<Vec<(String, i64)>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT method, SUM(amount_minor) FROM payments
             WHERE order_id = ?1 AND amount_minor > 0 GROUP BY method ORDER BY method",
        )
        .map_err(db_error)?;
    stmt.query_map([order_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)
}

/// What earlier refunds of an order came to: `(total, the part that cut outstanding credit)`.
pub fn sum_refunded(conn: &Connection, order_id: &str) -> Result<(i64, i64), AppError> {
    conn.query_row(
        "SELECT COALESCE(SUM(amount_minor), 0), COALESCE(SUM(credit_minor), 0)
         FROM refunds WHERE order_id = ?1",
        [order_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .map_err(db_error)
}

/// A credit note, as the Refund screen and the printer list it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefundRow {
    pub id: String,
    pub order_id: String,
    pub credit_note_no: String,
    pub reason: String,
    pub note: Option<String>,
    pub method: Option<String>,
    pub reference: Option<String>,
    pub amount_minor: i64,
    pub credit_minor: i64,
    pub tax_minor: i64,
    pub tax_breakup: Option<String>,
    pub lines: String,
    pub approved_by: String,
    pub created_at: i64,
}

const REFUND_COLUMNS: &str = "id, order_id, credit_note_no, reason, note, method, reference, \
                              amount_minor, credit_minor, tax_minor, tax_breakup, lines, \
                              approved_by, created_at";

fn refund_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<RefundRow> {
    Ok(RefundRow {
        id: r.get(0)?,
        order_id: r.get(1)?,
        credit_note_no: r.get(2)?,
        reason: r.get(3)?,
        note: r.get(4)?,
        method: r.get(5)?,
        reference: r.get(6)?,
        amount_minor: r.get(7)?,
        credit_minor: r.get(8)?,
        tax_minor: r.get(9)?,
        tax_breakup: r.get(10)?,
        lines: r.get(11)?,
        approved_by: r.get(12)?,
        created_at: r.get(13)?,
    })
}

/// The credit notes of an order, oldest first.
pub fn list_refunds(conn: &Connection, order_id: &str) -> Result<Vec<RefundRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {REFUND_COLUMNS} FROM refunds WHERE order_id = ?1 ORDER BY created_at, id"
        ))
        .map_err(db_error)?;
    stmt.query_map([order_id], refund_from)
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)
}

pub fn find_refund(conn: &Connection, id: &str) -> Result<Option<RefundRow>, AppError> {
    conn.query_row(
        &format!("SELECT {REFUND_COLUMNS} FROM refunds WHERE id = ?1"),
        [id],
        refund_from,
    )
    .optional()
    .map_err(db_error)
}

pub struct NewRefund<'a> {
    pub id: &'a str,
    pub order_id: &'a str,
    pub credit_note_no: &'a str,
    pub reason: &'a str,
    pub note: Option<&'a str>,
    pub method: Option<&'a str>,
    pub reference: Option<&'a str>,
    pub amount_minor: i64,
    pub credit_minor: i64,
    pub tax_minor: i64,
    pub tax_breakup: Option<&'a str>,
    pub lines: &'a str,
    pub staff_id: &'a str,
    pub approved_by: &'a str,
    pub business_day: &'a str,
    pub fy: &'a str,
}

pub fn insert_refund(conn: &Connection, r: &NewRefund<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO refunds (id, order_id, credit_note_no, reason, note, method, reference,
                              amount_minor, credit_minor, tax_minor, tax_breakup, lines, staff_id,
                              approved_by, business_day, fy, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 0, 0, 0)",
        params![
            r.id,
            r.order_id,
            r.credit_note_no,
            r.reason,
            r.note,
            r.method,
            r.reference,
            r.amount_minor,
            r.credit_minor,
            r.tax_minor,
            r.tax_breakup,
            r.lines,
            r.staff_id,
            r.approved_by,
            r.business_day,
            r.fy
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// A payment row of a refund: `amount_minor` is negative (money that left).
pub struct NewRefundPayment<'a> {
    pub id: &'a str,
    pub order_id: &'a str,
    pub refund_id: &'a str,
    pub method: &'a str,
    pub amount_minor: i64,
    pub reference: Option<&'a str>,
    pub staff_id: &'a str,
    pub position: i64,
    pub business_day: &'a str,
    pub fy: &'a str,
}

pub fn insert_refund_payment(conn: &Connection, p: &NewRefundPayment<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO payments (id, order_id, method, amount_minor, reference, staff_id, position,
                               business_day, fy, rev, created_at, updated_at, refund_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, 0, 0, ?10)",
        params![
            p.id,
            p.order_id,
            p.method,
            p.amount_minor,
            p.reference,
            p.staff_id,
            p.position,
            p.business_day,
            p.fy,
            p.refund_id
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Whether any session ever played on the owned plan `id`.
pub fn pack_ever_used(conn: &Connection, id: &str) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sessions WHERE pack_id = ?1)",
        [id],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// Deactivates an owned pack or pass: its balance is cleared and it expires now.
pub fn deactivate_pack(conn: &Connection, id: &str, now_ms: i64) -> Result<(), AppError> {
    conn.execute(
        "UPDATE player_packs
         SET minutes_left = CASE WHEN minutes_left IS NULL THEN NULL ELSE 0 END,
             expires_at = MIN(expires_at, ?2)
         WHERE id = ?1",
        params![id, now_ms],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Why a plan line cannot be refunded: `plan_used` when its pack or pass has been played on,
/// `plan_not_refundable` when no owned plan came of the sale.
pub fn plan_refusal(conn: &Connection, order_id: &str) -> Result<Option<&'static str>, AppError> {
    let Some(pack) = crate::find_pack_for_order(conn, order_id)? else {
        return Ok(Some("plan_not_refundable"));
    };
    let used = pack.minutes_total != pack.minutes_left || pack_ever_used(conn, &pack.id)?;
    Ok(used.then_some("plan_used"))
}

/// The invoice numbered `invoice_no` as the Refund screen shows it.
pub fn load_refund_invoice(
    conn: &Connection,
    invoice_no: &str,
) -> Result<Option<arena_proto::RefundInvoice>, AppError> {
    let Some(order) = find_order_by_invoice(conn, invoice_no.trim())? else {
        return Ok(None);
    };
    if order.status != "settled" {
        return Ok(None);
    }
    let plan = plan_refusal(conn, &order.id)?;
    let lines = list_refund_lines(conn, &order.id)?
        .into_iter()
        .map(|l| {
            let refusal = (l.kind == "plan").then_some(plan).flatten();
            arena_proto::RefundLineView {
                line_id: l.id,
                kind: l.kind,
                label: l.label,
                qty: l.qty,
                refunded_qty: l.refunded_qty,
                amount_minor: l.amount_minor,
                refunded_minor: l.refunded_minor,
                tracked: l.tracked || l.bundle,
                refusal: refusal.map(str::to_owned),
            }
        })
        .collect();
    let paid_minor = paid_by_method(conn, &order.id)?
        .iter()
        .map(|(_, v)| v)
        .sum();
    let refunds = list_refunds(conn, &order.id)?;
    Ok(Some(arena_proto::RefundInvoice {
        invoice_no: order.invoice_no.clone().unwrap_or_default(),
        settled_at: order.settled_at.unwrap_or(0),
        total_minor: order.total_minor,
        paid_minor,
        refunded_minor: refunds.iter().map(|r| r.amount_minor).sum(),
        has_player: order.player_id.is_some(),
        lines,
        refunds: refunds
            .into_iter()
            .map(|r| arena_proto::RefundSummary {
                refund_id: r.id,
                credit_note_no: r.credit_note_no,
                amount_minor: r.amount_minor,
                created_at: r.created_at,
                reason: r.reason,
            })
            .collect(),
        order_id: order.id,
    }))
}

/// What a credit note is printed from: the refund, the invoice it is against and who issued it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreditNoteSource {
    pub refund: RefundRow,
    pub invoice_no: String,
    pub invoice_at: i64,
    pub player: Option<String>,
    pub buyer: Option<(String, String)>,
    /// The display name of the staff member who issued the refund.
    pub staff: String,
}

pub fn load_credit_note_source(
    conn: &Connection,
    refund_id: &str,
) -> Result<Option<CreditNoteSource>, AppError> {
    let Some(refund) = find_refund(conn, refund_id)? else {
        return Ok(None);
    };
    conn.query_row(
        "SELECT COALESCE(o.invoice_no, ''), COALESCE(o.settled_at, 0), p.username,
                o.buyer_gstin, o.buyer_name,
                COALESCE((SELECT display_name FROM staff WHERE staff_id = ?2), ?2)
         FROM orders o LEFT JOIN players p ON p.player_id = o.player_id
         WHERE o.id = ?1",
        params![refund.order_id, refund.approved_by],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?
                    .zip(r.get::<_, Option<String>>(4)?),
                r.get::<_, String>(5)?,
            ))
        },
    )
    .optional()
    .map_err(db_error)
    .map(|found| {
        found.map(
            |(invoice_no, invoice_at, player, buyer, staff)| CreditNoteSource {
                refund,
                invoice_no,
                invoice_at,
                player,
                buyer,
                staff,
            },
        )
    })
}
