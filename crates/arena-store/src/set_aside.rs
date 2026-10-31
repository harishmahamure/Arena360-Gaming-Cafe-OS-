//! What the databases in `orphaned/` hold that the live database does not (Story 9.23): the
//! invoice numbers only they have, with amount and time, and how many changes they kept above a
//! fork. Each is read on a read-only connection of its own; a file that cannot be opened is
//! reported as unreadable and skipped, never fatal.

use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension};

use crate::restore::{OrphanFile, list_orphan_files};

/// An invoice number that only an orphaned database holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanInvoice {
    pub invoice_no: String,
    pub amount_minor: i64,
    /// Settled (else created) at, UTC ms.
    pub at: Option<i64>,
}

/// One database in `orphaned/` and what it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanDb {
    pub file: OrphanFile,
    /// In invoice-number order.
    pub invoices: Vec<OrphanInvoice>,
    /// The file could not be read.
    pub unreadable: bool,
}

/// Runs `read` on a read-only connection to `file`. Reading a database in WAL mode makes SQLite
/// create `-wal` and `-shm` files beside it; the ones this read created are removed again, so
/// `orphaned/` holds only what was put there.
fn read<T>(
    file: &Path,
    read: impl FnOnce(&Connection) -> Result<T, rusqlite::Error>,
) -> Result<T, rusqlite::Error> {
    let sidecars: Vec<(std::path::PathBuf, bool)> = ["-wal", "-shm"]
        .iter()
        .map(|suffix| {
            let mut name = file.as_os_str().to_owned();
            name.push(suffix);
            let path = std::path::PathBuf::from(name);
            let existed = path.exists();
            (path, existed)
        })
        .collect();
    let out = Connection::open_with_flags(
        file,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .and_then(|conn| read(&conn));
    for (path, existed) in sidecars {
        if !existed {
            let _ = std::fs::remove_file(path);
        }
    }
    out
}

fn invoices_in(file: &Path, live: &Connection) -> Result<Vec<OrphanInvoice>, rusqlite::Error> {
    read(file, |conn| {
        let mut stmt = conn.prepare(
            "SELECT invoice_no, total_minor, COALESCE(settled_at, created_at)
         FROM orders WHERE invoice_no IS NOT NULL ORDER BY invoice_no",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(OrphanInvoice {
                invoice_no: r.get(0)?,
                amount_minor: r.get(1)?,
                at: r.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            let row = row?;
            if !held_live(live, &row.invoice_no) {
                out.push(row);
            }
        }
        Ok(out)
    })
}

/// Whether the live database has an order with `number`. A lookup that fails counts as not held:
/// an extra orphan is harmless, a lost one is not.
fn held_live(live: &Connection, number: &str) -> bool {
    live.query_row(
        "SELECT EXISTS (SELECT 1 FROM orders WHERE invoice_no = ?1)",
        [number],
        |r| r.get(0),
    )
    .unwrap_or(false)
}

/// Every database in `orphaned/` beside `db_path` with the invoice numbers `live` does not hold.
/// Blocking (disk).
pub fn scan_orphan_databases(db_path: &Path, live: &Connection) -> Vec<OrphanDb> {
    list_orphan_files(db_path)
        .into_iter()
        .map(|file| match invoices_in(&file.path, live) {
            Ok(invoices) => OrphanDb {
                file,
                invoices,
                unreadable: false,
            },
            Err(_) => OrphanDb {
                file,
                invoices: Vec::new(),
                unreadable: true,
            },
        })
        .collect()
}

/// How many changes of `epoch` above `seq` the orphaned database `file` holds; `None` when it
/// cannot be read.
pub fn count_changes_above(file: &Path, epoch: &str, seq: i64) -> Option<i64> {
    read(file, |conn| {
        conn.query_row(
            "SELECT COUNT(*) FROM changes WHERE epoch = ?1 AND seq > ?2",
            rusqlite::params![epoch, seq],
            |r| r.get(0),
        )
        .optional()
    })
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrations::MIGRATIONS;

    fn migrated(path: &Path) -> Connection {
        let conn = Connection::open(path).unwrap();
        for m in MIGRATIONS {
            conn.execute_batch(m.sql).unwrap();
        }
        conn
    }

    fn order(conn: &Connection, id: &str, no: Option<&str>, total: i64, settled: Option<i64>) {
        conn.execute(
            "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy, rev,
                                 created_at, updated_at, settled_at, kind, invoice_no, tax_minor,
                                 tax_breakup)
             VALUES (?1, 'settled', ?3, ?3, '2027-01-10', 'FY2026-27', 0, 5, 5, ?4, 'pos_sale', ?2, 0, '[]')",
            rusqlite::params![id, no, total, settled],
        )
        .unwrap();
    }

    fn change(conn: &Connection, epoch: &str, seq: i64) {
        conn.execute(
            "INSERT INTO changes (epoch, seq, tbl, row_id, op, row, intent_id, intent_kind,
                                  actor_kind, actor_id, at)
             VALUES (?1, ?2, 't', 'r', 'insert', x'00', 'i', 'k', 'system', 'a', 0)",
            rusqlite::params![epoch, seq],
        )
        .unwrap();
    }

    #[test]
    fn only_the_invoices_the_live_database_lacks_are_listed_with_amount_and_time() {
        let dir = tempfile::tempdir().unwrap();
        let live_path = dir.path().join("branch.db");
        let live = migrated(&live_path);
        order(&live, "L1", Some("INV/0001"), 500, Some(10));
        let orphaned = dir.path().join(crate::ORPHANED_DIR);
        std::fs::create_dir(&orphaned).unwrap();
        let old = migrated(&orphaned.join("3-OLD-20270115T133000.db"));
        order(&old, "O1", Some("INV/0001"), 500, Some(10));
        order(&old, "O2", Some("INV/0002"), 11_800, Some(77));
        order(&old, "O3", Some("INV/0003"), 900, None);
        order(&old, "O4", None, 100, Some(1));
        change(&old, "3:OLD", 1);
        change(&old, "3:OLD", 2);
        change(&old, "3:OLD", 3);
        drop(old);
        std::fs::write(orphaned.join("9-BAD-20270101T000000.db"), b"not sqlite").unwrap();
        std::fs::write(orphaned.join("notes.txt"), b"x").unwrap();

        let found = scan_orphan_databases(&live_path, &live);
        assert_eq!(found.len(), 2, "notes.txt is not a database");
        let good = &found[0];
        assert_eq!(good.file.name, "3-OLD-20270115T133000.db");
        assert!(!good.unreadable);
        assert_eq!(
            good.invoices,
            vec![
                OrphanInvoice {
                    invoice_no: "INV/0002".into(),
                    amount_minor: 11_800,
                    at: Some(77)
                },
                OrphanInvoice {
                    invoice_no: "INV/0003".into(),
                    amount_minor: 900,
                    at: Some(5)
                },
            ]
        );
        assert!(found[1].unreadable && found[1].invoices.is_empty());

        assert_eq!(count_changes_above(&good.file.path, "3:OLD", 1), Some(2));
        assert_eq!(count_changes_above(&good.file.path, "3:OLD", 3), Some(0));
        assert_eq!(count_changes_above(&good.file.path, "9:X", 0), Some(0));
        assert_eq!(count_changes_above(&found[1].file.path, "3:OLD", 0), None);

        // Reading leaves no `-wal` or `-shm` file behind in `orphaned/`.
        let mut left: Vec<String> = std::fs::read_dir(&orphaned)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            vec![
                "3-OLD-20270115T133000.db",
                "9-BAD-20270101T000000.db",
                "notes.txt"
            ]
        );
    }
}
