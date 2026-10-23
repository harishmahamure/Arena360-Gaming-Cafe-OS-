//! Process exit codes of the `arena-hub` binary.

use std::process::Command;

use arena_store::{Store, record_schema_version};

fn hub() -> Command {
    Command::new(env!("CARGO_BIN_EXE_arena-hub"))
}

#[test]
fn schema_too_new_exits_2_and_leaves_the_file_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("branch.db");
    {
        let mut store = Store::open(&db, 0).unwrap();
        store
            .write_tx(|tx| record_schema_version(tx, 999, 0))
            .unwrap();
    }
    let before = std::fs::read(&db).unwrap();
    let out = hub()
        .arg("--data-dir")
        .arg(dir.path())
        .args(["--health-port", "1"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("999") && stderr.contains("SCHEMA_TOO_NEW"),
        "{stderr}"
    );
    assert_eq!(std::fs::read(&db).unwrap(), before);
}

#[test]
fn unknown_flag_exits_64() {
    let out = hub().arg("--bogus").output().unwrap();
    assert_eq!(out.status.code(), Some(64), "{out:?}");
}

#[test]
fn help_and_version_exit_0() {
    for flag in ["--help", "-h", "--version"] {
        let out = hub().arg(flag).output().unwrap();
        assert_eq!(out.status.code(), Some(0), "{flag}: {out:?}");
        assert!(!out.stdout.is_empty());
    }
}
