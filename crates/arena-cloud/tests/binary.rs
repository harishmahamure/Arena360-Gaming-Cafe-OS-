//! The `arena-cloud` binary end to end: exit codes, start-up against a real libsql server and
//! `/healthz` over TCP.

#[path = "../../arena-store/tests/support/sqld.rs"]
mod sqld;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const TOKEN: &str = "tok-SECRET-do-not-log-0123456789";

fn cloud() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_arena-cloud"));
    c.env_remove("ARENA_CONTROL_DB_URL")
        .env_remove("ARENA_CONTROL_DB_TOKEN")
        .env_remove("ARENA_CONTROL_DB_RETRY_SECS")
        .env("RUST_LOG", "info")
        // The sign-up and MFA settings every serving role needs (Stories 2.4, 2.5).
        .env("ARENA_OTP_PEPPER", "test-pepper-0123456789abcdef0123456789")
        .env("ARENA_WEB_ORIGINS", "https://start.arena360.cloud")
        .env("ARENA_SMS_PROVIDER", "gupshup")
        .env(
            "ARENA_GUPSHUP_ENDPOINT",
            "http://127.0.0.1:9/GatewayAPI/rest",
        )
        .env("ARENA_GUPSHUP_USERID", "u")
        .env("ARENA_GUPSHUP_PASSWORD", "p")
        .env("ARENA_GUPSHUP_SENDER", "ARENA")
        .env("ARENA_GUPSHUP_DLT_TEMPLATE_ID", "1")
        .env(
            "ARENA_MFA_KEY",
            "dGVzdC1tZmEta2V5LTAxMjM0NTY3ODlhYmNkZWZnaGk=",
        )
        .env("ARENA_LICENCE_SIGNING_KEY", licence_pem())
        .env("ARENA_LICENCE_KID", "test-1");
    c
}

fn licence_pem() -> &'static str {
    static PEM: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PEM.get_or_init(|| arena_cloud::licence::generate(Some("test-1")).unwrap().pem)
}

fn run(c: &mut Command) -> Output {
    c.stdin(Stdio::null()).output().unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn help_and_version_exit_zero() {
    let o = run(cloud().arg("--help"));
    assert_eq!(o.status.code(), Some(0));
    assert!(text(&o).contains("--role control"));
    let o = run(cloud().arg("--version"));
    assert_eq!(o.status.code(), Some(0));
    assert!(text(&o).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn bad_role_exits_64() {
    let o = run(cloud()
        .args(["--role", "builder"])
        .env("ARENA_CONTROL_DB_URL", "libsql://x.turso.io")
        .env("ARENA_CONTROL_DB_TOKEN", "t"));
    assert_eq!(o.status.code(), Some(64));
    assert!(text(&o).contains("role not available yet"), "{}", text(&o));
}

#[test]
fn a_missing_or_dev_licence_key_exits_64_naming_the_variable() {
    let o = run(cloud()
        .args(["--role", "control"])
        .env("ARENA_CONTROL_DB_URL", "libsql://x.turso.io")
        .env("ARENA_CONTROL_DB_TOKEN", TOKEN)
        .env_remove("ARENA_LICENCE_SIGNING_KEY"));
    assert_eq!(o.status.code(), Some(64));
    assert!(
        text(&o).contains("ARENA_LICENCE_SIGNING_KEY is not set"),
        "{}",
        text(&o)
    );
    let o = run(cloud()
        .args(["--role", "control"])
        .env("ARENA_CONTROL_DB_URL", "libsql://x.turso.io")
        .env("ARENA_CONTROL_DB_TOKEN", TOKEN)
        .env(
            "ARENA_LICENCE_SIGNING_KEY",
            arena_cloud::licence::DEV_SIGNING_KEY_PEM,
        ));
    assert_eq!(o.status.code(), Some(64));
    assert!(text(&o).contains("dev key"), "{}", text(&o));
}

#[test]
fn bad_or_missing_url_exits_64_naming_the_variable() {
    let o = run(cloud().args(["--role", "control"]));
    assert_eq!(o.status.code(), Some(64));
    assert!(text(&o).contains("ARENA_CONTROL_DB_URL"));
    let o = run(cloud()
        .args(["--role", "control"])
        .env("ARENA_CONTROL_DB_URL", "file:/tmp/x.db")
        .env("ARENA_CONTROL_DB_TOKEN", TOKEN));
    assert_eq!(o.status.code(), Some(64));
    assert!(text(&o).contains("ARENA_CONTROL_DB_URL"));
    assert!(!text(&o).contains(TOKEN));
}

#[test]
fn unreachable_at_start_retries_then_exits_1_without_logging_the_token() {
    let started = Instant::now();
    let o = run(cloud()
        .args(["--role", "control", "--listen", "127.0.0.1:0"])
        .env("ARENA_CONTROL_DB_URL", "http://127.0.0.1:9")
        .env("ARENA_CONTROL_DB_TOKEN", TOKEN)
        .env("ARENA_CONTROL_DB_RETRY_SECS", "2"));
    let out = text(&o);
    assert_eq!(o.status.code(), Some(1), "{out}");
    assert!(started.elapsed() >= Duration::from_secs(2), "{out}");
    assert!(out.contains("retrying"), "{out}");
    assert!(out.contains("giving up"), "{out}");
    assert!(!out.contains(TOKEN), "{out}");
    for line in String::from_utf8_lossy(&o.stdout).lines() {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|_| panic!("not JSON: {line}"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn too_new_exits_2_naming_both_versions_and_writes_nothing() {
    let Some(t) = sqld::fresh().await else { return };
    sqld::exec(
        &t.url,
        "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);
         INSERT INTO schema_migrations VALUES (99, 5);",
    )
    .await;
    let url = t.url.clone();
    let o = tokio::task::spawn_blocking(move || {
        run(cloud()
            .args(["--role", "control", "--listen", "127.0.0.1:0"])
            .env("ARENA_CONTROL_DB_URL", url))
    })
    .await
    .unwrap();
    let out = text(&o);
    assert_eq!(o.status.code(), Some(2), "{out}");
    assert!(out.contains("SCHEMA_TOO_NEW"), "{out}");
    assert!(out.contains("\"db_schema_version\":99"), "{out}");
    assert!(
        out.contains(&format!(
            "\"binary_schema_version\":{}",
            arena_store::control::LATEST_CONTROL_VERSION
        )),
        "{out}"
    );
    assert_eq!(sqld::migration_rows(&t.url).await, vec![(99, 5)]);
    assert_eq!(sqld::tables(&t.url).await, vec!["schema_migrations"]);
}

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn http_get(port: u16, path: &str) -> Option<(u16, String)> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut buf = String::new();
    s.read_to_string(&mut buf).ok()?;
    let status = buf.split(' ').nth(1)?.parse().ok()?;
    let body = buf.split_once("\r\n\r\n")?.1.to_owned();
    Some((status, body))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fresh_database_serves_healthz_at_the_latest_migration() {
    let Some(t) = sqld::fresh().await else { return };
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let child = cloud()
        .args([
            "--role",
            "control",
            "--listen",
            &format!("127.0.0.1:{port}"),
        ])
        .env("ARENA_CONTROL_DB_URL", &t.url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _running = Running(child);
    let found = tokio::task::spawn_blocking(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if let Some(r) = http_get(port, "/healthz") {
                return Some(r);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        None
    })
    .await
    .unwrap();
    let (status, body) = found.expect("healthz answered");
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["role"], "control");
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(v["gz_control"]["reachable"], true);
    assert!(v["gz_control"]["checked_at"].as_i64().unwrap() > 0);
    let latest = arena_store::control::LATEST_CONTROL_VERSION;
    assert_eq!(v["migration_version"], latest);
    assert_eq!(
        sqld::migration_rows(&t.url)
            .await
            .iter()
            .map(|r| r.0)
            .collect::<Vec<_>>(),
        (1..=i64::from(latest)).collect::<Vec<_>>()
    );
}
