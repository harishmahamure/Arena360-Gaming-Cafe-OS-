//! `gz_control`: the cloud control database on Turso, reached over libsql remote (Hrana over
//! HTTPS). Feature `control` only; the Hub never links it.
//!
//! Migrations are embedded, numbered and forward-only, like the Hub's, but kept in their own
//! list (`control-migrations/`). [`ControlDb::migrate`] refuses a database written by a newer
//! binary ([`SchemaTooNew`]) without writing to it.

use std::fmt;
use std::ops::Range;
use std::time::Duration;

/// One control migration: its number and its SQL.
#[derive(Debug, Clone, Copy)]
pub struct ControlMigration {
    pub version: u32,
    pub sql: &'static str,
}

/// Every control migration, in order. Append new files here; never edit or reorder applied
/// ones.
pub const CONTROL_MIGRATIONS: &[ControlMigration] = &[ControlMigration {
    version: 1,
    sql: include_str!("../control-migrations/0001_baseline.sql"),
}];

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
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SchemaTooNew(e) => e.fmt(f),
            Self::Db(m) => write!(f, "gz_control: {m}"),
            Self::Migration { version, message } => {
                write!(f, "gz_control migration {version} failed: {message}")
            }
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

/// A `gz_control` connection.
pub struct ControlDb {
    _db: libsql::Database,
    conn: libsql::Connection,
    token: String,
}

impl fmt::Debug for ControlDb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ControlDb").finish_non_exhaustive()
    }
}

impl ControlDb {
    /// Builds a remote client for `url` (`libsql://`, `https://` or, for tests, `http://`)
    /// with `token`. This does not touch the network; the first query does.
    pub async fn connect(url: &str, token: &str) -> Result<Self, ControlError> {
        let redact = |e: libsql::Error| Self::redact_with(token, e);
        let db = libsql::Builder::new_remote(url.to_owned(), token.to_owned())
            .connector(connector())
            .build()
            .await
            .map_err(redact)?;
        let conn = db.connect().map_err(redact)?;
        Ok(Self {
            _db: db,
            conn,
            token: token.to_owned(),
        })
    }

    fn redact_with(token: &str, e: libsql::Error) -> ControlError {
        let mut msg = e.to_string();
        if !token.is_empty() {
            msg = msg.replace(token, "[redacted]");
        }
        ControlError::Db(msg)
    }

    fn err(&self, e: libsql::Error) -> ControlError {
        Self::redact_with(&self.token, e)
    }

    /// `SELECT 1`: succeeds when the database answers.
    pub async fn ping(&self) -> Result<(), ControlError> {
        let mut rows = self
            .conn
            .query("SELECT 1", ())
            .await
            .map_err(|e| self.err(e))?;
        rows.next().await.map_err(|e| self.err(e))?;
        Ok(())
    }

    async fn has_migrations_table(&self) -> Result<bool, ControlError> {
        let mut rows = self
            .conn
            .query(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'",
                (),
            )
            .await
            .map_err(|e| self.err(e))?;
        let row = rows.next().await.map_err(|e| self.err(e))?;
        let n: i64 = match row {
            Some(r) => r.get(0).map_err(|e| self.err(e))?,
            None => 0,
        };
        Ok(n > 0)
    }

    /// The database's `MAX(version)`, or 0 when `schema_migrations` is missing or empty.
    /// Read-only.
    pub async fn schema_version(&self) -> Result<u32, ControlError> {
        if !self.has_migrations_table().await? {
            return Ok(0);
        }
        let mut rows = self
            .conn
            .query("SELECT MAX(version) FROM schema_migrations", ())
            .await
            .map_err(|e| self.err(e))?;
        let row = rows.next().await.map_err(|e| self.err(e))?;
        let v: Option<i64> = match row {
            Some(r) => r.get(0).map_err(|e| self.err(e))?,
            None => None,
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
    /// one transaction, in order.
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
            self.conn
                .execute(
                    "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL)",
                    (),
                )
                .await
                .map_err(|e| self.err(e))?;
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
            match self.conn.execute_transactional_batch(&batch).await {
                Ok(_) => return Ok(m.version),
                Err(e) => {
                    // Lost the race (or a lock): if another runner applied it, carry on.
                    let err = self.err(e);
                    let now = self.schema_version().await?;
                    if now >= m.version {
                        return Ok(now);
                    }
                    // A deterministic SQL error (bad migration) fails at once; retrying it
                    // cannot help. Lock/busy errors and transport failures are retried.
                    if let ControlError::Db(msg) = &err
                        && is_sql_error(msg)
                        && !is_lock_error(msg)
                    {
                        return Err(ControlError::Migration {
                            version: m.version,
                            message: msg.clone(),
                        });
                    }
                    last_err = Some(err);
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
