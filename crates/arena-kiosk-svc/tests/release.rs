use std::path::{Path, PathBuf};

fn kiosk_shell() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/kiosk/src-tauri")
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        let name = e.file_name();
        if name == "target" || name == "gen" || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

const FORBIDDEN: [&str; 4] = [
    "config.json",
    "token.bin",
    "selected_hub.json",
    "hub_fingerprint",
];

#[test]
fn the_installer_bundles_no_kiosk_config_token_or_hub_address() {
    for name in ["tauri.conf.json", "tauri.installer.conf.json"] {
        let text = std::fs::read_to_string(kiosk_shell().join(name)).unwrap();
        let conf: serde_json::Value = serde_json::from_str(&text).unwrap();
        let bundle = conf["bundle"].to_string();
        for word in FORBIDDEN {
            assert!(!bundle.contains(word), "{name} bundles {word}");
        }
        assert!(
            conf["bundle"]["resources"].is_null(),
            "{name} bundles extra resources"
        );
    }
    let hooks = std::fs::read_to_string(kiosk_shell().join("windows/hooks.nsh")).unwrap();
    for word in FORBIDDEN {
        assert!(!hooks.contains(word), "the installer hooks write {word}");
    }
}

#[test]
fn no_default_kiosk_config_ships_with_the_service_or_the_shell() {
    let mut files = Vec::new();
    files_under(Path::new(env!("CARGO_MANIFEST_DIR")), &mut files);
    files_under(&kiosk_shell(), &mut files);
    for f in files {
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        assert!(
            !matches!(name, "config.json" | "token.bin" | "selected_hub.json"),
            "{} ships a default kiosk file",
            f.display()
        );
    }
}

#[cfg(not(feature = "dev-fixtures"))]
#[test]
fn a_release_service_refuses_dev_provision() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_arena-kiosk-svc"))
        .args([
            "dev",
            "provision",
            "--hub",
            "127.0.0.1:7400",
            "--fingerprint",
            &"ab".repeat(32),
            "--token",
            "t",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(64), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown command dev"));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("dev provision"));
}
