//! `arena-store`: the branch database. All SQL lives in this crate.
//!
//! [`Store::open`] opens SQLite in WAL mode with `synchronous=FULL` (both verified by reading
//! them back), refuses a schema newer than this binary and applies the embedded migrations.

mod ad;
mod admin;
mod alerts;
mod audit;
mod branch;
mod commands;
#[cfg(feature = "control")]
pub mod control;
mod coupons;
mod credit;
mod dedupe;
mod devices;
mod group_bills;
mod gst;
mod identity;
mod invoices;
mod kiosk_scans;
mod library;
mod library_installs;
mod migrations;
mod notify;
mod outbox;
mod plans;
mod player_packs;
mod players;
mod pricing_rules;
mod products;
mod reference;
mod refunds;
mod reports;
mod sessions;
mod shifts;
mod staff;
mod stock;
mod stock_reports;
mod wallet;
mod zreport;

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;

use arena_proto::{AppError, ErrorCode};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

pub use ad::{AdKind, AdRow, ad_acknowledge, ad_cursor, ad_rows_after, insert_ad};
pub use admin::{
    AdminExitRow, META_VERIFIER_COUNTER, NewAdminExit, find_admin_exit, insert_admin_exit,
    insert_verifier_version, latest_verifier, max_pin_verifier_version, set_admin_exit_relocked,
    verifier_staff, verifier_version_at,
};
pub use alerts::{
    AlertRow, NewAlert, ack_alert, find_alert, find_open_alert, find_open_alert_for_product,
    insert_alert, latest_branch_alert_at, list_open_alerts, set_alert_qty, set_alert_threshold,
};
pub use audit::{
    AuditCursor, AuditFilter, AuditRow, find_change, list_audit, list_device_labels,
    list_staff_names, previous_row_image, reverted_intent,
};
pub use branch::{
    BranchLocale, BranchSummary, ChangeOp, ChangeRecord, NewBranch, NewDeviceEvent, NewReadGrant,
    Row, RowKey, RowValue, SettingRef, SettingRow, Table, branch_exists, current_branch_id,
    delete_row, dump_table, find_branch_locale, find_branch_summary, find_setting, has_branch_data,
    insert_branch, insert_change, insert_device_event, insert_read_grant, insert_setting,
    list_settings, next_change_seq, read_changes, read_row, stamp_row, update_setting,
    update_setting_value, upsert_row,
};
pub use commands::{
    AppliedCommand, find_applied_command, insert_applied_command, mark_command_reported,
    max_applied_command_id, unreported_commands,
};
pub use coupons::{
    CouponLineRow, CouponRow, CouponWithTargets, clear_order_coupon, clear_order_discount,
    find_coupon, find_coupon_by_code, give_coupon_use, insert_coupon, insert_coupon_target,
    list_coupon_lines, list_coupon_targets, list_coupons, order_coupon, set_order_coupon,
    set_order_discount, take_coupon_use, update_coupon,
};
pub use credit::{
    CreditRow, adjust_outstanding, civil_day_number, credit_aging, credit_aging_with,
    credit_collection, decode_days, encode_days, fifo_aging, find_credit, insert_credit,
    list_credit_debtors, set_credit_paused, set_override_day, update_credit_limit,
};
pub use dedupe::{
    AppliedIntentRecord, StoredIntent, count_applied_intents, find_applied_intent,
    insert_applied_intent, prune_applied_intents,
};
pub use devices::{
    DeviceMetaRow, DeviceRow, GroupRow, NewDevice, RateRow, ZoneRow, cell_taken,
    count_group_devices, count_groups_without_rate, count_live_devices, count_zone_devices,
    deactivate_rate, devices_meta_exists, find_active_rate, find_device, find_device_meta,
    find_group, find_group_by_name, find_rate, find_zone, find_zone_by_name, insert_device,
    insert_device_meta, insert_group, insert_rate, insert_zone, list_active_rates,
    list_device_meta, list_devices, list_groups, list_rates, list_zones, set_device_group,
    set_device_label, set_device_meta_deleted, set_device_place, set_device_reported_version,
    set_device_revoked, set_device_state, set_group_deleted, set_zone_deleted, tv_number_taken,
    update_device_meta, update_group, update_zone,
};
pub use group_bills::{
    GroupBillRow, find_group_bill, find_group_order, insert_group_bill, list_active_group_bills,
    list_ended_group_segments, list_ended_group_sessions, list_group_sessions, set_group_payer,
    set_group_state, set_order_group, set_order_payer, set_session_group,
};
pub use gst::{GstCreditRow, GstInvoiceRow, GstLineRow, GstRows, load_gst_rows};
pub use identity::{
    CachedLicence, HubIdentity, TrustedClockRow, find_cached_licence, find_hub_identity,
    find_trusted_clock, insert_hub_identity, replace_hub_identity, store_licence,
    store_trusted_clock,
};
pub use invoices::{
    INVOICED_KINDS, InvoiceOrder, find_invoice_order, find_sequence, list_unnumbered,
    set_invoice_no, set_order_buyer, take_sequence,
};
pub use kiosk_scans::{
    KioskScanRow, KioskScanSummary, KioskScanWrite, find_kiosk_scan, insert_kiosk_scan,
    list_kiosk_scan_summaries, list_kiosk_scans, replace_kiosk_scan,
};
pub use library::{
    LibraryRow, LibraryWrite, find_library_entry, insert_library_entry, list_library_entries,
    set_library_entry_deleted, update_library_entry,
};
pub use library_installs::{
    GameMatchesRow, InstallRow, find_game_matches, insert_game_matches, insert_library_dismissal,
    insert_library_install, list_game_matches, list_library_dismissals, list_library_installs,
    replace_game_matches, update_library_install,
};
pub use migrations::LATEST_SCHEMA_VERSION;
pub use notify::{
    NewNotify, NotifyRow, find_notify, insert_notify, latest_receipt_state, order_player_id,
    queued_notify_ids, skip_notify,
};
pub use outbox::{
    KIND_DRAWER, KIND_PRINT, OutboxRow, ReceiptHead, ReceiptLineRow, ReceiptPaymentRow,
    ReceiptSource, STATE_DONE, STATE_FAILED, STATE_QUEUED, find_outbox, finish_outbox,
    insert_outbox, latest_print_state, list_queued_outbox, load_receipt_source,
};
pub use plans::{
    PlanRow, find_plan, insert_plan, insert_plan_group, list_plan_groups, list_plans, update_plan,
};
pub use player_packs::{
    NewPlanLine, NewPlayerPack, PlayerPackRow, deduct_pack_minutes, find_pack_for_order,
    find_player_pack, has_unexpired_pass, insert_plan_line, insert_player_pack, list_player_packs,
    pack_in_use, restore_pack_minutes,
};
pub use players::{
    PlayerRow, find_player, find_player_by_username, find_player_key, insert_consent,
    insert_player, insert_player_key, latest_consent, list_player_revs, phone_holder,
    player_import_keys, set_player_password, update_player, username_taken,
};
pub use pricing_rules::{
    PricingRuleRow, find_pricing_rule, insert_pricing_rule, list_pricing_rules, update_pricing_rule,
};
pub use products::{
    ProductRow, barcode_holder, count_active_products, find_product, insert_component,
    insert_product, list_components, list_products, live_bundles_with, set_product_deleted,
    update_product,
};
pub use reference::{
    FileKind, FileState, RefFile, RefGame, RefSignature, ReferenceDb, is_sha256_hex,
};
pub use refunds::{
    CreditNoteSource, NewRefund, NewRefundPayment, RefundLineRow, RefundOrder, RefundRow,
    add_line_refunded, deactivate_pack, find_order_by_invoice, find_refund, insert_refund,
    insert_refund_payment, list_refund_lines, list_refunds, load_credit_note_source,
    load_refund_invoice, pack_ever_used, paid_by_method, plan_refusal, sum_refunded,
};
/// The transaction a write job runs in (always `BEGIN IMMEDIATE`), and the connection it derefs to.
pub use reports::{
    DebtorRow, GuestSalesRow, PlayerSalesRow, ReceivedRow, ReportDevice, SalesLineRow,
    SalesOrderRow, SegmentSpan, SessionReportRow, StaffCountRow, StaffShiftRow,
};
pub use rusqlite::{Connection as DbConn, Transaction};
pub use sessions::{
    Ending, ForcedEnd, LiveSession, NewExtensionOrder, NewOrder, NewOrderLine, NewPayment,
    NewPosOrder, NewProductLine, NewSession, OpenOrder, OrderLineRow, OrderRow, SegmentRow,
    Settlement, close_segments, device_has_live_session, effect_recorded, end_session,
    expire_session, extend_session, find_ending, find_order, find_order_for_session, find_session,
    find_settlement, forced_pack_minutes, forward_intent_id, forward_kind, grace_session,
    hold_session_expired, insert_effect, insert_extension_order, insert_order, insert_order_line,
    insert_payment, insert_pos_order, insert_product_line, insert_segment, insert_session,
    list_live_sessions, list_open_order_lines, list_open_orders, list_order_line_ids,
    list_payment_ids, list_pos_sales, list_segments, live_session_device_of_player,
    live_session_on_device, mark_forced, next_guest_no, reopen_order, reopen_segments,
    reopen_session, settle_order, sum_reserved_minor,
};
pub use sessions::{
    LineRow, OrderVoid, PRODUCT_LINE_BASE, PlacedLine, add_order_amount, count_product_lines,
    count_unserved_lines, find_line, find_placed_order, list_ordered_counts, list_placement_lines,
    list_segment_line_ids, mark_line_served, next_product_position, sum_segment_amounts,
    void_order,
};
pub use sessions::{
    SessionListing, add_segment_pause, assign_order_player, assign_session_player, fold_pause,
    list_sessions_for_day, move_open_orders, move_session, pause_session, reprice_wallet_session,
    resume_session,
};
pub use sessions::{list_unchecked_consoles, set_controller_check, unchecked_console_session};
pub use shifts::{
    NewCashEvent, NewCashMove, NewShift, OpenSale, ShiftClosing, ShiftDetail, ShiftRow,
    close_shift, count_cash_events, expected_cash, find_shift, insert_cash_event, insert_cash_move,
    insert_shift, last_opening_float, list_closed_shifts, open_sales_of_order, open_shift,
    open_shift_id, shift_breakdown, shift_count, shift_of_order_sale, staff_display_name,
    variance_threshold,
};
pub use staff::{
    PinCandidate, RoleRow, StaffListing, StaffRow, StaffWrite, count_active_staff,
    count_active_staff_with_role, deactivate_staff, find_role, find_role_by_name, find_staff,
    find_staff_by_login, insert_role, insert_role_perm, insert_staff, list_pin_candidates,
    list_role_perms, list_roles, list_staff, list_unlock_candidates, role_perm_effect,
    set_staff_credentials, set_staff_setup_code, update_role_name, update_staff,
};
pub use stock::{
    HoldRow, LotRow, MoveRow, NewHold, StockRow, ThresholdRow, expired_qty, find_lot, find_move,
    find_stock, insert_hold, insert_lot, insert_move, insert_stock, latest_move_id,
    list_expiring_lots, list_line_holds, list_lots, list_low_stock, list_moves, list_order_moves,
    list_placement_holds, list_placement_moves, list_tracked_thresholds, low_stock_qty,
    open_hold_qty, open_hold_totals, release_hold, set_lot_remaining, set_low_stock_qty,
    update_stock,
};
pub use stock_reports::{StockLevelRow, StockMoveReportRow, StockMovesRows};
pub use wallet::{
    NewTopupOrder, NewWalletTxn, OrderTxn, WalletRow, WalletTxnRow, adjust_wallet, find_wallet,
    insert_import_wallet_txn, insert_topup_order, insert_wallet, insert_wallet_txn,
    list_order_wallet_txns, list_wallet_txns, order_player_and_kind, wallet_balance,
};
pub use zreport::{
    ZCashEventRow, ZPaymentRow, ZRefundRow, ZReportRows, ZVoidRow, ZWasteRow, load_z_report_rows,
};

/// The transaction `apply()` and the replayer run in.
pub type StoreTx<'a> = Transaction<'a>;

/// The `hub_meta` key holding this Hub's instance id.
pub const META_HUB_INSTANCE_ID: &str = "hub_instance_id";
/// The `hub_meta` key holding the SHA-256 fingerprint (lower-case hex) of the Hub's LAN TLS
/// certificate.
pub const META_TLS_FINGERPRINT: &str = "tls_fingerprint";

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// What Daily sales reads (Story 5.27).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SalesReportRows {
    pub orders: Vec<SalesOrderRow>,
    pub lines: Vec<SalesLineRow>,
    pub received: Vec<ReceivedRow>,
    pub refunded_minor: i64,
    /// `(group id, name)`.
    pub groups: Vec<(String, String)>,
}

/// What Sessions reads (Story 5.27).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionsReportRows {
    pub sessions: Vec<SessionReportRow>,
    pub spans: Vec<SegmentSpan>,
    pub devices: Vec<ReportDevice>,
    /// `(group id, name)`.
    pub groups: Vec<(String, String)>,
}

/// What the Players report reads (Story 5.30).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayersReportRows {
    pub players: Vec<PlayerSalesRow>,
    pub guest: GuestSalesRow,
    pub debtors: Vec<DebtorRow>,
}

/// What the Staff report reads (Story 5.30): one list per figure, keyed by staff id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StaffReportRows {
    pub sales: Vec<StaffCountRow>,
    pub sessions: Vec<StaffCountRow>,
    pub discounts: Vec<StaffCountRow>,
    pub voids: Vec<StaffCountRow>,
    pub refunds: Vec<StaffCountRow>,
    /// Closed shifts near the range (by UTC time); the fold keeps the ones closed in it.
    pub shifts: Vec<StaffShiftRow>,
    /// `(staff id, display name)`.
    pub names: Vec<(String, String)>,
}

/// Maps a SQLite failure to an `INTERNAL` error.
pub fn db_error(err: rusqlite::Error) -> AppError {
    AppError::new(ErrorCode::Internal, format!("database error: {err}"))
}

#[cfg(not(target_os = "android"))]
fn try_lock_file(file: &File) -> std::io::Result<bool> {
    match file.try_lock() {
        Ok(()) => Ok(true),
        Err(std::fs::TryLockError::WouldBlock) => Ok(false),
        Err(std::fs::TryLockError::Error(e)) => Err(e),
    }
}

#[cfg(target_os = "android")]
fn try_lock_file(file: &File) -> std::io::Result<bool> {
    flock_try(file)
}

/// std's `File::try_lock` is unsupported on Android: take the same advisory lock with `flock`.
#[cfg(all(unix, any(test, target_os = "android")))]
fn flock_try(file: &File) -> std::io::Result<bool> {
    use std::os::fd::AsRawFd;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(true);
    }
    let err = std::io::Error::last_os_error();
    if err.kind() == std::io::ErrorKind::WouldBlock {
        return Ok(false);
    }
    Err(err)
}

fn io_error(err: std::io::Error) -> AppError {
    AppError::new(ErrorCode::Internal, format!("i/o error: {err}"))
}

/// The single row of the `lease` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseRow {
    pub counter: i64,
    pub hub_instance_id: String,
    pub provisional: bool,
    pub renewed_at: i64,
    pub expires_at: i64,
}

/// An open branch database.
pub struct Store {
    conn: Connection,
    path: PathBuf,
    writer_lock: Option<File>,
    /// Set once the schema is known to have the `devices` table, so device reads stop
    /// re-checking the schema version.
    has_devices: std::sync::atomic::AtomicBool,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("path", &self.path)
            .field("writer_lock", &self.writer_lock.is_some())
            .finish()
    }
}

impl Store {
    /// Opens (creating if needed) the database at `path`, verifies the pragmas, takes the
    /// process writer lock and, only when it holds that lock, migrates it (recording
    /// `applied_at = now_ms`).
    ///
    /// Without the lock (another process on this data dir is the writer) the store is a
    /// read-only follower: it never migrates or writes, even when its schema is older than this
    /// binary's. See [`Store::holds_writer_lock`].
    ///
    /// Fails `SCHEMA_TOO_NEW` without writing anything when the file was written by a newer
    /// binary.
    pub fn open(path: impl AsRef<Path>, now_ms: i64) -> Result<Self, AppError> {
        let path = path.as_ref().to_path_buf();
        let conn = Connection::open(&path).map_err(db_error)?;
        conn.busy_timeout(BUSY_TIMEOUT).map_err(db_error)?;

        // Schema guard first: nothing below may run against a newer schema.
        let found = current_version(&conn)?;
        if found > LATEST_SCHEMA_VERSION {
            return Err(schema_too_new(found));
        }

        let mode: String = conn
            .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
            .map_err(db_error)?;
        conn.execute_batch("PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON")
            .map_err(db_error)?;
        let mut store = Self {
            conn,
            path,
            writer_lock: None,
            has_devices: Default::default(),
        };
        let journal = store.journal_mode()?;
        if !mode.eq_ignore_ascii_case("wal") || !journal.eq_ignore_ascii_case("wal") {
            return Err(AppError::new(
                ErrorCode::Internal,
                format!("journal_mode is {journal}, expected wal"),
            ));
        }
        let sync = store.synchronous()?;
        if sync != 2 {
            return Err(AppError::new(
                ErrorCode::Internal,
                format!("synchronous is {sync}, expected 2 (FULL)"),
            ));
        }
        let fk: i64 = store
            .conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .map_err(db_error)?;
        if fk != 1 {
            return Err(AppError::new(
                ErrorCode::Internal,
                format!("foreign_keys is {fk}, expected 1"),
            ));
        }
        if store.try_lock_writer()? {
            store.migrate(now_ms)?;
        }
        Ok(store)
    }

    /// Opens a read-only connection to an existing database: it never takes the writer lock,
    /// migrates or writes. Fails `SCHEMA_TOO_NEW` on a newer schema. The Hub's LAN sessions use
    /// one to verify device tokens without going through the writer.
    pub fn open_reader(path: impl AsRef<Path>) -> Result<Self, AppError> {
        use rusqlite::OpenFlags;
        let path = path.as_ref().to_path_buf();
        let conn = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(db_error)?;
        conn.busy_timeout(BUSY_TIMEOUT).map_err(db_error)?;
        let found = current_version(&conn)?;
        if found > LATEST_SCHEMA_VERSION {
            return Err(schema_too_new(found));
        }
        Ok(Self {
            conn,
            path,
            writer_lock: None,
            has_devices: Default::default(),
        })
    }

    fn migrate(&mut self, applied_at: i64) -> Result<(), AppError> {
        if current_version(&self.conn)? == LATEST_SCHEMA_VERSION {
            return Ok(());
        }
        self.write_tx(|tx| {
            // Re-read under the write lock: another process may have migrated meanwhile.
            let found = current_version(tx)?;
            if found > LATEST_SCHEMA_VERSION {
                return Err(schema_too_new(found));
            }
            for m in migrations::MIGRATIONS.iter().filter(|m| m.version > found) {
                tx.execute_batch(m.sql).map_err(db_error)?;
                record_schema_version(tx, m.version, applied_at)?;
            }
            Ok(())
        })
    }

    /// The database file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The highest applied migration.
    pub fn schema_version(&self) -> Result<u32, AppError> {
        current_version(&self.conn)
    }

    /// `PRAGMA journal_mode`, read back.
    pub fn journal_mode(&self) -> Result<String, AppError> {
        self.conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .map_err(db_error)
    }

    /// `PRAGMA synchronous`, read back (2 = FULL).
    pub fn synchronous(&self) -> Result<i64, AppError> {
        self.conn
            .query_row("PRAGMA synchronous", [], |r| r.get(0))
            .map_err(db_error)
    }

    /// `PRAGMA integrity_check` (`"ok"` when healthy).
    pub fn integrity_check(&self) -> Result<String, AppError> {
        self.conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(db_error)
    }

    /// Runs `f` in one `BEGIN IMMEDIATE` transaction: committed on `Ok`, rolled back on `Err`.
    pub fn write_tx<R>(
        &mut self,
        f: impl FnOnce(&Transaction<'_>) -> Result<R, AppError>,
    ) -> Result<R, AppError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let out = f(&tx)?; // dropping `tx` rolls back
        tx.commit().map_err(db_error)?;
        Ok(out)
    }

    /// Like [`Store::write_tx`], but hands `f` the transaction mutably (`apply()`, replay).
    pub fn write_tx_mut<R>(
        &mut self,
        f: impl FnOnce(&mut StoreTx<'_>) -> Result<R, AppError>,
    ) -> Result<R, AppError> {
        let mut tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let out = f(&mut tx)?; // dropping `tx` rolls back
        tx.commit().map_err(db_error)?;
        Ok(out)
    }

    /// Every `changes` row, outside any write transaction.
    pub fn changes(&self) -> Result<Vec<ChangeRecord>, AppError> {
        read_changes(&self.conn)
    }

    /// Every row of a change-log table, ordered by primary key.
    pub fn rows(&self, table: Table) -> Result<Vec<Row>, AppError> {
        dump_table(&self.conn, table)
    }

    /// The device `device_id`, outside any write transaction (`None` on a database not yet
    /// migrated to the devices table).
    pub fn device(&self, device_id: &str) -> Result<Option<DeviceRow>, AppError> {
        if !self.devices_table_exists()? {
            return Ok(None);
        }
        find_device(&self.conn, device_id)
    }

    /// Whether the schema has the `devices` table. Only a positive answer is cached: a reader
    /// opened before the writer migrated sees the table once it exists.
    fn devices_table_exists(&self) -> Result<bool, AppError> {
        use std::sync::atomic::Ordering;
        if self.has_devices.load(Ordering::Relaxed) {
            return Ok(true);
        }
        let yes = self.schema_version()? >= 4;
        if yes {
            self.has_devices.store(true, Ordering::Relaxed);
        }
        Ok(yes)
    }

    /// The branch's display fields, outside any write transaction (`None` before the branch is
    /// bootstrapped or on a database not yet migrated to the branch table).
    pub fn branch_summary(&self) -> Result<Option<BranchSummary>, AppError> {
        if !self.table_exists("branch")? {
            return Ok(None);
        }
        find_branch_summary(&self.conn)
    }

    /// Whether the schema has table `name`.
    fn table_exists(&self, name: &str) -> Result<bool, AppError> {
        self.conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                [name],
                |r| r.get(0),
            )
            .map_err(db_error)
    }

    /// Runs `f` inside one read transaction, so every read in it sees the same snapshot of the
    /// database. Never writes.
    pub fn read_consistent<R>(
        &self,
        f: impl FnOnce(&Self) -> Result<R, AppError>,
    ) -> Result<R, AppError> {
        self.conn
            .execute_batch("BEGIN DEFERRED")
            .map_err(db_error)?;
        let out = f(self);
        let end = if out.is_ok() { "COMMIT" } else { "ROLLBACK" };
        let ended = self.conn.execute_batch(end).map_err(db_error);
        let out = out?;
        ended?;
        Ok(out)
    }

    /// The staff member with `login` (case-insensitive) and their role, outside any write
    /// transaction, in one read snapshot (`None` when there is no such member, or on a database
    /// not yet migrated to the staff table).
    pub fn staff_with_role(&self, login: &str) -> Result<Option<(StaffRow, RoleRow)>, AppError> {
        if !self.table_exists("staff")? {
            return Ok(None);
        }
        self.read_consistent(|s| {
            let Some(staff) = find_staff_by_login(&s.conn, login)? else {
                return Ok(None);
            };
            Ok(find_role(&s.conn, &staff.role_id)?.map(|role| (staff, role)))
        })
    }

    /// The staff member `staff_id` and their role, outside any write transaction, in one read
    /// snapshot (`None` when either is missing or before the staff table exists).
    pub fn staff_by_id_with_role(
        &self,
        staff_id: &str,
    ) -> Result<Option<(StaffRow, RoleRow)>, AppError> {
        if !self.table_exists("staff")? {
            return Ok(None);
        }
        self.read_consistent(|s| {
            let Some(staff) = find_staff(&s.conn, staff_id)? else {
                return Ok(None);
            };
            Ok(find_role(&s.conn, &staff.role_id)?.map(|role| (staff, role)))
        })
    }

    /// Up to `limit` active staff with a PIN whose role allows every one of `permissions`,
    /// other than `exclude`, outside any write transaction (empty before the staff table
    /// exists).
    pub fn pin_candidates(
        &self,
        permissions: &[&str],
        exclude: &str,
        limit: usize,
    ) -> Result<Vec<PinCandidate>, AppError> {
        if !self.table_exists("staff")? {
            return Ok(Vec::new());
        }
        list_pin_candidates(&self.conn, permissions, exclude, limit)
    }

    /// The admin exit `exit_id`, outside any write transaction (`None` before migration 0010).
    pub fn admin_exit(&self, exit_id: &str) -> Result<Option<AdminExitRow>, AppError> {
        if !self.table_exists("device_admin_exits")? {
            return Ok(None);
        }
        find_admin_exit(&self.conn, exit_id)
    }

    /// The newest manager-verifier version and up to `limit` of its entries (active staff with
    /// a PIN whose role allows `permission`, ordered by id), read in one snapshot. The version
    /// is 0 before any was recorded.
    pub fn manager_verifier(
        &self,
        permission: &str,
        limit: usize,
    ) -> Result<(i64, Vec<PinCandidate>), AppError> {
        if !self.table_exists("verifier_history")? {
            return Ok((0, Vec::new()));
        }
        self.read_consistent(|s| {
            let version = latest_verifier(&s.conn)?.map_or(0, |(v, _)| v);
            Ok((
                version,
                list_pin_candidates(&s.conn, &[permission], "", limit)?,
            ))
        })
    }

    /// Up to `limit` active staff with a PIN, `first` ahead of the others (see
    /// [`list_unlock_candidates`]).
    pub fn unlock_candidates(
        &self,
        first: &str,
        limit: usize,
    ) -> Result<Vec<PinCandidate>, AppError> {
        if !self.table_exists("staff")? {
            return Ok(Vec::new());
        }
        list_unlock_candidates(&self.conn, first, limit)
    }

    /// Up to `limit` changes matching `filter`, newest first (see [`list_audit`]).
    pub fn audit_page(&self, filter: &AuditFilter, limit: u32) -> Result<Vec<AuditRow>, AppError> {
        list_audit(&self.conn, filter, limit)
    }

    /// The change `(epoch, seq)`, if it exists.
    pub fn change(&self, epoch: &str, seq: i64) -> Result<Option<ChangeRecord>, AppError> {
        find_change(&self.conn, epoch, seq)
    }

    /// The forward intent a `*.reverted` change undoes (see [`reverted_intent`]).
    pub fn reverted_by(&self, change: &ChangeRecord) -> Result<Option<String>, AppError> {
        reverted_intent(
            &self.conn,
            &change.tbl,
            &change.row_id,
            &change.intent_kind,
            change.at,
            &change.epoch,
            change.seq,
        )
    }

    /// The row image of the change before `(epoch, seq)` to the same row (see
    /// [`previous_row_image`]).
    pub fn previous_row_image(
        &self,
        tbl: &str,
        row_id: &str,
        epoch: &str,
        seq: i64,
    ) -> Result<Option<Vec<u8>>, AppError> {
        previous_row_image(&self.conn, tbl, row_id, epoch, seq)
    }

    /// The staff list, the number of people counted against the `users` limit and the stored
    /// `licence.user_cap` (`None` when unset: the Free default applies). Empty before the staff
    /// table exists.
    pub fn staff_overview(&self) -> Result<(Vec<StaffListing>, i64, Option<String>), AppError> {
        if !self.table_exists("staff")? {
            return Ok((Vec::new(), 0, None));
        }
        self.read_consistent(|s| {
            let cap = find_setting(&s.conn, "licence.user_cap")?.map(|r| r.value);
            Ok((list_staff(&s.conn)?, count_active_staff(&s.conn)?, cap))
        })
    }

    /// Every staff member's `(staff_id, display_name)` (empty before the staff table exists).
    pub fn staff_names(&self) -> Result<Vec<(String, String)>, AppError> {
        if !self.table_exists("staff")? {
            return Ok(Vec::new());
        }
        list_staff_names(&self.conn)
    }

    /// Every device's `(device_id, label)`.
    pub fn device_labels(&self) -> Result<Vec<(String, String)>, AppError> {
        if !self.devices_table_exists()? {
            return Ok(Vec::new());
        }
        list_device_labels(&self.conn)
    }

    /// The branch's locale columns, outside any write transaction (`None` before bootstrap).
    pub fn branch_locale(&self) -> Result<Option<BranchLocale>, AppError> {
        find_branch_locale(&self.conn)
    }

    /// The permission rows of `role_id`, as `(permission, effect)`.
    pub fn role_perms(&self, role_id: &str) -> Result<Vec<(String, String)>, AppError> {
        list_role_perms(&self.conn, role_id)
    }

    /// Every branch setting, outside any write transaction (empty before bootstrap).
    pub fn settings(&self) -> Result<Vec<SettingRow>, AppError> {
        if !self.table_exists("settings")? {
            return Ok(Vec::new());
        }
        list_settings(&self.conn)
    }

    /// Every device, outside any write transaction.
    pub fn devices(&self) -> Result<Vec<DeviceRow>, AppError> {
        if !self.devices_table_exists()? {
            return Ok(Vec::new());
        }
        list_devices(&self.conn)
    }

    pub fn devices_meta(&self) -> Result<Vec<DeviceMetaRow>, AppError> {
        list_device_meta(&self.conn)
    }

    /// The phone lookups and usernames of the live players, for a CSV import dry-run.
    pub fn player_import_keys(&self) -> Result<(HashSet<String>, HashSet<String>), AppError> {
        if !self.table_exists("players")? {
            return Ok((HashSet::new(), HashSet::new()));
        }
        player_import_keys(&self.conn)
    }

    pub fn groups(&self) -> Result<Vec<GroupRow>, AppError> {
        if !self.table_exists("device_groups")? {
            return Ok(Vec::new());
        }
        list_groups(&self.conn)
    }

    pub fn rates(&self) -> Result<Vec<RateRow>, AppError> {
        if !self.table_exists("rates")? {
            return Ok(Vec::new());
        }
        list_active_rates(&self.conn)
    }

    /// Every rate, active or not.
    pub fn all_rates(&self) -> Result<Vec<RateRow>, AppError> {
        if !self.table_exists("rates")? {
            return Ok(Vec::new());
        }
        list_rates(&self.conn)
    }

    pub fn groups_without_rate(&self) -> Result<i64, AppError> {
        if !self.table_exists("rates")? {
            return Ok(0);
        }
        count_groups_without_rate(&self.conn)
    }

    pub fn zones(&self) -> Result<Vec<ZoneRow>, AppError> {
        if !self.table_exists("floor_zones")? {
            return Ok(Vec::new());
        }
        list_zones(&self.conn)
    }

    pub fn product(&self, id: &str) -> Result<Option<ProductRow>, AppError> {
        find_product(&self.conn, id)
    }

    pub fn manager_exists(&self) -> Result<bool, AppError> {
        if !self.table_exists("staff")? {
            return Ok(false);
        }
        self.conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM staff s JOIN roles r ON r.role_id = s.role_id
                 WHERE s.active = 1 AND r.name IN ('Manager', 'Owner'))",
                [],
                |r| r.get(0),
            )
            .map_err(db_error)
    }

    pub fn products_count(&self) -> Result<i64, AppError> {
        if !self.table_exists("products")? {
            return Ok(0);
        }
        count_active_products(&self.conn)
    }

    pub fn components(&self, bundle_id: &str) -> Result<Vec<(String, i64)>, AppError> {
        if !self.table_exists("bundle_components")? {
            return Ok(Vec::new());
        }
        list_components(&self.conn, bundle_id)
    }

    pub fn bundles_with(&self, component_id: &str) -> Result<Vec<String>, AppError> {
        if !self.table_exists("bundle_components")? {
            return Ok(Vec::new());
        }
        live_bundles_with(&self.conn, component_id)
    }

    pub fn stock(&self, product_id: &str) -> Result<Option<StockRow>, AppError> {
        if !self.table_exists("stock")? {
            return Ok(None);
        }
        find_stock(&self.conn, product_id)
    }

    pub fn player_key(&self, player_id: &str) -> Result<Option<String>, AppError> {
        if !self.table_exists("player_keys")? {
            return Ok(None);
        }
        find_player_key(&self.conn, player_id)
    }

    pub fn player_revs(&self) -> Result<Vec<(String, i64)>, AppError> {
        if !self.table_exists("players")? {
            return Ok(Vec::new());
        }
        list_player_revs(&self.conn)
    }

    pub fn player(&self, player_id: &str) -> Result<Option<(PlayerRow, i64, String)>, AppError> {
        if !self.table_exists("players")? || !self.table_exists("player_keys")? {
            return Ok(None);
        }
        find_player(&self.conn, player_id)
    }

    /// The live player with this normalised username and their wrapped key (Story 6.8).
    pub fn player_by_username(
        &self,
        username: &str,
    ) -> Result<Option<(PlayerRow, String)>, AppError> {
        if !self.table_exists("players")? || !self.table_exists("player_keys")? {
            return Ok(None);
        }
        find_player_by_username(&self.conn, username)
    }

    /// The device a player has a live session on, if any (Story 6.8).
    pub fn live_session_device_of_player(
        &self,
        player_id: &str,
    ) -> Result<Option<String>, AppError> {
        if !self.table_exists("sessions")? {
            return Ok(None);
        }
        live_session_device_of_player(&self.conn, player_id)
    }

    pub fn live_sessions(&self) -> Result<Vec<LiveSession>, AppError> {
        if !self.table_exists("sessions")? {
            return Ok(Vec::new());
        }
        list_live_sessions(&self.conn)
    }

    /// Every open alert, oldest first.
    pub fn alerts(&self) -> Result<Vec<AlertRow>, AppError> {
        if !self.table_exists("alerts")? {
            return Ok(Vec::new());
        }
        list_open_alerts(&self.conn)
    }

    /// Every open alert with the name of the product it is about (if any), oldest first.
    pub fn alerts_with_products(&self) -> Result<Vec<(AlertRow, Option<String>)>, AppError> {
        self.alerts()?
            .into_iter()
            .map(|a| {
                let name = match &a.product_id {
                    Some(id) => find_product(&self.conn, id)?.map(|p| p.name),
                    None => None,
                };
                Ok((a, name))
            })
            .collect()
    }

    /// Live tracked products at or under their low-stock threshold, by name.
    pub fn low_stock(&self) -> Result<Vec<ThresholdRow>, AppError> {
        if !self.table_exists("stock")? {
            return Ok(Vec::new());
        }
        list_low_stock(&self.conn)
    }

    /// Every live tracked non-bundle product with its quantity and low-stock threshold.
    pub fn tracked_thresholds(&self) -> Result<Vec<ThresholdRow>, AppError> {
        if !self.table_exists("stock")? {
            return Ok(Vec::new());
        }
        list_tracked_thresholds(&self.conn)
    }

    /// Every console with an ended session awaiting its controller check, as (device, session).
    pub fn unchecked_consoles(&self) -> Result<Vec<(String, String)>, AppError> {
        if !self.table_exists("sessions")? {
            return Ok(Vec::new());
        }
        list_unchecked_consoles(&self.conn)
    }

    pub fn sessions_for_day(&self, business_day: &str) -> Result<Vec<SessionListing>, AppError> {
        if !self.table_exists("sessions")? {
            return Ok(Vec::new());
        }
        list_sessions_for_day(&self.conn, business_day)
    }

    /// The business day's settled counter sales, newest first.
    pub fn pos_sales(&self, business_day: &str) -> Result<Vec<arena_proto::PosSaleView>, AppError> {
        if !self.table_exists("orders")? || !self.table_exists("shifts")? {
            return Ok(Vec::new());
        }
        list_pos_sales(&self.conn, business_day)
    }

    /// What the credit note `refund_id` is printed from (`None`: no such refund).
    pub fn credit_note_source(
        &self,
        refund_id: &str,
    ) -> Result<Option<CreditNoteSource>, AppError> {
        if !self.table_exists("refunds")? {
            return Ok(None);
        }
        load_credit_note_source(&self.conn, refund_id)
    }

    /// The invoice numbered `invoice_no`, for the Refund screen (`None`: no such settled invoice).
    pub fn refund_invoice(
        &self,
        invoice_no: &str,
    ) -> Result<Option<arena_proto::RefundInvoice>, AppError> {
        if !self.table_exists("refunds")? {
            return Ok(None);
        }
        load_refund_invoice(&self.conn, invoice_no)
    }

    /// How many items each session has had placed on its bill (sessions with none are absent).
    pub fn ordered_counts(&self) -> Result<std::collections::BTreeMap<String, i64>, AppError> {
        if !self.table_exists("orders")? {
            return Ok(std::collections::BTreeMap::new());
        }
        list_ordered_counts(&self.conn)
    }

    pub fn live_segments(&self) -> Result<Vec<SegmentRow>, AppError> {
        if !self.table_exists("session_segments")? {
            return Ok(Vec::new());
        }
        list_segments(&self.conn, None)
    }

    pub fn open_orders(&self) -> Result<(Vec<OpenOrder>, Vec<OrderLineRow>), AppError> {
        if !self.table_exists("orders")? {
            return Ok((Vec::new(), Vec::new()));
        }
        Ok((
            list_open_orders(&self.conn)?,
            list_open_order_lines(&self.conn)?,
        ))
    }

    pub fn wallet_txns(&self, player_id: &str) -> Result<Vec<WalletTxnRow>, AppError> {
        if !self.table_exists("wallet_txns")? {
            return Ok(Vec::new());
        }
        list_wallet_txns(&self.conn, player_id)
    }

    pub fn credit(&self, player_id: &str) -> Result<Option<CreditRow>, AppError> {
        if !self.table_exists("credit_balances")? {
            return Ok(None);
        }
        find_credit(&self.conn, player_id)
    }

    pub fn credit_aging(
        &self,
        player_id: &str,
        today: &str,
    ) -> Result<arena_proto::CreditAging, AppError> {
        credit_aging(&self.conn, player_id, today)
    }

    pub fn credit_debtors(&self) -> Result<Vec<CreditRow>, AppError> {
        if !self.table_exists("credit_balances")? {
            return Ok(Vec::new());
        }
        list_credit_debtors(&self.conn)
    }

    pub fn credit_collection(
        &self,
        order_id: &str,
    ) -> Result<Option<arena_proto::CreditCollection>, AppError> {
        if !self.table_exists("credit_balances")? {
            return Ok(None);
        }
        credit_collection(&self.conn, order_id)
    }

    pub fn wallet_balance(&self, player_id: &str) -> Result<i64, AppError> {
        if !self.table_exists("wallets")? {
            return Ok(0);
        }
        wallet_balance(&self.conn, player_id)
    }

    /// What the player's running wallet sessions have set aside (Story 6.9).
    pub fn reserved_minor(&self, player_id: &str) -> Result<i64, AppError> {
        if !self.table_exists("sessions")? {
            return Ok(0);
        }
        sum_reserved_minor(&self.conn, player_id)
    }

    /// An order's `rev` and total, and when `placement_id` put its lines on it (`None` when the
    /// placement left no lines).
    pub fn placement(
        &self,
        order_id: &str,
        placement_id: &str,
    ) -> Result<Option<(i64, i64, i64)>, AppError> {
        if !self.table_exists("orders")? {
            return Ok(None);
        }
        let Some(order) = find_order(&self.conn, order_id)? else {
            return Ok(None);
        };
        let lines = list_placement_lines(&self.conn, order_id, placement_id)?;
        Ok(lines
            .first()
            .and_then(|l| l.placed_at)
            .map(|at| (order.rev, order.total_minor, at)))
    }

    /// The session an order is billed to (`None` for a counter sale or an unknown order).
    pub fn order_session(&self, order_id: &str) -> Result<Option<String>, AppError> {
        if !self.table_exists("orders")? {
            return Ok(None);
        }
        Ok(find_order(&self.conn, order_id)?.and_then(|o| o.session_id))
    }

    /// The session a line sits on (`None` when the line or its order is gone).
    pub fn line_session(&self, line_id: &str) -> Result<Option<String>, AppError> {
        if !self.table_exists("orders")? {
            return Ok(None);
        }
        Ok(find_line(&self.conn, line_id)?
            .and_then(|l| find_order(&self.conn, &l.order_id).transpose())
            .transpose()?
            .and_then(|o| o.session_id))
    }

    /// The kind of the intent that wrote `intent_id`'s changes.
    pub fn intent_kind(&self, intent_id: &str) -> Result<Option<String>, AppError> {
        forward_kind(&self.conn, intent_id)
    }

    /// The session and order a placement put products on (`None` when it left no lines).
    pub fn placed_order(&self, placement_id: &str) -> Result<Option<(String, String)>, AppError> {
        if !self.table_exists("orders")? {
            return Ok(None);
        }
        find_placed_order(&self.conn, placement_id)
    }

    pub fn settlement(&self, order_id: &str) -> Result<Option<Settlement>, AppError> {
        if !self.table_exists("orders")? {
            return Ok(None);
        }
        find_settlement(&self.conn, order_id)
    }

    /// Whether the order is a walk-up product sale (`pos_sale`).
    pub fn is_pos_sale(&self, order_id: &str) -> Result<bool, AppError> {
        if !self.table_exists("orders")? {
            return Ok(false);
        }
        self.conn
            .query_row(
                "SELECT 1 FROM orders WHERE id = ?1 AND kind = 'pos_sale'",
                [order_id],
                |_| Ok(()),
            )
            .optional()
            .map(|r| r.is_some())
            .map_err(|e| AppError::new(ErrorCode::Internal, format!("db error: {e}")))
    }

    pub fn ending(&self, session_id: &str) -> Result<Option<Ending>, AppError> {
        if !self.table_exists("irreversible_effects")? {
            return Ok(None);
        }
        find_ending(&self.conn, session_id)
    }

    pub fn latest_consent(
        &self,
        player_id: &str,
        purpose: &str,
    ) -> Result<Option<(bool, String)>, AppError> {
        if !self.table_exists("consents")? {
            return Ok(None);
        }
        latest_consent(&self.conn, player_id, purpose)
    }

    pub fn lots(&self, product_id: &str) -> Result<Vec<LotRow>, AppError> {
        if !self.table_exists("stock_lots")? {
            return Ok(Vec::new());
        }
        list_lots(&self.conn, product_id)
    }

    /// Lots with stock left that expire on or before `until` (with the product name).
    pub fn expiring_lots(&self, until: &str) -> Result<Vec<(LotRow, String)>, AppError> {
        if !self.table_exists("stock_lots")? {
            return Ok(Vec::new());
        }
        list_expiring_lots(&self.conn, until)
    }

    /// How many units of a product are in lots that expired before `today`.
    pub fn expired_qty(&self, product_id: &str, today: &str) -> Result<i64, AppError> {
        if !self.table_exists("stock_lots")? {
            return Ok(0);
        }
        expired_qty(&self.conn, product_id, today)
    }

    /// Settled sales without an invoice number, oldest settle first (see `list_unnumbered`).
    pub fn unnumbered_orders(&self, limit: i64) -> Result<Vec<(String, i64)>, AppError> {
        if !self.table_exists("invoice_sequences")? {
            return Ok(Vec::new());
        }
        list_unnumbered(&self.conn, limit)
    }

    /// Receipts waiting to print on the side-effect outbox, oldest first (Story 5.17).
    pub fn queued_prints(&self, limit: i64) -> Result<Vec<OutboxRow>, AppError> {
        if !self.table_exists("side_effect_outbox")? {
            return Ok(Vec::new());
        }
        list_queued_outbox(&self.conn, KIND_PRINT, limit)
    }

    /// Drawer kicks waiting on the side-effect outbox, oldest first (Story 5.18).
    pub fn queued_drawers(&self, limit: i64) -> Result<Vec<OutboxRow>, AppError> {
        if !self.table_exists("side_effect_outbox")? {
            return Ok(Vec::new());
        }
        list_queued_outbox(&self.conn, KIND_DRAWER, limit)
    }

    /// The committed rows a receipt is built from; `None` when there is no such order.
    pub fn receipt_source(&self, order_id: &str) -> Result<Option<ReceiptSource>, AppError> {
        if !self.table_exists("orders")? {
            return Ok(None);
        }
        load_receipt_source(&self.conn, order_id)
    }

    /// How many units of a product open holds claim (kitchen lines not yet served).
    pub fn held_qty(&self, product_id: &str) -> Result<i64, AppError> {
        if !self.table_exists("stock_holds")? {
            return Ok(0);
        }
        open_hold_qty(&self.conn, product_id)
    }

    pub fn moves(&self, product_id: &str) -> Result<Vec<MoveRow>, AppError> {
        if !self.table_exists("stock_moves")? {
            return Ok(Vec::new());
        }
        list_moves(&self.conn, product_id)
    }

    pub fn player_packs(&self, player_id: &str) -> Result<Vec<PlayerPackRow>, AppError> {
        if !self.table_exists("player_packs")? {
            return Ok(Vec::new());
        }
        list_player_packs(&self.conn, player_id)
    }

    pub fn find_pack_for_order(&self, order_id: &str) -> Result<Option<PlayerPackRow>, AppError> {
        find_pack_for_order(&self.conn, order_id)
    }

    /// The reveal for a stated count against the open shift (Story 5.26), or `None` with no
    /// shift open. The expected cash is never read without a count.
    pub fn shift_count(
        &self,
        counted_minor: i64,
    ) -> Result<Option<arena_proto::ShiftCount>, AppError> {
        if !self.table_exists("shifts")? {
            return Ok(None);
        }
        match open_shift_id(&self.conn)? {
            Some(id) => Ok(Some(shifts::shift_count(&self.conn, &id, counted_minor)?)),
            None => Ok(None),
        }
    }

    /// The rows Daily sales is built from, for the business days `from` to `to` (Story 5.27), in
    /// one read snapshot. An older database without the tables reads as no sales.
    pub fn sales_report_rows(&self, from: &str, to: &str) -> Result<SalesReportRows, AppError> {
        if !self.table_exists("orders")? || !self.table_exists("device_groups")? {
            return Ok(SalesReportRows::default());
        }
        let refunds = self.table_exists("refunds")?;
        self.read_consistent(|s| {
            Ok(SalesReportRows {
                orders: reports::sales_orders(&s.conn, from, to)?,
                lines: reports::sales_lines(&s.conn, from, to)?,
                received: reports::received(&s.conn, from, to)?,
                refunded_minor: if refunds {
                    reports::refunded(&s.conn, from, to)?
                } else {
                    0
                },
                groups: reports::group_names(&s.conn)?,
            })
        })
    }

    /// The rows Sessions is built from, for the business days `from` to `to` (Story 5.27); an open
    /// time slice runs to `now_ms`.
    pub fn sessions_report_rows(
        &self,
        from: &str,
        to: &str,
        now_ms: i64,
    ) -> Result<SessionsReportRows, AppError> {
        if !self.table_exists("sessions")? || !self.table_exists("session_segments")? {
            return Ok(SessionsReportRows::default());
        }
        self.read_consistent(|s| {
            Ok(SessionsReportRows {
                sessions: reports::session_rows(&s.conn, from, to)?,
                spans: reports::segment_spans(&s.conn, from, to, now_ms)?,
                devices: reports::report_devices(&s.conn)?,
                groups: reports::group_names(&s.conn)?,
            })
        })
    }

    /// The rows the Players report is built from, for the business days `from` to `to` (Story
    /// 5.30), in one read snapshot; credit is aged as of the business day `today`. An older
    /// database without the tables reads as no players.
    pub fn players_report_rows(
        &self,
        from: &str,
        to: &str,
        today: &str,
    ) -> Result<PlayersReportRows, AppError> {
        if !self.table_exists("orders")?
            || !self.table_exists("players")?
            || !self.table_exists("player_keys")?
        {
            return Ok(PlayersReportRows::default());
        }
        let credit = self.table_exists("credit_balances")? && self.table_exists("wallet_txns")?;
        self.read_consistent(|s| {
            Ok(PlayersReportRows {
                players: reports::player_sales(&s.conn, from, to)?,
                guest: reports::guest_sales(&s.conn, from, to)?,
                debtors: if credit {
                    reports::debtors(&s.conn, today)?
                } else {
                    Vec::new()
                },
            })
        })
    }

    /// The rows the Staff report is built from, for the business days `from` to `to` (Story
    /// 5.30), in one read snapshot. Shifts are read from `shift_from_ms` to `shift_to_ms`
    /// (a slack around the range; the fold keeps the ones that closed on a day in the range).
    pub fn staff_report_rows(
        &self,
        from: &str,
        to: &str,
        shift_from_ms: i64,
        shift_to_ms: i64,
    ) -> Result<StaffReportRows, AppError> {
        if !self.table_exists("orders")? || !self.table_exists("staff")? {
            return Ok(StaffReportRows::default());
        }
        let refunds = self.table_exists("refunds")?;
        let sessions = self.table_exists("sessions")?;
        let shifts = self.table_exists("shifts")?;
        self.read_consistent(|s| {
            Ok(StaffReportRows {
                sales: reports::staff_sales(&s.conn, from, to)?,
                sessions: if sessions {
                    reports::staff_sessions(&s.conn, from, to)?
                } else {
                    Vec::new()
                },
                discounts: reports::staff_discounts(&s.conn, from, to)?,
                voids: reports::staff_voids(&s.conn, from, to)?,
                refunds: if refunds {
                    reports::staff_refunds(&s.conn, from, to)?
                } else {
                    Vec::new()
                },
                shifts: if shifts {
                    reports::closed_shift_variances(&s.conn, shift_from_ms, shift_to_ms)?
                } else {
                    Vec::new()
                },
                names: reports::staff_names(&s.conn)?,
            })
        })
    }

    /// Every tracked product with what is on hand (Story 5.31), in one read snapshot. An older
    /// database without the tables reads as no stock.
    pub fn stock_levels(&self) -> Result<Vec<StockLevelRow>, AppError> {
        if !self.table_exists("products")? || !self.table_exists("stock")? {
            return Ok(Vec::new());
        }
        self.read_consistent(|s| stock_reports::stock_levels(&s.conn))
    }

    /// The stock moves with `at` from `from_ms` to `to_ms` (UTC ms, inclusive), newest first, with
    /// the staff names (Story 5.31). `product_id` and `reason` narrow them; `waste_only` keeps the
    /// waste moves and expired write-offs.
    pub fn stock_moves_rows(
        &self,
        from_ms: i64,
        to_ms: i64,
        product_id: Option<&str>,
        reason: Option<&str>,
        waste_only: bool,
    ) -> Result<StockMovesRows, AppError> {
        if !self.table_exists("stock_moves")? || !self.table_exists("staff")? {
            return Ok(StockMovesRows::default());
        }
        self.read_consistent(|s| {
            Ok(StockMovesRows {
                moves: stock_reports::stock_moves_between(
                    &s.conn, from_ms, to_ms, product_id, reason, waste_only,
                )?,
                staff: reports::staff_names(&s.conn)?,
            })
        })
    }

    /// The shift `id` (open or closed) with its Z-report, if one was stored (Story 5.28).
    pub fn shift_detail(&self, id: &str) -> Result<Option<ShiftDetail>, AppError> {
        if !self.table_exists("shifts")? {
            return Ok(None);
        }
        find_shift(&self.conn, id)
    }

    /// Closed shifts, newest first, for the Shifts report (Story 5.28).
    pub fn closed_shifts(&self, limit: i64) -> Result<Vec<ShiftDetail>, AppError> {
        if !self.table_exists("shifts")? {
            return Ok(Vec::new());
        }
        list_closed_shifts(&self.conn, limit)
    }

    /// The rows the Z-report of the shift `id` is built from; the window runs from its opening to
    /// its `closed_at`. `None` when there is no such shift or it is still open.
    pub fn z_report_rows(&self, id: &str) -> Result<Option<ZReportRows>, AppError> {
        if !self.table_exists("shifts")? || !self.table_exists("orders")? {
            return Ok(None);
        }
        self.read_consistent(|s| {
            let Some(shift) = find_shift(&s.conn, id)? else {
                return Ok(None);
            };
            let Some(closed_at) = shift.closed_at.filter(|_| !shift.open) else {
                return Ok(None);
            };
            Ok(Some(load_z_report_rows(&s.conn, shift, closed_at)?))
        })
    }

    /// The rows the GSTR-1 export of the business days `from` to `to` is built from (Story 5.29),
    /// in one read snapshot. An older database without the tables reads as an empty month.
    pub fn gst_rows(&self, from: &str, to: &str) -> Result<GstRows, AppError> {
        if !self.table_exists("orders")? || !self.table_exists("refunds")? {
            return Ok(GstRows::default());
        }
        self.read_consistent(|s| load_gst_rows(&s.conn, from, to))
    }

    /// The branch's open shift and the float the next one would offer, with the number of cash
    /// events so far. An older database without the tables reads as no shift and a float of 0.
    pub fn shift_state(&self) -> Result<arena_proto::ShiftState, AppError> {
        if !self.table_exists("shifts")? {
            return Ok(arena_proto::ShiftState {
                open: None,
                carried_minor: 0,
            });
        }
        let open = match open_shift(&self.conn)? {
            Some(s) => Some(arena_proto::SnapshotShift {
                events: count_cash_events(&self.conn, &s.id)?,
                id: s.id,
                staff_id: s.staff_id,
                staff_name: s.staff_name,
                opened_at: s.opened_at,
                float_minor: s.float_minor,
            }),
            None => None,
        };
        Ok(arena_proto::ShiftState {
            open,
            carried_minor: last_opening_float(&self.conn)?,
        })
    }

    /// Group bills still open or unsettled.
    pub fn group_bills(&self) -> Result<Vec<GroupBillRow>, AppError> {
        if !self.table_exists("group_bills")? {
            return Ok(Vec::new());
        }
        list_active_group_bills(&self.conn)
    }

    /// Ended sessions of open group bills, with their segments.
    pub fn ended_group_sessions(&self) -> Result<(Vec<LiveSession>, Vec<SegmentRow>), AppError> {
        if !self.table_exists("group_bills")? {
            return Ok((Vec::new(), Vec::new()));
        }
        Ok((
            list_ended_group_sessions(&self.conn)?,
            list_ended_group_segments(&self.conn)?,
        ))
    }

    /// Runs `f` with the read connection inside one read snapshot, for reads that go through the
    /// shared `apply()` query functions (a coupon preview).
    pub fn read_with<R>(
        &self,
        f: impl FnOnce(&Connection) -> Result<R, AppError>,
    ) -> Result<R, AppError> {
        self.read_consistent(|s| f(&s.conn))
    }

    /// Every coupon with its targets (empty before migration 54).
    pub fn coupons(&self) -> Result<Vec<CouponWithTargets>, AppError> {
        if !self.table_exists("coupons")? {
            return Ok(Vec::new());
        }
        list_coupons(&self.conn)?
            .into_iter()
            .map(|c| {
                let targets = list_coupon_targets(&self.conn, &c.coupon_id)?;
                Ok((c, targets))
            })
            .collect()
    }

    /// Every live paired device with its last scan summary (empty before migration 63).
    pub fn kiosk_scan_summaries(&self) -> Result<Vec<KioskScanSummary>, AppError> {
        if !self.table_exists("kiosk_scans")? {
            return Ok(Vec::new());
        }
        list_kiosk_scan_summaries(&self.conn)
    }

    /// Every device's stored scan, ordered by device id (empty before migration 63).
    pub fn kiosk_scans_full(&self) -> Result<Vec<KioskScanRow>, AppError> {
        if !self.table_exists("kiosk_scans")? {
            return Ok(Vec::new());
        }
        list_kiosk_scans(&self.conn)
    }

    /// Every device's catalogue matches (empty before migration 64).
    pub fn game_matches(&self) -> Result<Vec<GameMatchesRow>, AppError> {
        if !self.table_exists("game_matches")? {
            return Ok(Vec::new());
        }
        list_game_matches(&self.conn)
    }

    /// The install paths of live library entries (empty before migration 64).
    pub fn library_installs(&self) -> Result<Vec<InstallRow>, AppError> {
        if !self.table_exists("library_installs")? {
            return Ok(Vec::new());
        }
        list_library_installs(&self.conn)
    }

    /// The dismissed proposal keys (empty before migration 64).
    pub fn library_dismissals(&self) -> Result<Vec<String>, AppError> {
        if !self.table_exists("library_dismissals")? {
            return Ok(Vec::new());
        }
        list_library_dismissals(&self.conn)
    }

    /// Every live library entry, ordered by name (empty before migration 62).
    pub fn library_entries(&self) -> Result<Vec<LibraryRow>, AppError> {
        if !self.table_exists("library_entries")? {
            return Ok(Vec::new());
        }
        list_library_entries(&self.conn)
    }

    pub fn pricing_rules(&self) -> Result<Vec<PricingRuleRow>, AppError> {
        if !self.table_exists("pricing_rules")? {
            return Ok(Vec::new());
        }
        list_pricing_rules(&self.conn)
    }

    pub fn plans(&self) -> Result<Vec<(PlanRow, Vec<String>)>, AppError> {
        if !self.table_exists("plans")? {
            return Ok(Vec::new());
        }
        list_plans(&self.conn)?
            .into_iter()
            .map(|p| {
                let groups = list_plan_groups(&self.conn, &p.plan_id)?;
                Ok((p, groups))
            })
            .collect()
    }

    pub fn products(&self) -> Result<Vec<ProductRow>, AppError> {
        if !self.table_exists("products")? {
            return Ok(Vec::new());
        }
        list_products(&self.conn)
    }

    pub fn change_actor_kind(&self, epoch: &str, seq: i64) -> Result<Option<String>, AppError> {
        devices::change_actor_kind(&self.conn, epoch, seq)
    }

    pub fn device_meta(&self, device_id: &str) -> Result<Option<DeviceMetaRow>, AppError> {
        if !devices_meta_exists(&self.conn)? {
            return Ok(None);
        }
        find_device_meta(&self.conn, device_id)
    }

    /// How many `applied_intents` (dedupe) records exist, outside any write transaction.
    pub fn applied_intent_count(&self) -> Result<i64, AppError> {
        count_applied_intents(&self.conn)
    }

    pub fn has_applied_intent(&self, idempotency_key: &str) -> Result<bool, AppError> {
        Ok(dedupe::find_applied_intent(&self.conn, idempotency_key)?.is_some())
    }

    /// Takes the process-level writer lock (`<db>.lock`), so two processes on one data dir can
    /// never both write. Returns `false` when another process holds it. The OS releases it when
    /// this process exits, however it exits. [`Store::open`] already tries once; this retries.
    pub fn try_lock_writer(&mut self) -> Result<bool, AppError> {
        if self.writer_lock.is_some() {
            return Ok(true);
        }
        let mut lock_path = self.path.clone().into_os_string();
        lock_path.push(".lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(PathBuf::from(lock_path))
            .map_err(io_error)?;
        if !try_lock_file(&file).map_err(io_error)? {
            return Ok(false);
        }
        self.writer_lock = Some(file);
        Ok(true)
    }

    /// Whether this store holds the process writer lock (taken by [`Store::open`]).
    pub fn holds_writer_lock(&self) -> bool {
        self.writer_lock.is_some()
    }

    /// Reads one `hub_meta` value (`None` on a database not yet migrated).
    pub fn meta(&self, key: &str) -> Result<Option<String>, AppError> {
        if self.schema_version()? == 0 {
            return Ok(None);
        }
        get_meta(&self.conn, key)
    }

    pub fn hub_identity(&self) -> Result<Option<HubIdentity>, AppError> {
        if self.schema_version()? == 0 {
            return Ok(None);
        }
        find_hub_identity(&self.conn)
    }

    pub fn trusted_clock(&self) -> Result<Option<TrustedClockRow>, AppError> {
        if self.schema_version()? == 0 {
            return Ok(None);
        }
        find_trusted_clock(&self.conn)
    }

    pub fn cached_licence(&self) -> Result<Option<CachedLicence>, AppError> {
        if self.schema_version()? == 0 {
            return Ok(None);
        }
        find_cached_licence(&self.conn)
    }

    /// Reads the lease row outside any write transaction (`None` on a database not yet
    /// migrated).
    pub fn lease(&self) -> Result<Option<LeaseRow>, AppError> {
        if self.schema_version()? == 0 {
            return Ok(None);
        }
        read_lease(&self.conn)
    }
}

/// Inserts a `schema_version` row. Used by migrations (and by tests that fake a newer schema).
pub fn record_schema_version(
    conn: &Connection,
    version: u32,
    applied_at: i64,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO schema_version (version, applied_at) VALUES (?1, ?2)",
        params![version, applied_at],
    )
    .map_err(db_error)?;
    Ok(())
}

fn schema_too_new(found: u32) -> AppError {
    AppError::new(
        ErrorCode::SchemaTooNew,
        format!(
            "database schema version {found} is newer than this binary's version {LATEST_SCHEMA_VERSION}"
        ),
    )
}

/// The highest `schema_version`, or 0 for an empty database. Read-only.
fn current_version(conn: &Connection) -> Result<u32, AppError> {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_version')",
            [],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if !exists {
        return Ok(0);
    }
    let v: Option<i64> = conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))
        .map_err(db_error)?;
    Ok(v.unwrap_or(0).clamp(0, i64::from(u32::MAX)) as u32)
}

/// Reads one `hub_meta` value.
pub fn get_meta(conn: &Connection, key: &str) -> Result<Option<String>, AppError> {
    conn.query_row("SELECT value FROM hub_meta WHERE key = ?1", [key], |r| {
        r.get::<_, Option<String>>(0)
    })
    .optional()
    .map(Option::flatten)
    .map_err(db_error)
}

/// Inserts `value` for `key` unless a value exists; returns the stored value either way.
pub fn insert_meta_if_absent(
    conn: &Connection,
    key: &str,
    value: &str,
) -> Result<String, AppError> {
    conn.execute(
        "INSERT INTO hub_meta (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO NOTHING",
        params![key, value],
    )
    .map_err(db_error)?;
    get_meta(conn, key)?.ok_or_else(|| {
        AppError::new(
            ErrorCode::Internal,
            format!("hub_meta {key} missing after insert"),
        )
    })
}

/// Sets `key` to `value`, replacing any existing value.
pub fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO hub_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Reads the lease row, if any.
pub fn read_lease(conn: &Connection) -> Result<Option<LeaseRow>, AppError> {
    conn.query_row(
        "SELECT counter, hub_instance_id, provisional, renewed_at, expires_at FROM lease WHERE id = 1",
        [],
        |r| {
            Ok(LeaseRow {
                counter: r.get(0)?,
                hub_instance_id: r.get(1)?,
                provisional: r.get::<_, i64>(2)? != 0,
                renewed_at: r.get(3)?,
                expires_at: r.get(4)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

/// Deletes the lease row (used by tests and by a later manual-promotion flow).
pub fn delete_lease(conn: &Connection) -> Result<(), AppError> {
    conn.execute("DELETE FROM lease WHERE id = 1", [])
        .map_err(db_error)?;
    Ok(())
}

/// Writes (inserts or replaces) the lease row.
pub fn write_lease(conn: &Connection, row: &LeaseRow) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO lease (id, counter, hub_instance_id, provisional, renewed_at, expires_at)
         VALUES (1, ?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (id) DO UPDATE SET
           counter = excluded.counter,
           hub_instance_id = excluded.hub_instance_id,
           provisional = excluded.provisional,
           renewed_at = excluded.renewed_at,
           expires_at = excluded.expires_at",
        params![
            row.counter,
            row.hub_instance_id,
            i64::from(row.provisional),
            row.renewed_at,
            row.expires_at
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: i64 = 1_800_000_000_000;

    fn temp_db() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        (dir, path)
    }

    #[test]
    fn first_start_creates_file_with_wal_full_and_migrations() {
        let (_dir, path) = temp_db();
        assert!(!path.exists());
        let store = Store::open(&path, T).unwrap();
        assert!(path.exists());
        assert_eq!(store.journal_mode().unwrap(), "wal");
        assert_eq!(store.synchronous().unwrap(), 2);
        assert_eq!(store.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
        let rows: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, migrations::MIGRATIONS.len() as i64);
        assert_eq!(store.lease().unwrap(), None);
    }

    #[test]
    fn restart_does_not_reapply_migrations() {
        let (_dir, path) = temp_db();
        let first = Store::open(&path, T).unwrap();
        let applied: i64 = first
            .conn
            .query_row(
                "SELECT applied_at FROM schema_version WHERE version = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        drop(first);
        let again = Store::open(&path, T).unwrap();
        let (count, applied_again): (i64, i64) = again
            .conn
            .query_row(
                "SELECT COUNT(*), MAX(applied_at) FROM schema_version",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, migrations::MIGRATIONS.len() as i64);
        assert_eq!(applied_again, applied);
    }

    #[test]
    fn schema_too_new_fails_and_leaves_file_untouched() {
        let (_dir, path) = temp_db();
        {
            let store = Store::open(&path, T).unwrap();
            record_schema_version(&store.conn, 999, 0).unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        let err = Store::open(&path, T).unwrap_err();
        assert_eq!(err.code, ErrorCode::SchemaTooNew);
        assert!(err.message.contains("999"), "{}", err.message);
        assert!(
            err.message.contains(&LATEST_SCHEMA_VERSION.to_string()),
            "{}",
            err.message
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn meta_insert_if_absent_keeps_first_value() {
        let (_dir, path) = temp_db();
        let mut store = Store::open(&path, T).unwrap();
        let a = store
            .write_tx(|tx| insert_meta_if_absent(tx, META_HUB_INSTANCE_ID, "A"))
            .unwrap();
        let b = store
            .write_tx(|tx| insert_meta_if_absent(tx, META_HUB_INSTANCE_ID, "B"))
            .unwrap();
        assert_eq!((a.as_str(), b.as_str()), ("A", "A"));
        assert_eq!(
            store.meta(META_HUB_INSTANCE_ID).unwrap().as_deref(),
            Some("A")
        );
    }

    #[test]
    fn lease_round_trips_and_failed_tx_rolls_back() {
        let (_dir, path) = temp_db();
        let mut store = Store::open(&path, T).unwrap();
        let row = LeaseRow {
            counter: 1,
            hub_instance_id: "X".into(),
            provisional: true,
            renewed_at: 10,
            expires_at: 20,
        };
        store.write_tx(|tx| write_lease(tx, &row)).unwrap();
        assert_eq!(store.lease().unwrap(), Some(row.clone()));
        let err = store
            .write_tx(|tx| {
                write_lease(
                    tx,
                    &LeaseRow {
                        counter: 9,
                        ..row.clone()
                    },
                )?;
                Err::<(), _>(AppError::new(ErrorCode::Stale, "abort"))
            })
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Stale);
        assert_eq!(store.lease().unwrap(), Some(row));
    }

    #[cfg(unix)]
    #[test]
    fn flock_excludes_a_second_handle_until_the_first_closes() {
        let (_dir, path) = temp_db();
        let lock = path.with_extension("flock");
        let a = File::create(&lock).unwrap();
        let b = File::open(&lock).unwrap();
        assert!(flock_try(&a).unwrap());
        assert!(!flock_try(&b).unwrap());
        drop(a);
        assert!(flock_try(&b).unwrap());
    }

    fn hold_lock(path: &Path) -> File {
        let mut lock = path.as_os_str().to_owned();
        lock.push(".lock");
        let f = File::create(PathBuf::from(lock)).unwrap();
        f.try_lock().unwrap();
        f
    }

    #[test]
    fn applied_at_comes_from_the_caller() {
        let (_dir, path) = temp_db();
        let store = Store::open(&path, T).unwrap();
        let applied: i64 = store
            .conn
            .query_row("SELECT applied_at FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(applied, T);
    }

    #[test]
    fn follower_without_lock_never_migrates_a_new_file() {
        let (_dir, path) = temp_db();
        let _held = hold_lock(&path);
        let store = Store::open(&path, T).unwrap();
        assert!(!store.holds_writer_lock());
        assert_eq!(store.schema_version().unwrap(), 0);
        assert_eq!(store.lease().unwrap(), None);
        assert_eq!(store.meta(META_HUB_INSTANCE_ID).unwrap(), None);
        let tables: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tables, 0);
    }

    #[test]
    fn follower_without_lock_leaves_existing_file_untouched() {
        let (_dir, path) = temp_db();
        drop(Store::open(&path, T).unwrap());
        let before = std::fs::read(&path).unwrap();
        let _held = hold_lock(&path);
        let store = Store::open(&path, T + 1).unwrap();
        assert!(!store.holds_writer_lock());
        assert_eq!(store.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
        drop(store);
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn v1_database_upgrades_to_latest_keeping_the_lease() {
        let (_dir, path) = temp_db();
        let lease = LeaseRow {
            counter: 3,
            hub_instance_id: "01JV1".into(),
            provisional: false,
            renewed_at: 10,
            expires_at: 20,
        };
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(migrations::MIGRATIONS[0].sql).unwrap();
            record_schema_version(&conn, 1, 0).unwrap();
            write_lease(&conn, &lease).unwrap();
        }
        let store = Store::open(&path, T).unwrap();
        assert_eq!(store.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
        for table in [
            "branch",
            "settings",
            "changes",
            "applied_intents",
            "devices",
            "roles",
            "role_perms",
            "staff",
        ] {
            let exists: bool = store
                .conn
                .query_row(
                    "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(exists, "{table}");
        }
        assert_eq!(store.lease().unwrap(), Some(lease));
    }

    #[test]
    fn v19_database_upgrades_to_stock_and_moves_stay_append_only() {
        let (_dir, path) = temp_db();
        {
            let conn = Connection::open(&path).unwrap();
            for m in &migrations::MIGRATIONS[..19] {
                conn.execute_batch(m.sql).unwrap();
                record_schema_version(&conn, m.version, 0).unwrap();
            }
        }
        let mut store = Store::open(&path, T).unwrap();
        assert_eq!(store.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
        store
            .write_tx(|tx| {
                tx.execute_batch(
                    "INSERT INTO products (product_id, name, category, price_minor, tax_rate_bp, hsn,
                         track_stock, image, barcode, kitchen, favourite, active, bundle, rev,
                         deleted_at, created_at, updated_at)
                     VALUES ('P', 'P', '', 1, 0, '', 1, '', NULL, 0, 0, 1, 0, 1, NULL, 0, 0);
                     INSERT INTO stock_moves (move_id, product_id, reason, delta, before_qty,
                         before_avg_cost_minor, unit_cost_minor, supplier, lot_id, lots_used,
                         staff_id, at, rev, created_at, updated_at)
                     VALUES ('M', 'P', 'stock_in', 1, 0, 0, 1, '', NULL, '[]', 's', 0, 1, 0, 0);
                     UPDATE stock_moves SET rev = 2, updated_at = 5;",
                )
                .map_err(db_error)?;
                for sql in [
                    "UPDATE stock_moves SET delta = 2",
                    "UPDATE stock_moves SET lots_used = '[1]'",
                    "DELETE FROM stock_moves",
                ] {
                    assert!(tx.execute(sql, []).is_err(), "{sql}");
                }
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn v23_database_upgrades_to_players_and_consents_stay_append_only() {
        let (_dir, path) = temp_db();
        {
            let conn = Connection::open(&path).unwrap();
            for m in &migrations::MIGRATIONS[..23] {
                conn.execute_batch(m.sql).unwrap();
                record_schema_version(&conn, m.version, 0).unwrap();
            }
        }
        let mut store = Store::open(&path, T).unwrap();
        assert_eq!(store.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
        let player = |id: &str, username: &str, phone: &str| PlayerRow {
            player_id: id.into(),
            username: username.into(),
            password_hash: "h".into(),
            phone_lookup: phone.into(),
            name_enc: "n".into(),
            phone_enc: "p".into(),
            email_enc: None,
            dob_enc: None,
            notes_enc: None,
            tags: "[]".into(),
        };
        store
            .write_tx(|tx| {
                insert_player(tx, &player("A", "asha", "L1"))?;
                insert_consent(tx, "C", "A", "transactional", "1", true, 5)?;
                insert_player_key(tx, "A", "k")?;
                assert!(username_taken(tx, "asha")? && !username_taken(tx, "ravi")?);
                assert!(phone_holder(tx, "L1")?.is_some() && phone_holder(tx, "L2")?.is_none());
                assert_eq!(find_player_key(tx, "A")?.as_deref(), Some("k"));
                assert!(insert_player(tx, &player("B", "asha", "L2")).is_err());
                assert!(insert_player(tx, &player("B", "ravi", "L1")).is_err());
                tx.execute_batch("UPDATE consents SET rev = 2, updated_at = 5;")
                    .map_err(db_error)?;
                for sql in [
                    "UPDATE consents SET granted = 0",
                    "UPDATE consents SET policy_version = '2'",
                    "DELETE FROM consents",
                ] {
                    assert!(tx.execute(sql, []).is_err(), "{sql}");
                }
                tx.execute_batch("UPDATE players SET deleted_at = 9")
                    .map_err(db_error)?;
                insert_player(tx, &player("B", "asha", "L1"))
            })
            .unwrap();
    }

    #[test]
    fn v2_database_upgrades_to_v3_keeping_its_rows() {
        let (_dir, path) = temp_db();
        {
            let conn = Connection::open(&path).unwrap();
            for m in &migrations::MIGRATIONS[..2] {
                conn.execute_batch(m.sql).unwrap();
                record_schema_version(&conn, m.version, 0).unwrap();
            }
            insert_branch(
                &conn,
                &NewBranch {
                    branch_id: "B",
                    name: "N",
                    branch_code: "C",
                    currency: "INR",
                    tz: "UTC",
                    day_start_min: 0,
                    fy_start_month: 1,
                },
            )
            .unwrap();
        }
        let store = Store::open(&path, T).unwrap();
        assert_eq!(store.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
        assert_eq!(store.rows(Table::Branch).unwrap().len(), 1);
        let index: bool = store
            .conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'index'
                                AND name = 'applied_intents_applied_at')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(index);
        assert_eq!(count_applied_intents(&store.conn).unwrap(), 0);
    }

    #[test]
    fn v18_database_upgrades_to_v19_keeping_products_as_plain_items() {
        let (_dir, path) = temp_db();
        {
            let conn = Connection::open(&path).unwrap();
            for m in &migrations::MIGRATIONS[..18] {
                conn.execute_batch(m.sql).unwrap();
                record_schema_version(&conn, m.version, 0).unwrap();
            }
            conn.execute(
                "INSERT INTO products (product_id, name, category, price_minor, tax_rate_bp, hsn,
                     track_stock, image, barcode, kitchen, favourite, active, rev, created_at, updated_at)
                 VALUES ('P', 'Cola', '', 100, 0, '', 1, '', NULL, 0, 0, 1, 1, 0, 0)",
                [],
            )
            .unwrap();
        }
        let store = Store::open(&path, T).unwrap();
        assert_eq!(store.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
        let p = store.product("P").unwrap().unwrap();
        assert!(!p.bundle);
        assert!(store.rows(Table::BundleComponents).unwrap().is_empty());
        assert!(store.components("P").unwrap().is_empty());
        assert!(store.bundles_with("P").unwrap().is_empty());
    }

    #[test]
    fn v6_database_upgrades_to_v7_keeping_changes_and_triggers() {
        let (_dir, path) = temp_db();
        {
            let conn = Connection::open(&path).unwrap();
            for m in &migrations::MIGRATIONS[..6] {
                conn.execute_batch(m.sql).unwrap();
                record_schema_version(&conn, m.version, 0).unwrap();
            }
            conn.execute(
                "INSERT INTO changes (epoch, seq, tbl, row_id, op, row, intent_id, intent_kind,
                                      actor_kind, actor_id, at, business_day, fy)
                 VALUES ('1:A', 1, 'branch', 'B', 'insert', x'80', 'I', 'branch.bootstrap',
                         'system', 't', 5, '2027-01-01', 'FY2026-27')",
                [],
            )
            .unwrap();
        }
        let mut store = Store::open(&path, T).unwrap();
        assert_eq!(store.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
        let changes = store.changes().unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].business_day.as_deref(), Some("2027-01-01"));
        let err = store
            .write_tx(|tx| tx.execute("DELETE FROM changes", []).map_err(db_error))
            .unwrap_err();
        assert!(err.message.contains("append-only"), "{err}");
    }

    #[test]
    fn foreign_keys_are_on_and_orphan_settings_fail() {
        let (_dir, path) = temp_db();
        let mut store = Store::open(&path, T).unwrap();
        let fk: i64 = store
            .conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
        assert!(
            store
                .write_tx(|tx| insert_setting(tx, "no-such-branch", "k", "v"))
                .is_err()
        );
    }

    #[test]
    fn reader_never_locks_or_writes() {
        let (_dir, path) = temp_db();
        drop(Store::open(&path, T).unwrap());
        let reader = Store::open_reader(&path).unwrap();
        assert!(!reader.holds_writer_lock());
        assert_eq!(reader.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
        assert!(reader.devices().unwrap().is_empty());
        let mut writer = Store::open(&path, T).unwrap();
        assert!(writer.holds_writer_lock());
        writer
            .write_tx(|tx| set_meta(tx, META_TLS_FINGERPRINT, "ab"))
            .unwrap();
        assert_eq!(
            reader.meta(META_TLS_FINGERPRINT).unwrap().as_deref(),
            Some("ab")
        );
        assert!(reader.conn.execute("DELETE FROM hub_meta", []).is_err());
    }

    #[test]
    fn branch_summary_is_none_until_bootstrapped() {
        let (_dir, path) = temp_db();
        let _held = hold_lock(&path);
        let unmigrated = Store::open(&path, T).unwrap();
        assert_eq!(unmigrated.branch_summary().unwrap(), None);
        drop((unmigrated, _held));
        let mut store = Store::open(&path, T).unwrap();
        assert_eq!(store.branch_summary().unwrap(), None);
        store
            .write_tx(|tx| {
                insert_branch(
                    tx,
                    &NewBranch {
                        branch_id: "B",
                        name: "Dev Branch",
                        branch_code: "DEV01",
                        currency: "INR",
                        tz: "UTC",
                        day_start_min: 0,
                        fy_start_month: 1,
                    },
                )
            })
            .unwrap();
        let reader = Store::open_reader(&path).unwrap();
        let b = reader.read_consistent(|r| r.branch_summary()).unwrap();
        // The read transaction ended: the reader is usable again.
        assert_eq!(reader.devices().unwrap(), vec![]);
        assert_eq!(
            b,
            Some(BranchSummary {
                branch_id: "B".into(),
                name: "Dev Branch".into(),
                branch_code: "DEV01".into(),
                code_locked: false,
            })
        );
    }

    #[test]
    fn v31_database_upgrades_to_credit_collect_orders_keeping_its_children() {
        let (_dir, path) = temp_db();
        {
            let conn = Connection::open(&path).unwrap();
            for m in &migrations::MIGRATIONS[..31] {
                conn.execute_batch(m.sql).unwrap();
                record_schema_version(&conn, m.version, 0).unwrap();
            }
            conn.execute_batch(
                "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy,
                                     kind, rev, created_at, updated_at)
                 VALUES ('O1', 'settled', 100, 100, '2026-10-27', '2026-27', 'topup', 1, 0, 0);
                 INSERT INTO payments (id, order_id, method, amount_minor, staff_id, position,
                                       business_day, fy, rev, created_at, updated_at)
                 VALUES ('P1', 'O1', 'cash', 100, 'S', 0, '2026-10-27', '2026-27', 1, 0, 0);",
            )
            .unwrap();
        }
        let mut store = Store::open(&path, T).unwrap();
        assert_eq!(store.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
        store
            .write_tx(|tx| {
                let kind: String = tx
                    .query_row("SELECT kind FROM orders WHERE id = 'O1'", [], |r| r.get(0))
                    .map_err(db_error)?;
                assert_eq!(kind, "topup");
                tx.execute_batch(
                    "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy,
                                         kind, rev, created_at, updated_at)
                     VALUES ('O2', 'settled', 50, 50, '2026-10-27', '2026-27', 'credit_collect',
                             1, 0, 0);
                     INSERT INTO payments (id, order_id, method, amount_minor, staff_id, position,
                                           business_day, fy, rev, created_at, updated_at)
                     VALUES ('P2', 'O2', 'cash', 50, 'S', 0, '2026-10-27', '2026-27', 1, 0, 0);",
                )
                .map_err(db_error)?;
                let rows: i64 = tx
                    .query_row("SELECT COUNT(*) FROM payments", [], |r| r.get(0))
                    .map_err(db_error)?;
                assert_eq!(rows, 2);
                assert!(
                    tx.execute("INSERT INTO payments (id, order_id, method, amount_minor, staff_id, position, business_day, fy, rev, created_at, updated_at) VALUES ('P3', 'NOPE', 'cash', 1, 'S', 0, 'd', 'f', 1, 0, 0)", [])
                        .is_err()
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn v33_database_upgrades_to_plan_sale_orders_keeping_its_children() {
        let (_dir, path) = temp_db();
        {
            let conn = Connection::open(&path).unwrap();
            for m in &migrations::MIGRATIONS[..33] {
                conn.execute_batch(m.sql).unwrap();
                record_schema_version(&conn, m.version, 0).unwrap();
            }
            conn.execute_batch(
                "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy,
                                     kind, rev, created_at, updated_at)
                 VALUES ('O1', 'settled', 100, 100, '2026-10-27', '2026-27', 'credit_collect', 1, 0, 0);
                 INSERT INTO payments (id, order_id, method, amount_minor, staff_id, position,
                                       business_day, fy, rev, created_at, updated_at)
                 VALUES ('P1', 'O1', 'cash', 100, 'S', 0, '2026-10-27', '2026-27', 1, 0, 0);",
            )
            .unwrap();
        }
        let mut store = Store::open(&path, T).unwrap();
        assert_eq!(store.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
        store
            .write_tx(|tx| {
                let kept: i64 = tx
                    .query_row("SELECT COUNT(*) FROM payments WHERE order_id = 'O1'", [], |r| {
                        r.get(0)
                    })
                    .map_err(db_error)?;
                assert_eq!(kept, 1);
                tx.execute_batch(
                    "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy,
                                         kind, rev, created_at, updated_at)
                     VALUES ('O2', 'settled', 50, 50, '2026-10-27', '2026-27', 'plan_sale', 1, 0, 0);",
                )
                .map_err(db_error)?;
                assert!(
                    tx.execute(
                        "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy, kind, rev, created_at, updated_at) VALUES ('O3', 'settled', 1, 1, 'd', 'f', 'nope', 1, 0, 0)",
                        []
                    )
                    .is_err()
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn writer_lock_is_exclusive() {
        let (_dir, path) = temp_db();
        let mut a = Store::open(&path, T).unwrap();
        let mut b = Store::open(&path, T).unwrap();
        assert!(a.holds_writer_lock());
        assert!(!b.holds_writer_lock());
        assert!(a.try_lock_writer().unwrap());
        assert!(!b.try_lock_writer().unwrap());
        drop(a);
        assert!(b.try_lock_writer().unwrap());
    }
}
