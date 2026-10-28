//! Invoice numbers (Story 5.14): the per-`(fy, segment)` sequence and the order's `invoice_no`.

use arena_proto::AppError;
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

/// The order kinds that are sales and so get an invoice number. A wallet top-up and a credit
/// collection are payments, never sales.
pub const INVOICED_KINDS: [&str; 3] = ["session", "plan_sale", "pos_sale"];

/// [`INVOICED_KINDS`] as an SQL list.
const INVOICED_KINDS_SQL: &str = "'session', 'plan_sale', 'pos_sale'";

/// What `invoice.issue` needs to know about an order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvoiceOrder {
    pub id: String,
    pub status: String,
    pub kind: String,
    pub total_minor: i64,
    pub fy: String,
    pub settled_at: Option<i64>,
    pub invoice_no: Option<String>,
}

pub fn find_invoice_order(conn: &Connection, id: &str) -> Result<Option<InvoiceOrder>, AppError> {
    conn.query_row(
        "SELECT id, status, kind, total_minor, fy, settled_at, invoice_no FROM orders WHERE id = ?1",
        [id],
        |r| {
            Ok(InvoiceOrder {
                id: r.get(0)?,
                status: r.get(1)?,
                kind: r.get(2)?,
                total_minor: r.get(3)?,
                fy: r.get(4)?,
                settled_at: r.get(5)?,
                invoice_no: r.get(6)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

/// The next sequence number of `(fy, segment)`, if the segment has been opened.
pub fn find_sequence(conn: &Connection, fy: &str, segment: &str) -> Result<Option<i64>, AppError> {
    conn.query_row(
        "SELECT next FROM invoice_sequences WHERE fy = ?1 AND segment = ?2",
        params![fy, segment],
        |r| r.get(0),
    )
    .optional()
    .map_err(db_error)
}

/// Takes the next number of `(fy, segment)`: opens the segment at 1 or advances it by exactly
/// one. Returns the number taken and whether the segment row is new.
pub fn take_sequence(conn: &Connection, fy: &str, segment: &str) -> Result<(i64, bool), AppError> {
    match find_sequence(conn, fy, segment)? {
        Some(next) => {
            conn.execute(
                "UPDATE invoice_sequences SET next = ?3 WHERE fy = ?1 AND segment = ?2",
                params![fy, segment, next + 1],
            )
            .map_err(db_error)?;
            Ok((next, false))
        }
        None => {
            conn.execute(
                "INSERT INTO invoice_sequences (fy, segment, next, rev, created_at, updated_at)
                 VALUES (?1, ?2, 2, 0, 0, 0)",
                params![fy, segment],
            )
            .map_err(db_error)?;
            Ok((1, true))
        }
    }
}

pub fn set_invoice_no(conn: &Connection, order_id: &str, invoice_no: &str) -> Result<(), AppError> {
    conn.execute(
        "UPDATE orders SET invoice_no = ?2 WHERE id = ?1 AND invoice_no IS NULL",
        params![order_id, invoice_no],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Settled sales without a number, oldest settle first: `(order id, settled_at)`.
pub fn list_unnumbered(conn: &Connection, limit: i64) -> Result<Vec<(String, i64)>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT id, settled_at FROM orders
             WHERE status = 'settled' AND invoice_no IS NULL AND total_minor > 0
               AND kind IN ({INVOICED_KINDS_SQL}) AND settled_at IS NOT NULL
             ORDER BY settled_at, id LIMIT ?1"
        ))
        .map_err(db_error)?;
    stmt.query_map([limit], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    #[test]
    fn a_segment_opens_at_one_and_advances_by_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("b.db"), 1).unwrap();
        store
            .write_tx(|conn| {
                assert_eq!(find_sequence(conn, "26-27", "E1a")?, None);
                assert_eq!(take_sequence(conn, "26-27", "E1a")?, (1, true));
                assert_eq!(take_sequence(conn, "26-27", "E1a")?, (2, false));
                assert_eq!(take_sequence(conn, "26-27", "E2a")?, (1, true));
                assert_eq!(take_sequence(conn, "27-28", "E1a")?, (1, true));
                assert_eq!(find_sequence(conn, "26-27", "E1a")?, Some(3));
                Ok(())
            })
            .unwrap();
    }
}
