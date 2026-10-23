//! The Windows service host (windows-service): `service install` registers `arena-kiosk-svc`
//! as an auto-start LocalSystem service that restarts on failure, `service uninstall` stops and
//! removes it, and `service run` is what the Service Control Manager starts.

use std::ffi::OsString;
use std::sync::Arc;
use std::time::Duration;

use windows_service::service::{
    ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept,
    ServiceErrorControl, ServiceExitCode, ServiceFailureActions, ServiceFailureResetPeriod,
    ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

use crate::launcher::{SessionLauncher, configured_app_path};
use crate::paths::{DataDir, default_data_dir};

pub const SERVICE_NAME: &str = "arena-kiosk-svc";
const DISPLAY_NAME: &str = "Arena 360 Kiosk Service";
const DESCRIPTION: &str =
    "Holds the kiosk's device token and Hub connection and supervises the Arena 360 Kiosk app.";

fn data_dir() -> DataDir {
    DataDir(default_data_dir(|k| std::env::var(k).ok(), true))
}

const ERROR_SERVICE_ALREADY_RUNNING: i32 = 1056;
const ERROR_SERVICE_DOES_NOT_EXIST: i32 = 1060;
const ERROR_SERVICE_NOT_ACTIVE: i32 = 1062;
const ERROR_SERVICE_MARKED_FOR_DELETE: i32 = 1072;
const ERROR_SERVICE_EXISTS: i32 = 1073;

fn os_code(e: &windows_service::Error) -> Option<i32> {
    match e {
        windows_service::Error::Winapi(io) => io.raw_os_error(),
        _ => None,
    }
}

fn service_info() -> windows_service::Result<ServiceInfo> {
    Ok(ServiceInfo {
        name: SERVICE_NAME.into(),
        display_name: DISPLAY_NAME.into(),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: std::env::current_exe().map_err(windows_service::Error::Winapi)?,
        launch_arguments: vec!["service".into(), "run".into()],
        dependencies: vec![],
        // LocalSystem.
        account_name: None,
        account_password: None,
    })
}

fn configure(service: &windows_service::service::Service) -> windows_service::Result<()> {
    service.set_description(DESCRIPTION)?;
    let restart = ServiceAction {
        action_type: ServiceActionType::Restart,
        delay: Duration::from_secs(5),
    };
    service.update_failure_actions(ServiceFailureActions {
        reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(24 * 60 * 60)),
        reboot_msg: None,
        command: None,
        actions: Some(vec![restart.clone(), restart.clone(), restart]),
    })?;
    service.set_failure_actions_on_non_crash_failures(true)?;
    match service.start::<&str>(&[]) {
        Err(e) if os_code(&e) == Some(ERROR_SERVICE_ALREADY_RUNNING) => Ok(()),
        r => r,
    }
}

/// Protects the data directory, then registers (or, when it exists, reconfigures), configures
/// and starts the service. A service this call created is deleted again when a later step
/// fails, so no half-made service is left behind.
pub fn install() -> windows_service::Result<()> {
    crate::winsec::protect_data_dir(data_dir().root()).map_err(windows_service::Error::Winapi)?;
    let manager = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )?;
    let info = service_info()?;
    let access = ServiceAccess::CHANGE_CONFIG
        | ServiceAccess::START
        | ServiceAccess::QUERY_STATUS
        | ServiceAccess::DELETE;
    // A service being deleted (an uninstall still finishing) cannot be recreated yet.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let (service, created) = loop {
        match manager.create_service(&info, access) {
            Ok(s) => break (s, true),
            Err(e) if os_code(&e) == Some(ERROR_SERVICE_EXISTS) => {
                let s = manager.open_service(SERVICE_NAME, access)?;
                s.change_config(&info)?;
                break (s, false);
            }
            Err(e)
                if os_code(&e) == Some(ERROR_SERVICE_MARKED_FOR_DELETE)
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(500));
            }
            Err(e) => return Err(e),
        }
    };
    if let Err(e) = configure(&service) {
        if created {
            let _ = service.delete();
        }
        return Err(e);
    }
    Ok(())
}

/// Stops (waiting up to 15 s) and deletes the service. Only "not installed" counts as done; a
/// service that does not stop in time is an error and is not deleted while running.
pub fn uninstall() -> windows_service::Result<()> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = match manager.open_service(
        SERVICE_NAME,
        ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
    ) {
        Ok(s) => s,
        Err(e) if os_code(&e) == Some(ERROR_SERVICE_DOES_NOT_EXIST) => return Ok(()),
        Err(e) => return Err(e),
    };
    if service.query_status()?.current_state != ServiceState::Stopped {
        match service.stop() {
            Ok(_) => {}
            Err(e) if os_code(&e) == Some(ERROR_SERVICE_NOT_ACTIVE) => {}
            Err(e) => return Err(e),
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while service.query_status()?.current_state != ServiceState::Stopped {
            if std::time::Instant::now() >= deadline {
                return Err(windows_service::Error::Winapi(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "the service did not stop within 15 s; not deleting it",
                )));
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    service.delete()
}

define_windows_service!(ffi_service_main, service_main);

/// Hands the process to the Service Control Manager (`service run`).
pub fn run_dispatcher() -> windows_service::Result<()> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)
}

fn service_main(_args: Vec<OsString>) {
    if let Err(e) = run_service() {
        tracing::error!(detail = %e, "kiosk service failed");
    }
}

fn status(state: ServiceState, accept: ServiceControlAccept, exit: u32) -> ServiceStatus {
    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: accept,
        exit_code: ServiceExitCode::Win32(exit),
        checkpoint: 0,
        wait_hint: Duration::from_secs(10),
        process_id: None,
    }
}

fn run_service() -> Result<(), String> {
    let data = data_dir();
    let _ = crate::host::init_logs(&data);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let mut stop_tx = Some(stop_tx);
    let handle = service_control_handler::register(SERVICE_NAME, move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            if let Some(tx) = stop_tx.take() {
                let _ = tx.send(());
            }
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })
    .map_err(|e| e.to_string())?;
    handle
        .set_service_status(status(
            ServiceState::Running,
            ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
            0,
        ))
        .map_err(|e| e.to_string())?;

    let exe = configured_app_path(&data.config());
    let config = data.config();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let result = runtime.block_on(crate::host::run_until(
        data,
        Arc::new(SessionLauncher::new(exe, config)),
        async {
            let _ = stop_rx.await;
        },
    ));
    let exit = u32::from(result.is_err());
    let _ = handle.set_service_status(status(
        ServiceState::Stopped,
        ServiceControlAccept::empty(),
        exit,
    ));
    result.map_err(|e| e.to_string())
}
