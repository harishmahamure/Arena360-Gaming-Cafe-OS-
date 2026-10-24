//! A `gz_control` database for tests: a libsql server (sqld).
//!
//! - `ARENA_TEST_SQLD_URL` set (CI's service container): that server, wiped first, one test at
//!   a time (a process-wide lock).
//! - Otherwise `sqld` on `PATH` is spawned per test on a free loopback port with a temporary
//!   data directory.
//! - Otherwise the test is skipped with a printed reason, unless `ARENA_REQUIRE_SQLD=1`, which
//!   makes the absence fail.

#![allow(dead_code)]

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

// Async-aware: the guard lives in `TestDb` across awaits for the whole test.
static SHARED: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A running test database. Dropping it stops a spawned server.
pub struct TestDb {
    pub url: String,
    child: Option<Child>,
    _dir: Option<tempfile::TempDir>,
    _lock: Option<tokio::sync::MutexGuard<'static, ()>>,
}

impl Drop for TestDb {
    fn drop(&mut self) {
        if let Some(c) = &mut self.child {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn require() -> bool {
    std::env::var("ARENA_REQUIRE_SQLD").is_ok_and(|v| v == "1")
}

fn skip(reason: &str) -> Option<TestDb> {
    assert!(!require(), "ARENA_REQUIRE_SQLD=1 but {reason}");
    eprintln!("skipping: {reason}");
    None
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn wait_for(addr: SocketAddr, child: &mut Child) -> bool {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Ok(Some(_)) = child.try_wait() {
            return false;
        }
        if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// Removes every user table from the shared server's database.
async fn wipe(url: &str) {
    let db = libsql::Builder::new_remote(url.to_owned(), String::new())
        .connector(hyper::client::HttpConnector::new())
        .build()
        .await
        .unwrap();
    let conn = db.connect().unwrap();
    let mut rows = conn
        .query(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '_litestream%' AND name NOT LIKE 'libsql_%'",
            (),
        )
        .await
        .unwrap();
    let mut names = Vec::new();
    while let Some(r) = rows.next().await.unwrap() {
        names.push(r.get::<String>(0).unwrap());
    }
    for n in names {
        conn.execute(&format!("DROP TABLE IF EXISTS \"{n}\""), ())
            .await
            .unwrap();
    }
}

/// A fresh, empty database, or `None` (skipped).
pub async fn fresh() -> Option<TestDb> {
    if let Ok(url) = std::env::var("ARENA_TEST_SQLD_URL")
        && !url.is_empty()
    {
        let lock = SHARED.lock().await;
        wipe(&url).await;
        return Some(TestDb {
            url,
            child: None,
            _dir: None,
            _lock: Some(lock),
        });
    }
    let dir = tempfile::tempdir().unwrap();
    let port = free_port();
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let spawned = Command::new("sqld")
        .arg("--http-listen-addr")
        .arg(addr.to_string())
        .arg("--db-path")
        .arg(dir.path().join("data.sqld"))
        .arg("--no-welcome")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            return skip(&format!(
                "sqld is not available ({e}); set ARENA_TEST_SQLD_URL"
            ));
        }
    };
    if !wait_for(addr, &mut child) {
        let _ = child.kill();
        return skip("sqld did not start listening");
    }
    Some(TestDb {
        url: format!("http://{addr}"),
        child: Some(child),
        _dir: Some(dir),
        _lock: None,
    })
}

/// Runs `sql` on the test database directly (fixtures only).
pub async fn exec(url: &str, sql: &str) {
    let db = libsql::Builder::new_remote(url.to_owned(), String::new())
        .connector(hyper::client::HttpConnector::new())
        .build()
        .await
        .unwrap();
    db.connect().unwrap().execute_batch(sql).await.unwrap();
}

/// The rows of `schema_migrations` as `(version, applied_at)`.
pub async fn migration_rows(url: &str) -> Vec<(i64, i64)> {
    let db = libsql::Builder::new_remote(url.to_owned(), String::new())
        .connector(hyper::client::HttpConnector::new())
        .build()
        .await
        .unwrap();
    let conn = db.connect().unwrap();
    let mut rows = conn
        .query(
            "SELECT version, applied_at FROM schema_migrations ORDER BY version",
            (),
        )
        .await
        .unwrap();
    let mut out = Vec::new();
    while let Some(r) = rows.next().await.unwrap() {
        out.push((r.get::<i64>(0).unwrap(), r.get::<i64>(1).unwrap()));
    }
    out
}

/// The user tables in the test database.
pub async fn tables(url: &str) -> Vec<String> {
    let db = libsql::Builder::new_remote(url.to_owned(), String::new())
        .connector(hyper::client::HttpConnector::new())
        .build()
        .await
        .unwrap();
    let conn = db.connect().unwrap();
    let mut rows = conn
        .query(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE 'libsql_%' ORDER BY name",
            (),
        )
        .await
        .unwrap();
    let mut out = Vec::new();
    while let Some(r) = rows.next().await.unwrap() {
        out.push(r.get::<String>(0).unwrap());
    }
    out
}
