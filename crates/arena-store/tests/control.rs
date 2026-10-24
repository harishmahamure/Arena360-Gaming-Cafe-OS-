//! `gz_control` migrations against a real libsql server (see `support/sqld.rs`).

#![cfg(feature = "control")]

#[path = "support/sqld.rs"]
mod sqld;

use arena_store::control::{
    CONTROL_MIGRATIONS, ControlDb, ControlError, ControlMigration, LATEST_CONTROL_VERSION,
    SchemaTooNew,
};

#[tokio::test(flavor = "multi_thread")]
async fn a_fresh_database_gets_the_baseline_and_no_business_tables() {
    let Some(t) = sqld::fresh().await else { return };
    let db = ControlDb::connect(&t.url, "").await.unwrap();
    assert_eq!(db.schema_version().await.unwrap(), 0);
    assert_eq!(db.migrate(1_000).await.unwrap(), 1);
    assert_eq!(db.schema_version().await.unwrap(), LATEST_CONTROL_VERSION);
    assert_eq!(sqld::migration_rows(&t.url).await, vec![(1, 1_000)]);
    assert_eq!(sqld::tables(&t.url).await, vec!["schema_migrations"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restart_at_latest_applies_nothing() {
    let Some(t) = sqld::fresh().await else { return };
    let db = ControlDb::connect(&t.url, "").await.unwrap();
    db.migrate(1_000).await.unwrap();
    let again = ControlDb::connect(&t.url, "").await.unwrap();
    assert_eq!(again.migrate(2_000).await.unwrap(), 1);
    assert_eq!(sqld::migration_rows(&t.url).await, vec![(1, 1_000)]);
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
    let mut list = CONTROL_MIGRATIONS.to_vec();
    list.push(ControlMigration {
        version: 2,
        sql: "CREATE TABLE t2 (id INTEGER PRIMARY KEY);",
    });
    list.push(ControlMigration {
        version: 3,
        sql: "ALTER TABLE t2 ADD COLUMN name TEXT;",
    });
    let db = ControlDb::connect(&t.url, "").await.unwrap();
    db.migrate(1).await.unwrap();
    assert_eq!(db.migrate_with(&list, 7).await.unwrap(), 3);
    assert_eq!(
        sqld::migration_rows(&t.url).await,
        vec![(1, 1), (2, 7), (3, 7)]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_migration_leaves_neither_its_changes_nor_its_row() {
    let Some(t) = sqld::fresh().await else { return };
    let mut list = CONTROL_MIGRATIONS.to_vec();
    list.push(ControlMigration {
        version: 2,
        sql: "CREATE TABLE half (id INTEGER); INSERT INTO missing VALUES (1);",
    });
    let db = ControlDb::connect(&t.url, "").await.unwrap();
    let started = std::time::Instant::now();
    let err = db.migrate_with(&list, 7).await.unwrap_err();
    assert!(
        matches!(&err, ControlError::Migration { version: 2, message } if message.contains("no such table")),
        "{err:?}"
    );
    // Not retried: a bad migration fails at once.
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    assert_eq!(sqld::migration_rows(&t.url).await, vec![(1, 7)]);
    assert_eq!(sqld::tables(&t.url).await, vec!["schema_migrations"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_runners_both_succeed_with_one_row_per_version() {
    let Some(t) = sqld::fresh().await else { return };
    let list: &'static [ControlMigration] = Box::leak(
        [
            CONTROL_MIGRATIONS.to_vec(),
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
