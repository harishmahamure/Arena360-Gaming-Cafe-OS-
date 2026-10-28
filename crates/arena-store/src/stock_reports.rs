//! Row loaders for the Stock reports (Story 5.31). They only read.

use arena_proto::AppError;
use rusqlite::{Connection, ToSql, params_from_iter};

use crate::db_error;

/// A tracked product with what is on hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockLevelRow {
    pub product_id: String,
    pub name: String,
    pub qty: i64,
    pub avg_cost_minor: i64,
}

/// One `stock_moves` row with its product's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockMoveReportRow {
    pub move_id: String,
    pub product_id: String,
    pub product: String,
    pub reason: String,
    pub waste_reason: Option<String>,
    pub delta: i64,
    pub unit_cost_minor: i64,
    /// What the move was worth at cost as stored (0 for a stock-in).
    pub value_minor: i64,
    pub staff_id: String,
    pub at: i64,
}

/// What the movement and waste reports read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StockMovesRows {
    /// Newest first.
    pub moves: Vec<StockMoveReportRow>,
    /// `(staff id, display name)`.
    pub staff: Vec<(String, String)>,
}

/// Every live tracked non-bundle product with its quantity and average cost, by name.
pub fn stock_levels(conn: &Connection) -> Result<Vec<StockLevelRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT p.product_id, p.name, COALESCE(s.qty, 0), COALESCE(s.avg_cost_minor, 0)
             FROM products p LEFT JOIN stock s ON s.product_id = p.product_id
             WHERE p.deleted_at IS NULL AND p.track_stock = 1 AND p.bundle = 0
             ORDER BY p.name COLLATE NOCASE, p.product_id",
        )
        .map_err(db_error)?;
    stmt.query_map([], |r| {
        Ok(StockLevelRow {
            product_id: r.get(0)?,
            name: r.get(1)?,
            qty: r.get(2)?,
            avg_cost_minor: r.get(3)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// The moves with `at` from `from_ms` to `to_ms` inclusive (UTC ms), newest first, optionally of
/// one product and one move reason. `waste_only` keeps the waste moves and expired write-offs.
/// The caller keeps the ones whose local business day is in range.
pub fn stock_moves_between(
    conn: &Connection,
    from_ms: i64,
    to_ms: i64,
    product_id: Option<&str>,
    reason: Option<&str>,
    waste_only: bool,
) -> Result<Vec<StockMoveReportRow>, AppError> {
    let mut sql = String::from(
        "SELECT m.move_id, m.product_id, COALESCE(p.name, ''), m.reason, m.waste_reason, m.delta,
                m.unit_cost_minor, m.value_minor, m.staff_id, m.at
         FROM stock_moves m LEFT JOIN products p ON p.product_id = m.product_id
         WHERE m.at BETWEEN ?1 AND ?2",
    );
    let mut args: Vec<&dyn ToSql> = vec![&from_ms, &to_ms];
    if let Some(id) = product_id.as_ref() {
        args.push(id);
        sql.push_str(&format!(" AND m.product_id = ?{}", args.len()));
    }
    if let Some(reason) = reason.as_ref() {
        args.push(reason);
        sql.push_str(&format!(" AND m.reason = ?{}", args.len()));
    }
    if waste_only {
        sql.push_str(" AND (m.waste_reason IS NOT NULL OR m.reason = 'expired')");
    }
    sql.push_str(" ORDER BY m.at DESC, m.rowid DESC");
    let mut stmt = conn.prepare(&sql).map_err(db_error)?;
    stmt.query_map(params_from_iter(args), |r| {
        Ok(StockMoveReportRow {
            move_id: r.get(0)?,
            product_id: r.get(1)?,
            product: r.get(2)?,
            reason: r.get(3)?,
            waste_reason: r.get(4)?,
            delta: r.get(5)?,
            unit_cost_minor: r.get(6)?,
            value_minor: r.get(7)?,
            staff_id: r.get(8)?,
            at: r.get(9)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

#[cfg(test)]
mod tests {
    use crate::Store;

    const T: i64 = 1_800_000_000_000;

    /// A migrated database with two tracked products, one bundle, one untracked product, and
    /// moves of several reasons written with plain SQL.
    fn fixture() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("branch.db"), T).unwrap();
        store
            .conn
            .execute_batch(
                "PRAGMA foreign_keys = OFF;
                 INSERT INTO products (product_id, name, category, price_minor, tax_rate_bp, hsn,
                                       track_stock, image, kitchen, favourite, active, bundle,
                                       rev, created_at, updated_at)
                 VALUES ('A', 'Cola', 'Drinks', 50, 0, '', 1, '', 0, 0, 1, 0, 1, 0, 0),
                        ('B', 'apple', 'Food', 30, 0, '', 1, '', 0, 0, 1, 0, 1, 0, 0),
                        ('C', 'Combo', 'Food', 90, 0, '', 0, '', 0, 0, 1, 1, 1, 0, 0),
                        ('D', 'Napkin', 'Misc', 1, 0, '', 0, '', 0, 0, 1, 0, 1, 0, 0);
                 INSERT INTO stock (product_id, qty, avg_cost_minor, rev, created_at, updated_at)
                 VALUES ('A', 10, 20, 1, 0, 0);
                 INSERT INTO staff (staff_id, branch_id, login, display_name, role_id, active, rev,
                                    created_at, updated_at)
                 VALUES ('S1', 'B', 's1', 'Sita', 'R', 1, 1, 0, 0);",
            )
            .unwrap();
        let moves = [
            ("M1", "A", "stock_in", None, 10, 20, 0, 1_000),
            ("M2", "A", "waste", Some("spoiled"), -2, 20, 40, 2_000),
            ("M3", "B", "expired", None, -1, 10, 10, 3_000),
        ];
        for (id, product, reason, waste, delta, unit, value, at) in moves {
            store
                .conn
                .execute(
                    "INSERT INTO stock_moves (move_id, product_id, reason, delta, before_qty,
                        before_avg_cost_minor, unit_cost_minor, supplier, lots_used, staff_id, at,
                        rev, created_at, updated_at, waste_reason, value_minor)
                     VALUES (?1, ?2, ?3, ?4, 0, 0, ?5, '', '[]', 'S1', ?6, 1, 0, 0, ?7, ?8)",
                    rusqlite::params![id, product, reason, delta, unit, T + at, waste, value],
                )
                .unwrap();
        }
        (dir, store)
    }

    #[test]
    fn levels_list_tracked_products_by_name_with_zero_for_no_stock() {
        let (_dir, store) = fixture();
        let levels = store.stock_levels().unwrap();
        let seen: Vec<_> = levels
            .iter()
            .map(|l| (l.name.as_str(), l.qty, l.avg_cost_minor))
            .collect();
        assert_eq!(seen, [("apple", 0, 0), ("Cola", 10, 20)]);
    }

    #[test]
    fn moves_come_newest_first_and_filter_by_product_reason_and_waste() {
        let (_dir, store) = fixture();
        let all = store
            .stock_moves_rows(T, T + 5_000, None, None, false)
            .unwrap();
        let ids: Vec<_> = all.moves.iter().map(|m| m.move_id.as_str()).collect();
        assert_eq!(ids, ["M3", "M2", "M1"]);
        assert_eq!(all.moves[0].product, "apple");
        assert_eq!(all.staff, [("S1".to_owned(), "Sita".to_owned())]);

        let cola = store
            .stock_moves_rows(T, T + 5_000, Some("A"), None, false)
            .unwrap();
        assert_eq!(cola.moves.len(), 2);
        let stock_in = store
            .stock_moves_rows(T, T + 5_000, None, Some("stock_in"), false)
            .unwrap();
        assert_eq!(stock_in.moves.len(), 1);
        let waste = store
            .stock_moves_rows(T, T + 5_000, None, None, true)
            .unwrap();
        let ids: Vec<_> = waste.moves.iter().map(|m| m.move_id.as_str()).collect();
        assert_eq!(ids, ["M3", "M2"]);
        let none = store
            .stock_moves_rows(T + 4_000, T + 5_000, None, None, false)
            .unwrap();
        assert!(none.moves.is_empty());
    }
}
