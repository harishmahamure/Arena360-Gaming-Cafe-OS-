#![cfg(feature = "dev-fixtures")]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use arena_core::printer::{PaperWidth, test_page, test_page_date};
use serde_json::{Value, json};

fn hub() -> Command {
    Command::new(env!("CARGO_BIN_EXE_arena-hub"))
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn request(
    port: u16,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&Value>,
) -> Option<Value> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let body = body.map(Value::to_string).unwrap_or_default();
    let auth = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    serde_json::from_str(out.split_once("\r\n\r\n")?.1).ok()
}

struct Running(Option<Child>);

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn rev(port: u16, token: &str, key: &str) -> i64 {
    let list = request(port, "GET", "/ui/settings", Some(token), None).unwrap();
    list.as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == key)
        .unwrap()["rev"]
        .as_i64()
        .unwrap()
}

fn set(port: u16, token: &str, key: &str, value: &str) {
    let now = now_ms();
    let body = json!({"intent": {
        "intent_id": ulid::Ulid::generate().to_string(),
        "kind": "settings.set",
        "idempotency_key": ulid::Ulid::generate().to_string(),
        "issued_at": now,
        "base_rev": rev(port, token, key),
        "actor": {"kind": "staff", "id": "session"},
        "payload": {"kind": "settings.set", "key": key, "value": value},
    }});
    let reply = request(port, "POST", "/ui/intent", Some(token), Some(&body)).unwrap();
    assert!(reply.get("applied").is_some(), "{key}: {reply}");
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn a_manager_finds_the_fake_picks_58_mm_and_the_test_print_reports_ok() {
    let dir = tempfile::tempdir().unwrap();
    let seeded = hub()
        .args(["dev", "seed", "--data-dir"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(seeded.status.code(), Some(0), "{seeded:?}");
    let (health, lan, web) = (free_port(), free_port(), free_port());
    let _hub = Running(Some(
        hub()
            .arg("--data-dir")
            .arg(dir.path())
            .args(["--health-port", &health.to_string()])
            .args(["--lan-port", &lan.to_string()])
            .args(["--web-port", &web.to_string()])
            .args(["--printer", "fake"])
            .spawn()
            .unwrap(),
    ));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if request(health, "GET", "/healthz", None, None).is_some_and(|b| b["role"] == "primary") {
            break;
        }
        assert!(Instant::now() < deadline, "the hub never became primary");
        std::thread::sleep(Duration::from_millis(50));
    }
    let before = request(health, "GET", "/healthz", None, None).unwrap();
    assert_eq!(before["setup"]["printer"], "unset");

    let signed_in = request(
        health,
        "POST",
        "/ui/auth/sign-in",
        None,
        Some(&json!({"login": "manager", "password": "dev-manager-pass"})),
    )
    .unwrap();
    let token = signed_in["token"].as_str().unwrap().to_owned();

    let found = request(health, "POST", "/ui/printer/find", Some(&token), None).unwrap();
    let printers = found["printers"].as_array().unwrap();
    assert_eq!(printers.len(), 2, "{found}");
    let usb = printers.iter().find(|p| p["kind"] == "usb").unwrap();
    let selected = json!({"kind": "usb", "id_or_addr": usb["id"], "name": usb["name"]});
    set(health, &token, "printer.selected", &selected.to_string());
    set(health, &token, "printer.paper_mm", "58");

    let tz = request(health, "GET", "/ui/snapshot", None, None).unwrap()["locale"]["tz"]
        .as_str()
        .unwrap()
        .to_owned();
    let date_before = test_page_date(now_ms(), &tz);
    let tested = request(health, "POST", "/ui/printer/test", Some(&token), None).unwrap();
    let date_after = test_page_date(now_ms(), &tz);
    assert_eq!(tested["status"]["state"], "ok", "{tested}");

    let jobs = request(health, "GET", "/dev/printer/jobs", None, None).unwrap();
    let jobs = jobs["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0]["target"], "usb:fake-usb");
    let printed = jobs[0]["bytes_hex"].as_str().unwrap();
    let golden =
        [date_before, date_after].map(|d| hex(&test_page("Dev Branch", &d, PaperWidth::Mm58)));
    assert!(golden.iter().any(|g| g == printed), "{printed}");

    let after = request(health, "GET", "/healthz", None, None).unwrap();
    assert_eq!(after["setup"]["printer"], "ok");
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
