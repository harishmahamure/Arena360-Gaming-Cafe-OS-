//! Story 9.7: a staged restore is applied when the Hub starts, before the writer opens, and the
//! Hub records `ops.restore` once it is primary.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use arena_proto::OpsRestore;
use arena_store::{Store, set_meta};
use rusqlite::Connection;

const T: i64 = 1_800_000_000_000;
const BRANCH: &str = "01JBRANCH";

/// `arena_hub::stop` is process-wide, so the tests here take turns.
static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn healthz(port: u16) -> Option<String> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    s.write_all(b"GET /healthz HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    Some(out)
}

fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for {what}");
}

fn spawn_hub(dir: &Path, port: u16) -> std::thread::JoinHandle<ExitCode> {
    let args: Vec<OsString> = vec![
        "--data-dir".into(),
        dir.into(),
        "--health-port".into(),
        port.to_string().into(),
        "--lan-port".into(),
        free_port().to_string().into(),
        "--web-port".into(),
        free_port().to_string().into(),
    ];
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(arena_hub::start(args.into_iter()))
    })
}

fn add_changes(db: &Path, epoch: &str, from: i64, to: i64) {
    let conn = Connection::open(db).unwrap();
    for seq in from..=to {
        conn.execute(
            "INSERT INTO changes (epoch, seq, tbl, row_id, op, row, intent_id, intent_kind,
                                  actor_kind, actor_id, at)
             VALUES (?1, ?2, 't', 'r', 'insert', x'00', 'i', 'k', 'system', 'a', 0)",
            rusqlite::params![epoch, seq],
        )
        .unwrap();
    }
}

/// A branch database on epoch `1:HUBA` with three changes.
fn branch(dir: &Path) -> PathBuf {
    let db = dir.join("branch.db");
    let mut store = Store::open(&db, T).unwrap();
    store
        .write_tx(|tx| {
            tx.execute(
                "INSERT INTO branch (branch_id, name, branch_code, currency, tz, day_start_min,
                                     fy_start_month, rev, created_at, updated_at)
                 VALUES (?1, 'Level Up', 'LVL01', 'INR', 'Asia/Kolkata', 240, 4, 1, 0, 0)",
                [BRANCH],
            )
            .unwrap();
            set_meta(tx, arena_store::META_HUB_INSTANCE_ID, "HUBA")?;
            arena_store::write_lease(
                tx,
                &arena_store::LeaseRow {
                    counter: 1,
                    hub_instance_id: "HUBA".into(),
                    provisional: false,
                    renewed_at: T,
                    expires_at: T,
                },
            )
        })
        .unwrap();
    drop(store);
    add_changes(&db, "1:HUBA", 1, 3);
    db
}

#[test]
fn a_staged_restore_is_applied_at_start_and_recorded_once_the_hub_is_primary() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let db = branch(dir.path());
    let file = arena_store::write_export(&db, out.path(), "LVL01-x.a360", T)
        .unwrap()
        .path;
    // Trading carried on after the file was made: three rows to set aside.
    add_changes(&db, "1:HUBA", 4, 6);
    arena_store::stage_export(&db, &file, "an-owner-the-file-never-had").unwrap();

    let port = free_port();
    let hub = spawn_hub(dir.path(), port);
    wait_until("healthz to report primary", || {
        healthz(port).is_some_and(|b| b.contains("\"role\":\"primary\""))
    });
    let record = || -> Option<OpsRestore> {
        let reader = Store::open_reader(&db).ok()?;
        let value = reader
            .settings()
            .ok()?
            .into_iter()
            .find(|s| s.key == "backup.last_restore")?
            .value;
        OpsRestore::parse(&value)
    };
    wait_until("ops.restore to be recorded", || record().is_some());
    let done = record().unwrap();

    assert_eq!(done.old_epoch, "1:HUBA");
    assert_eq!(done.new_epoch, "2:HUBA");
    assert_eq!(done.rows_set_aside, 3);
    assert_eq!(done.header.epoch, "1:HUBA");
    assert_eq!(done.header.last_seq, 3);
    assert_eq!(done.file_name, "LVL01-x.a360");
    // The Owner who confirmed is not in the file, so the Hub recorded it as itself.
    assert_eq!(done.by, None);

    // The old database is set aside whole; the live one holds the file's rows and the new epoch.
    let orphaned: Vec<_> = std::fs::read_dir(dir.path().join("orphaned"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(orphaned.len(), 1, "{orphaned:?}");
    assert_eq!(
        orphaned[0].file_name().unwrap(),
        done.orphaned_file.as_str()
    );
    let count = |db: &Path, sql: &str| -> i64 {
        Connection::open(db)
            .unwrap()
            .query_row(sql, [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(count(&orphaned[0], "SELECT COUNT(*) FROM changes"), 6);
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM changes WHERE epoch = '1:HUBA'"),
        3
    );
    assert!(
        count(&db, "SELECT COUNT(*) FROM changes WHERE epoch = '2:HUBA'") >= 1,
        "the new epoch wrote the record"
    );
    let reader = Store::open_reader(&db).unwrap();
    let lease = reader.lease().unwrap().unwrap();
    assert_eq!((lease.counter, lease.hub_instance_id.as_str()), (2, "HUBA"));
    assert!(!dir.path().join("restore-pending").exists());
    assert!(!dir.path().join("restore-result.json").exists());
    drop(reader);

    arena_hub::stop();
    wait_until("the hub thread to end", || hub.is_finished());
    assert_eq!(
        format!("{:?}", hub.join().unwrap()),
        format!("{:?}", ExitCode::SUCCESS)
    );
}

#[test]
fn a_staged_restore_that_fails_leaves_the_database_and_the_hub_starts_normally() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let db = branch(dir.path());
    let file = arena_store::write_export(&db, out.path(), "LVL01-x.a360", T)
        .unwrap()
        .path;
    arena_store::stage_export(&db, &file, "owner").unwrap();
    // The staged copy is damaged after it was staged.
    let staged = dir.path().join("restore-pending").join("restore.a360");
    let mut bytes = std::fs::read(&staged).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&staged, bytes).unwrap();

    let port = free_port();
    let hub = spawn_hub(dir.path(), port);
    wait_until("healthz to report primary", || {
        healthz(port).is_some_and(|b| b.contains("\"role\":\"primary\""))
    });
    assert!(!dir.path().join("restore-pending").exists());
    assert!(!dir.path().join("orphaned").exists());
    let reader = Store::open_reader(&db).unwrap();
    assert_eq!(reader.lease().unwrap().unwrap().counter, 1);
    drop(reader);

    arena_hub::stop();
    wait_until("the hub thread to end", || hub.is_finished());
    hub.join().unwrap();
}
