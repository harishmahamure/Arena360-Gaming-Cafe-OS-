//! Crash safety: a child process commits a row and is killed (SIGKILL / TerminateProcess)
//! right after; the committed row must survive and the database must pass `integrity_check`.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use arena_store::{Store, get_meta, set_meta};

const CHILD_ENV: &str = "ARENA_STORE_CRASH_CHILD_DB";
const TEST_NAME: &str = "committed_row_survives_kill";
const KEY: &str = "crash_probe";
const VALUE: &str = "committed";
const COMMIT_TIMEOUT: Duration = Duration::from_secs(30);

fn child(db: PathBuf) -> ! {
    let mut store = Store::open(&db, 1).expect("child open");
    store
        .write_tx(|tx| set_meta(tx, KEY, VALUE))
        .expect("child commit");
    println!("COMMITTED");
    // Keep the connection open (no clean close, no checkpoint) until killed.
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[test]
fn committed_row_survives_kill() {
    if let Some(db) = std::env::var_os(CHILD_ENV) {
        child(PathBuf::from(db));
    }

    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("branch.db");
    let mut proc = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST_NAME, "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, &db)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    // Read the child's stdout on a thread so the wait for COMMITTED can time out.
    let stdout = proc.stdout.take().unwrap();
    let (seen_tx, seen_rx) = std::sync::mpsc::channel::<bool>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            // libtest may print "test <name> ... " on the same line first.
            if line.trim_end().ends_with("COMMITTED") {
                let _ = seen_tx.send(true);
                return;
            }
        }
        let _ = seen_tx.send(false);
    });
    let committed = seen_rx.recv_timeout(COMMIT_TIMEOUT);
    proc.kill().unwrap();
    proc.wait().unwrap();
    match committed {
        Ok(true) => {}
        Ok(false) => panic!("child exited before committing"),
        Err(_) => panic!("child did not print COMMITTED within {COMMIT_TIMEOUT:?}"),
    }

    let store = Store::open(&db, 2).unwrap();
    assert_eq!(store.integrity_check().unwrap(), "ok");
    let conn_value = store.meta(KEY).unwrap();
    assert_eq!(conn_value.as_deref(), Some(VALUE));
    // Also readable through the free function used inside write jobs.
    let mut store = store;
    let in_tx = store.write_tx(|tx| get_meta(tx, KEY)).unwrap();
    assert_eq!(in_tx.as_deref(), Some(VALUE));
}
