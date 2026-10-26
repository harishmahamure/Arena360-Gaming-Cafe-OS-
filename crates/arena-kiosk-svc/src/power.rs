use arena_proto::PcAction;

pub fn supported() -> bool {
    cfg!(windows)
}

#[cfg(not(windows))]
pub fn run(_action: PcAction) -> Result<(), String> {
    Err("restart and shut down are only available on Windows".into())
}

#[cfg(windows)]
pub fn run(action: PcAction) -> Result<(), String> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID};
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
        SE_SHUTDOWN_NAME, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows::Win32::System::Shutdown::{
        InitiateSystemShutdownExW, SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_OTHER,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::core::PCWSTR;

    let reboot = match action {
        PcAction::Restart => true,
        PcAction::Shutdown => false,
        _ => return Err("not a power action".into()),
    };
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
        .map_err(|e| format!("OpenProcessToken: {e}"))?;
        let mut luid = LUID::default();
        let enabled = LookupPrivilegeValueW(PCWSTR::null(), SE_SHUTDOWN_NAME, &mut luid)
            .and_then(|()| {
                let tp = TOKEN_PRIVILEGES {
                    PrivilegeCount: 1,
                    Privileges: [LUID_AND_ATTRIBUTES {
                        Luid: luid,
                        Attributes: SE_PRIVILEGE_ENABLED,
                    }],
                };
                AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None)
            })
            .map_err(|e| format!("enabling the shutdown privilege: {e}"));
        let _ = CloseHandle(token);
        enabled?;
        InitiateSystemShutdownExW(
            PCWSTR::null(),
            PCWSTR::null(),
            3,
            true,
            reboot,
            SHTDN_REASON_MAJOR_OTHER | SHTDN_REASON_FLAG_PLANNED,
        )
        .map_err(|e| format!("InitiateSystemShutdownExW: {e}"))
    }
}
