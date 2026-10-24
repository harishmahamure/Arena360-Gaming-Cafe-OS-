//! `serve_control` wires the background `SELECT 1`: `/healthz` turns reachable once a check
//! ran, and unreachable (with `checked_at` advancing) once `gz_control` goes away.
//!
//! The database is reached through an in-process TCP proxy, so "going away" works the same
//! against a spawned sqld and a shared one (`ARENA_TEST_SQLD_URL`).

#[path = "../../arena-store/tests/support/sqld.rs"]
mod sqld;

use std::time::{Duration, Instant};

use arena_cloud::config::{Config, DEFAULT_RETRY_WINDOW, Role};
use arena_cloud::{connect_and_migrate, serve_control};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// Forwards loopback connections to `upstream` until [`Proxy::stop`] closes the listener and
/// every open connection.
struct Proxy {
    port: u16,
    accept: JoinHandle<()>,
    conns: std::sync::Arc<std::sync::Mutex<Vec<JoinHandle<()>>>>,
}

impl Proxy {
    async fn start(upstream: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let conns = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let c = std::sync::Arc::clone(&conns);
        let accept = tokio::spawn(async move {
            loop {
                let Ok((mut down, _)) = listener.accept().await else {
                    return;
                };
                let upstream = upstream.clone();
                let h = tokio::spawn(async move {
                    if let Ok(mut up) = TcpStream::connect(&upstream).await {
                        let _ = tokio::io::copy_bidirectional(&mut down, &mut up).await;
                    }
                });
                c.lock().unwrap().push(h);
            }
        });
        Self {
            port,
            accept,
            conns,
        }
    }

    fn stop(&self) {
        self.accept.abort();
        for h in self.conns.lock().unwrap().drain(..) {
            h.abort();
        }
    }
}

async fn healthz(port: u16) -> Value {
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(b"GET /healthz HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = String::new();
    s.read_to_string(&mut buf).await.unwrap();
    assert!(buf.starts_with("HTTP/1.1 200"), "{buf}");
    serde_json::from_str(buf.split_once("\r\n\r\n").unwrap().1).unwrap()
}

/// Polls `/healthz` until `pred` holds (10 s at most) and returns that body.
async fn until(port: u16, pred: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let v = healthz(port).await;
        if pred(&v) {
            return v;
        }
        assert!(Instant::now() < deadline, "timed out; last {v}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn healthz_follows_the_background_check() {
    let Some(t) = sqld::fresh().await else { return };
    let upstream = t.url.trim_start_matches("http://").to_owned();
    let proxy = Proxy::start(upstream).await;
    let cfg = Config {
        role: Role::Control,
        listen: "127.0.0.1:0".parse().unwrap(),
        db_url: format!("http://127.0.0.1:{}", proxy.port),
        db_token: String::new(),
        retry_window: DEFAULT_RETRY_WINDOW,
    };
    let (db, version) = connect_and_migrate(&cfg).await.unwrap();
    assert_eq!(version, 1);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_control(
        listener,
        db,
        version,
        Duration::from_millis(200),
        async {
            let _ = stop_rx.await;
        },
    ));

    let up = until(port, |v| v["gz_control"]["reachable"] == true).await;
    let first = up["gz_control"]["checked_at"]
        .as_i64()
        .expect("checked_at set");
    assert_eq!(up["migration_version"], 1);

    proxy.stop();
    let down = until(port, |v| v["gz_control"]["reachable"] == false).await;
    let t1 = down["gz_control"]["checked_at"].as_i64().unwrap();
    assert!(t1 > first, "{first} -> {t1}");
    let later = until(port, |v| {
        v["gz_control"]["checked_at"].as_i64().unwrap() > t1
    })
    .await;
    assert_eq!(later["gz_control"]["reachable"], false);

    stop_tx.send(()).unwrap();
    server.await.unwrap().unwrap();
}
