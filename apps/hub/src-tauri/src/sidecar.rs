//! Starting the Hub service: probe `127.0.0.1:<port>/healthz`, and when nothing healthy answers,
//! spawn the `arena-hub` sidecar that the installer puts next to this executable. The service
//! is not a child that dies with the UI: closing the window leaves it running.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The Hub service's loopback health port (`arena-api::DEFAULT_HEALTH_PORT`).
pub const HEALTH_PORT: u16 = 7409;
/// The sidecar's file stem (Tauri strips the target triple when it bundles `externalBin`).
pub const SIDECAR_NAME: &str = "arena-hub";
/// Same variable `arena-hub` reads its data dir from.
pub const DATA_DIR_ENV: &str = "ARENA_HUB_DATA_DIR";

/// The longest a single probe may take.
const PROBE_TIMEOUT: Duration = Duration::from_millis(800);
/// How long to wait for a freshly spawned service to answer before showing the window anyway
/// (the UI then shows its "Hub unreachable" state and retries).
pub const START_TIMEOUT: Duration = Duration::from_secs(10);

/// What the health probe saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// `/healthz` answered 200 with an `arena-hub` health body.
    Healthy,
    /// Nothing listening, a timeout, or any other answer.
    Down,
}

/// What to do about the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// A healthy service is already running: use it.
    UseRunning,
    /// Spawn the sidecar at this path.
    Spawn(PathBuf),
    /// No service and no sidecar at this path: show the window (it shows the error state).
    Missing(PathBuf),
}

/// The probe-or-spawn decision.
pub fn plan(probe: Probe, sidecar: &Path, sidecar_exists: bool) -> Plan {
    match (probe, sidecar_exists) {
        (Probe::Healthy, _) => Plan::UseRunning,
        (Probe::Down, true) => Plan::Spawn(sidecar.to_path_buf()),
        (Probe::Down, false) => Plan::Missing(sidecar.to_path_buf()),
    }
}

/// How [`ensure_service`] ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    AlreadyRunning,
    /// Spawned, and it answered healthy.
    Started,
    /// Spawned, but it exited before answering (its exit status).
    Exited(String),
    /// Spawned, but it did not answer within the timeout.
    TimedOut,
    Missing(PathBuf),
    SpawnFailed(String),
}

/// The sidecar's path: `arena-hub` (`.exe` on Windows) next to the app executable.
pub fn sidecar_path(app_exe: &Path) -> PathBuf {
    let dir = app_exe.parent().unwrap_or_else(|| Path::new("."));
    dir.join(format!("{SIDECAR_NAME}{}", std::env::consts::EXE_SUFFIX))
}

/// The Hub's data dir, resolved as `arena-hub` does without `--data-dir`:
/// `ARENA_HUB_DATA_DIR`, else `%PROGRAMDATA%\Arena360\Hub` on Windows and
/// `$XDG_DATA_HOME/arena360/hub` (falling back to `~/.local/share/arena360/hub`) elsewhere.
pub fn data_dir(env: impl Fn(&str) -> Option<OsString>, windows: bool) -> Option<PathBuf> {
    let get = |k: &str| env(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(dir) = get(DATA_DIR_ENV) {
        return Some(dir);
    }
    if windows {
        let base = get("PROGRAMDATA").unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
        return Some(base.join("Arena360").join("Hub"));
    }
    if let Some(xdg) = get("XDG_DATA_HOME") {
        return Some(xdg.join("arena360").join("hub"));
    }
    Some(
        get("HOME")?
            .join(".local")
            .join("share")
            .join("arena360")
            .join("hub"),
    )
}

/// How to start the sidecar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarCommand {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub envs: Vec<(OsString, OsString)>,
}

impl SidecarCommand {
    /// The bundled sidecar next to `app_exe`, with `--data-dir` when the data dir is known.
    pub fn bundled(app_exe: &Path, data_dir: Option<&Path>) -> Self {
        let mut args = Vec::new();
        if let Some(dir) = data_dir {
            args.push(OsString::from("--data-dir"));
            args.push(dir.as_os_str().to_owned());
        }
        Self {
            program: sidecar_path(app_exe),
            args,
            envs: Vec::new(),
        }
    }
}

/// Whether a raw HTTP response is a 200 whose body is JSON with a `self_instance_id`
/// (an `arena-hub` `/healthz`, not some other program on the port).
pub fn is_healthy_response(raw: &[u8]) -> bool {
    let line = raw.split(|&b| b == b'\n').next().unwrap_or_default();
    let mut parts = line.split(|&b| b == b' ');
    let ok_status = matches!(
        (parts.next(), parts.next()),
        (Some(version), Some(b"200")) if version.starts_with(b"HTTP/1.")
    );
    let body = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| &raw[i + 4..]);
    ok_status
        && body
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(b).ok())
            .is_some_and(|v| v.get("self_instance_id").is_some_and(|id| id.is_string()))
}

/// `GET /healthz` on `127.0.0.1:port`, taking at most `timeout`.
pub fn probe(port: u16, timeout: Duration) -> Probe {
    if timeout.is_zero() {
        return Probe::Down;
    }
    let deadline = Instant::now() + timeout;
    let left = || {
        deadline
            .saturating_duration_since(Instant::now())
            .max(Duration::from_millis(1))
    };
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let attempt = || -> std::io::Result<Vec<u8>> {
        let mut s = TcpStream::connect_timeout(&addr, left())?;
        s.set_write_timeout(Some(left()))?;
        s.write_all(b"GET /healthz HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")?;
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            if Instant::now() >= deadline {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            s.set_read_timeout(Some(left()))?;
            match s.read(&mut buf)? {
                0 => return Ok(out),
                n if out.len() + n > 64 * 1024 => return Ok(out),
                n => out.extend_from_slice(&buf[..n]),
            }
        }
    };
    match attempt() {
        Ok(raw) if is_healthy_response(&raw) => Probe::Healthy,
        _ => Probe::Down,
    }
}

fn spawn_detached(cmd: &SidecarCommand) -> std::io::Result<std::process::Child> {
    let mut c = Command::new(&cmd.program);
    c.args(&cmd.args)
        .envs(cmd.envs.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window, and its own process group so the UI's console events never
        // reach it.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        c.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }
    c.spawn()
}

/// Makes sure the Hub service on `port` is running: probe, spawn the sidecar if needed and
/// wait up to `timeout` for it to answer, stopping early if it exits. Never panics; every
/// failure goes to `log`, and the UI is left to show its unreachable state.
pub fn ensure_service(
    cmd: &SidecarCommand,
    port: u16,
    timeout: Duration,
    log: &mut dyn FnMut(&str),
) -> Outcome {
    let decided = plan(
        probe(port, PROBE_TIMEOUT),
        &cmd.program,
        cmd.program.is_file(),
    );
    let path = match decided {
        Plan::UseRunning => return Outcome::AlreadyRunning,
        Plan::Missing(path) => {
            log(&format!(
                "the hub service is not running and {} is missing",
                path.display()
            ));
            return Outcome::Missing(path);
        }
        Plan::Spawn(path) => path,
    };
    let mut child = match spawn_detached(cmd) {
        Ok(child) => child,
        Err(e) => {
            let msg = format!("cannot start {}: {e}", path.display());
            log(&msg);
            return Outcome::SpawnFailed(msg);
        }
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                log(&format!(
                    "{} exited before answering /healthz: {status}",
                    path.display()
                ));
                return Outcome::Exited(status.to_string());
            }
            Ok(None) => {}
            Err(e) => log(&format!("cannot check {}: {e}", path.display())),
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            log(&format!(
                "{} did not answer /healthz within {} s",
                path.display(),
                timeout.as_secs()
            ));
            return Outcome::TimedOut;
        }
        if probe(port, left.min(PROBE_TIMEOUT)) == Probe::Healthy {
            // The child handle is dropped on purpose: the service outlives the UI.
            return Outcome::Started;
        }
        std::thread::sleep(Duration::from_millis(100).min(left));
    }
}

/// Appends timestamped lines to `<data dir>/logs/hub-ui.log` (and stderr).
pub struct FileLog(pub Option<PathBuf>);

impl FileLog {
    pub fn in_data_dir(data_dir: Option<&Path>) -> Self {
        Self(data_dir.map(|d| d.join("logs").join("hub-ui.log")))
    }

    pub fn line(&self, msg: &str) {
        eprintln!("arena-hub-ui: {msg}");
        let Some(path) = &self.0 else { return };
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let write = || -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;
            writeln!(f, "{at} {msg}")
        };
        let _ = write();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    const STUB_PORT: &str = "ARENA_HUB_UI_STUB_PORT";
    const STUB_MODE: &str = "ARENA_HUB_UI_STUB_MODE";

    fn free_port() -> u16 {
        TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    /// Not a test: the stub "sidecar". The test binary re-runs itself with only this test
    /// selected; it then serves a fake `/healthz` on `$ARENA_HUB_UI_STUB_PORT` for 5 s, or
    /// exits 3 at once when `$ARENA_HUB_UI_STUB_MODE` is `exit`.
    #[test]
    #[ignore = "run by the sidecar tests as a stub arena-hub"]
    fn stub_hub() {
        let Some(port) = std::env::var(STUB_PORT)
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
        else {
            return;
        };
        if std::env::var(STUB_MODE).as_deref() == Ok("exit") {
            std::process::exit(3);
        }
        // Start late, so the first probe of the waiting loop sees nothing.
        std::thread::sleep(Duration::from_millis(300));
        let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            match listener.accept() {
                Ok((mut s, _)) => {
                    s.set_nonblocking(false).unwrap();
                    let mut buf = [0u8; 512];
                    let _ = s.read(&mut buf);
                    let body = r#"{"db_open":true,"self_instance_id":"01JSTUB"}"#;
                    let _ = write!(
                        s,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                }
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    }

    fn stub(port: u16, mode: &str) -> SidecarCommand {
        SidecarCommand {
            program: std::env::current_exe().unwrap(),
            args: ["sidecar::tests::stub_hub", "--exact", "--ignored"]
                .map(OsString::from)
                .to_vec(),
            envs: vec![
                (STUB_PORT.into(), port.to_string().into()),
                (STUB_MODE.into(), mode.into()),
            ],
        }
    }

    #[test]
    fn down_then_spawn_then_healthy() {
        let port = free_port();
        assert_eq!(probe(port, PROBE_TIMEOUT), Probe::Down);
        let mut lines = Vec::new();
        let out = ensure_service(&stub(port, "serve"), port, START_TIMEOUT, &mut |l| {
            lines.push(l.to_owned())
        });
        assert_eq!(out, Outcome::Started, "{lines:?}");
        assert_eq!(probe(port, PROBE_TIMEOUT), Probe::Healthy);
        // Now it is running: a second start uses it.
        assert_eq!(
            ensure_service(&stub(port, "serve"), port, START_TIMEOUT, &mut |_| {}),
            Outcome::AlreadyRunning
        );
    }

    #[test]
    fn a_sidecar_that_exits_stops_the_wait_early() {
        let port = free_port();
        let mut lines = Vec::new();
        let started = Instant::now();
        let out = ensure_service(&stub(port, "exit"), port, START_TIMEOUT, &mut |l| {
            lines.push(l.to_owned())
        });
        assert!(matches!(out, Outcome::Exited(_)), "{out:?}");
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(lines.iter().any(|l| l.contains("exited")), "{lines:?}");
    }

    #[test]
    fn a_missing_sidecar_is_logged() {
        let port = free_port();
        let cmd = SidecarCommand::bundled(Path::new("/nonexistent/dir/arena-hub-ui"), None);
        let mut lines = Vec::new();
        let out = ensure_service(&cmd, port, START_TIMEOUT, &mut |l| lines.push(l.to_owned()));
        assert!(matches!(out, Outcome::Missing(_)), "{out:?}");
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn plan_uses_a_healthy_service_and_spawns_otherwise() {
        let p = Path::new("/opt/app/arena-hub");
        assert_eq!(plan(Probe::Healthy, p, true), Plan::UseRunning);
        assert_eq!(plan(Probe::Healthy, p, false), Plan::UseRunning);
        assert_eq!(plan(Probe::Down, p, true), Plan::Spawn(p.to_path_buf()));
        assert_eq!(plan(Probe::Down, p, false), Plan::Missing(p.to_path_buf()));
    }

    #[test]
    fn sidecar_sits_next_to_the_app_and_gets_the_data_dir() {
        let exe = Path::new("dir").join(format!("arena-hub-ui{}", std::env::consts::EXE_SUFFIX));
        let cmd = SidecarCommand::bundled(&exe, Some(Path::new("/data")));
        assert_eq!(
            cmd.program,
            Path::new("dir").join(format!("arena-hub{}", std::env::consts::EXE_SUFFIX))
        );
        assert_eq!(
            cmd.args,
            ["--data-dir", "/data"].map(OsString::from).to_vec()
        );
        assert!(SidecarCommand::bundled(&exe, None).args.is_empty());
    }

    #[test]
    fn data_dir_matches_arena_hub() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| OsString::from(v))
            }
        };
        assert_eq!(
            data_dir(env(&[(DATA_DIR_ENV, "/env"), ("HOME", "/h")]), false),
            Some(PathBuf::from("/env"))
        );
        assert_eq!(
            data_dir(env(&[("PROGRAMDATA", r"D:\PD")]), true),
            Some(PathBuf::from(r"D:\PD").join("Arena360").join("Hub"))
        );
        assert_eq!(
            data_dir(env(&[("XDG_DATA_HOME", "/xdg"), ("HOME", "/h")]), false),
            Some(PathBuf::from("/xdg/arena360/hub"))
        );
        assert_eq!(
            data_dir(env(&[("HOME", "/h")]), false),
            Some(PathBuf::from("/h/.local/share/arena360/hub"))
        );
        assert_eq!(data_dir(env(&[]), false), None);
    }

    #[test]
    fn only_an_arena_hub_health_body_is_healthy() {
        let ok = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{\"self_instance_id\":\"01J\"}";
        assert!(is_healthy_response(ok));
        assert!(!is_healthy_response(b"HTTP/1.1 200 OK\r\n\r\n{}"));
        assert!(!is_healthy_response(b"HTTP/1.1 200 OK\r\n\r\n<html>"));
        assert!(!is_healthy_response(
            b"HTTP/1.1 500 Internal Server Error\r\n\r\n{\"self_instance_id\":\"x\"}"
        ));
        assert!(!is_healthy_response(b"HTTP/1.1 2000 Nope\r\n"));
        assert!(!is_healthy_response(b"SSH-2.0-OpenSSH\r\n"));
        assert!(!is_healthy_response(b""));
    }

    #[test]
    fn the_file_log_appends_under_logs() {
        let dir = tempfile_dir();
        let log = FileLog::in_data_dir(Some(&dir));
        log.line("one");
        log.line("two");
        let text = std::fs::read_to_string(dir.join("logs").join("hub-ui.log")).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.ends_with("two\n"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "arena-hub-ui-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
