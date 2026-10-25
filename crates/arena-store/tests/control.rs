//! `gz_control` migrations against a real libsql server (see `support/sqld.rs`).

#![cfg(feature = "control")]

#[path = "support/sqld.rs"]
mod sqld;

use arena_store::control::{
    CONTROL_MIGRATIONS, ControlDb, ControlError, ControlMigration, LATEST_CONTROL_VERSION,
    SchemaTooNew,
};

/// The baseline alone (no tables), for runner and dump tests that bring their own schema.
const BASELINE: &[ControlMigration] = CONTROL_MIGRATIONS.split_at(1).0;

/// The tables every migration creates, sorted.
const ALL_TABLES: &[&str] = &[
    "branches",
    "entitlements",
    "hubs",
    "onboarding_progress",
    "ops_audit",
    "otp_challenges",
    "owner_backup_codes",
    "owner_devices",
    "owner_mfa",
    "owner_sessions",
    "owners",
    "pairing_codes",
    "rate_limits",
    "schema_migrations",
    "signin_challenges",
    "tenants",
];

/// `schema_migrations` after `migrate(at)` on a fresh database.
fn all_rows(at: i64) -> Vec<(i64, i64)> {
    CONTROL_MIGRATIONS
        .iter()
        .map(|m| (i64::from(m.version), at))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fresh_database_gets_every_migration() {
    let Some(t) = sqld::fresh().await else { return };
    let db = ControlDb::connect(&t.url, "").await.unwrap();
    assert_eq!(db.schema_version().await.unwrap(), 0);
    assert_eq!(db.migrate(1_000).await.unwrap(), LATEST_CONTROL_VERSION);
    assert_eq!(db.schema_version().await.unwrap(), LATEST_CONTROL_VERSION);
    assert_eq!(sqld::migration_rows(&t.url).await, all_rows(1_000));
    assert_eq!(sqld::tables(&t.url).await, ALL_TABLES);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restart_at_latest_applies_nothing() {
    let Some(t) = sqld::fresh().await else { return };
    let db = ControlDb::connect(&t.url, "").await.unwrap();
    db.migrate(1_000).await.unwrap();
    let again = ControlDb::connect(&t.url, "").await.unwrap();
    assert_eq!(again.migrate(2_000).await.unwrap(), LATEST_CONTROL_VERSION);
    assert_eq!(sqld::migration_rows(&t.url).await, all_rows(1_000));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_newer_database_is_refused_and_left_untouched() {
    let Some(t) = sqld::fresh().await else { return };
    let too_new = i64::from(LATEST_CONTROL_VERSION) + 1;
    sqld::exec(
        &t.url,
        &format!(
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);
             INSERT INTO schema_migrations VALUES ({too_new}, 5);"
        ),
    )
    .await;
    let db = ControlDb::connect(&t.url, "").await.unwrap();
    let err = db.migrate(9_000).await.unwrap_err();
    assert_eq!(
        err,
        ControlError::SchemaTooNew(SchemaTooNew {
            db: LATEST_CONTROL_VERSION + 1,
            binary: LATEST_CONTROL_VERSION,
        })
    );
    assert_eq!(sqld::migration_rows(&t.url).await, vec![(too_new, 5)]);
    assert_eq!(sqld::tables(&t.url).await, vec!["schema_migrations"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn pending_migrations_apply_in_order_each_with_its_row() {
    let Some(t) = sqld::fresh().await else { return };
    let mut list = BASELINE.to_vec();
    list.push(ControlMigration {
        version: 2,
        sql: "CREATE TABLE t2 (id INTEGER PRIMARY KEY);",
    });
    list.push(ControlMigration {
        version: 3,
        sql: "ALTER TABLE t2 ADD COLUMN name TEXT;",
    });
    let db = ControlDb::connect(&t.url, "").await.unwrap();
    db.migrate_with(BASELINE, 1).await.unwrap();
    assert_eq!(db.migrate_with(&list, 7).await.unwrap(), 3);
    assert_eq!(
        sqld::migration_rows(&t.url).await,
        vec![(1, 1), (2, 7), (3, 7)]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_migration_leaves_neither_its_changes_nor_its_row() {
    let Some(t) = sqld::fresh().await else { return };
    let mut list = BASELINE.to_vec();
    list.push(ControlMigration {
        version: 2,
        sql: "CREATE TABLE half (id INTEGER); INSERT INTO missing VALUES (1);",
    });
    let db = ControlDb::connect(&t.url, "").await.unwrap();
    let err = db.migrate_with(&list, 7).await.unwrap_err();
    assert!(
        matches!(&err, ControlError::Migration { version: 2, message } if message.contains("no such table")),
        "{err:?}"
    );
    assert_eq!(sqld::migration_rows(&t.url).await, vec![(1, 7)]);
    assert_eq!(sqld::tables(&t.url).await, vec!["schema_migrations"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_runners_both_succeed_with_one_row_per_version() {
    let Some(t) = sqld::fresh().await else { return };
    let list: &'static [ControlMigration] = Box::leak(
        [
            BASELINE.to_vec(),
            vec![
                ControlMigration {
                    version: 2,
                    sql: "CREATE TABLE r2 (id INTEGER PRIMARY KEY);",
                },
                ControlMigration {
                    version: 3,
                    sql: "CREATE TABLE r3 (id INTEGER PRIMARY KEY);",
                },
            ],
        ]
        .concat()
        .into_boxed_slice(),
    );
    let mut runners = Vec::new();
    for i in 0..4 {
        let url = t.url.clone();
        runners.push(tokio::spawn(async move {
            let db = ControlDb::connect(&url, "").await.unwrap();
            db.migrate_with(list, 100 + i).await
        }));
    }
    for r in runners {
        assert_eq!(r.await.unwrap(), Ok(3));
    }
    let rows = sqld::migration_rows(&t.url).await;
    assert_eq!(
        rows.iter().map(|r| r.0).collect::<Vec<_>>(),
        vec![1, 2, 3],
        "{rows:?}"
    );
    assert_eq!(
        sqld::tables(&t.url).await,
        vec!["r2", "r3", "schema_migrations"]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn ping_fails_on_an_unreachable_server() {
    let db = ControlDb::connect("http://127.0.0.1:9", "").await.unwrap();
    assert!(matches!(db.ping().await, Err(ControlError::Db(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn errors_never_carry_the_token() {
    let token = "tok-SECRET-0123456789";
    let db = ControlDb::connect("http://127.0.0.1:9", token)
        .await
        .unwrap();
    let err = db.ping().await.unwrap_err().to_string();
    assert!(!err.contains(token), "{err}");
}

// The local backend (`arena-cloud --dev`): the same runner over a rusqlite file.
mod local {
    use super::*;
    use std::path::Path;

    pub(super) fn rows_of(path: &Path) -> Vec<(i64, i64)> {
        rows(path)
    }

    pub(super) fn tables_of(path: &Path) -> Vec<String> {
        tables(path)
    }

    fn rows(path: &Path) -> Vec<(i64, i64)> {
        let c = rusqlite::Connection::open(path).unwrap();
        let mut st = c
            .prepare("SELECT version, applied_at FROM schema_migrations ORDER BY version")
            .unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn tables(path: &Path) -> Vec<String> {
        let c = rusqlite::Connection::open(path).unwrap();
        let mut st = c
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .unwrap();
        st.query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_fresh_file_gets_every_migration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gz_control.db");
        let db = ControlDb::open_local(&path).await.unwrap();
        db.ping().await.unwrap();
        assert_eq!(db.schema_version().await.unwrap(), 0);
        assert_eq!(db.migrate(1_000).await.unwrap(), LATEST_CONTROL_VERSION);
        assert_eq!(db.schema_version().await.unwrap(), LATEST_CONTROL_VERSION);
        assert_eq!(rows(&path), all_rows(1_000));
        assert_eq!(tables(&path), ALL_TABLES);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_restart_at_latest_applies_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gz_control.db");
        ControlDb::open_local(&path)
            .await
            .unwrap()
            .migrate(1_000)
            .await
            .unwrap();
        let again = ControlDb::open_local(&path).await.unwrap();
        assert_eq!(again.migrate(2_000).await.unwrap(), LATEST_CONTROL_VERSION);
        assert_eq!(rows(&path), all_rows(1_000));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_newer_file_is_refused_and_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gz_control.db");
        let too_new = i64::from(LATEST_CONTROL_VERSION) + 1;
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(&format!(
                "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);
                 INSERT INTO schema_migrations VALUES ({too_new}, 5);"
            ))
            .unwrap();
        let db = ControlDb::open_local(&path).await.unwrap();
        let err = db.migrate(9_000).await.unwrap_err();
        assert_eq!(
            err,
            ControlError::SchemaTooNew(SchemaTooNew {
                db: LATEST_CONTROL_VERSION + 1,
                binary: LATEST_CONTROL_VERSION,
            })
        );
        assert_eq!(rows(&path), vec![(too_new, 5)]);
        assert_eq!(tables(&path), vec!["schema_migrations"]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_failing_migration_leaves_neither_its_changes_nor_its_row() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gz_control.db");
        let mut list = BASELINE.to_vec();
        list.push(ControlMigration {
            version: 2,
            sql: "CREATE TABLE half (id INTEGER); INSERT INTO missing VALUES (1);",
        });
        let db = ControlDb::open_local(&path).await.unwrap();
        let err = db.migrate_with(&list, 7).await.unwrap_err();
        assert!(
            matches!(&err, ControlError::Migration { version: 2, message } if message.contains("no such table")),
            "{err:?}"
        );
        assert_eq!(rows(&path), vec![(1, 7)]);
        assert_eq!(tables(&path), vec!["schema_migrations"]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn pending_migrations_apply_in_order_each_with_its_row() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gz_control.db");
        let mut list = BASELINE.to_vec();
        list.push(ControlMigration {
            version: 2,
            sql: "CREATE TABLE t2 (id INTEGER PRIMARY KEY);",
        });
        let db = ControlDb::open_local(&path).await.unwrap();
        db.migrate_with(BASELINE, 1).await.unwrap();
        assert_eq!(db.migrate_with(&list, 7).await.unwrap(), 2);
        assert_eq!(rows(&path), vec![(1, 1), (2, 7)]);
    }
}

// Dump and restore (Story 2.3), on both backends and across them.
mod dump_restore {
    use super::*;
    use std::path::{Path, PathBuf};

    /// Tables of every shape: rowid with an INTEGER PRIMARY KEY, WITHOUT ROWID, AUTOINCREMENT
    /// fed by a trigger, no key with a generated column; an index and a view. More than one page
    /// in the first two.
    const FIXTURE: &str = "
        CREATE TABLE owners (id INTEGER PRIMARY KEY, name TEXT NOT NULL, note TEXT, avatar BLOB,
            balance INTEGER, ratio REAL);
        -- a comment inside the schema
        CREATE TABLE licences (code TEXT PRIMARY KEY, owner_id INTEGER REFERENCES owners(id),
            n INTEGER) WITHOUT ROWID;
        CREATE TABLE audit (id INTEGER PRIMARY KEY AUTOINCREMENT, msg TEXT);
        CREATE TABLE plain (a, b, full TEXT GENERATED ALWAYS AS (a || '-' || b) VIRTUAL);
        CREATE INDEX owners_name ON owners (name);
        CREATE VIEW owner_names AS SELECT name FROM owners;
        CREATE TRIGGER owners_audit AFTER INSERT ON owners BEGIN
            INSERT INTO audit (msg) VALUES ('owner ' || NEW.id);
        END;
        INSERT INTO owners VALUES (1, 'O''Brien \"q\"' || char(10) || 'line 2' || char(13) || char(10)
            || '😀 नमस्ते' || char(0) || 'after nul', NULL, X'00FF10', 9223372036854775807, 0.1);
        INSERT INTO owners VALUES (2, 'plain', '', X'', -9223372036854775807 - 1, 1e300);
        INSERT INTO owners VALUES (3, 'reals', 'x', NULL, 0, 3.0);
        INSERT INTO owners VALUES (4, 'tiny', NULL, NULL, NULL, 2.2250738585072014e-308);
        WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 2500)
            INSERT INTO owners (id, name, balance) SELECT x + 10, 'bulk ' || x, x FROM c;
        WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 1200)
            INSERT INTO licences SELECT printf('L%05d', x), NULL, x FROM c;
        INSERT INTO plain (a, b) VALUES ('a', 1), ('b', 2), ('c', 3);
        DELETE FROM plain WHERE a = 'b';
    ";

    fn local_exec(path: &Path, sql: &str) {
        rusqlite::Connection::open(path)
            .unwrap()
            .execute_batch(sql)
            .unwrap();
    }

    fn local_tables(path: &Path) -> Vec<String> {
        super::local::tables_of(path)
    }

    async fn seeded_local(dir: &Path) -> (ControlDb, PathBuf) {
        let path = dir.join("source.db");
        let db = ControlDb::open_local(&path).await.unwrap();
        db.migrate_with(BASELINE, 1_000).await.unwrap();
        local_exec(&path, FIXTURE);
        (db, path)
    }

    fn expected_counts() -> Vec<(&'static str, u64)> {
        vec![
            ("audit", 2504),
            ("licences", 1200),
            ("owners", 2504),
            ("plain", 2),
            ("schema_migrations", 1),
        ]
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_local_round_trip_keeps_every_value_count_and_object() {
        let dir = tempfile::tempdir().unwrap();
        let (src, _) = seeded_local(dir.path()).await;
        let dump = src.dump(42).await.unwrap();
        assert!(
            dump.starts_with("-- arena-control-dump v1\n-- manifest: {"),
            "{}",
            &dump[..80]
        );
        assert!(dump.contains("\"taken_at\":42"));
        // One INSERT per row, each on one line.
        assert_eq!(
            dump.lines()
                .filter(|l| l.starts_with("INSERT INTO"))
                .count(),
            2504 * 2 + 1200 + 2 + 1
        );

        let target_path = dir.path().join("target.db");
        let target = ControlDb::open_local(&target_path).await.unwrap();
        let report = target.restore(&dump).await.unwrap();
        assert_eq!(report.schema_version, 1);
        assert_eq!(
            report
                .tables
                .iter()
                .map(|(k, v)| (k.as_str(), *v))
                .collect::<Vec<_>>(),
            expected_counts()
        );
        // Byte-identical dump: same objects, same values, same storage classes.
        assert_eq!(target.dump(42).await.unwrap(), dump);
        // A restored database needs no migration.
        assert_eq!(target.migrate_with(BASELINE, 9_999).await.unwrap(), 1);

        // Read back independently.
        let c = rusqlite::Connection::open(&target_path).unwrap();
        let (name, note, avatar, balance, ratio): (String, Option<String>, Vec<u8>, i64, f64) = c
            .query_row(
                "SELECT name, note, avatar, balance, ratio FROM owners WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(name, "O'Brien \"q\"\nline 2\r\n😀 नमस्ते\0after nul");
        assert_eq!(note, None);
        assert_eq!(avatar, vec![0x00, 0xFF, 0x10]);
        assert_eq!(balance, i64::MAX);
        assert_eq!(ratio, 0.1);
        let (min, big, empty, kind): (i64, f64, Vec<u8>, String) = c
            .query_row(
                "SELECT balance, ratio, avatar, typeof(avatar) FROM owners WHERE id = 2",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            (min, big, empty.len(), kind.as_str()),
            (i64::MIN, 1e300, 0, "blob")
        );
        let kind: String = c
            .query_row("SELECT typeof(ratio) FROM owners WHERE id = 3", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(kind, "real");
        let full: String = c
            .query_row("SELECT full FROM plain WHERE a = 'c'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(full, "c-3");
        // The trigger exists again but did not fire during the restore.
        c.execute("INSERT INTO owners (id, name) VALUES (99999, 'new')", [])
            .unwrap();
        let audit: i64 = c
            .query_row("SELECT COUNT(*) FROM audit", [], |r| r.get(0))
            .unwrap();
        assert_eq!(audit, 2505);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_non_empty_target_is_refused_and_left_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let (src, _) = seeded_local(dir.path()).await;
        let dump = src.dump(1).await.unwrap();
        let path = dir.path().join("target.db");
        let target = ControlDb::open_local(&path).await.unwrap();
        target.migrate_with(BASELINE, 5).await.unwrap();
        let err = target.restore(&dump).await.unwrap_err();
        assert!(
            matches!(&err, ControlError::TargetNotEmpty { objects } if objects == &vec!["table schema_migrations".to_owned()]),
            "{err:?}"
        );
        assert_eq!(err.code(), "RESTORE_TARGET_NOT_EMPTY");
        assert_eq!(
            err.to_string().matches("RESTORE_TARGET_NOT_EMPTY").count(),
            1
        );
        assert_eq!(local_tables(&path), vec!["schema_migrations"]);
        assert_eq!(super::local::rows_of(&path), vec![(1, 5)]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_tampered_dump_fails_naming_the_table() {
        let dir = tempfile::tempdir().unwrap();
        let (src, _) = seeded_local(dir.path()).await;
        let dump = src.dump(1).await.unwrap();
        let tampered: String = dump
            .lines()
            .filter(|l| !l.starts_with("INSERT INTO \"plain\" (\"a\", \"b\") VALUES ('c'"))
            .map(|l| format!("{l}\n"))
            .collect();
        assert_ne!(tampered, dump);
        let target = ControlDb::open_local(&dir.path().join("t.db"))
            .await
            .unwrap();
        let err = target.restore(&tampered).await.unwrap_err();
        assert_eq!(
            err,
            ControlError::CountMismatch {
                table: "plain".into(),
                expected: Some(2),
                found: Some(1),
            }
        );
        assert!(err.to_string().contains("plain"), "{err}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_newer_or_truncated_dump_is_refused_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let (src, _) = seeded_local(dir.path()).await;
        let dump = src.dump(1).await.unwrap();
        let path = dir.path().join("t.db");
        let target = ControlDb::open_local(&path).await.unwrap();

        let newer = dump.replacen(
            "\"schema_version\":1",
            &format!("\"schema_version\":{}", LATEST_CONTROL_VERSION + 1),
            1,
        );
        let err = target.restore(&newer).await.unwrap_err();
        assert_eq!(
            err,
            ControlError::SchemaTooNew(SchemaTooNew {
                db: LATEST_CONTROL_VERSION + 1,
                binary: LATEST_CONTROL_VERSION
            })
        );
        let truncated = &dump[..dump.len() / 2];
        assert!(matches!(
            target.restore(truncated).await,
            Err(ControlError::DumpInvalid(_))
        ));
        assert!(local_tables(&path).is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_empty_database_dumps_and_restores() {
        let dir = tempfile::tempdir().unwrap();
        let src = ControlDb::open_local(&dir.path().join("s.db"))
            .await
            .unwrap();
        src.migrate_with(BASELINE, 1).await.unwrap();
        let dump = src.dump(1).await.unwrap();
        let target = ControlDb::open_local(&dir.path().join("t.db"))
            .await
            .unwrap();
        let report = target.restore(&dump).await.unwrap();
        assert_eq!(
            report.tables.into_iter().collect::<Vec<_>>(),
            vec![("schema_migrations".to_owned(), 1)]
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_remote_round_trip_matches_and_crosses_backends() {
        let Some(t) = sqld::fresh().await else { return };
        let db = ControlDb::connect(&t.url, "").await.unwrap();
        db.migrate_with(BASELINE, 1_000).await.unwrap();
        sqld::exec(&t.url, FIXTURE).await;
        let dump = db.dump(7).await.unwrap();

        // The local backend renders the same database identically.
        let dir = tempfile::tempdir().unwrap();
        let local = ControlDb::open_local(&dir.path().join("l.db"))
            .await
            .unwrap();
        local.restore(&dump).await.unwrap();
        assert_eq!(local.dump(7).await.unwrap(), dump);

        // Non-empty remote target.
        let err = db.restore(&dump).await.unwrap_err();
        assert!(
            matches!(err, ControlError::TargetNotEmpty { .. }),
            "{err:?}"
        );

        // Restore into the emptied server.
        sqld::wipe(&t.url).await;
        assert!(sqld::tables(&t.url).await.is_empty());
        let fresh = ControlDb::connect(&t.url, "").await.unwrap();
        let report = fresh.restore(&dump).await.unwrap();
        assert_eq!(
            report
                .tables
                .iter()
                .map(|(k, v)| (k.as_str(), *v))
                .collect::<Vec<_>>(),
            expected_counts()
        );
        assert_eq!(fresh.dump(7).await.unwrap(), dump);
        assert_eq!(sqld::migration_rows(&t.url).await, vec![(1, 1_000)]);
        assert_eq!(fresh.migrate_with(BASELINE, 2_000).await.unwrap(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_remote_restore_refuses_newer_and_tampered_dumps() {
        let Some(t) = sqld::fresh().await else { return };
        let dir = tempfile::tempdir().unwrap();
        let (src, _) = seeded_local(dir.path()).await;
        let dump = src.dump(1).await.unwrap();
        let db = ControlDb::connect(&t.url, "").await.unwrap();

        let newer = dump.replacen(
            "\"schema_version\":1",
            &format!("\"schema_version\":{}", LATEST_CONTROL_VERSION + 1),
            1,
        );
        assert!(matches!(
            db.restore(&newer).await,
            Err(ControlError::SchemaTooNew(_))
        ));
        assert!(sqld::tables(&t.url).await.is_empty());

        let tampered = dump.replacen("\"licences\":1200", "\"licences\":1201", 1);
        let err = db.restore(&tampered).await.unwrap_err();
        assert_eq!(
            err,
            ControlError::CountMismatch {
                table: "licences".into(),
                expected: Some(1201),
                found: Some(1200),
            }
        );
    }

    /// A child table created (so dumped) before its parent, with more rows than one restore
    /// batch, so children land in batches before their parents.
    const FK_FIXTURE: &str = "
        PRAGMA foreign_keys = OFF;
        CREATE TABLE child (id INTEGER PRIMARY KEY, parent_id INTEGER NOT NULL REFERENCES parent(id));
        CREATE TABLE parent (id INTEGER PRIMARY KEY);
        WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 700)
            INSERT INTO child SELECT x, x FROM c;
        WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 700)
            INSERT INTO parent SELECT x FROM c;
    ";

    #[tokio::test(flavor = "multi_thread")]
    async fn children_restored_before_their_parents_pass_and_orphans_fail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        let src = ControlDb::open_local(&path).await.unwrap();
        src.migrate_with(BASELINE, 1).await.unwrap();
        local_exec(&path, FK_FIXTURE);
        let dump = src.dump(1).await.unwrap();
        let t = ControlDb::open_local(&dir.path().join("t.db"))
            .await
            .unwrap();
        let report = t.restore(&dump).await.unwrap();
        assert_eq!(report.tables["child"], 700);

        // An orphan (written with foreign keys off) fails the restore naming the table.
        local_exec(
            &path,
            "PRAGMA foreign_keys = OFF; INSERT INTO child VALUES (9999, 424242);",
        );
        let dump = src.dump(1).await.unwrap();
        let t = ControlDb::open_local(&dir.path().join("t2.db"))
            .await
            .unwrap();
        let err = t.restore(&dump).await.unwrap_err();
        assert_eq!(
            err,
            ControlError::ForeignKeyViolation {
                table: "child".into(),
                parent: "parent".into(),
            }
        );

        // The same on a remote target, where foreign keys may be enforced.
        let Some(sq) = sqld::fresh().await else {
            return;
        };
        let remote = ControlDb::connect(&sq.url, "").await.unwrap();
        local_exec(&path, "DELETE FROM child WHERE id = 9999;");
        let report = remote.restore(&src.dump(1).await.unwrap()).await.unwrap();
        assert_eq!(report.tables["parent"], 700);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn autoincrement_resumes_above_the_deleted_maximum() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        let src = ControlDb::open_local(&path).await.unwrap();
        src.migrate_with(BASELINE, 1).await.unwrap();
        local_exec(
            &path,
            "CREATE TABLE seq (id INTEGER PRIMARY KEY AUTOINCREMENT, v TEXT);
             WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 10)
                 INSERT INTO seq (v) SELECT 'v' || x FROM c;
             DELETE FROM seq WHERE id > 5;",
        );
        let dump = src.dump(1).await.unwrap();
        assert!(dump.contains("INSERT INTO sqlite_sequence (name, seq) VALUES ('seq', 10);"));
        let tpath = dir.path().join("t.db");
        ControlDb::open_local(&tpath)
            .await
            .unwrap()
            .restore(&dump)
            .await
            .unwrap();
        local_exec(&tpath, "INSERT INTO seq (v) VALUES ('new');");
        let id: i64 = rusqlite::Connection::open(&tpath)
            .unwrap()
            .query_row("SELECT id FROM seq WHERE v = 'new'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(id, 11);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn extreme_rowids_and_composite_keys_page_without_loss() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        let src = ControlDb::open_local(&path).await.unwrap();
        src.migrate_with(BASELINE, 1).await.unwrap();
        local_exec(
            &path,
            "CREATE TABLE r (v TEXT);
             INSERT INTO r (rowid, v) VALUES (-9223372036854775807 - 1, 'min'), (9223372036854775807, 'max');
             CREATE TABLE k (a TEXT, b INTEGER, v, PRIMARY KEY (b, a)) WITHOUT ROWID;
             WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 2100)
                 INSERT INTO k SELECT printf('a%d', x % 3), x / 3, x FROM c;",
        );
        let dump = src.dump(1).await.unwrap();
        assert!(dump.contains("'min'") && dump.contains("'max'"));
        let t = ControlDb::open_local(&dir.path().join("t.db"))
            .await
            .unwrap();
        let report = t.restore(&dump).await.unwrap();
        assert_eq!(report.tables["r"], 2);
        assert_eq!(report.tables["k"], 2100);
        assert_eq!(t.dump(1).await.unwrap(), dump);
    }
}
