//! A seeded Hub, end to end: a paired client connects with the token `dev seed` printed, gets
//! `hello.ok`, and `dev revoke-device` closes it (feature `dev-fixtures`).
#![cfg(feature = "dev-fixtures")]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Output};
use std::time::{Duration, Instant};

use arena_core::META_TLS_FINGERPRINT;
use arena_kiosk_core::HubClient;
use arena_proto::{ClientClass, ErrorCode, Hello, Versions};
use arena_store::Store;

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

fn healthz(port: u16) -> Option<serde_json::Value> {
    get_json(port, "/healthz")
}

/// `GET <path>` on the loopback health port, as JSON.
fn get_json(port: u16, path: &str) -> Option<serde_json::Value> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    serde_json::from_str(out.split_once("\r\n\r\n")?.1).ok()
}

/// `POST <path>` with a JSON body on the loopback health port, as JSON.
fn post_json(port: u16, path: &str, body: &serde_json::Value) -> Option<serde_json::Value> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let body = body.to_string();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    serde_json::from_str(out.split_once("\r\n\r\n")?.1).ok()
}

/// Opens `GET /ui/events?token=` and returns the raw stream.
fn open_events(port: u16, token: &str) -> TcpStream {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    let req = format!("GET /ui/events?token={token} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    s.write_all(req.as_bytes()).unwrap();
    s
}

/// Reads `stream` until `needle` appears (or 5 s pass).
fn wait_for(stream: &mut TcpStream, needle: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut seen = Vec::new();
    let mut buf = [0u8; 4096];
    while Instant::now() < deadline {
        match stream.read(&mut buf) {
            Ok(0) => return false,
            Ok(n) => seen.extend_from_slice(&buf[..n]),
            Err(_) => {}
        }
        if String::from_utf8_lossy(&seen).contains(needle) {
            return true;
        }
    }
    false
}

/// The value after `prefix` on the line of `stdout` that has it.
fn after(stdout: &str, prefix: &str) -> String {
    stdout
        .lines()
        .find_map(|l| l.split_once(prefix).map(|(_, v)| v.trim().to_owned()))
        .unwrap_or_else(|| panic!("no {prefix:?} in {stdout}"))
}

fn seed(dir: &Path) -> (String, String) {
    let out: Output = hub()
        .args(["dev", "seed", "--data-dir"])
        .arg(dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let device_id = after(&stdout, "registered paired device \"Dev Kiosk\" ");
    let token = after(&stdout, "device token (shown once): ");
    assert!(token.starts_with(&format!("{device_id}.")), "{token}");
    (device_id, token)
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

fn hello(token: &str) -> Hello {
    Hello {
        credential: token.to_owned(),
        class: ClientClass::Paired,
        versions: Versions {
            app: env!("CARGO_PKG_VERSION").to_owned(),
        },
        capabilities: vec![],
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn seeded_hub_accepts_the_printed_token_and_revocation_closes_it() {
    let dir = tempfile::tempdir().unwrap();
    let (device_id, token) = seed(dir.path());

    let (health, lan, web) = (free_port(), free_port(), free_port());
    let _hub = Running(Some(
        hub()
            .arg("--data-dir")
            .arg(dir.path())
            .args(["--health-port", &health.to_string()])
            .args(["--lan-port", &lan.to_string()])
            .args(["--web-port", &web.to_string()])
            .spawn()
            .unwrap(),
    ));
    let deadline = Instant::now() + Duration::from_secs(20);
    let body = loop {
        if let Some(b) = healthz(health).filter(|b| b["role"] == "primary") {
            break b;
        }
        assert!(Instant::now() < deadline, "the hub never became primary");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let fingerprint = body["tls_fingerprint"].as_str().unwrap().to_owned();
    assert_eq!(fingerprint.len(), 64);
    assert!(dir.path().join("tls").join("hub.crt").exists());
    assert!(dir.path().join("tls").join("hub.key").exists());
    let stored = Store::open_reader(dir.path().join("branch.db"))
        .unwrap()
        .meta(META_TLS_FINGERPRINT)
        .unwrap();
    assert_eq!(stored.as_deref(), Some(fingerprint.as_str()));

    // The Hub UI's snapshot names the seeded branch and its kiosk.
    let snapshot = get_json(health, "/ui/snapshot").unwrap();
    assert_eq!(
        snapshot["branch"],
        serde_json::json!({"name": "Dev Branch", "branch_code": "DEV01"})
    );
    assert_eq!(
        snapshot["devices"],
        serde_json::json!([{"device_id": device_id, "label": "Dev Kiosk", "class": "paired", "zone": null, "online": false}])
    );
    // The seeded branch locale (from the `branch.*` settings bootstrap wrote).
    assert_eq!(
        snapshot["locale"],
        serde_json::json!({
            "currency": "INR", "exponent": 2, "symbol": "₹", "symbol_position": "before",
            "number_format": "indian", "tz": "Asia/Kolkata", "day_start_min": 360,
            "fy_start_month": 4, "pending": null
        })
    );
    assert_eq!(snapshot["lan"]["lan_port"], lan);
    assert_eq!(snapshot["lan"]["web_port"], web);
    assert_eq!(snapshot["lan"]["fingerprint"], fingerprint);
    assert!(snapshot["lan"]["addresses"].is_array());

    let mut s = TcpStream::connect(("127.0.0.1", web)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    s.write_all(b"GET /hub-info HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    assert!(out.starts_with("HTTP/1.1 200 OK"), "{out}");
    let info: serde_json::Value =
        serde_json::from_str(out.split_once("\r\n\r\n").unwrap().1).unwrap();
    let hub_name =
        arena_proto::sanitize_hub_name(&arena_api::lan::tls::hostname().unwrap_or_default());
    assert_eq!(info["product"], "arena360-hub");
    assert_eq!(info["branch_name"], "Dev Branch");
    assert_eq!(info["hub_name"], hub_name);
    assert_eq!(info["fingerprint"], fingerprint);
    assert_eq!(info["lan_port"], lan);
    assert_eq!(info["web_port"], web);

    // A signed-in Hub UI view gets `devices.changed` when the kiosk comes online.
    let signed_in = post_json(
        health,
        "/ui/auth/sign-in",
        &serde_json::json!({"login": "manager", "password": "dev-manager-pass"}),
    )
    .expect("the seeded manager signs in");
    let ui_token = signed_in["token"].as_str().unwrap().to_owned();
    let mut events = open_events(health, &ui_token);
    assert!(wait_for(&mut events, "200 OK"), "the event stream opens");

    let url = format!("wss://127.0.0.1:{lan}/");
    let client = HubClient::connect(&url, &fingerprint, hello(&token))
        .await
        .unwrap();
    assert_eq!(
        client.hello_ok().device_id.as_deref(),
        Some(device_id.as_str())
    );
    assert!(client.hello_ok().primary);
    // With its paired session open, the kiosk shows online in the snapshot (the presence
    // handle is wired from the LAN registry into the loopback router).
    let snapshot = get_json(health, "/ui/snapshot").unwrap();
    assert_eq!(snapshot["devices"][0]["online"], true);
    assert!(
        wait_for(&mut events, "event: devices.changed"),
        "a paired hello pushes devices.changed"
    );

    let out = hub()
        .args(["dev", "revoke-device", &device_id, "--health-port"])
        .arg(health.to_string())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let closed = tokio::time::timeout(Duration::from_secs(1), client.closed())
        .await
        .expect("the session closes within 1 s of the revoke");
    assert_eq!(closed.error.map(|e| e.code), Some(ErrorCode::TokenRevoked));
    // A revoked device leaves the snapshot.
    let snapshot = get_json(health, "/ui/snapshot").unwrap();
    assert_eq!(snapshot["devices"], serde_json::json!([]));

    let err = HubClient::connect(&url, &fingerprint, hello(&token))
        .await
        .err()
        .unwrap();
    assert_eq!(err.code(), Some(ErrorCode::TokenRevoked));

    // Revoking again is refused (already revoked), with a non-zero exit.
    let again = hub()
        .args(["dev", "revoke-device", &device_id, "--health-port"])
        .arg(health.to_string())
        .output()
        .unwrap();
    assert_eq!(again.status.code(), Some(1), "{again:?}");
    assert!(String::from_utf8_lossy(&again.stderr).contains("VALIDATION_FAILED"));
}
