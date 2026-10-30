//! What the yearly GST archive (Story 9.14) reads, and the `gst_archive_log` row a publish writes.
//!
//! Everything is read as stored: no tax is recomputed. The log table carries no business data; it
//! is the one row `gst_archive.publish` writes, so the upload appears in the audit and the Hub can
//! tell a year whose rows have changed since from one that has not.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use arena_proto::AppError;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::restore::ORPHANED_DIR;
use crate::{GstLineRow, db_error};

const SALES_KINDS: &str = "('session', 'plan_sale', 'pos_sale')";

/// A row of `gst_archive_log`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GstArchiveLogRow {
    pub fy: String,
    pub key: String,
    pub sha256: String,
    pub bytes: i64,
    pub invoices: i64,
    pub credit_notes: i64,
    /// Invoice numbers a fork set aside, as recorded by the last publish.
    pub orphaned: Vec<String>,
    pub built_at: i64,
    pub rev: i64,
}

/// What the stored rows of one financial year add up to, to tell whether they changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GstFingerprint {
    /// Numbered, settled sales invoices of the year.
    pub invoices: i64,
    /// Credit notes of the year.
    pub credit_notes: i64,
    /// The newest `updated_at` of any of them (0 when there are none).
    pub newest_updated_at: i64,
}

impl GstArchiveLogRow {
    /// Whether the year's rows differ from what the logged archive was built from: a different
    /// count, or a row written since the file was built.
    pub fn is_stale(&self, now: &GstFingerprint) -> bool {
        self.invoices != now.invoices
            || self.credit_notes != now.credit_notes
            || now.newest_updated_at >= self.built_at
    }
}

fn row_of(r: &rusqlite::Row<'_>) -> rusqlite::Result<(GstArchiveLogRow, String)> {
    Ok((
        GstArchiveLogRow {
            fy: r.get(0)?,
            key: r.get(1)?,
            sha256: r.get(2)?,
            bytes: r.get(3)?,
            invoices: r.get(4)?,
            credit_notes: r.get(5)?,
            orphaned: Vec::new(),
            built_at: r.get(7)?,
            rev: r.get(8)?,
        },
        r.get(6)?,
    ))
}

fn with_orphans((mut row, orphaned): (GstArchiveLogRow, String)) -> GstArchiveLogRow {
    row.orphaned = serde_json::from_str(&orphaned).unwrap_or_default();
    row
}

const LOG_COLUMNS: &str = "fy, key, sha256, bytes, invoices, credit_notes, orphaned, built_at, rev";

/// The `gst_archive_log` row of `fy`, if the Hub has published it.
pub fn find_gst_archive_log(
    conn: &Connection,
    fy: &str,
) -> Result<Option<GstArchiveLogRow>, AppError> {
    conn.query_row(
        &format!("SELECT {LOG_COLUMNS} FROM gst_archive_log WHERE fy = ?1"),
        [fy],
        row_of,
    )
    .optional()
    .map_err(db_error)
    .map(|r| r.map(with_orphans))
}

/// Every year archived, newest first.
pub fn list_gst_archive_logs(conn: &Connection) -> Result<Vec<GstArchiveLogRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {LOG_COLUMNS} FROM gst_archive_log ORDER BY fy DESC"
        ))
        .map_err(db_error)?;
    stmt.query_map([], row_of)
        .map_err(db_error)?
        .map(|r| r.map(with_orphans).map_err(db_error))
        .collect()
}

/// The fields a publish writes.
#[derive(Debug, Clone, Copy)]
pub struct NewGstArchiveLog<'a> {
    pub fy: &'a str,
    pub key: &'a str,
    pub sha256: &'a str,
    pub bytes: i64,
    pub invoices: i64,
    pub credit_notes: i64,
    pub orphaned: &'a [String],
    pub built_at: i64,
}

fn orphaned_json(orphaned: &[String]) -> String {
    serde_json::to_string(orphaned).unwrap_or_else(|_| "[]".to_owned())
}

/// Opens the `fy` row (`rev` and the stamps are set by the apply step that follows).
pub fn insert_gst_archive_log(conn: &Connection, n: &NewGstArchiveLog<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO gst_archive_log (fy, key, sha256, bytes, invoices, credit_notes, orphaned,
                                      built_at, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, 0, 0)",
        params![
            n.fy,
            n.key,
            n.sha256,
            n.bytes,
            n.invoices,
            n.credit_notes,
            orphaned_json(n.orphaned),
            n.built_at
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn update_gst_archive_log(conn: &Connection, n: &NewGstArchiveLog<'_>) -> Result<(), AppError> {
    conn.execute(
        "UPDATE gst_archive_log
         SET key = ?2, sha256 = ?3, bytes = ?4, invoices = ?5, credit_notes = ?6, orphaned = ?7,
             built_at = ?8
         WHERE fy = ?1",
        params![
            n.fy,
            n.key,
            n.sha256,
            n.bytes,
            n.invoices,
            n.credit_notes,
            orphaned_json(n.orphaned),
            n.built_at
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// The fingerprint of every financial year that has a numbered invoice or a credit note.
pub fn gst_fingerprints(conn: &Connection) -> Result<BTreeMap<String, GstFingerprint>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT fy, SUM(inv), SUM(cn), MAX(upd) FROM (
                 SELECT fy, COUNT(*) AS inv, 0 AS cn, MAX(updated_at) AS upd
                 FROM orders
                 WHERE status = 'settled' AND kind IN {SALES_KINDS} AND invoice_no IS NOT NULL
                 GROUP BY fy
                 UNION ALL
                 SELECT fy, 0, COUNT(*), MAX(updated_at) FROM refunds GROUP BY fy)
             GROUP BY fy"
        ))
        .map_err(db_error)?;
    stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            GstFingerprint {
                invoices: r.get(1)?,
                credit_notes: r.get(2)?,
                newest_updated_at: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
            },
        ))
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// A numbered, settled sales invoice of the year.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveInvoiceRow {
    pub order_id: String,
    pub invoice_no: String,
    pub kind: String,
    pub business_day: String,
    pub settled_at: i64,
    pub total_minor: i64,
    pub tax_minor: i64,
    pub tax_breakup: Option<String>,
    pub buyer_gstin: Option<String>,
    pub buyer_name: Option<String>,
}

/// A credit note of the year, with the buyer and invoice number of the order it refunds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveCreditRow {
    pub order_id: String,
    pub credit_note_no: String,
    pub invoice_no: String,
    pub business_day: String,
    pub created_at: i64,
    pub reason: String,
    pub amount_minor: i64,
    pub tax_minor: i64,
    pub tax_breakup: Option<String>,
    /// The refunded lines, as stored (a JSON array).
    pub lines: String,
    pub buyer_gstin: Option<String>,
    pub buyer_name: Option<String>,
}

/// The stored rows of one financial year.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GstArchiveRows {
    pub invoices: Vec<ArchiveInvoiceRow>,
    pub credits: Vec<ArchiveCreditRow>,
    /// The lines of the year's invoices.
    pub lines: Vec<GstLineRow>,
}

/// The invoices, credit notes and invoice lines stamped with `fy`.
pub fn load_gst_archive_rows(conn: &Connection, fy: &str) -> Result<GstArchiveRows, AppError> {
    let mut invoices = conn
        .prepare(&format!(
            "SELECT id, invoice_no, kind, business_day, COALESCE(settled_at, updated_at),
                    total_minor, tax_minor, tax_breakup, buyer_gstin, buyer_name
             FROM orders
             WHERE fy = ?1 AND status = 'settled' AND kind IN {SALES_KINDS}
               AND invoice_no IS NOT NULL
             ORDER BY business_day, settled_at, invoice_no"
        ))
        .map_err(db_error)?;
    let invoices = invoices
        .query_map([fy], |r| {
            Ok(ArchiveInvoiceRow {
                order_id: r.get(0)?,
                invoice_no: r.get(1)?,
                kind: r.get(2)?,
                business_day: r.get(3)?,
                settled_at: r.get(4)?,
                total_minor: r.get(5)?,
                tax_minor: r.get(6)?,
                tax_breakup: r.get(7)?,
                buyer_gstin: r.get(8)?,
                buyer_name: r.get(9)?,
            })
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;

    let mut credits = conn
        .prepare(
            "SELECT f.order_id, f.credit_note_no, COALESCE(o.invoice_no, ''), f.business_day,
                    f.created_at, f.reason, f.amount_minor, f.tax_minor, f.tax_breakup, f.lines,
                    o.buyer_gstin, o.buyer_name
             FROM refunds f LEFT JOIN orders o ON o.id = f.order_id
             WHERE f.fy = ?1
             ORDER BY f.business_day, f.created_at, f.credit_note_no",
        )
        .map_err(db_error)?;
    let credits = credits
        .query_map([fy], |r| {
            Ok(ArchiveCreditRow {
                order_id: r.get(0)?,
                credit_note_no: r.get(1)?,
                invoice_no: r.get(2)?,
                business_day: r.get(3)?,
                created_at: r.get(4)?,
                reason: r.get(5)?,
                amount_minor: r.get(6)?,
                tax_minor: r.get(7)?,
                tax_breakup: r.get(8)?,
                lines: r.get(9)?,
                buyer_gstin: r.get(10)?,
                buyer_name: r.get(11)?,
            })
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;

    let mut lines = conn
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
                 WHERE fy = ?1 AND status = 'settled' AND kind IN {SALES_KINDS}
                   AND invoice_no IS NOT NULL)
             ORDER BY l.order_id, l.position, l.id"
        ))
        .map_err(db_error)?;
    let lines = lines
        .query_map([fy], |r| {
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
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    Ok(GstArchiveRows {
        invoices,
        credits,
        lines,
    })
}

/// The invoice numbers of `fy` found in databases under `orphaned/` (beside `db_path`) that the
/// live database `live` does not hold, and how many files could not be read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrphanScan {
    pub numbers: BTreeSet<String>,
    pub unreadable: usize,
}

/// Looks for invoice numbers a fork set aside: numbers of `fy` in the databases a restore or a
/// promote moved to `orphaned/` that `live` does not hold. A file that cannot be opened or lacks
/// the tables is counted in `unreadable` and skipped. Blocking (disk).
pub fn scan_orphaned_invoices(db_path: &Path, fy: &str, live: &Connection) -> OrphanScan {
    let mut scan = OrphanScan::default();
    let dir = db_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map_or_else(|| Path::new(".").to_path_buf(), Path::to_path_buf)
        .join(ORPHANED_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return scan;
    };
    let mut files: Vec<_> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "db"))
        .collect();
    files.sort();
    for file in files {
        match numbers_in(&file, fy) {
            Ok(numbers) => {
                for n in numbers {
                    if !held_live(live, &n) {
                        scan.numbers.insert(n);
                    }
                }
            }
            Err(_) => scan.unreadable += 1,
        }
    }
    scan
}

fn numbers_in(file: &Path, fy: &str) -> Result<Vec<String>, rusqlite::Error> {
    let conn = Connection::open_with_flags(
        file,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let mut stmt =
        conn.prepare("SELECT invoice_no FROM orders WHERE fy = ?1 AND invoice_no IS NOT NULL")?;
    stmt.query_map([fy], |r| r.get(0))?.collect()
}

fn held_live(live: &Connection, number: &str) -> bool {
    live.query_row(
        "SELECT EXISTS (SELECT 1 FROM orders WHERE invoice_no = ?1)",
        [number],
        |r| r.get(0),
    )
    .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        for m in crate::migrations::MIGRATIONS {
            conn.execute_batch(m.sql).unwrap();
        }
        conn
    }

    fn order(conn: &Connection, id: &str, fy: &str, no: Option<&str>, updated: i64) {
        conn.execute(
            "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy, rev,
                                 created_at, updated_at, settled_at, kind, invoice_no, tax_minor,
                                 tax_breakup)
             VALUES (?1, 'settled', 100, 118, '2026-05-01', ?2, 0, 1, ?3, 1, 'pos_sale', ?4, 18,
                     '[{\"rate_bp\":1800,\"taxable_minor\":100,\"tax_minor\":18}]')",
            params![id, fy, updated, no],
        )
        .unwrap();
    }

    fn refund(conn: &Connection, id: &str, order: &str, fy: &str, updated: i64) {
        conn.execute(
            "INSERT INTO refunds (id, order_id, credit_note_no, reason, amount_minor, tax_minor,
                                  tax_breakup, lines, staff_id, approved_by, business_day, fy, rev,
                                  created_at, updated_at)
             VALUES (?1, ?2, ?3, 'mistake', 59, 9,
                     '[{\"rate_bp\":1800,\"taxable_minor\":-50,\"tax_minor\":-9}]', '[]', 'S', 'S',
                     '2026-05-02', ?4, 0, ?5, ?5)",
            params![id, order, format!("CN/{id}"), fy, updated],
        )
        .unwrap();
    }

    #[test]
    fn the_fingerprint_counts_a_years_numbered_invoices_and_credit_notes() {
        let conn = conn();
        order(&conn, "O1", "FY2025-26", Some("A/1"), 10);
        order(&conn, "O2", "FY2025-26", Some("A/2"), 30);
        order(&conn, "O3", "FY2025-26", None, 99);
        order(&conn, "O4", "FY2026-27", Some("A/3"), 5);
        refund(&conn, "R1", "O1", "FY2025-26", 20);
        let fps = gst_fingerprints(&conn).unwrap();
        assert_eq!(
            fps["FY2025-26"],
            GstFingerprint {
                invoices: 2,
                credit_notes: 1,
                newest_updated_at: 30
            }
        );
        assert_eq!(fps["FY2026-27"].invoices, 1);
        assert_eq!(fps["FY2026-27"].credit_notes, 0);
        assert_eq!(fps.len(), 2);
    }

    #[test]
    fn a_log_row_is_stale_when_the_counts_move_or_a_row_is_written_after_the_build() {
        let row = GstArchiveLogRow {
            fy: "FY2025-26".into(),
            key: "gst-archive/FY2025-26.zst".into(),
            sha256: "ab".into(),
            bytes: 1,
            invoices: 2,
            credit_notes: 1,
            orphaned: vec![],
            built_at: 100,
            rev: 0,
        };
        let same = GstFingerprint {
            invoices: 2,
            credit_notes: 1,
            newest_updated_at: 99,
        };
        assert!(!row.is_stale(&same));
        assert!(row.is_stale(&GstFingerprint {
            invoices: 3,
            ..same
        }));
        assert!(row.is_stale(&GstFingerprint {
            credit_notes: 2,
            ..same
        }));
        assert!(row.is_stale(&GstFingerprint {
            newest_updated_at: 100,
            ..same
        }));
    }

    #[test]
    fn a_log_row_round_trips_with_its_orphans() {
        let conn = conn();
        let orphaned = vec!["A/E1/9".to_owned(), "A/E1/10".to_owned()];
        let mut n = NewGstArchiveLog {
            fy: "FY2025-26",
            key: "gst-archive/FY2025-26.zst",
            sha256: "00ff",
            bytes: 5,
            invoices: 2,
            credit_notes: 1,
            orphaned: &orphaned,
            built_at: 7,
        };
        insert_gst_archive_log(&conn, &n).unwrap();
        n.bytes = 6;
        n.orphaned = &[];
        update_gst_archive_log(&conn, &n).unwrap();
        let row = find_gst_archive_log(&conn, "FY2025-26").unwrap().unwrap();
        assert_eq!((row.bytes, row.orphaned.len()), (6, 0));
        assert!(find_gst_archive_log(&conn, "FY2024-25").unwrap().is_none());
        insert_gst_archive_log(
            &conn,
            &NewGstArchiveLog {
                fy: "FY2026-27",
                orphaned: &orphaned,
                ..n
            },
        )
        .unwrap();
        let all = list_gst_archive_logs(&conn).unwrap();
        assert_eq!(
            all.iter().map(|r| r.fy.as_str()).collect::<Vec<_>>(),
            ["FY2026-27", "FY2025-26"]
        );
        assert_eq!(all[0].orphaned, orphaned);
    }

    #[test]
    fn the_rows_of_a_year_are_the_stored_ones() {
        let conn = conn();
        order(&conn, "O1", "FY2025-26", Some("A/1"), 10);
        order(&conn, "O2", "FY2026-27", Some("A/2"), 10);
        refund(&conn, "R1", "O1", "FY2025-26", 20);
        refund(&conn, "R2", "O1", "FY2026-27", 20);
        let rows = load_gst_archive_rows(&conn, "FY2025-26").unwrap();
        assert_eq!(rows.invoices.len(), 1);
        assert_eq!(rows.invoices[0].invoice_no, "A/1");
        assert_eq!(rows.invoices[0].tax_minor, 18);
        assert_eq!(rows.credits.len(), 1);
        assert_eq!(rows.credits[0].credit_note_no, "CN/R1");
        assert_eq!(rows.credits[0].invoice_no, "A/1");
    }

    #[test]
    fn orphaned_numbers_are_those_of_the_year_the_live_database_lacks() {
        let dir = tempfile::tempdir().unwrap();
        let live = conn();
        order(&live, "O1", "FY2025-26", Some("A/1"), 10);
        let orphaned = dir.path().join(ORPHANED_DIR);
        std::fs::create_dir_all(&orphaned).unwrap();
        let old = Connection::open(orphaned.join("4-HUB-20260601T000000.db")).unwrap();
        for m in crate::migrations::MIGRATIONS {
            old.execute_batch(m.sql).unwrap();
        }
        order(&old, "O1", "FY2025-26", Some("A/1"), 10);
        order(&old, "O9", "FY2025-26", Some("A/E1/9"), 10);
        order(&old, "O8", "FY2026-27", Some("A/E1/8"), 10);
        drop(old);
        std::fs::write(orphaned.join("broken.db"), b"not a database at all").unwrap();
        std::fs::write(orphaned.join("notes.txt"), b"ignored").unwrap();
        let scan = scan_orphaned_invoices(&dir.path().join("branch.db"), "FY2025-26", &live);
        assert_eq!(
            scan.numbers.into_iter().collect::<Vec<_>>(),
            vec!["A/E1/9".to_owned()]
        );
        assert_eq!(scan.unreadable, 1);
        let none =
            scan_orphaned_invoices(&dir.path().join("x").join("branch.db"), "FY2025-26", &live);
        assert_eq!(none, OrphanScan::default());
    }
}
