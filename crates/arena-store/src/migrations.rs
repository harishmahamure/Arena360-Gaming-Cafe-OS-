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
    Migration {
        version: 52,
        sql: include_str!("../migrations/0052_notify_outbox.sql"),
    },
    Migration {
        version: 53,
        sql: include_str!("../migrations/0053_business_invoice.sql"),
    },
    Migration {
        version: 54,
        sql: include_str!("../migrations/0054_coupons.sql"),
    },
    Migration {
        version: 55,
        sql: include_str!("../migrations/0055_manual_discount.sql"),
    },
    Migration {
        version: 56,
        sql: include_str!("../migrations/0056_order_void.sql"),
    },
    Migration {
        version: 57,
        sql: include_str!("../migrations/0057_refunds.sql"),
    },
    Migration {
        version: 58,
        sql: include_str!("../migrations/0058_cash_moves.sql"),
    },
    Migration {
        version: 59,
        sql: include_str!("../migrations/0059_shift_close.sql"),
    },
    Migration {
        version: 60,
        sql: include_str!("../migrations/0060_z_report.sql"),
    },
    Migration {
        version: 61,
        sql: include_str!("../migrations/0061_import_wallet.sql"),
    },
    Migration {
        version: 62,
        sql: include_str!("../migrations/0062_library_entries.sql"),
    },
    Migration {
        version: 63,
        sql: include_str!("../migrations/0063_kiosk_scans.sql"),
    },
    Migration {
        version: 64,
        sql: include_str!("../migrations/0064_library_installs.sql"),
    },
    Migration {
        version: 65,
        sql: include_str!("../migrations/0065_game_plays.sql"),
    },
    Migration {
        version: 66,
        sql: include_str!("../migrations/0066_order_ready.sql"),
    },
    Migration {
        version: 67,
        sql: include_str!("../migrations/0067_player_pc_settings.sql"),
    },
    Migration {
        version: 68,
        sql: include_str!("../migrations/0068_kitchen_notes.sql"),
    },
    Migration {
        version: 69,
        sql: include_str!("../migrations/0069_kitchen_tickets.sql"),
    },
    Migration {
        version: 70,
        sql: include_str!("../migrations/0070_alert_ticket.sql"),
    },
    Migration {
        version: 71,
        sql: include_str!("../migrations/0071_kot_outbox.sql"),
    },
    Migration {
        version: 72,
        sql: include_str!("../migrations/0072_kitchen_displays.sql"),
    },
    Migration {
        version: 73,
        sql: include_str!("../migrations/0073_upload_cursor.sql"),
    },
    Migration {
        version: 74,
        sql: include_str!("../migrations/0074_rollup_log.sql"),
    },
    Migration {
        version: 75,
        sql: include_str!("../migrations/0075_upload_cursor_first.sql"),
    },
    Migration {
        version: 76,
        sql: include_str!("../migrations/0076_snapshot_log.sql"),
    },
    Migration {
        version: 77,
        sql: include_str!("../migrations/0077_owner_overrides.sql"),
    },
    Migration {
        version: 78,
        sql: include_str!("../migrations/0078_notify_drain.sql"),
    },
    Migration {
        version: 79,
        sql: include_str!("../migrations/0079_session_notices.sql"),
    },
    Migration {
        version: 80,
        sql: include_str!("../migrations/0080_credit_reminders.sql"),
    },
    Migration {
        version: 81,
        sql: include_str!("../migrations/0081_owner_alert_rows.sql"),
    },
    Migration {
        version: 82,
        sql: include_str!("../migrations/0082_player_erasure.sql"),
    },
    Migration {
        version: 83,
        sql: include_str!("../migrations/0083_backup_state.sql"),
    },
    Migration {
        version: 84,
        sql: include_str!("../migrations/0084_lease_max_seen.sql"),
    },
    Migration {
        version: 85,
        sql: include_str!("../migrations/0085_restore_pending.sql"),
    },
    Migration {
        version: 86,
        sql: include_str!("../migrations/0086_first_snapshot.sql"),
    },
    Migration {
        version: 87,
        sql: include_str!("../migrations/0087_gst_archive_log.sql"),
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

    /// Migration 53 adds the buyer columns of a business invoice; an existing order has none and a
    /// name never stands without its GSTIN.
    #[test]
    fn the_business_invoice_migration_adds_nullable_buyer_columns_set_together() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for m in MIGRATIONS.iter().filter(|m| m.version < 53) {
            conn.execute_batch(m.sql).unwrap();
        }
        conn.execute_batch(
            "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy, rev,
                                 created_at, updated_at, kind)
             VALUES ('O1', 'settled', 5, 5, 'd', 'f', 0, 0, 0, 'pos_sale');",
        )
        .unwrap();
        conn.execute_batch(MIGRATIONS[52].sql).unwrap();
        let buyer: (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT buyer_gstin, buyer_name FROM orders WHERE id = 'O1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(buyer, (None, None), "an existing order is a consumer sale");
        conn.execute_batch(
            "UPDATE orders SET buyer_gstin = '29AABCT1332L1ZA', buyer_name = 'Acme' WHERE id = 'O1'",
        )
        .unwrap();
        conn.execute_batch("UPDATE orders SET buyer_gstin = NULL, buyer_name = NULL")
            .unwrap();
        assert!(
            conn.execute_batch("UPDATE orders SET buyer_name = 'Acme'")
                .is_err(),
            "a name without a GSTIN is refused"
        );
    }

    /// Migration 54 adds the coupon tables and the order's coupon columns: a code is unique and
    /// upper-case, a coupon is a percent or an amount and never both, and an existing order has none.
    #[test]
    fn the_coupon_migration_adds_unique_upper_case_codes_and_nullable_order_columns() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for m in MIGRATIONS.iter().filter(|m| m.version < 54) {
            conn.execute_batch(m.sql).unwrap();
        }
        conn.execute_batch(
            "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy, rev,
                                 created_at, updated_at, kind)
             VALUES ('O1', 'settled', 5, 5, 'd', 'f', 0, 0, 0, 'pos_sale');",
        )
        .unwrap();
        conn.execute_batch(MIGRATIONS[53].sql).unwrap();
        let coupon: (Option<String>, Option<i64>) = conn
            .query_row(
                "SELECT coupon_code, coupon_minor FROM orders WHERE id = 'O1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(coupon, (None, None), "an existing order took no coupon");
        let insert = |id: &str, code: &str, pct: &str, amt: &str| {
            conn.execute_batch(&format!(
                "INSERT INTO coupons (coupon_id, code, percent_bp, amount_minor, active, rev,
                                      created_at, updated_at)
                 VALUES ('{id}', '{code}', {pct}, {amt}, 1, 0, 0, 0)"
            ))
        };
        insert("C1", "SAVE10", "1000", "NULL").unwrap();
        insert("C2", "FLAT5", "NULL", "500").unwrap();
        assert!(
            insert("C3", "SAVE10", "500", "NULL").is_err(),
            "a code is unique"
        );
        assert!(
            insert("C4", "lower", "500", "NULL").is_err(),
            "a code is upper-case"
        );
        assert!(
            insert("C5", "BOTH1", "500", "500").is_err(),
            "not both kinds"
        );
        assert!(
            insert("C6", "NONE1", "NULL", "NULL").is_err(),
            "one kind is needed"
        );
        assert!(
            insert("C7", "OVER1", "10001", "NULL").is_err(),
            "at most 100%"
        );
        conn.execute_batch("INSERT INTO coupon_targets VALUES ('C1', 'product', 'P')")
            .unwrap();
        assert!(
            conn.execute_batch("INSERT INTO coupon_targets VALUES ('C1', 'shelf', 'P')")
                .is_err()
        );
        assert!(
            conn.execute_batch("UPDATE orders SET coupon_minor = 0 WHERE id = 'O1'")
                .is_err(),
            "a coupon takes something off"
        );
    }

    /// Migration 55 adds the order's manual-discount columns: an existing order has none, a
    /// discount takes something off and its reason is one of the four the Hub offers.
    #[test]
    fn the_discount_migration_adds_nullable_order_columns_with_a_fixed_reason_set() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for m in MIGRATIONS.iter().filter(|m| m.version < 55) {
            conn.execute_batch(m.sql).unwrap();
        }
        conn.execute_batch(
            "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy, rev,
                                 created_at, updated_at, kind)
             VALUES ('O1', 'settled', 5, 5, 'd', 'f', 0, 0, 0, 'pos_sale');",
        )
        .unwrap();
        conn.execute_batch(MIGRATIONS[54].sql).unwrap();
        let none: (Option<i64>, Option<String>) = conn
            .query_row(
                "SELECT discount_minor, discount_reason FROM orders WHERE id = 'O1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(none, (None, None), "an existing order took no discount");
        conn.execute_batch(
            "UPDATE orders SET discount_minor = 1, discount_reason = 'staff_meal',
                               discount_by = 'S1' WHERE id = 'O1'",
        )
        .unwrap();
        assert!(
            conn.execute_batch("UPDATE orders SET discount_minor = 0 WHERE id = 'O1'")
                .is_err(),
            "a discount takes something off"
        );
        assert!(
            conn.execute_batch("UPDATE orders SET discount_reason = 'because' WHERE id = 'O1'")
                .is_err(),
            "the reason is one of the four"
        );
    }

    /// Migration 56 adds the order's void columns: an existing order has none, and a void reason
    /// is one of the three the Hub offers.
    #[test]
    fn the_void_migration_adds_nullable_order_columns_with_a_fixed_reason_set() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for m in MIGRATIONS.iter().filter(|m| m.version < 56) {
            conn.execute_batch(m.sql).unwrap();
        }
        conn.execute_batch(
            "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy, rev,
                                 created_at, updated_at, kind)
             VALUES ('O1', 'open', 5, 5, 'd', 'f', 0, 0, 0, 'session');",
        )
        .unwrap();
        conn.execute_batch(MIGRATIONS[55].sql).unwrap();
        let none: (Option<i64>, Option<String>) = conn
            .query_row(
                "SELECT voided_at, void_reason FROM orders WHERE id = 'O1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(none, (None, None), "an existing order was never voided");
        conn.execute_batch(
            "UPDATE orders SET status = 'void', voided_at = 1, voided_by = 'S1',
                               void_reason = 'changed_mind' WHERE id = 'O1'",
        )
        .unwrap();
        assert!(
            conn.execute_batch("UPDATE orders SET void_reason = 'because' WHERE id = 'O1'")
                .is_err(),
            "the reason is one of the three"
        );
    }

    /// Migration 57 lets `payments` hold a refund's negative rows (never a zero one), keeps the
    /// rows it had, and gives each order line its refunded counters at zero.
    #[test]
    fn the_refund_migration_rebuilds_payments_for_negative_rows_and_counts_refunds_per_line() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for m in MIGRATIONS.iter().filter(|m| m.version < 57) {
            conn.execute_batch(m.sql).unwrap();
        }
        conn.execute_batch(
            "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy, rev,
                                 created_at, updated_at, kind)
             VALUES ('O1', 'settled', 5, 5, 'd', 'f', 0, 0, 0, 'session');
             INSERT INTO payments (id, order_id, method, amount_minor, staff_id, position,
                                   business_day, fy, rev, created_at, updated_at)
             VALUES ('P1', 'O1', 'cash', 5, 'S', 0, 'd', 'f', 0, 0, 0);",
        )
        .unwrap();
        conn.execute_batch(MIGRATIONS[56].sql).unwrap();
        let kept: (i64, Option<String>) = conn
            .query_row(
                "SELECT amount_minor, refund_id FROM payments WHERE id = 'P1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(kept, (5, None), "an existing payment is kept as it was");
        let insert = |id: &str, amount: i64| {
            conn.execute_batch(&format!(
                "INSERT INTO payments (id, order_id, method, amount_minor, staff_id, position,
                                       business_day, fy, rev, created_at, updated_at, refund_id)
                 VALUES ('{id}', 'O1', 'cash', {amount}, 'S', 1, 'd', 'f', 0, 0, 0, 'R1')"
            ))
        };
        assert!(
            insert("P2", -3).is_ok(),
            "a refund pays back with a negative row"
        );
        assert!(
            insert("P3", 0).is_err(),
            "a payment of nothing is not a payment"
        );
        conn.execute_batch(
            "INSERT INTO refunds (id, order_id, credit_note_no, reason, amount_minor, lines,
                                  staff_id, approved_by, business_day, fy, rev, created_at,
                                  updated_at)
             VALUES ('R1', 'O1', 'CN/1', 'mistake', 3, '[]', 'S', 'S', 'd', 'f', 0, 0, 0)",
        )
        .unwrap();
        let again = |id: &str, no: &str, reason: &str, amount: i64| {
            conn.execute_batch(&format!(
                "INSERT INTO refunds (id, order_id, credit_note_no, reason, amount_minor, lines,
                                      staff_id, approved_by, business_day, fy, rev, created_at,
                                      updated_at)
                 VALUES ('{id}', 'O1', '{no}', '{reason}', {amount}, '[]', 'S', 'S', 'd', 'f', 0,
                         0, 0)"
            ))
        };
        assert!(
            again("R2", "CN/1", "mistake", 1).is_err(),
            "a credit note number is unique"
        );
        assert!(
            again("R3", "CN/2", "because", 1).is_err(),
            "the reason is one of the four"
        );
        assert!(
            again("R4", "CN/3", "mistake", 0).is_err(),
            "a refund pays back something"
        );
    }

    /// Migration 58 gives `cash_events` the detail columns of a deposit and an expense, keeps
    /// existing rows, checks the category and the payer, and keeps the table append-only.
    #[test]
    fn the_cash_move_migration_adds_detail_columns_and_stays_append_only() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for m in MIGRATIONS.iter().filter(|m| m.version < 58) {
            conn.execute_batch(m.sql).unwrap();
        }
        conn.execute_batch(
            "INSERT INTO shifts (id, staff_id, opened_at, float_minor, carried_minor, state, rev,
                                 created_at, updated_at)
             VALUES ('SH', 'S', 0, 0, 0, 'open', 0, 0, 0);
             INSERT INTO cash_events (id, shift_id, kind, amount_minor, staff_id, rev, created_at,
                                      updated_at)
             VALUES ('E1', 'SH', 'float_in', 5, 'S', 0, 0, 0);",
        )
        .unwrap();
        conn.execute_batch(MIGRATIONS[57].sql).unwrap();
        let kept: (i64, Option<String>) = conn
            .query_row(
                "SELECT amount_minor, reference FROM cash_events WHERE id = 'E1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(kept, (5, None), "an existing event is kept as it was");
        let insert = |id: &str, category: &str, paid_from: &str, spent: i64| {
            conn.execute_batch(&format!(
                "INSERT INTO cash_events (id, shift_id, kind, amount_minor, staff_id, category,
                                          paid_from, spent_minor, rev, created_at, updated_at)
                 VALUES ('{id}', 'SH', 'expense', 0, 'S', '{category}', '{paid_from}', {spent},
                         0, 0, 0)"
            ))
        };
        assert!(insert("E2", "food_stock", "owner", 10).is_ok());
        assert!(
            insert("E3", "gifts", "owner", 10).is_err(),
            "a known category"
        );
        assert!(
            insert("E4", "other", "bank", 10).is_err(),
            "the drawer or the owner"
        );
        assert!(
            insert("E5", "other", "drawer", 0).is_err(),
            "an expense costs something"
        );
        assert!(
            conn.execute_batch("UPDATE cash_events SET note = 'x' WHERE id = 'E2'")
                .is_err(),
            "the note is part of the record"
        );
        assert!(
            conn.execute_batch("UPDATE cash_events SET receipt_photo = 'x' WHERE id = 'E2'")
                .is_err(),
            "the photo is part of the record"
        );
    }

    /// Migration 59 gives `shifts` the close columns, keeps an open shift as it was and checks
    /// the variance reason.
    #[test]
    fn the_shift_close_migration_adds_the_close_columns() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for m in MIGRATIONS.iter().filter(|m| m.version < 59) {
            conn.execute_batch(m.sql).unwrap();
        }
        conn.execute_batch(
            "INSERT INTO shifts (id, staff_id, opened_at, float_minor, carried_minor, state, rev,
                                 created_at, updated_at)
             VALUES ('SH', 'S', 0, 500, 0, 'open', 0, 0, 0);",
        )
        .unwrap();
        conn.execute_batch(MIGRATIONS[58].sql).unwrap();
        let kept: (i64, Option<i64>, Option<i64>) = conn
            .query_row(
                "SELECT float_minor, counted_minor, next_float_minor FROM shifts WHERE id = 'SH'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(kept, (500, None, None), "an open shift has no count yet");
        let close = |reason: &str| {
            conn.execute_batch(&format!(
                "UPDATE shifts SET state = 'closed', counted_minor = 400, expected_minor = 500,
                                   variance_minor = -100, variance_reason = '{reason}'
                 WHERE id = 'SH'"
            ))
        };
        assert!(close("coffee").is_err(), "a known reason");
        assert!(close("counting_error").is_ok());
        assert!(
            conn.execute_batch("UPDATE shifts SET counted_minor = -1 WHERE id = 'SH'")
                .is_err(),
            "a count is never negative"
        );
    }

    /// Migration 60 gives `shifts` a Z-report column that is empty for every earlier shift.
    #[test]
    fn the_z_report_migration_adds_an_empty_column() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for m in MIGRATIONS.iter().filter(|m| m.version < 60) {
            conn.execute_batch(m.sql).unwrap();
        }
        conn.execute_batch(
            "INSERT INTO shifts (id, staff_id, opened_at, float_minor, carried_minor, state, rev,
                                 created_at, updated_at)
             VALUES ('SH', 'S', 0, 500, 0, 'open', 0, 0, 0);",
        )
        .unwrap();
        conn.execute_batch(MIGRATIONS[59].sql).unwrap();
        let kept: Option<String> = conn
            .query_row("SELECT z_report FROM shifts WHERE id = 'SH'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(kept, None, "an earlier shift has no stored Z-report");
        conn.execute_batch("UPDATE shifts SET z_report = '{}' WHERE id = 'SH'")
            .unwrap();
    }
}
