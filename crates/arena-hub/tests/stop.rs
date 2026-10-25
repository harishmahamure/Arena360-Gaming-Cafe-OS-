use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::ExitCode;
use std::time::{Duration, Instant};

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
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

fn spawn_hub(dir: &std::path::Path, port: u16) -> std::thread::JoinHandle<ExitCode> {
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

#[test]
fn stop_shuts_the_hub_down_cleanly_and_it_starts_again_in_the_same_process() {
    let dir = tempfile::tempdir().unwrap();
    let port = free_port();
    for round in 0..2 {
        let hub = spawn_hub(dir.path(), port);
        wait_until("healthz to report primary", || {
            healthz(port).is_some_and(|b| b.contains("\"role\":\"primary\""))
        });
        arena_hub::stop();
        wait_until("the hub thread to end", || hub.is_finished());
        assert_eq!(
            format!("{:?}", hub.join().unwrap()),
            format!("{:?}", ExitCode::SUCCESS),
            "round {round}"
        );
        assert!(healthz(port).is_none(), "round {round}: still serving");
    }
}
