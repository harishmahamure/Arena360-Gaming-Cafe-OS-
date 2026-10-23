//! The running Hub prunes stale dedupe records: a data dir seeded with an `applied_intents` row
//! older than 30 days loses it once the Hub is primary.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use arena_store::{AppliedIntentRecord, Store, insert_applied_intent};

const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const KEY: &str = "01JOLDAAAAAAAAAAAAAAAAAAAA";

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// The `/healthz` body, or `None` while the Hub is not answering yet.
fn healthz(port: u16) -> Option<String> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    s.write_all(b"GET /healthz HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    Some(out)
}

/// How many dedupe records exist, read on a second connection (read-only while the Hub holds
/// the writer lock).
fn records(db: &Path) -> i64 {
    Store::open(db, 0).unwrap().applied_intent_count().unwrap()
}

fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

fn stop(mut child: Child) {
    #[cfg(unix)]
    {
        let status = Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap();
        assert!(status.success());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "hub exited with {status:?}");
                return;
            }
            assert!(Instant::now() < deadline, "hub did not stop on SIGTERM");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    #[cfg(not(unix))]
    {
        child.kill().unwrap();
        child.wait().unwrap();
    }
}

#[test]
fn running_hub_prunes_dedupe_records_older_than_30_days() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("branch.db");
    {
        let mut store = Store::open(&db, now_ms()).unwrap();
        store
            .write_tx(|tx| {
                insert_applied_intent(
                    tx,
                    &AppliedIntentRecord {
                        idempotency_key: KEY.into(),
                        intent_id: KEY.into(),
                        intent_kind: "settings.set".into(),
                        fingerprint: "0".repeat(64),
                        outcome: vec![0x80],
                        applied_at: now_ms() - 31 * DAY_MS,
                    },
                )
            })
            .unwrap();
    }
    assert_eq!(records(&db), 1);

    let port = free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_arena-hub"))
        .arg("--data-dir")
        .arg(dir.path())
        .args(["--health-port", &port.to_string()])
        .spawn()
        .unwrap();
    let mut child = Some(child);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wait_until("healthz to report primary", || {
            healthz(port).is_some_and(|body| body.contains("\"role\":\"primary\""))
        });
        // Housekeeping runs at start; give its writer job a moment to land.
        wait_until("the stale record to be pruned", || records(&db) == 0);
        stop(child.take().unwrap());
    }));
    if let Some(mut c) = child {
        let _ = c.kill();
        let _ = c.wait();
    }
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
    assert_eq!(records(&db), 0);
}
