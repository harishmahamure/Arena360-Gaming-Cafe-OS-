//! Embedded, numbered, forward-only migrations.

/// One migration: its number and its SQL.
pub(crate) struct Migration {
    pub version: u32,
    pub sql: &'static str,
}

/// Every migration, in order. Append new files here; never edit or reorder applied ones.
pub(crate) const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("../migrations/0001_init.sql"),
    },
    Migration {
        version: 2,
        sql: include_str!("../migrations/0002_branch_settings_changes.sql"),
    },
    Migration {
        version: 3,
        sql: include_str!("../migrations/0003_applied_intents.sql"),
    },
    Migration {
        version: 4,
        sql: include_str!("../migrations/0004_devices.sql"),
    },
    Migration {
        version: 5,
        sql: include_str!("../migrations/0005_staff_roles.sql"),
    },
    Migration {
        version: 6,
        sql: include_str!("../migrations/0006_locale.sql"),
    },
    Migration {
        version: 7,
        sql: include_str!("../migrations/0007_elevation_kind.sql"),
    },
    Migration {
        version: 8,
        sql: include_str!("../migrations/0008_audit_indexes.sql"),
    },
    Migration {
        version: 9,
        sql: include_str!("../migrations/0009_device_events.sql"),
    },
    Migration {
        version: 10,
        sql: include_str!("../migrations/0010_admin_exit.sql"),
    },
    Migration {
        version: 11,
        sql: include_str!("../migrations/0011_devices_meta.sql"),
    },
    Migration {
        version: 12,
        sql: include_str!("../migrations/0012_hub_identity.sql"),
    },
    Migration {
        version: 13,
        sql: include_str!("../migrations/0013_device_groups.sql"),
    },
    Migration {
        version: 14,
        sql: include_str!("../migrations/0014_manual_devices.sql"),
    },
    Migration {
        version: 15,
        sql: include_str!("../migrations/0015_trusted_clock.sql"),
    },
    Migration {
        version: 16,
        sql: include_str!("../migrations/0016_floor_zones.sql"),
    },
    Migration {
        version: 17,
        sql: include_str!("../migrations/0017_products.sql"),
    },
    Migration {
        version: 18,
        sql: include_str!("../migrations/0018_staff_setup_code.sql"),
    },
    Migration {
        version: 19,
        sql: include_str!("../migrations/0019_bundles.sql"),
    },
    Migration {
        version: 20,
        sql: include_str!("../migrations/0020_stock.sql"),
    },
    Migration {
        version: 21,
        sql: include_str!("../migrations/0021_device_state.sql"),
    },
    Migration {
        version: 22,
        sql: include_str!("../migrations/0022_applied_commands.sql"),
    },
    Migration {
        version: 23,
        sql: include_str!("../migrations/0023_rates.sql"),
    },
    Migration {
        version: 24,
        sql: include_str!("../migrations/0024_players.sql"),
    },
    Migration {
        version: 25,
        sql: include_str!("../migrations/0025_sessions.sql"),
    },
    Migration {
        version: 26,
        sql: include_str!("../migrations/0026_orders.sql"),
    },
    Migration {
        version: 27,
        sql: include_str!("../migrations/0027_payments.sql"),
    },
    Migration {
        version: 28,
        sql: include_str!("../migrations/0028_undo.sql"),
    },
    Migration {
        version: 29,
        sql: include_str!("../migrations/0029_pause.sql"),
    },
    Migration {
        version: 30,
        sql: include_str!("../migrations/0030_wallet.sql"),
    },
    Migration {
        version: 31,
        sql: include_str!("../migrations/0031_credit.sql"),
    },
    Migration {
        version: 32,
        sql: include_str!("../migrations/0032_credit_collect.sql"),
    },
    Migration {
        version: 33,
        sql: include_str!("../migrations/0033_plans.sql"),
    },
    Migration {
        version: 34,
        sql: include_str!("../migrations/0034_plan_sales.sql"),
    },
    Migration {
        version: 35,
        sql: include_str!("../migrations/0035_prepaid_sessions.sql"),
    },
    Migration {
        version: 36,
        sql: include_str!("../migrations/0036_alerts.sql"),
    },
    Migration {
        version: 37,
        sql: include_str!("../migrations/0037_held_expired_sessions.sql"),
    },
    Migration {
        version: 38,
        sql: include_str!("../migrations/0038_session_extend.sql"),
    },
    Migration {
        version: 39,
        sql: include_str!("../migrations/0039_pricing_rules.sql"),
    },
    Migration {
        version: 40,
        sql: include_str!("../migrations/0040_group_bills.sql"),
    },
    Migration {
        version: 41,
        sql: include_str!("../migrations/0041_forced_end.sql"),
    },
    Migration {
        version: 42,
        sql: include_str!("../migrations/0042_controller_check.sql"),
    },
    Migration {
        version: 43,
        sql: include_str!("../migrations/0043_ad_metrics.sql"),
    },
    Migration {
        version: 44,
        sql: include_str!("../migrations/0044_stock_waste.sql"),
    },
    Migration {
        version: 45,
        sql: include_str!("../migrations/0045_low_stock.sql"),
    },
    Migration {
        version: 46,
        sql: include_str!("../migrations/0046_shifts.sql"),
    },
    Migration {
        version: 47,
        sql: include_str!("../migrations/0047_pos_sales.sql"),
    },
    Migration {
        version: 48,
        sql: include_str!("../migrations/0048_session_orders.sql"),
    },
    Migration {
        version: 49,
        sql: include_str!("../migrations/0049_stock_holds.sql"),
    },
    Migration {
        version: 50,
        sql: include_str!("../migrations/0050_invoices.sql"),
    },
    Migration {
        version: 51,
        sql: include_str!("../migrations/0051_side_effect_outbox.sql"),
    },
];

/// The schema version this binary writes.
pub const LATEST_SCHEMA_VERSION: u32 = MIGRATIONS[MIGRATIONS.len() - 1].version;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_contiguous_from_one() {
        for (i, m) in MIGRATIONS.iter().enumerate() {
            assert_eq!(m.version as usize, i + 1);
        }
    }

    /// Migration 50 adds `orders.invoice_no` (unique once set) and the `invoice_sequences` table.
    #[test]
    fn the_invoices_migration_adds_a_unique_number_and_the_sequence_table() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
        for m in MIGRATIONS.iter().filter(|m| m.version < 50) {
            conn.execute_batch(m.sql).unwrap();
        }
        let order = |id: &str| {
            format!(
                "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy, rev,
                                     created_at, updated_at, kind)
                 VALUES ('{id}', 'settled', 5, 5, 'd', 'f', 0, 0, 0, 'pos_sale');"
            )
        };
        conn.execute_batch(&order("O1")).unwrap();
        conn.execute_batch(&order("O2")).unwrap();
        conn.execute_batch(MIGRATIONS[49].sql).unwrap();

        let number: Option<String> = conn
            .query_row("SELECT invoice_no FROM orders WHERE id = 'O1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(number, None, "an existing order has no number");
        conn.execute_batch("UPDATE orders SET invoice_no = 'X/1' WHERE id = 'O1'")
            .unwrap();
        assert!(
            conn.execute_batch("UPDATE orders SET invoice_no = 'X/1' WHERE id = 'O2'")
                .is_err(),
            "a number is never repeated"
        );
        conn.execute_batch(
            "INSERT INTO invoice_sequences (fy, segment, next, rev, created_at, updated_at)
             VALUES ('FY2026-27', 'E1a', 2, 0, 0, 0);",
        )
        .unwrap();
        assert!(
            conn.execute_batch(
                "INSERT INTO invoice_sequences (fy, segment, next, rev, created_at, updated_at)
                 VALUES ('FY2026-27', 'E1a', 3, 0, 0, 0);",
            )
            .is_err(),
            "a segment is one row"
        );
    }

    /// Migration 49 adds the served flag in place (existing lines read as served) and the
    /// `stock_holds` table.
    #[test]
    fn the_stock_holds_migration_serves_existing_lines_and_adds_holds() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
        for m in MIGRATIONS.iter().filter(|m| m.version < 49) {
            conn.execute_batch(m.sql).unwrap();
        }
        conn.execute_batch(
            "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy, rev,
                                 created_at, updated_at, kind)
             VALUES ('O1', 'open', 5, 5, 'd', 'f', 0, 0, 0, 'session');
             INSERT INTO order_lines (id, order_id, from_at, to_at, minutes, rate_minor_per_hour,
                                      amount_minor, position, rev, created_at, updated_at)
             VALUES ('L1', 'O1', 0, 0, 0, 0, 5, 0, 0, 0, 0);
             INSERT INTO products (product_id, name, category, price_minor, tax_rate_bp, hsn,
                                   track_stock, image, active, bundle, favourite, kitchen, rev,
                                   created_at, updated_at)
             VALUES ('PR', 'Burger', '', 1, 0, '', 1, '', 1, 0, 0, 1, 0, 0, 0);",
        )
        .unwrap();
        conn.execute_batch("BEGIN").unwrap();
        conn.execute_batch(MIGRATIONS[48].sql).unwrap();
        conn.execute_batch("COMMIT").unwrap();

        let line: (i64, Option<i64>, Option<String>) = conn
            .query_row(
                "SELECT served, served_at, served_by FROM order_lines WHERE id = 'L1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(line, (1, None, None), "an existing line is served");
        conn.execute_batch(
            "INSERT INTO order_lines (id, order_id, from_at, to_at, minutes, rate_minor_per_hour,
                                      amount_minor, position, rev, created_at, updated_at, served)
             VALUES ('L2', 'O1', 0, 0, 0, 0, 5, 1000, 0, 0, 0, 0);
             INSERT INTO stock_holds (id, order_line_id, item_id, qty, rev, created_at, updated_at)
             VALUES ('H1', 'L2', 'PR', 2, 0, 0, 0);",
        )
        .unwrap();
        // A hold has a positive quantity and an existing item; the line may go before it.
        assert!(
            conn.execute(
                "INSERT INTO stock_holds (id, order_line_id, item_id, qty, rev, created_at,
                                          updated_at)
                 VALUES ('H2', 'L2', 'PR', 0, 0, 0, 0)",
                [],
            )
            .is_err()
        );
        assert!(
            conn.execute(
                "INSERT INTO stock_holds (id, order_line_id, item_id, qty, rev, created_at,
                                          updated_at)
                 VALUES ('H3', 'L2', 'NOPE', 1, 0, 0, 0)",
                [],
            )
            .is_err()
        );
        conn.execute_batch("DELETE FROM order_lines WHERE id = 'L2'")
            .unwrap();
        let held: i64 = conn
            .query_row("SELECT COUNT(*) FROM stock_holds", [], |r| r.get(0))
            .unwrap();
        assert_eq!(held, 1, "the hold outlives its line");
    }

    /// Migration 48 adds the placement columns in place: existing rows keep their data and read
    /// `NULL`, and the stock ledger stays append-only for the new column too.
    #[test]
    fn the_session_orders_migration_adds_placement_columns_without_a_rebuild() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
        for m in MIGRATIONS.iter().filter(|m| m.version < 48) {
            conn.execute_batch(m.sql).unwrap();
        }
        conn.execute_batch(
            "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy, rev,
                                 created_at, updated_at, kind)
             VALUES ('O1', 'open', 5, 5, 'd', 'f', 0, 0, 0, 'session');
             INSERT INTO order_lines (id, order_id, from_at, to_at, minutes, rate_minor_per_hour,
                                      amount_minor, position, rev, created_at, updated_at)
             VALUES ('L1', 'O1', 0, 0, 0, 0, 5, 0, 0, 0, 0);
             INSERT INTO products (product_id, name, category, price_minor, tax_rate_bp, hsn,
                                   track_stock, image, active, bundle, favourite, kitchen, rev,
                                   created_at, updated_at)
             VALUES ('PR', 'Cola', '', 1, 0, '', 1, '', 1, 0, 0, 0, 0, 0, 0);
             INSERT INTO stock_moves (move_id, product_id, reason, delta, before_qty,
                                      before_avg_cost_minor, unit_cost_minor, supplier, lots_used,
                                      staff_id, at, order_id, rev, created_at, updated_at)
             VALUES ('M1', 'PR', 'sale', -1, 1, 0, 0, '', '[]', 'S', 0, 'O1', 0, 0, 0);",
        )
        .unwrap();
        conn.execute_batch("BEGIN").unwrap();
        conn.execute_batch(MIGRATIONS[47].sql).unwrap();
        conn.execute_batch("COMMIT").unwrap();

        let line: (Option<String>, Option<String>, Option<i64>) = conn
            .query_row(
                "SELECT placement_id, placed_by, placed_at FROM order_lines WHERE id = 'L1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(line, (None, None, None));
        let moved: Option<String> = conn
            .query_row(
                "SELECT placement_id FROM stock_moves WHERE move_id = 'M1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(moved, None);
        conn.execute_batch(
            "INSERT INTO order_lines (id, order_id, from_at, to_at, minutes, rate_minor_per_hour,
                                      amount_minor, position, rev, created_at, updated_at,
                                      placement_id, placed_by, placed_at)
             VALUES ('L2', 'O1', 0, 0, 0, 0, 5, 1000, 0, 0, 0, 'P1', 'S', 7);",
        )
        .unwrap();
        // The old triggers still hold, and placement_id is covered by them.
        assert!(
            conn.execute("UPDATE stock_moves SET placement_id = 'P1'", [])
                .is_err()
        );
        assert!(
            conn.execute("UPDATE stock_moves SET note = 'x'", [])
                .is_err()
        );
        assert!(conn.execute("DELETE FROM stock_moves", []).is_err());
    }

    /// Migration 47 rebuilds `orders`: the rows of the tables that point at it must survive, the
    /// new columns default, and a counter sale is now a legal kind.
    #[test]
    fn the_pos_sales_migration_keeps_orders_and_their_children() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
        for m in MIGRATIONS.iter().filter(|m| m.version < 47) {
            conn.execute_batch(m.sql).unwrap();
        }
        conn.execute_batch(
            "INSERT INTO players (player_id, username, password_hash, phone_lookup, name_enc,
                                  phone_enc, tags, rev, created_at, updated_at)
             VALUES ('P1', 'asha', 'h', 'l', 'n', 'p', '', 1, 0, 0);
             INSERT INTO orders (id, player_id, status, subtotal_minor, total_minor, business_day,
                                 fy, rev, created_at, updated_at, settled_at, settled_by, kind)
             VALUES ('O1', 'P1', 'settled', 500, 500, '2027-01-15', '2026-27', 1, 0, 0, 5,
                     'S', 'topup');
             INSERT INTO payments (id, order_id, method, amount_minor, staff_id, position,
                                   business_day, fy, rev, created_at, updated_at)
             VALUES ('PAY1', 'O1', 'cash', 500, 'S', 0, '2027-01-15', '2026-27', 1, 0, 0);
             INSERT INTO wallet_txns (id, player_id, delta_minor, reason, staff_id, order_id,
                                      business_day, fy, rev, created_at, updated_at)
             VALUES ('T1', 'P1', 500, 'topup', 'S', 'O1', '2027-01-15', '2026-27', 1, 0, 0);
             INSERT INTO order_lines (id, order_id, from_at, to_at, minutes, rate_minor_per_hour,
                                      amount_minor, position, rev, created_at, updated_at)
             VALUES ('L1', 'O1', 0, 0, 0, 0, 500, 0, 1, 0, 0);",
        )
        .unwrap();
        conn.execute_batch("BEGIN").unwrap();
        conn.execute_batch(MIGRATIONS[46].sql).unwrap();
        conn.execute_batch("COMMIT").unwrap();

        let count = |table: &str| -> i64 {
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(
            [
                count("orders"),
                count("payments"),
                count("wallet_txns"),
                count("order_lines")
            ],
            [1, 1, 1, 1]
        );
        let (kind, tax, breakup, settled_by): (String, i64, Option<String>, String) = conn
            .query_row(
                "SELECT kind, tax_minor, tax_breakup, settled_by FROM orders WHERE id = 'O1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            (kind.as_str(), tax, breakup, settled_by.as_str()),
            ("topup", 0, None, "S")
        );
        let violations: i64 = conn
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(violations, 0);
        let order = |id: &str, kind: &str| {
            conn.execute(
                "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy, rev,
                                     created_at, updated_at, kind)
                 VALUES (?1, 'settled', 1, 1, 'd', 'f', 0, 0, 0, ?2)",
                [id, kind],
            )
        };
        assert!(order("O2", "pos_sale").is_ok());
        assert!(order("O3", "bogus").is_err());
        // A stock move can name a sale; the old triggers still hold.
        conn.execute_batch(
            "INSERT INTO products (product_id, name, category, price_minor, tax_rate_bp, hsn,
                                   track_stock, image, active, bundle, favourite, kitchen, rev,
                                   created_at, updated_at)
             VALUES ('PR', 'Cola', '', 1, 0, '', 1, '', 1, 0, 0, 0, 0, 0, 0);
             INSERT INTO stock_moves (move_id, product_id, reason, delta, before_qty,
                                      before_avg_cost_minor, unit_cost_minor, supplier, lots_used,
                                      staff_id, at, order_id, note, rev, created_at, updated_at)
             VALUES ('M1', 'PR', 'sale', -1, 1, 0, 0, '', '[]', 'S', 0, 'O2', NULL, 0, 0, 0);",
        )
        .unwrap();
        assert!(
            conn.execute("UPDATE stock_moves SET note = 'x'", [])
                .is_err()
        );
        assert!(conn.execute("DELETE FROM stock_moves", []).is_err());
    }
}
