//! `gz_control`: the cloud control database on Turso, reached over libsql remote (Hrana over
//! HTTPS), or a local SQLite file through rusqlite for `arena-cloud --dev`. Feature `control`
//! only; the Hub never links it.
//!
//! libsql is built remote-only (no C SQLite core), so rusqlite's bundled SQLite stays the only
//! SQLite C library in the binary (`scripts/check-sqlite-linkage.sh`).
//!
//! Migrations are embedded, numbered and forward-only, like the Hub's, but kept in their own
//! list (`control-migrations/`). [`ControlDb::migrate`] refuses a database written by a newer
//! binary ([`SchemaTooNew`]) without writing to it.
//!
//! [`ControlDb::dump`] and [`ControlDb::restore`] (in `control/dump.rs`) back the database up as
//! a self-contained SQL text and put it back into an empty database, verifying row counts.
//!
//! Owner sign-up (Story 2.4) has typed queries in `control/{owner,otp,session,rate_limit}.rs`,
//! and the owner's password, TOTP and backup codes (Story 2.5) in `control/mfa.rs`; every SQL
//! statement `arena-cloud` runs lives here.

mod commands;
mod dump;
mod entitlement;
mod hubs;
mod mfa;
mod otp;
mod owner;
mod pairing;
mod rate_limit;
mod session;
mod setup_codes;
mod setup_status;
mod signin;
mod staff_invites;
mod tenant;

pub use commands::{CommandRow, NewCommand};
pub use dump::{DUMP_HEADER, DUMP_PAGE_ROWS, Dump, DumpManifest, RestoreReport, read_manifest};
pub use entitlement::{CompGrant, Entitlement, FreeTierOutcome, GrantOutcome, OpsAuditRow};
pub use hubs::{Beat, HubRow, NewHub, PairOutcome};
pub use mfa::{BackupCodeRow, NewBackupCode, OwnerMfa};
pub use otp::{
    NewOtpChallenge, OTP_PURPOSE_SIGNUP, OTP_RETENTION_MS, OtpChallenge, SignupCompletion,
    SignupOutcome,
};
pub use owner::{NewOwner, Owner};
pub use pairing::{
    IssueOutcome, NewPairingCode, PAIRING_HISTORY_MS, PairingCodeRow, RedeemInvalid, Redeemed,
};
pub use rate_limit::{RateReservation, RateRule, RateWindow};
pub use session::{
    NewOwnerSession, OwnerSession, REFRESH_RACE_MS, RefreshOutcome, SessionWithOwner,
};
pub use setup_codes::{NewSetupCode, SealedSetupCode, SetupCodeRow};
pub use setup_status::SETUP_KINDS;
pub use signin::{
    NewOwnerDevice, NewSigninChallenge, OwnerDevice, SigninChallenge, SigninCompletion,
    SigninFactor, SigninFailure,
};
pub use staff_invites::{NewStaffInvite, StaffInviteRow};
pub use tenant::{Branch, BranchCodeOutcome, BranchProfile, NewTenantBranch, ProgressRow, Tenant};

use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use std::time::Duration;
use tokio::sync::{Mutex, OwnedMutexGuard};

/// One control migration: its number and its SQL.
#[derive(Debug, Clone, Copy)]
pub struct ControlMigration {
    pub version: u32,
    pub sql: &'static str,
}

/// Every control migration, in order. Append new files here; never edit or reorder applied
/// ones.
pub const CONTROL_MIGRATIONS: &[ControlMigration] = &[
    ControlMigration {
        version: 1,
        sql: include_str!("../control-migrations/0001_baseline.sql"),
    },
    ControlMigration {
        version: 2,
        sql: include_str!("../control-migrations/0002_owner_signup.sql"),
    },
    ControlMigration {
        version: 3,
        sql: include_str!("../control-migrations/0003_owner_mfa.sql"),
    },
    ControlMigration {
        version: 4,
        sql: include_str!("../control-migrations/0004_owner_signin.sql"),
    },
    ControlMigration {
        version: 5,
        sql: include_str!("../control-migrations/0005_tenants_branches.sql"),
    },
    ControlMigration {
        version: 6,
        sql: include_str!("../control-migrations/0006_entitlements.sql"),
    },
    ControlMigration {
        version: 7,
        sql: include_str!("../control-migrations/0007_pairing_codes.sql"),
    },
    ControlMigration {
        version: 8,
        sql: include_str!("../control-migrations/0008_hubs.sql"),
    },
    ControlMigration {
        version: 9,
        sql: include_str!("../control-migrations/0009_branch_fence.sql"),
    },
    ControlMigration {
        version: 10,
        sql: include_str!("../control-migrations/0010_owner_setup_codes.sql"),
    },
    ControlMigration {
        version: 11,
        sql: include_str!("../control-migrations/0011_commands.sql"),
    },
    ControlMigration {
        version: 12,
        sql: include_str!("../control-migrations/0012_staff_invites.sql"),
    },
    ControlMigration {
        version: 13,
        sql: include_str!("../control-migrations/0013_branch_setup_status.sql"),
    },
];

/// The `gz_control` schema version this binary writes.
pub const LATEST_CONTROL_VERSION: u32 = CONTROL_MIGRATIONS[CONTROL_MIGRATIONS.len() - 1].version;

/// The database was written by a newer binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaTooNew {
    /// `MAX(version)` in the database.
    pub db: u32,
    /// This binary's latest version.
    pub binary: u32,
}

impl fmt::Display for SchemaTooNew {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SCHEMA_TOO_NEW: gz_control schema version {} is newer than this binary's version {}",
            self.db, self.binary
        )
    }
}

/// The migrations to apply to a database at `db_version` for a binary at `latest`: the
/// (possibly empty) range of versions `db_version + 1 ..= latest`, or [`SchemaTooNew`].
pub fn plan(db_version: u32, latest: u32) -> Result<Range<u32>, SchemaTooNew> {
    if db_version > latest {
        return Err(SchemaTooNew {
            db: db_version,
            binary: latest,
        });
    }
    Ok(db_version + 1..latest + 1)
}

/// A `gz_control` failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlError {
    /// The database is newer than this binary; nothing was written.
    SchemaTooNew(SchemaTooNew),
    /// The database failed or could not be reached. The message never carries the token.
    Db(String),
    /// Migration `version` failed with an SQL error that retrying cannot fix (not a lock or
    /// busy error); it was rolled back and the database version did not advance.
    Migration { version: u32, message: String },
    /// The database holds something [`ControlDb::dump`] cannot represent (a virtual table).
    Dump(String),
    /// The text given to [`ControlDb::restore`] is not a complete v1 dump; nothing was written.
    DumpInvalid(String),
    /// [`ControlDb::restore`] refused a target that already has these objects; nothing was
    /// written.
    TargetNotEmpty { objects: Vec<String> },
    /// After a restore, `table` holds `found` rows where the manifest says `expected` (`None`:
    /// the table is missing on that side).
    CountMismatch {
        table: String,
        expected: Option<u64>,
        found: Option<u64>,
    },
    /// After a restore, a row of `table` references a missing row of `parent`.
    ForeignKeyViolation { table: String, parent: String },
}

impl ControlError {
    /// A stable code for logs, alerts and exit messages.
    pub fn code(&self) -> &'static str {
        match self {
            Self::SchemaTooNew(_) => "SCHEMA_TOO_NEW",
            Self::Db(_) => "CONTROL_DB_FAILED",
            Self::Migration { .. } => "MIGRATION_FAILED",
            Self::Dump(_) => "DUMP_FAILED",
            Self::DumpInvalid(_) => "DUMP_INVALID",
            Self::TargetNotEmpty { .. } => "RESTORE_TARGET_NOT_EMPTY",
            Self::CountMismatch { .. } => "RESTORE_COUNT_MISMATCH",
            Self::ForeignKeyViolation { .. } => "RESTORE_FOREIGN_KEY_VIOLATION",
        }
    }
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SchemaTooNew(e) => e.fmt(f),
            Self::Db(m) => write!(f, "gz_control: {m}"),
            Self::Migration { version, message } => {
                write!(f, "gz_control migration {version} failed: {message}")
            }
            Self::Dump(m) => write!(f, "DUMP_FAILED: {m}"),
            Self::DumpInvalid(m) => write!(f, "DUMP_INVALID: {m}"),
            Self::TargetNotEmpty { objects } => write!(
                f,
                "RESTORE_TARGET_NOT_EMPTY: the target database already has {}",
                objects.join(", ")
            ),
            Self::CountMismatch {
                table,
                expected,
                found,
            } => {
                let n = |v: &Option<u64>| {
                    v.map_or_else(|| "no table".to_owned(), |n| format!("{n} rows"))
                };
                write!(
                    f,
                    "RESTORE_COUNT_MISMATCH: table {table}: the dump says {}, the restored database has {}",
                    n(expected),
                    n(found)
                )
            }
            Self::ForeignKeyViolation { table, parent } => write!(
                f,
                "RESTORE_FOREIGN_KEY_VIOLATION: table {table} has rows whose {parent} row is missing"
            ),
        }
    }
}

impl std::error::Error for ControlError {}

/// How many times a migration step that failed without the version advancing is retried (it
/// may have lost a write lock to a concurrent runner that has not committed yet).
const STEP_ATTEMPTS: u32 = 10;
const STEP_RETRY_DELAY: Duration = Duration::from_millis(200);

/// HTTPS (Mozilla roots, ring) or plain HTTP: the caller restricts plain HTTP to loopback.
fn connector() -> hyper_rustls::HttpsConnector<hyper::client::HttpConnector> {
    let mut http = hyper::client::HttpConnector::new();
    http.enforce_http(false);
    http.set_nodelay(true);
    hyper_rustls::HttpsConnectorBuilder::new()
        .with_webpki_roots()
        .https_or_http()
        .enable_http1()
        .wrap_connector(http)
}

/// Is `msg` an SQL error the server returned (rather than a transport failure)?
fn is_sql_error(msg: &str) -> bool {
    msg.contains("api error") || msg.contains("SQLite error") || msg.contains("stream error")
}

/// Is `msg` a lock/busy error, which a concurrent runner holding the write lock causes?
fn is_lock_error(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("busy") || m.contains("locked")
}

/// Why one migration transaction failed.
struct StepFailure {
    error: ControlError,
    /// An SQL error that retrying cannot fix (not a lock, busy or transport error).
    deterministic: bool,
}

/// One SQLite value, as read or bound by [`ControlDb::query`].
#[derive(Debug, Clone, PartialEq)]
enum SqlValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl From<libsql::Value> for SqlValue {
    fn from(v: libsql::Value) -> Self {
        match v {
            libsql::Value::Null => Self::Null,
            libsql::Value::Integer(i) => Self::Integer(i),
            libsql::Value::Real(r) => Self::Real(r),
            libsql::Value::Text(t) => Self::Text(t),
            libsql::Value::Blob(b) => Self::Blob(b),
        }
    }
}

impl From<SqlValue> for libsql::Value {
    fn from(v: SqlValue) -> Self {
        match v {
            SqlValue::Null => Self::Null,
            SqlValue::Integer(i) => Self::Integer(i),
            SqlValue::Real(r) => Self::Real(r),
            SqlValue::Text(t) => Self::Text(t),
            SqlValue::Blob(b) => Self::Blob(b),
        }
    }
}

impl From<rusqlite::types::Value> for SqlValue {
    fn from(v: rusqlite::types::Value) -> Self {
        use rusqlite::types::Value as V;
        match v {
            V::Null => Self::Null,
            V::Integer(i) => Self::Integer(i),
            V::Real(r) => Self::Real(r),
            V::Text(t) => Self::Text(t),
            V::Blob(b) => Self::Blob(b),
        }
    }
}

impl From<SqlValue> for rusqlite::types::Value {
    fn from(v: SqlValue) -> Self {
        match v {
            SqlValue::Null => Self::Null,
            SqlValue::Integer(i) => Self::Integer(i),
            SqlValue::Real(r) => Self::Real(r),
            SqlValue::Text(t) => Self::Text(t),
            SqlValue::Blob(b) => Self::Blob(b),
        }
    }
}

/// A remote (libsql, Hrana over HTTPS) connection.
struct Remote {
    _db: libsql::Database,
    conn: libsql::Connection,
    token: String,
}

/// Runs `sql` on a rusqlite connection and returns every row.
fn local_query(
    c: &rusqlite::Connection,
    sql: &str,
    params: Vec<SqlValue>,
) -> Result<Vec<Vec<SqlValue>>, ControlError> {
    let run = || {
        let mut st = c.prepare(sql)?;
        let n = st.column_count();
        let params: Vec<rusqlite::types::Value> = params.into_iter().map(Into::into).collect();
        let mut rows = st.query(rusqlite::params_from_iter(params))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let mut values = Vec::with_capacity(n);
            for i in 0..n {
                values.push(row.get::<_, rusqlite::types::Value>(i)?.into());
            }
            out.push(values);
        }
        Ok::<_, rusqlite::Error>(out)
    };
    run().map_err(local_err)
}

/// One statement of a [`ControlDb::transact`] batch.
struct Stmt {
    sql: &'static str,
    params: Vec<SqlValue>,
    /// When set, the batch is rolled back (and `transact` answers `false`) unless the statement
    /// changed exactly this many rows (for a [`Stmt::fetch`] statement: returned this many).
    expect_changes: Option<u64>,
    /// Run as a query (a `SELECT`, or a write with `RETURNING`) and keep its rows.
    fetch: bool,
}

impl Stmt {
    fn new(sql: &'static str, params: Vec<SqlValue>) -> Self {
        Self {
            sql,
            params,
            expect_changes: None,
            fetch: false,
        }
    }

    fn expect(mut self, n: u64) -> Self {
        self.expect_changes = Some(n);
        self
    }

    fn fetch(mut self) -> Self {
        self.fetch = true;
        self
    }
}

/// The rows of each statement of a committed [`ControlDb::transact_rows`] batch (empty for a
/// statement that is not [`Stmt::fetch`]).
type BatchRows = Vec<Vec<Vec<SqlValue>>>;

/// Runs `stmts` in one rusqlite transaction; `None` (rolled back) when a guard failed.
fn local_transact(
    c: &mut rusqlite::Connection,
    stmts: Vec<Stmt>,
) -> Result<Option<BatchRows>, ControlError> {
    let run = |c: &mut rusqlite::Connection| {
        let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut out = Vec::with_capacity(stmts.len());
        for s in stmts {
            let params: Vec<rusqlite::types::Value> =
                s.params.into_iter().map(Into::into).collect();
            let (n, rows) = if s.fetch {
                let mut st = tx.prepare(s.sql)?;
                let cols = st.column_count();
                let mut q = st.query(rusqlite::params_from_iter(params))?;
                let mut rows = Vec::new();
                while let Some(row) = q.next()? {
                    let mut values = Vec::with_capacity(cols);
                    for i in 0..cols {
                        values.push(row.get::<_, rusqlite::types::Value>(i)?.into());
                    }
                    rows.push(values);
                }
                (rows.len() as u64, rows)
            } else {
                let n = tx.execute(s.sql, rusqlite::params_from_iter(params))?;
                (n as u64, Vec::new())
            };
            if s.expect_changes.is_some_and(|want| want != n) {
                tx.rollback()?;
                return Ok(None);
            }
            out.push(rows);
        }
        tx.commit()?;
        Ok::<_, rusqlite::Error>(Some(out))
    };
    run(c).map_err(local_err)
}

/// A text column.
fn col_text(row: &[SqlValue], i: usize) -> Result<String, ControlError> {
    match row.get(i) {
        Some(SqlValue::Text(t)) => Ok(t.clone()),
        other => Err(ControlError::Db(format!(
            "column {i}: expected text, got {other:?}"
        ))),
    }
}

/// An integer column.
fn col_int(row: &[SqlValue], i: usize) -> Result<i64, ControlError> {
    match row.get(i) {
        Some(SqlValue::Integer(n)) => Ok(*n),
        other => Err(ControlError::Db(format!(
            "column {i}: expected integer, got {other:?}"
        ))),
    }
}

/// A blob column.
fn col_blob(row: &[SqlValue], i: usize) -> Result<Vec<u8>, ControlError> {
    match row.get(i) {
        Some(SqlValue::Blob(b)) => Ok(b.clone()),
        other => Err(ControlError::Db(format!(
            "column {i}: expected blob, got {:?}",
            other.map(|_| "another type")
        ))),
    }
}

/// A nullable text column.
fn col_opt_text(row: &[SqlValue], i: usize) -> Result<Option<String>, ControlError> {
    match row.get(i) {
        Some(SqlValue::Null) => Ok(None),
        Some(SqlValue::Text(t)) => Ok(Some(t.clone())),
        _ => Err(ControlError::Db(format!(
            "column {i}: expected text or null"
        ))),
    }
}

/// A nullable integer column.
fn col_opt_int(row: &[SqlValue], i: usize) -> Result<Option<i64>, ControlError> {
    match row.get(i) {
        Some(SqlValue::Null) => Ok(None),
        Some(SqlValue::Integer(n)) => Ok(Some(*n)),
        other => Err(ControlError::Db(format!(
            "column {i}: expected integer or null, got {other:?}"
        ))),
    }
}

fn text(s: &str) -> SqlValue {
    SqlValue::Text(s.to_owned())
}

/// See [`ControlDb::reader`].
enum Reader<'a> {
    Remote(&'a Remote),
    /// `None` only while a query runs on the blocking pool.
    Local(Option<OwnedMutexGuard<rusqlite::Connection>>),
}

impl Reader<'_> {
    async fn query(
        &mut self,
        sql: &str,
        params: Vec<SqlValue>,
    ) -> Result<Vec<Vec<SqlValue>>, ControlError> {
        match self {
            Self::Remote(r) => r.query(sql, params).await,
            Self::Local(slot) => {
                let guard = slot
                    .take()
                    .ok_or_else(|| ControlError::Db("local reader lost its connection".into()))?;
                let sql = sql.to_owned();
                let (guard, res) = tokio::task::spawn_blocking(move || {
                    let res = local_query(&guard, &sql, params);
                    (guard, res)
                })
                .await
                .map_err(|e| ControlError::Db(format!("local task failed: {e}")))?;
                *slot = Some(guard);
                res
            }
        }
    }
}

/// The two places `gz_control` can live.
enum Backend {
    /// Turso (or a test sqld) over libsql remote.
    Remote(Remote),
    /// A local SQLite file through rusqlite (`arena-cloud --dev`). Every call runs on the
    /// blocking pool.
    Local(Arc<Mutex<rusqlite::Connection>>),
}

/// A `gz_control` connection: remote ([`ControlDb::connect`]) or a local file
/// ([`ControlDb::open_local`]), behind one API.
pub struct ControlDb {
    backend: Backend,
}

impl fmt::Debug for ControlDb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.backend {
            Backend::Remote(_) => "remote",
            Backend::Local(_) => "local",
        };
        f.debug_struct("ControlDb")
            .field("backend", &kind)
            .finish_non_exhaustive()
    }
}

/// How long a local call waits on a lock held by another connection.
const LOCAL_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

fn local_err(e: rusqlite::Error) -> ControlError {
    ControlError::Db(e.to_string())
}

/// Is `e` a SQLite busy/locked error (another connection holds the lock)?
fn is_local_lock_error(e: &rusqlite::Error) -> bool {
    matches!(
        e.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
    )
}

impl Remote {
    fn err(&self, e: libsql::Error) -> ControlError {
        redact_with(&self.token, e)
    }

    async fn ping(&self) -> Result<(), ControlError> {
        let mut rows = self
            .conn
            .query("SELECT 1", ())
            .await
            .map_err(|e| self.err(e))?;
        rows.next().await.map_err(|e| self.err(e))?;
        Ok(())
    }

    async fn query(
        &self,
        sql: &str,
        params: Vec<SqlValue>,
    ) -> Result<Vec<Vec<SqlValue>>, ControlError> {
        let params: Vec<libsql::Value> = params.into_iter().map(Into::into).collect();
        let mut rows = self
            .conn
            .query(sql, libsql::params_from_iter(params))
            .await
            .map_err(|e| self.err(e))?;
        let n = rows.column_count();
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| self.err(e))? {
            let mut values = Vec::with_capacity(usize::try_from(n).unwrap_or(0));
            for i in 0..n {
                values.push(row.get_value(i).map_err(|e| self.err(e))?.into());
            }
            out.push(values);
        }
        Ok(out)
    }

    async fn has_migrations_table(&self) -> Result<bool, ControlError> {
        let mut rows = self
            .conn
            .query(HAS_MIGRATIONS_TABLE, ())
            .await
            .map_err(|e| self.err(e))?;
        let row = rows.next().await.map_err(|e| self.err(e))?;
        let n: i64 = match row {
            Some(r) => r.get(0).map_err(|e| self.err(e))?,
            None => 0,
        };
        Ok(n > 0)
    }

    async fn max_version(&self) -> Result<Option<i64>, ControlError> {
        let mut rows = self
            .conn
            .query(MAX_VERSION, ())
            .await
            .map_err(|e| self.err(e))?;
        let row = rows.next().await.map_err(|e| self.err(e))?;
        match row {
            Some(r) => r.get(0).map_err(|e| self.err(e)),
            None => Ok(None),
        }
    }

    async fn create_migrations_table(&self) -> Result<(), ControlError> {
        self.conn
            .execute(CREATE_MIGRATIONS_TABLE, ())
            .await
            .map_err(|e| self.err(e))?;
        Ok(())
    }

    async fn apply_batch(&self, batch: &str) -> Result<(), StepFailure> {
        match self.conn.execute_transactional_batch(batch).await {
            Ok(_) => Ok(()),
            Err(e) => {
                let error = self.err(e);
                let deterministic = matches!(&error, ControlError::Db(msg)
                    if is_sql_error(msg) && !is_lock_error(msg));
                Err(StepFailure {
                    error,
                    deterministic,
                })
            }
        }
    }
}

fn redact_with(token: &str, e: libsql::Error) -> ControlError {
    let mut msg = e.to_string();
    if !token.is_empty() {
        msg = msg.replace(token, "[redacted]");
    }
    ControlError::Db(msg)
}

const HAS_MIGRATIONS_TABLE: &str =
    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'";
const MAX_VERSION: &str = "SELECT MAX(version) FROM schema_migrations";
const CREATE_MIGRATIONS_TABLE: &str = "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL)";

impl ControlDb {
    /// Builds a remote client for `url` (`libsql://`, `https://` or, for tests, `http://`)
    /// with `token`. This does not touch the network; the first query does.
    pub async fn connect(url: &str, token: &str) -> Result<Self, ControlError> {
        let redact = |e: libsql::Error| redact_with(token, e);
        let db = libsql::Builder::new_remote(url.to_owned(), token.to_owned())
            .connector(connector())
            .build()
            .await
            .map_err(redact)?;
        let conn = db.connect().map_err(redact)?;
        Ok(Self {
            backend: Backend::Remote(Remote {
                _db: db,
                conn,
                token: token.to_owned(),
            }),
        })
    }

    /// Opens (creating it when missing) the local SQLite file at `path` through rusqlite: the
    /// `arena-cloud --dev` backend. The parent directory must exist.
    pub async fn open_local(path: &Path) -> Result<Self, ControlError> {
        let path: PathBuf = path.to_owned();
        let conn = tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&path).map_err(local_err)?;
            conn.busy_timeout(LOCAL_BUSY_TIMEOUT).map_err(local_err)?;
            conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0))
                .map_err(local_err)?;
            Ok::<_, ControlError>(conn)
        })
        .await
        .map_err(|e| ControlError::Db(format!("local open task failed: {e}")))??;
        Ok(Self {
            backend: Backend::Local(Arc::new(Mutex::new(conn))),
        })
    }

    /// Runs `f` on the local connection on the blocking pool.
    async fn local<T: Send + 'static>(
        conn: &Arc<Mutex<rusqlite::Connection>>,
        f: impl FnOnce(&mut rusqlite::Connection) -> T + Send + 'static,
    ) -> Result<T, ControlError> {
        let mut guard = Arc::clone(conn).lock_owned().await;
        tokio::task::spawn_blocking(move || f(&mut guard))
            .await
            .map_err(|e| ControlError::Db(format!("local task failed: {e}")))
    }

    /// `SELECT 1`: succeeds when the database answers.
    pub async fn ping(&self) -> Result<(), ControlError> {
        match &self.backend {
            Backend::Remote(r) => r.ping().await,
            Backend::Local(c) => {
                Self::local(c, |c| {
                    c.query_row("SELECT 1", [], |r| r.get::<_, i64>(0))
                        .map(|_| ())
                        .map_err(local_err)
                })
                .await?
            }
        }
    }

    /// Runs `sql` with positional `params` and returns every row.
    async fn query(
        &self,
        sql: &str,
        params: Vec<SqlValue>,
    ) -> Result<Vec<Vec<SqlValue>>, ControlError> {
        match &self.backend {
            Backend::Remote(r) => r.query(sql, params).await,
            Backend::Local(c) => {
                let sql = sql.to_owned();
                Self::local(c, move |c| local_query(c, &sql, params)).await?
            }
        }
    }

    /// Runs `sql` with positional `params` and returns how many rows it changed.
    async fn execute(&self, sql: &'static str, params: Vec<SqlValue>) -> Result<u64, ControlError> {
        match &self.backend {
            Backend::Remote(r) => {
                let params: Vec<libsql::Value> = params.into_iter().map(Into::into).collect();
                r.conn
                    .execute(sql, libsql::params_from_iter(params))
                    .await
                    .map_err(|e| r.err(e))
            }
            Backend::Local(c) => {
                Self::local(c, move |c| {
                    let params: Vec<rusqlite::types::Value> =
                        params.into_iter().map(Into::into).collect();
                    c.execute(sql, rusqlite::params_from_iter(params))
                        .map(|n| n as u64)
                        .map_err(local_err)
                })
                .await?
            }
        }
    }

    /// Runs `stmts` in one transaction. Answers `false`, with nothing written, when a statement
    /// with [`Stmt::expect`] changed another number of rows. On the remote backend the
    /// transaction has its own Hrana stream, so concurrent requests never share it.
    async fn transact(&self, stmts: Vec<Stmt>) -> Result<bool, ControlError> {
        Ok(self.transact_rows(stmts).await?.is_some())
    }

    /// [`ControlDb::transact`], answering the rows of each [`Stmt::fetch`] statement read
    /// inside the transaction (`None`: a guard failed and nothing was written).
    async fn transact_rows(&self, stmts: Vec<Stmt>) -> Result<Option<BatchRows>, ControlError> {
        match &self.backend {
            Backend::Remote(r) => {
                let tx = r.conn.transaction().await.map_err(|e| r.err(e))?;
                let mut out = Vec::with_capacity(stmts.len());
                for s in stmts {
                    let params: Vec<libsql::Value> = s.params.into_iter().map(Into::into).collect();
                    let step = async {
                        if s.fetch {
                            let mut q = tx.query(s.sql, libsql::params_from_iter(params)).await?;
                            let cols = q.column_count();
                            let mut rows = Vec::new();
                            while let Some(row) = q.next().await? {
                                let mut values = Vec::new();
                                for i in 0..cols {
                                    values.push(SqlValue::from(row.get_value(i)?));
                                }
                                rows.push(values);
                            }
                            Ok::<_, libsql::Error>((rows.len() as u64, rows))
                        } else {
                            let n = tx.execute(s.sql, libsql::params_from_iter(params)).await?;
                            Ok((n, Vec::new()))
                        }
                    };
                    let (n, rows) = match step.await {
                        Ok(v) => v,
                        Err(e) => {
                            let err = r.err(e);
                            let _ = tx.rollback().await;
                            return Err(err);
                        }
                    };
                    if s.expect_changes.is_some_and(|want| want != n) {
                        tx.rollback().await.map_err(|e| r.err(e))?;
                        return Ok(None);
                    }
                    out.push(rows);
                }
                tx.commit().await.map_err(|e| r.err(e))?;
                Ok(Some(out))
            }
            Backend::Local(c) => Self::local(c, move |c| local_transact(c, stmts)).await?,
        }
    }

    /// A reader that keeps one connection for a sequence of queries: on the local backend it
    /// holds the connection's lock throughout, so a transaction it opens is not shared.
    async fn reader(&self) -> Reader<'_> {
        match &self.backend {
            Backend::Remote(r) => Reader::Remote(r),
            Backend::Local(c) => Reader::Local(Some(Arc::clone(c).lock_owned().await)),
        }
    }

    async fn has_migrations_table(&self) -> Result<bool, ControlError> {
        match &self.backend {
            Backend::Remote(r) => r.has_migrations_table().await,
            Backend::Local(c) => {
                Self::local(c, |c| {
                    c.query_row(HAS_MIGRATIONS_TABLE, [], |r| r.get::<_, i64>(0))
                        .map(|n| n > 0)
                        .map_err(local_err)
                })
                .await?
            }
        }
    }

    async fn create_migrations_table(&self) -> Result<(), ControlError> {
        match &self.backend {
            Backend::Remote(r) => r.create_migrations_table().await,
            Backend::Local(c) => {
                Self::local(c, |c| {
                    c.execute(CREATE_MIGRATIONS_TABLE, [])
                        .map(|_| ())
                        .map_err(local_err)
                })
                .await?
            }
        }
    }

    /// Runs `batch` as one transaction.
    async fn apply_batch(&self, batch: String) -> Result<(), StepFailure> {
        match &self.backend {
            Backend::Remote(r) => r.apply_batch(&batch).await,
            Backend::Local(c) => Self::local(c, move |c| {
                let run = |c: &mut rusqlite::Connection| {
                    let tx =
                        c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                    tx.execute_batch(&batch)?;
                    tx.commit()
                };
                run(c).map_err(|e| StepFailure {
                    deterministic: !is_local_lock_error(&e),
                    error: local_err(e),
                })
            })
            .await
            .unwrap_or_else(|error| {
                Err(StepFailure {
                    error,
                    deterministic: false,
                })
            }),
        }
    }

    /// The database's `MAX(version)`, or 0 when `schema_migrations` is missing or empty.
    /// Read-only.
    pub async fn schema_version(&self) -> Result<u32, ControlError> {
        if !self.has_migrations_table().await? {
            return Ok(0);
        }
        let v = match &self.backend {
            Backend::Remote(r) => r.max_version().await?,
            Backend::Local(c) => {
                Self::local(c, |c| {
                    c.query_row(MAX_VERSION, [], |r| r.get::<_, Option<i64>>(0))
                        .map_err(local_err)
                })
                .await??
            }
        };
        let v = v.unwrap_or(0);
        u32::try_from(v).map_err(|_| ControlError::Db(format!("invalid schema version {v}")))
    }

    /// Brings the database up to [`LATEST_CONTROL_VERSION`] and returns the version it ends
    /// at. See [`ControlDb::migrate_with`].
    pub async fn migrate(&self, now_ms: i64) -> Result<u32, ControlError> {
        self.migrate_with(CONTROL_MIGRATIONS, now_ms).await
    }

    /// Creates `schema_migrations` when it is missing, refuses a database newer than the last
    /// of `migrations` ([`ControlError::SchemaTooNew`], nothing written), then applies each
    /// pending migration together with its `schema_migrations` row (`applied_at = now_ms`) in
    /// one transaction, in order. The same runner drives both backends.
    ///
    /// A concurrent runner (a rolling update) is tolerated: the version row is inserted first
    /// in each transaction, so the runner that loses the primary-key race rolls back, re-reads
    /// the version and continues from there. It never fails or applies a migration twice.
    pub async fn migrate_with(
        &self,
        migrations: &[ControlMigration],
        now_ms: i64,
    ) -> Result<u32, ControlError> {
        let latest = migrations.last().map_or(0, |m| m.version);
        // Read before any write, so a too-new database stays untouched.
        let found = self.schema_version().await?;
        plan(found, latest).map_err(ControlError::SchemaTooNew)?;
        if !self.has_migrations_table().await? {
            self.create_migrations_table().await?;
        }
        let mut current = found;
        loop {
            let pending = plan(current, latest).map_err(ControlError::SchemaTooNew)?;
            let Some(next) = pending.clone().next() else {
                return Ok(current);
            };
            let m = migrations
                .iter()
                .find(|m| m.version == next)
                .ok_or_else(|| ControlError::Db(format!("migration {next} is missing")))?;
            current = self.apply_one(m, now_ms).await?;
        }
    }

    /// Applies `m` in one transaction and returns the database version afterwards (which may
    /// be higher when a concurrent runner got there first).
    async fn apply_one(&self, m: &ControlMigration, now_ms: i64) -> Result<u32, ControlError> {
        let batch = format!(
            "INSERT INTO schema_migrations (version, applied_at) VALUES ({}, {});\n{}",
            m.version, now_ms, m.sql
        );
        let mut last_err = None;
        for attempt in 0..STEP_ATTEMPTS {
            if attempt > 0 {
                tokio::time::sleep(STEP_RETRY_DELAY).await;
            }
            match self.apply_batch(batch.clone()).await {
                Ok(()) => return Ok(m.version),
                Err(StepFailure {
                    error,
                    deterministic,
                }) => {
                    // Lost the race (or a lock): if another runner applied it, carry on.
                    let now = self.schema_version().await?;
                    if now >= m.version {
                        return Ok(now);
                    }
                    // A deterministic SQL error (bad migration) fails at once; retrying it
                    // cannot help. Lock/busy errors and transport failures are retried.
                    if deterministic {
                        let message = match error {
                            ControlError::Db(msg) => msg,
                            other => other.to_string(),
                        };
                        return Err(ControlError::Migration {
                            version: m.version,
                            message,
                        });
                    }
                    last_err = Some(error);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| ControlError::Db("migration failed".to_owned())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_contiguous_from_one() {
        for (i, m) in CONTROL_MIGRATIONS.iter().enumerate() {
            assert_eq!(m.version as usize, i + 1);
        }
        assert_eq!(LATEST_CONTROL_VERSION as usize, CONTROL_MIGRATIONS.len());
    }

    #[test]
    fn plan_on_a_fresh_database_applies_everything() {
        assert_eq!(plan(0, 1), Ok(1..2));
        assert_eq!(plan(0, 3).unwrap().collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn plan_at_latest_applies_nothing() {
        assert!(plan(1, 1).unwrap().is_empty());
        assert!(plan(0, 0).unwrap().is_empty());
    }

    #[test]
    fn plan_applies_only_the_pending_tail() {
        assert_eq!(plan(2, 4).unwrap().collect::<Vec<_>>(), vec![3, 4]);
    }

    #[test]
    fn plan_refuses_a_newer_database_naming_both_versions() {
        let e = plan(2, 1).unwrap_err();
        assert_eq!(e, SchemaTooNew { db: 2, binary: 1 });
        let s = e.to_string();
        assert!(
            s.contains("SCHEMA_TOO_NEW") && s.contains('2') && s.contains('1'),
            "{s}"
        );
    }

    #[test]
    fn migrations_never_control_transactions() {
        // Each file runs inside the runner's transaction; BEGIN/COMMIT would break it.
        for m in CONTROL_MIGRATIONS {
            let code: String = m
                .sql
                .lines()
                .map(|l| l.split("--").next().unwrap_or(""))
                .collect::<Vec<_>>()
                .join("\n");
            for stmt in code.split(';') {
                let first = stmt
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_uppercase();
                assert!(
                    !["BEGIN", "COMMIT", "END", "ROLLBACK", "SAVEPOINT", "RELEASE"]
                        .contains(&first.as_str()),
                    "migration {} has a transaction-control statement: {stmt:?}",
                    m.version
                );
            }
        }
    }

    #[test]
    fn sql_errors_are_told_apart_from_lock_and_transport_errors() {
        let sql = "Hrana: `api error: `SQLite error: no such table: missing``";
        assert!(is_sql_error(sql) && !is_lock_error(sql));
        let busy = "Hrana: `api error: `SQLite error: database is locked``";
        assert!(is_lock_error(busy));
        assert!(!is_sql_error("Hrana: `http error: `connection refused``"));
    }

    #[test]
    fn baseline_creates_no_tables() {
        let sql = CONTROL_MIGRATIONS[0].sql.to_ascii_uppercase();
        assert!(
            !sql.lines()
                .any(|l| !l.trim_start().starts_with("--") && l.contains("CREATE"))
        );
    }
}
