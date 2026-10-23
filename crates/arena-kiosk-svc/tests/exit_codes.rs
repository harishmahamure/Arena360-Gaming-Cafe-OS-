//! `lockdown` off Windows prints the "only on Windows" line and exits 2.

#[cfg(not(windows))]
#[test]
fn lockdown_off_windows_exits_2_with_the_message() {
    use std::process::Command;

    let exe = env!("CARGO_BIN_EXE_arena-kiosk-svc");
    for action in ["apply", "restore", "reset-shell"] {
        let out = Command::new(exe)
            .args(["lockdown", action, "--player-user", "player"])
            .output()
            .expect("run arena-kiosk-svc");
        assert_eq!(out.status.code(), Some(2), "lockdown {action} exit code");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("lockdown is only available on Windows"),
            "lockdown {action} stderr: {stderr}"
        );
    }
}
