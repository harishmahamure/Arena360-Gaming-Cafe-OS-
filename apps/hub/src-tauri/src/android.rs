use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use arena_proto::DeviceSpec;
use arena_tasks::{Advertiser, NetworkName, SecretWrapper};
use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::jboolean;
use tauri::{AppHandle, Manager};

use crate::keystore_jni::JniWrapper;
use crate::nsd_jni;
use crate::spec::{DB_FILE, Gate, probe};

const HEALTH_PORT: u16 = 7409;
const START_WAIT: Duration = Duration::from_secs(10);
const STOP_WAIT: Duration = Duration::from_secs(15);

static GATE: Mutex<Gate> = Mutex::new(Gate::new());

/// The Android Keystore wrapper, built once on the Java thread that calls `nativeStart` (the
/// app's class loader is only reachable there). The Hub is never started without it, so the
/// token and branch key are never written in clear.
static WRAPPER: OnceLock<Arc<dyn SecretWrapper>> = OnceLock::new();

fn wrapper(env: &mut JNIEnv) -> Option<Arc<dyn SecretWrapper>> {
    if let Some(w) = WRAPPER.get() {
        return Some(Arc::clone(w));
    }
    match JniWrapper::new(env) {
        Ok(w) => Some(Arc::clone(WRAPPER.get_or_init(|| Arc::new(w)))),
        Err(e) => {
            eprintln!("arena-hub: the Android Keystore bridge is unavailable: {e}");
            None
        }
    }
}

fn gate() -> MutexGuard<'static, Gate> {
    GATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn data_dir(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?.join("files");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// What the Android shell injects into the Hub.
struct Services {
    wrapper: Arc<dyn SecretWrapper>,
    advertiser: Box<dyn Advertiser>,
    network: Arc<dyn NetworkName>,
}

fn run_hub(dir: PathBuf, services: Services) {
    let args = vec![std::ffi::OsString::from("--data-dir"), dir.into_os_string()];
    let options = arena_hub::StartOptions {
        secret_wrapper: Some(services.wrapper),
        advertiser: Some(services.advertiser),
        network_wake: Some(nsd_jni::network_wake()),
        network_name: Some(services.network),
    };
    let spawned = std::thread::Builder::new()
        .name("arena-hub".into())
        .spawn(move || {
            match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => {
                    let code = rt.block_on(arena_hub::start_with(args.into_iter(), options));
                    eprintln!("arena-hub: stopped with {code:?}");
                }
                Err(e) => eprintln!("arena-hub: no Tokio runtime: {e}"),
            }
            gate().stopped();
        });
    if let Err(e) = spawned {
        eprintln!("arena-hub: cannot start the Hub thread: {e}");
        gate().stopped();
    }
}

fn wait_for_hub() -> bool {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, HEALTH_PORT));
    let deadline = Instant::now() + START_WAIT;
    while Instant::now() < deadline {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn ensure(dir: &Path, services: Services) -> DeviceSpec {
    gate().run(
        dir,
        probe,
        || run_hub(dir.to_path_buf(), services),
        |line| eprintln!("arena-hub: {line}"),
    )
}

fn stop_and_wait() {
    arena_hub::stop();
    let deadline = Instant::now() + STOP_WAIT;
    while gate().is_started() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
}

struct NoNetworkName;

impl NetworkName for NoNetworkName {
    fn name(&self) -> Option<String> {
        None
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_in_arena360_hub_HubService_nativeStart(
    mut env: JNIEnv,
    _class: JClass,
    dir: JString,
) -> jboolean {
    let Ok(dir) = env.get_string(&dir).map(String::from) else {
        return 0;
    };
    let dir = PathBuf::from(dir);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("arena-hub: cannot create {}: {e}", dir.display());
        return 0;
    }
    let Some(wrapper) = wrapper(&mut env) else {
        return 0;
    };
    // NsdManager is how kiosks find the Hub; without the bridge the Hub falls back to the
    // mdns-sd daemon (which may not reach the LAN on Android), and kiosks can still type its IP.
    let (advertiser, network) = match nsd_jni::connect(&mut env) {
        Ok((advertiser, network)) => (
            Box::new(advertiser) as Box<dyn Advertiser>,
            Arc::new(network) as Arc<dyn NetworkName>,
        ),
        Err(e) => {
            // A failed class lookup leaves a ClassNotFoundException pending.
            let _ = env.exception_clear();
            eprintln!("arena-hub: the NsdManager bridge is unavailable: {e}");
            (
                Box::new(arena_tasks::MdnsAdvertiser::default()) as Box<dyn Advertiser>,
                Arc::new(NoNetworkName) as Arc<dyn NetworkName>,
            )
        }
    };
    let services = Services {
        wrapper,
        advertiser,
        network,
    };
    jboolean::from(ensure(&dir, services).ok())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_in_arena360_hub_HubService_nativeStop(_env: JNIEnv, _class: JClass) {
    stop_and_wait();
}

#[tauri::command]
pub async fn device_spec(app: AppHandle) -> Result<DeviceSpec, String> {
    tauri::async_runtime::spawn_blocking(move || match data_dir(&app) {
        Some(dir) => gate().check(&dir, probe, |line| eprintln!("arena-hub: {line}")),
        None => {
            eprintln!("arena-hub: no app data directory");
            DeviceSpec { failures: vec![] }
        }
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn hub_ready() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(wait_for_hub)
        .await
        .map_err(|e| e.to_string())
}

pub fn setup(app: &AppHandle) {
    let handle = app.clone();
    std::thread::spawn(move || {
        if data_dir(&handle).is_some_and(|d| d.join(DB_FILE).exists()) {
            wait_for_hub();
        }
        if let Some(window) = handle.get_webview_window("main") {
            let _ = window.eval("window.dispatchEvent(new Event('arena:hub-ready'))");
            let _ = window.show();
        }
    });
}
