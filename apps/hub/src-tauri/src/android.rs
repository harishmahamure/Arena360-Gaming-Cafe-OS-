use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::sync::Mutex;
use std::time::Duration;

use arena_proto::DeviceSpec;
use tauri::{AppHandle, Manager};

use crate::spec::{Gate, probe};

const HEALTH_PORT: u16 = 7409;
const START_WAIT: Duration = Duration::from_secs(10);

#[derive(Default)]
pub struct Hub(Mutex<Gate>);

fn data_dir(app: &AppHandle) -> Option<std::path::PathBuf> {
    let dir = app.path().app_data_dir().ok()?.join("files");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn run_hub(app: AppHandle, dir: std::path::PathBuf) {
    let args = vec![std::ffi::OsString::from("--data-dir"), dir.into_os_string()];
    let handle = app.clone();
    let spawned = std::thread::Builder::new()
        .name("arena-hub".into())
        .spawn(move || {
            match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => {
                    let code = rt.block_on(arena_hub::start(args.into_iter()));
                    eprintln!("arena-hub: stopped with {code:?}");
                }
                Err(e) => eprintln!("arena-hub: no Tokio runtime: {e}"),
            }
            lock(&handle.state::<Hub>()).stopped();
        });
    if let Err(e) = spawned {
        eprintln!("arena-hub: cannot start the Hub thread: {e}");
        lock(&app.state::<Hub>()).stopped();
    }
}

fn lock(hub: &Hub) -> std::sync::MutexGuard<'_, Gate> {
    hub.0.lock().unwrap_or_else(|e| e.into_inner())
}

fn wait_for_hub() {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, HEALTH_PORT));
    let deadline = std::time::Instant::now() + START_WAIT;
    while std::time::Instant::now() < deadline {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn check(app: &AppHandle) -> DeviceSpec {
    let Some(dir) = data_dir(app) else {
        eprintln!("arena-hub: no app data directory");
        return DeviceSpec { failures: vec![] };
    };
    let started = std::cell::Cell::new(false);
    let spec = lock(&app.state::<Hub>()).run(
        &dir,
        probe,
        || {
            started.set(true);
            run_hub(app.clone(), dir.clone());
        },
        |line| eprintln!("arena-hub: {line}"),
    );
    if started.get() {
        wait_for_hub();
    }
    spec
}

#[tauri::command]
pub async fn device_spec(app: AppHandle) -> Result<DeviceSpec, String> {
    tauri::async_runtime::spawn_blocking(move || check(&app))
        .await
        .map_err(|e| e.to_string())
}

pub fn setup(app: &AppHandle) {
    let handle = app.clone();
    std::thread::spawn(move || {
        check(&handle);
        if let Some(window) = handle.get_webview_window("main") {
            let _ = window.eval("window.dispatchEvent(new Event('arena:hub-ready'))");
            let _ = window.show();
        }
    });
}
