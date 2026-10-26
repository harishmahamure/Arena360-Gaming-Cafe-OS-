//! `arena-store`: the branch database. All SQL lives in this crate.
//!
//! [`Store::open`] opens SQLite in WAL mode with `synchronous=FULL` (both verified by reading
//! them back), refuses a schema newer than this binary and applies the embedded migrations.

mod admin;
mod audit;
mod branch;
mod commands;
#[cfg(feature = "control")]
pub mod control;
mod dedupe;
mod devices;
mod identity;
mod migrations;
mod players;
mod products;
mod staff;
mod stock;

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;

use arena_proto::{AppError, ErrorCode};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

pub use admin::{
    AdminExitRow, META_VERIFIER_COUNTER, NewAdminExit, find_admin_exit, insert_admin_exit,
    insert_verifier_version, latest_verifier, max_pin_verifier_version, set_admin_exit_relocked,
    verifier_staff, verifier_version_at,
};
pub use audit::{
    AuditCursor, AuditFilter, AuditRow, find_change, list_audit, list_device_labels,
    list_staff_names, previous_row_image,
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
pub use dedupe::{
    AppliedIntentRecord, StoredIntent, count_applied_intents, find_applied_intent,
    insert_applied_intent, prune_applied_intents,
};
pub use devices::{
    DeviceMetaRow, DeviceRow, GroupRow, NewDevice, RateRow, ZoneRow, cell_taken,
    count_group_devices, count_groups_without_rate, count_live_devices, count_zone_devices,
    deactivate_rate, devices_meta_exists, find_active_rate, find_device, find_device_meta,
    find_group, find_group_by_name, find_zone, find_zone_by_name, insert_device,
    insert_device_meta, insert_group, insert_rate, insert_zone, list_active_rates,
    list_device_meta, list_devices, list_groups, list_zones, set_device_group, set_device_label,
    set_device_meta_deleted, set_device_place, set_device_reported_version, set_device_revoked,
    set_device_state, set_group_deleted, set_zone_deleted, tv_number_taken, update_device_meta,
    update_group, update_zone,
};
pub use identity::{
    CachedLicence, HubIdentity, TrustedClockRow, find_cached_licence, find_hub_identity,
    find_trusted_clock, insert_hub_identity, store_licence, store_trusted_clock,
};
pub use migrations::LATEST_SCHEMA_VERSION;
pub use players::{
    PlayerRow, find_player_key, insert_consent, insert_player, insert_player_key, latest_consent,
    list_players_after, phone_taken, username_taken,
};
pub use products::{
    ProductRow, barcode_holder, count_active_products, find_product, insert_component,
    insert_product, list_components, list_products, live_bundles_with, set_product_deleted,
    update_product,
};
/// The transaction a write job runs in (always `BEGIN IMMEDIATE`).
pub use rusqlite::Transaction;
pub use staff::{
    PinCandidate, RoleRow, StaffRow, StaffWrite, count_active_staff, count_active_staff_with_role,
    find_role, find_role_by_name, find_staff, find_staff_by_login, insert_role, insert_role_perm,
    insert_staff, list_pin_candidates, list_role_perms, list_roles, list_unlock_candidates,
    role_perm_effect, set_staff_credentials, set_staff_setup_code, update_role_name, update_staff,
};
pub use stock::{
    LotRow, MoveRow, StockRow, find_lot, find_move, find_stock, insert_lot, insert_move,
    insert_stock, latest_move_id, list_lots, list_moves, set_lot_remaining, update_stock,
};

/// The transaction `apply()` and the replayer run in.
pub type StoreTx<'a> = Transaction<'a>;

/// The `hub_meta` key holding this Hub's instance id.
pub const META_HUB_INSTANCE_ID: &str = "hub_instance_id";
/// The `hub_meta` key holding the SHA-256 fingerprint (lower-case hex) of the Hub's LAN TLS
/// certificate.
pub const META_TLS_FINGERPRINT: &str = "tls_fingerprint";

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

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

    pub fn player_rows_after(&self, after: i64) -> Result<Vec<(i64, PlayerRow, String)>, AppError> {
        if !self.table_exists("players")? || !self.table_exists("player_keys")? {
            return Ok(Vec::new());
        }
        list_players_after(&self.conn, after)
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

    pub fn moves(&self, product_id: &str) -> Result<Vec<MoveRow>, AppError> {
        if !self.table_exists("stock_moves")? {
            return Ok(Vec::new());
        }
        list_moves(&self.conn, product_id)
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
                assert!(phone_taken(tx, "L1")? && !phone_taken(tx, "L2")?);
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
