//! The `/v1` error envelope end to end: `arena-cloud --role all --dev` on a temporary data dir
//! and port, every failure answered with the flat `AppError` body. Needs no sqld.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use arena_cloud::config::Role;
use arena_cloud::health::Reachability;
use arena_store::control::LATEST_CONTROL_VERSION as LATEST;
use axum::body::{Body, to_bytes};
use axum::http::Request;
use serde_json::Value;
use tokio::sync::watch;
use tower::ServiceExt;

fn cloud() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_arena-cloud"));
    c.env_remove("ARENA_CONTROL_DB_URL")
        .env_remove("ARENA_CONTROL_DB_TOKEN")
        .env_remove("ARENA_CONTROL_DB_RETRY_SECS")
        .env("RUST_LOG", "info");
    c
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|_| panic!("not JSON: {}", self.body))
    }
}

/// One HTTP/1.1 request over a fresh connection.
fn request(port: u16, method: &str, path: &str, body: Option<(&str, &[u8])>) -> Option<Reply> {
    let mut raw = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
    if let Some((ct, b)) = body {
        raw.push_str(&format!(
            "Content-Type: {ct}\r\nContent-Length: {}\r\n",
            b.len()
        ));
    }
    raw.push_str("\r\n");
    let mut raw = raw.into_bytes();
    if let Some((_, b)) = body {
        raw.extend_from_slice(b);
    }
    send(port, &raw)
}

/// Writes `raw` as-is and reads the reply until the server closes.
fn send(port: u16, raw: &[u8]) -> Option<Reply> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    s.write_all(raw).ok()?;
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).into_owned();
    let (head, body) = text.split_once("\r\n\r\n")?;
    let mut lines = head.lines();
    let status = lines.next()?.split(' ').nth(1)?.parse().ok()?;
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    Some(Reply {
        status,
        headers,
        body: body.to_owned(),
    })
}

/// Polls `/healthz` until `gz_control.reachable` is true (the first background check ran).
fn healthz_reachable(port: u16) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let h = request(port, "GET", "/healthz", None).unwrap().json();
        if h["gz_control"]["reachable"] == true {
            return h;
        }
        assert!(Instant::now() < deadline, "never reachable: {h}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Starts `arena-cloud --role all --dev` on `data_dir` and waits for `/healthz`.
fn start_dev(data_dir: &Path) -> (Running, u16) {
    let port = free_port();
    let child = cloud()
        .args(["--role", "all", "--dev", "--data-dir"])
        .arg(data_dir)
        .args(["--listen", &format!("127.0.0.1:{port}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let running = Running(child);
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if request(port, "GET", "/healthz", None).is_some_and(|r| r.status == 200) {
            return (running, port);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("arena-cloud --dev did not serve /healthz");
}

/// Asserts the flat envelope: status, code, JSON content type, a non-empty message and no
/// internals (stack trace, panic text, file path, Rust type name) anywhere in the body.
fn assert_envelope(r: &Reply, status: u16, code: &str) -> Value {
    assert_eq!(r.status, status, "{}", r.body);
    assert_eq!(
        r.header("content-type"),
        Some("application/json"),
        "{}",
        r.body
    );
    let v = r.json();
    assert_eq!(v["code"], code, "{}", r.body);
    assert!(
        v["message"].as_str().is_some_and(|m| !m.is_empty()),
        "{}",
        r.body
    );
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    assert!(
        keys.iter()
            .all(|k| ["code", "message", "field"].contains(&k.as_str())),
        "flat AppError only: {}",
        r.body
    );
    let lower = r.body.to_ascii_lowercase();
    for bad in [
        "panicked",
        "backtrace",
        "stack",
        ".rs",
        "::",
        "src/",
        "\\src",
        "rusqlite",
        "serde",
    ] {
        assert!(!lower.contains(bad), "{bad:?} in {}", r.body);
    }
    v
}

const JSON: &str = "application/json";

#[test]
fn every_failure_is_the_flat_envelope() {
    let dir = tempfile::tempdir().unwrap();
    let (_running, port) = start_dev(dir.path());
    let req = |m: &str, p: &str, b: Option<(&str, &[u8])>| {
        request(port, m, p, b).unwrap_or_else(|| panic!("{m} {p}: no answer"))
    };

    // Unknown path, inside and outside /v1.
    assert_envelope(&req("GET", "/v1/nope", None), 404, "NOT_FOUND");
    assert_envelope(&req("GET", "/nope", None), 404, "NOT_FOUND");

    // Wrong method.
    assert_envelope(&req("DELETE", "/v1/dev/echo", None), 405, "UNSUPPORTED");

    // A good body round-trips.
    let ok = req("POST", "/v1/dev/echo", Some((JSON, br#"{"text":"hi"}"#)));
    assert_eq!(ok.status, 200, "{}", ok.body);
    assert_eq!(ok.json(), serde_json::json!({"text": "hi"}));

    // Malformed JSON.
    let v = assert_envelope(
        &req("POST", "/v1/dev/echo", Some((JSON, br#"{"text":"#))),
        400,
        "BAD_REQUEST",
    );
    assert!(v.get("field").is_none(), "{v}");

    // Type mismatch names the field.
    let v = assert_envelope(
        &req("POST", "/v1/dev/echo", Some((JSON, br#"{"text": 5}"#))),
        400,
        "BAD_REQUEST",
    );
    assert_eq!(v["field"], "text");

    // Missing top-level field.
    let v = assert_envelope(
        &req("POST", "/v1/dev/echo", Some((JSON, b"{}"))),
        400,
        "BAD_REQUEST",
    );
    assert_eq!(v["field"], "text");

    // Not sent as JSON.
    assert_envelope(
        &req(
            "POST",
            "/v1/dev/echo",
            Some(("text/plain", br#"{"text":"hi"}"#)),
        ),
        400,
        "BAD_REQUEST",
    );

    // Over 64 KiB: exactly one byte over the limit. The server stops reading once the body
    // passes the limit (it does not refuse on Content-Length alone), so sending only
    // limit + 1 bytes leaves nothing unread that could reset the connection before the
    // reply is read.
    let over = vec![b'a'; arena_cloud::api::BODY_LIMIT + 1];
    assert_envelope(
        &req("POST", "/v1/dev/echo", Some((JSON, &over))),
        413,
        "BAD_REQUEST",
    );

    // Internal failure: the fixed message, never the detail.
    let fail = req("GET", "/v1/dev/fail", None);
    assert_envelope(&fail, 500, "INTERNAL");
    assert!(!fail.body.contains("deliberate"), "{}", fail.body);

    // A panic: INTERNAL, and the server keeps serving.
    let panic = req("GET", "/v1/dev/panic", None);
    assert_envelope(&panic, 500, "INTERNAL");
    assert!(!panic.body.contains("deliberate"), "{}", panic.body);
    let after = req("GET", "/healthz", None);
    assert_eq!(after.status, 200);
    let again = req("POST", "/v1/dev/echo", Some((JSON, br#"{"text":"still"}"#)));
    assert_eq!(again.status, 200, "{}", again.body);
}

fn migration_rows(path: &Path) -> Vec<(i64, i64)> {
    let c = rusqlite::Connection::open(path).unwrap();
    let mut st = c
        .prepare("SELECT version, applied_at FROM schema_migrations ORDER BY version")
        .unwrap();
    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn dev_starts_on_an_empty_dir_and_a_restart_applies_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("nested").join("dev");
    let db = data.join("gz_control.db");

    let (running, port) = start_dev(&data);
    let h = healthz_reachable(port);
    assert_eq!(h["role"], "all");
    assert_eq!(h["migration_version"], LATEST);
    drop(running);
    assert!(db.is_file(), "{} created", db.display());
    let first = migration_rows(&db);
    assert_eq!(
        first.iter().map(|r| r.0).collect::<Vec<_>>(),
        (1..=i64::from(LATEST)).collect::<Vec<_>>()
    );

    let (_running, port) = start_dev(&data);
    let h = request(port, "GET", "/healthz", None).unwrap().json();
    assert_eq!(h["role"], "all");
    assert_eq!(h["migration_version"], LATEST);
    assert_eq!(migration_rows(&db), first, "nothing applied on restart");
}

fn run(c: &mut Command) -> Output {
    c.stdin(Stdio::null()).output().unwrap()
}

#[test]
fn dev_on_a_too_new_file_exits_2_and_leaves_it_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("gz_control.db");
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch(
            &format!("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);
             INSERT INTO schema_migrations VALUES ({}, 5);", LATEST + 1),
        )
        .unwrap();
    let o = run(cloud()
        .args(["--role", "all", "--dev", "--data-dir"])
        .arg(dir.path())
        .args(["--listen", "127.0.0.1:0"]));
    let out = String::from_utf8_lossy(&o.stdout);
    assert_eq!(o.status.code(), Some(2), "{out}");
    assert!(out.contains("SCHEMA_TOO_NEW"), "{out}");
    assert_eq!(migration_rows(&db), vec![(i64::from(LATEST) + 1, 5)]);
}

#[test]
fn dev_refuses_a_non_loopback_listen_with_exit_64() {
    let dir = tempfile::tempdir().unwrap();
    let o = run(cloud()
        .args(["--role", "all", "--dev", "--data-dir"])
        .arg(dir.path())
        .args(["--listen", "0.0.0.0:8080"]));
    assert_eq!(o.status.code(), Some(64));
    assert!(String::from_utf8_lossy(&o.stderr).contains("loopback"));
    assert!(!dir.path().join("gz_control.db").exists());
}

async fn oneshot(dev: bool, method: &str, path: &str) -> (u16, Value) {
    let (_tx, rx) = watch::channel(Reachability {
        reachable: true,
        checked_at: Some(1),
    });
    let res = arena_cloud::app(Role::Control, dev, rx, 1)
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", JSON)
                .body(Body::from(r#"{"text":"x"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status().as_u16();
    let bytes = to_bytes(res.into_body(), 64 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn the_production_router_never_mounts_the_dev_routes() {
    for (method, path) in [
        ("GET", "/v1/dev/fail"),
        ("GET", "/v1/dev/panic"),
        ("POST", "/v1/dev/echo"),
    ] {
        let (status, body) = oneshot(false, method, path).await;
        assert_eq!(status, 404, "{method} {path}: {body}");
        assert_eq!(body["code"], "NOT_FOUND");
    }
    // The same paths exist with --dev.
    assert_eq!(oneshot(true, "GET", "/v1/dev/fail").await.0, 500);
    assert_eq!(oneshot(true, "POST", "/v1/dev/echo").await.0, 200);
}
