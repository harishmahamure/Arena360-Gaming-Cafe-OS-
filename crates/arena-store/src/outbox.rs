//! The side-effect outbox (Story 5.17) and the rows a printed receipt is built from.
//!
//! `side_effect_outbox` is local to the Hub (not in the change log). `apply()` queues a row in the
//! same transaction as the sale, so a crash between commit and print loses nothing; the print task
//! sends it after commit and a result intent closes it.

use arena_proto::AppError;
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

pub const KIND_PRINT: &str = "print";
pub const KIND_DRAWER: &str = "drawer";
pub const STATE_QUEUED: &str = "queued";
pub const STATE_DONE: &str = "done";
pub const STATE_FAILED: &str = "failed";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxRow {
    pub id: String,
    pub kind: String,
    pub ref_id: String,
    pub payload: String,
    pub state: String,
    pub at: i64,
    pub finished_at: Option<i64>,
    pub error: Option<String>,
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<OutboxRow> {
    Ok(OutboxRow {
        id: r.get(0)?,
        kind: r.get(1)?,
        ref_id: r.get(2)?,
        payload: r.get(3)?,
        state: r.get(4)?,
        at: r.get(5)?,
        finished_at: r.get(6)?,
        error: r.get(7)?,
    })
}

const COLUMNS: &str = "id, kind, ref_id, payload, state, at, finished_at, error";

/// Queues a row for the task that owns `kind`.
pub fn insert_outbox(
    conn: &Connection,
    id: &str,
    kind: &str,
    ref_id: &str,
    payload: &str,
    at: i64,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO side_effect_outbox (id, kind, ref_id, payload, state, at)
         VALUES (?1, ?2, ?3, ?4, 'queued', ?5)",
        params![id, kind, ref_id, payload, at],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn find_outbox(conn: &Connection, id: &str) -> Result<Option<OutboxRow>, AppError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM side_effect_outbox WHERE id = ?1"),
        [id],
        row,
    )
    .optional()
    .map_err(db_error)
}

/// Queued rows of `kind`, oldest first.
pub fn list_queued_outbox(
    conn: &Connection,
    kind: &str,
    limit: i64,
) -> Result<Vec<OutboxRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM side_effect_outbox
             WHERE kind = ?1 AND state = 'queued' ORDER BY at, id LIMIT ?2"
        ))
        .map_err(db_error)?;
    stmt.query_map(params![kind, limit], row)
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)
}

/// Closes a queued row as `done` or `failed`; false when it was not queued.
pub fn finish_outbox(
    conn: &Connection,
    id: &str,
    state: &str,
    at: i64,
    error: Option<&str>,
) -> Result<bool, AppError> {
    let n = conn
        .execute(
            "UPDATE side_effect_outbox SET state = ?2, finished_at = ?3, error = ?4
             WHERE id = ?1 AND state = 'queued'",
            params![id, state, at, error],
        )
        .map_err(db_error)?;
    Ok(n == 1)
}

/// What the latest print of `order_id` came to: `queued`, `done` or `failed`.
pub fn latest_print_state(conn: &Connection, order_id: &str) -> Result<Option<String>, AppError> {
    conn.query_row(
        "SELECT state FROM side_effect_outbox WHERE kind = 'print' AND ref_id = ?1
         ORDER BY at DESC, id DESC LIMIT 1",
        [order_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(db_error)
}

/// The order a receipt is printed for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptHead {
    pub kind: String,
    pub status: String,
    pub subtotal_minor: i64,
    pub total_minor: i64,
    pub tax_breakup: Option<String>,
    pub settled_at: i64,
    pub staff: String,
    pub player: Option<String>,
    pub invoice_no: Option<String>,
    /// The business the invoice is made out to (Story 5.20): GSTIN and legal name, B2B only.
    pub buyer: Option<(String, String)>,
    /// The coupon the sale took (Story 5.21): code and the amount taken off.
    pub coupon: Option<(String, i64)>,
    /// The manual discount the sale took (Story 5.22): the amount taken off.
    pub discount: Option<i64>,
}

/// One order line: play time (`device` and `minutes` set), a product (`qty`, `hsn`) or a plan
/// (`label`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptLineRow {
    pub device: Option<String>,
    pub label: Option<String>,
    pub minutes: i64,
    pub rate_minor_per_hour: i64,
    pub qty: Option<i64>,
    pub unit_price_minor: Option<i64>,
    pub hsn: Option<String>,
    pub amount_minor: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptPaymentRow {
    pub method: String,
    pub amount_minor: i64,
    pub reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptSource {
    pub head: ReceiptHead,
    pub lines: Vec<ReceiptLineRow>,
    pub payments: Vec<ReceiptPaymentRow>,
}

/// The committed rows of a settled order, or `None` when there is no such order.
pub fn load_receipt_source(
    conn: &Connection,
    order_id: &str,
) -> Result<Option<ReceiptSource>, AppError> {
    let head = conn
        .query_row(
            "SELECT o.kind, o.status, o.subtotal_minor, o.total_minor, o.tax_breakup,
                    COALESCE(o.settled_at, 0), COALESCE(st.display_name, o.settled_by, ''),
                    p.username, o.invoice_no, o.buyer_gstin, o.buyer_name, o.coupon_code,
                    o.coupon_minor, o.discount_minor
             FROM orders o
             LEFT JOIN staff st ON st.staff_id = o.settled_by
             LEFT JOIN players p ON p.player_id = o.player_id
             WHERE o.id = ?1",
            [order_id],
            |r| {
                Ok(ReceiptHead {
                    kind: r.get(0)?,
                    status: r.get(1)?,
                    subtotal_minor: r.get(2)?,
                    total_minor: r.get(3)?,
                    tax_breakup: r.get(4)?,
                    settled_at: r.get(5)?,
                    staff: r.get(6)?,
                    player: r.get(7)?,
                    invoice_no: r.get(8)?,
                    buyer: r
                        .get::<_, Option<String>>(9)?
                        .zip(r.get::<_, Option<String>>(10)?),
                    coupon: r
                        .get::<_, Option<String>>(11)?
                        .zip(r.get::<_, Option<i64>>(12)?),
                    discount: r.get(13)?,
                })
            },
        )
        .optional()
        .map_err(db_error)?;
    let Some(head) = head else { return Ok(None) };
    let mut lines = conn
        .prepare(
            "SELECT d.label, l.label, l.minutes, l.rate_minor_per_hour, l.qty, l.unit_price_minor,
                    pr.hsn, l.amount_minor
             FROM order_lines l
             LEFT JOIN devices d ON d.device_id = l.device_id
             LEFT JOIN products pr ON pr.product_id = l.product_id
             WHERE l.order_id = ?1 ORDER BY l.position, l.id",
        )
        .map_err(db_error)?;
    let lines = lines
        .query_map([order_id], |r| {
            Ok(ReceiptLineRow {
                device: r.get(0)?,
                label: r.get(1)?,
                minutes: r.get(2)?,
                rate_minor_per_hour: r.get(3)?,
                qty: r.get(4)?,
                unit_price_minor: r.get(5)?,
                hsn: r.get(6)?,
                amount_minor: r.get(7)?,
            })
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    let mut payments = conn
        .prepare(
            "SELECT method, amount_minor, reference FROM payments
             WHERE order_id = ?1 AND amount_minor > 0 ORDER BY position, id",
        )
        .map_err(db_error)?;
    let payments = payments
        .query_map([order_id], |r| {
            Ok(ReceiptPaymentRow {
                method: r.get(0)?,
                amount_minor: r.get(1)?,
                reference: r.get(2)?,
            })
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    Ok(Some(ReceiptSource {
        head,
        lines,
        payments,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    #[test]
    fn a_row_is_queued_listed_oldest_first_and_closed_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("b.db"), 1).unwrap();
        store
            .write_tx(|conn| {
                insert_outbox(conn, "B", KIND_PRINT, "O2", "{}", 20)?;
                insert_outbox(conn, "A", KIND_PRINT, "O1", "{}", 10)?;
                let queued = list_queued_outbox(conn, KIND_PRINT, 10)?;
                assert_eq!(
                    queued.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
                    ["A", "B"]
                );
                assert_eq!(latest_print_state(conn, "O1")?.as_deref(), Some("queued"));
                assert!(finish_outbox(conn, "A", STATE_FAILED, 30, Some("offline"))?);
                assert!(
                    !finish_outbox(conn, "A", STATE_DONE, 31, None)?,
                    "a closed row stays closed"
                );
                let a = find_outbox(conn, "A")?.unwrap();
                assert_eq!(a.state, "failed");
                assert_eq!(a.error.as_deref(), Some("offline"));
                assert_eq!(list_queued_outbox(conn, KIND_PRINT, 10)?.len(), 1);
                assert_eq!(latest_print_state(conn, "none")?, None);
                Ok(())
            })
            .unwrap();
    }
}
