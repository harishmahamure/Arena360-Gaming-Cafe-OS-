//! `arena-hub dev seed`: present only with the `dev-fixtures` feature.

use std::process::{Command, Output};

fn hub(args: &[&str], data_dir: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_arena-hub"))
        .args(args)
        .arg("--data-dir")
        .arg(data_dir)
        .output()
        .unwrap()
}

#[test]
#[cfg(not(feature = "dev-fixtures"))]
fn default_build_does_not_recognise_dev() {
    let dir = tempfile::tempdir().unwrap();
    let out = hub(&["dev", "seed"], dir.path());
    assert_eq!(out.status.code(), Some(64), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown argument dev"), "{stderr}");
    assert!(!dir.path().join("branch.db").exists());
}

#[cfg(feature = "dev-fixtures")]
mod with_feature {
    use super::*;
    use arena_store::{RowValue, Store, Table, record_schema_version};

    #[test]
    fn seed_on_newer_schema_exits_2() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("branch.db");
        {
            let mut store = Store::open(&db, 0).unwrap();
            store
                .write_tx(|tx| record_schema_version(tx, 999, 0))
                .unwrap();
        }
        let before = std::fs::read(&db).unwrap();
        let out = hub(&["dev", "seed"], dir.path());
        assert_eq!(out.status.code(), Some(2), "{out:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("SCHEMA_TOO_NEW"));
        assert_eq!(std::fs::read(&db).unwrap(), before);
    }

    #[test]
    fn seed_succeeds_once_then_fails_already_bootstrapped() {
        let dir = tempfile::tempdir().unwrap();
        let first = hub(&["dev", "seed"], dir.path());
        assert_eq!(first.status.code(), Some(0), "{first:?}");
        assert!(String::from_utf8_lossy(&first.stdout).contains("DEV01"));

        let store = Store::open(dir.path().join("branch.db"), 0).unwrap();
        let branches = store.rows(Table::Branch).unwrap();
        assert_eq!(branches.len(), 1);
        let b = &branches[0];
        assert_eq!(b["name"], RowValue::Text("Dev Branch".into()));
        assert_eq!(b["branch_code"], RowValue::Text("DEV01".into()));
        assert_eq!(b["currency"], RowValue::Text("INR".into()));
        assert_eq!(b["tz"], RowValue::Text("Asia/Kolkata".into()));
        assert_eq!(b["day_start_min"], RowValue::Integer(360));
        assert_eq!(b["fy_start_month"], RowValue::Integer(4));
        let changes = store.changes().unwrap();
        // Bootstrap as the system; the device and the staff as the owner.
        assert!(changes.iter().all(|c| c.actor_id == "dev-seed"));
        assert!(changes.iter().all(|c| match c.intent_kind.as_str() {
            "branch.bootstrap" => c.actor_kind == "system",
            _ => c.actor_kind == "owner",
        }));
        let staff = store.rows(Table::Staff).unwrap();
        let logins: Vec<_> = staff.iter().map(|s| s["login"].clone()).collect();
        for login in ["owner", "manager", "counter", "kitchen"] {
            assert!(logins.contains(&RowValue::Text(login.into())), "{login}");
        }
        assert!(staff.iter().all(|s| s["active"] == RowValue::Integer(1)));
        let stdout = String::from_utf8_lossy(&first.stdout);
        assert!(stdout.contains("dev-counter-pass"), "{stdout}");
        assert!(stdout.contains("3333"), "{stdout}");
        // The credentials verify against the stored hashes.
        drop(store);
        let dir_ = arena_core::StaffDirectory::open(dir.path().join("branch.db")).unwrap();
        let me = dir_.sign_in("counter", "dev-counter-pass").unwrap();
        assert_eq!(
            (me.display_name.as_str(), me.role_name.as_str()),
            ("Counter", "Counter")
        );
        assert!(dir_.sign_in("counter", "dev-owner-pass").is_err());
        drop(dir_);
        let store = Store::open(dir.path().join("branch.db"), 0).unwrap();
        let changes = store.changes().unwrap();
        let before = changes.len();

        // The lease was released on exit: it has already expired.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        assert!(store.lease().unwrap().unwrap().expires_at <= now);
        drop(store);

        let second = hub(&["dev", "seed"], dir.path());
        assert_eq!(second.status.code(), Some(1), "{second:?}");
        assert!(
            String::from_utf8_lossy(&second.stderr).contains("ALREADY_BOOTSTRAPPED"),
            "{second:?}"
        );
        let store = Store::open(dir.path().join("branch.db"), 0).unwrap();
        assert_eq!(store.changes().unwrap().len(), before);
    }

    #[test]
    fn seed_while_another_process_holds_the_db_is_not_primary() {
        let dir = tempfile::tempdir().unwrap();
        // Holding the store open in this process takes the writer lock.
        let held = Store::open(dir.path().join("branch.db"), 0).unwrap();
        assert!(held.holds_writer_lock());
        let out = hub(&["dev", "seed"], dir.path());
        assert_eq!(out.status.code(), Some(1), "{out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("NOT_PRIMARY"),
            "{out:?}"
        );
        assert!(held.rows(Table::Branch).unwrap().is_empty());
    }
}
