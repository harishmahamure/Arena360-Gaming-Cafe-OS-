//! `--role`, `--listen` and the `gz_control` environment.

use std::collections::HashMap;
use std::ffi::OsString;
use std::time::Duration;

use std::path::PathBuf;

use arena_cloud::config::{
    Command, DEFAULT_RETRY_WINDOW, Database, Decryption, JobConfig, RestoreConfig, RestoreSource,
    Role, Task, parse,
};

fn remote(db: &Database) -> (&str, &str) {
    match db {
        Database::Remote { url, token } => (url, token),
        Database::Local { .. } => panic!("expected a remote database"),
    }
}

fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let map: HashMap<String, OsString> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), OsString::from(v)))
        .collect();
    move |k| map.get(k).cloned()
}

fn args(a: &[&str]) -> Vec<OsString> {
    a.iter().map(OsString::from).collect()
}

const TURSO: &[(&str, &str)] = &[
    (
        "ARENA_CONTROL_DB_URL",
        "libsql://gz-control-arena.aws-ap-south-1.turso.io",
    ),
    ("ARENA_CONTROL_DB_TOKEN", "tok"),
];

#[test]
fn control_with_defaults() {
    let Ok(Command::Run(c)) = parse(args(&["--role", "control"]), env_of(TURSO)) else {
        panic!("expected run");
    };
    assert_eq!(c.role, Role::Control);
    assert_eq!(c.listen.to_string(), "0.0.0.0:8080");
    assert_eq!(
        remote(&c.database),
        ("libsql://gz-control-arena.aws-ap-south-1.turso.io", "tok")
    );
    assert!(!c.dev());
    assert_eq!(c.retry_window, DEFAULT_RETRY_WINDOW);
}

#[test]
fn listen_and_inline_values() {
    let Ok(Command::Run(c)) = parse(
        args(&["--role=control", "--listen", "127.0.0.1:9000"]),
        env_of(TURSO),
    ) else {
        panic!("expected run");
    };
    assert_eq!(c.listen.to_string(), "127.0.0.1:9000");
    assert!(
        parse(
            args(&["--role", "control", "--listen", "nope"]),
            env_of(TURSO)
        )
        .is_err()
    );
}

#[test]
fn help_and_version() {
    assert_eq!(parse(args(&["--help"]), env_of(&[])), Ok(Command::Help));
    assert_eq!(
        parse(args(&["--version"]), env_of(&[])),
        Ok(Command::Version)
    );
}

#[test]
fn the_command_role_parses() {
    let Ok(Command::Run(c)) = parse(args(&["--role", "command"]), env_of(TURSO)) else {
        panic!("expected run");
    };
    assert_eq!(c.role, Role::Command);
    assert_eq!(c.role.as_str(), "command");
}

#[test]
fn the_builder_role_parses_and_serves_like_the_others() {
    let Ok(Command::Run(c)) = parse(args(&["--role", "builder"]), env_of(TURSO)) else {
        panic!("expected run");
    };
    assert_eq!(c.role, Role::Builder);
    assert_eq!(c.role.as_str(), "builder");
}

#[test]
fn other_roles_are_not_available_yet() {
    for role in ["worker", "ingester"] {
        let e = parse(args(&["--role", role]), env_of(TURSO)).unwrap_err();
        assert!(e.contains("role not available yet"), "{e}");
        assert!(e.contains(role), "{e}");
    }
    let e = parse(args(&[]), env_of(TURSO)).unwrap_err();
    assert!(e.contains("--role"), "{e}");
    assert!(parse(args(&["--role"]), env_of(TURSO)).is_err());
    assert!(parse(args(&["--bogus"]), env_of(TURSO)).is_err());
}

#[test]
fn the_url_must_be_libsql_or_https() {
    let control = args(&["--role", "control"]);
    let e = parse(control.clone(), env_of(&[("ARENA_CONTROL_DB_TOKEN", "t")])).unwrap_err();
    assert!(e.contains("ARENA_CONTROL_DB_URL"), "{e}");
    for bad in [
        "file:/tmp/x.db",
        "/tmp/x.db",
        "http://db.example:8080",
        "ws://127.0.0.1",
        "",
    ] {
        let e = parse(
            control.clone(),
            env_of(&[
                ("ARENA_CONTROL_DB_URL", bad),
                ("ARENA_CONTROL_DB_TOKEN", "t"),
            ]),
        )
        .unwrap_err();
        assert!(e.contains("ARENA_CONTROL_DB_URL"), "{bad}: {e}");
    }
    for good in ["libsql://a.turso.io", "https://a.turso.io"] {
        assert!(
            parse(
                control.clone(),
                env_of(&[
                    ("ARENA_CONTROL_DB_URL", good),
                    ("ARENA_CONTROL_DB_TOKEN", "t")
                ]),
            )
            .is_ok(),
            "{good}"
        );
    }
}

#[test]
fn the_token_may_be_empty_only_for_loopback_http() {
    let control = args(&["--role", "control"]);
    for url in ["libsql://a.turso.io", "https://a.turso.io"] {
        let e = parse(control.clone(), env_of(&[("ARENA_CONTROL_DB_URL", url)])).unwrap_err();
        assert!(e.contains("ARENA_CONTROL_DB_TOKEN"), "{e}");
        let e = parse(
            control.clone(),
            env_of(&[
                ("ARENA_CONTROL_DB_URL", url),
                ("ARENA_CONTROL_DB_TOKEN", ""),
            ]),
        )
        .unwrap_err();
        assert!(e.contains("ARENA_CONTROL_DB_TOKEN"), "{e}");
    }
    for url in ["http://127.0.0.1:8081", "http://localhost:8081"] {
        let Ok(Command::Run(c)) = parse(control.clone(), env_of(&[("ARENA_CONTROL_DB_URL", url)]))
        else {
            panic!("{url}");
        };
        assert_eq!(remote(&c.database).1, "");
    }
}

#[test]
fn errors_never_echo_the_token() {
    let e = parse(
        args(&["--role", "control"]),
        env_of(&[
            ("ARENA_CONTROL_DB_URL", "file:x"),
            ("ARENA_CONTROL_DB_TOKEN", "tok-SECRET"),
        ]),
    )
    .unwrap_err();
    assert!(!e.contains("tok-SECRET"), "{e}");
}

#[test]
fn the_retry_window_can_only_be_shortened() {
    let with = |v: &str| {
        let mut e = TURSO.to_vec();
        e.push(("ARENA_CONTROL_DB_RETRY_SECS", v));
        parse(args(&["--role", "control"]), env_of(&e))
    };
    let Ok(Command::Run(c)) = with("2") else {
        panic!()
    };
    assert_eq!(c.retry_window, Duration::from_secs(2));
    assert!(with("61").is_err());
    assert!(with("x").is_err());
}

#[test]
fn url_and_token_are_trimmed() {
    let Ok(Command::Run(c)) = parse(
        args(&["--role", "control"]),
        env_of(&[
            ("ARENA_CONTROL_DB_URL", "  libsql://a.turso.io\n"),
            ("ARENA_CONTROL_DB_TOKEN", "tok \n"),
        ]),
    ) else {
        panic!("expected run");
    };
    assert_eq!(remote(&c.database), ("libsql://a.turso.io", "tok"));
    // Whitespace alone is no token.
    let e = parse(
        args(&["--role", "control"]),
        env_of(&[
            ("ARENA_CONTROL_DB_URL", "libsql://a.turso.io"),
            ("ARENA_CONTROL_DB_TOKEN", " \n"),
        ]),
    )
    .unwrap_err();
    assert!(e.contains("ARENA_CONTROL_DB_TOKEN"), "{e}");
}

#[test]
fn role_all_is_every_role_this_build_has() {
    let Ok(Command::Run(c)) = parse(args(&["--role", "all"]), env_of(TURSO)) else {
        panic!("expected run");
    };
    assert_eq!(c.role, Role::All);
    assert_eq!(c.role.as_str(), "all");
}

#[test]
fn dev_needs_no_env_and_defaults_to_loopback_and_the_dev_data_dir() {
    let Ok(Command::Run(c)) = parse(args(&["--role", "all", "--dev"]), env_of(&[])) else {
        panic!("expected run");
    };
    assert!(c.dev());
    assert_eq!(c.listen.to_string(), "127.0.0.1:8080");
    assert_eq!(
        c.database,
        Database::Local {
            data_dir: PathBuf::from("./.arena-cloud-dev")
        }
    );
    assert_eq!(
        c.database.local_path(),
        Some(PathBuf::from("./.arena-cloud-dev").join("gz_control.db"))
    );
    // The remote env is ignored in dev mode.
    let Ok(Command::Run(c)) = parse(
        args(&["--role", "control", "--dev", "--data-dir", "/tmp/d"]),
        env_of(TURSO),
    ) else {
        panic!("expected run");
    };
    assert_eq!(
        c.database,
        Database::Local {
            data_dir: PathBuf::from("/tmp/d")
        }
    );
}

#[test]
fn dev_refuses_a_non_loopback_listen() {
    for l in ["0.0.0.0:8080", "192.168.1.5:8080", "[::]:8080"] {
        let e = parse(
            args(&["--role", "all", "--dev", "--listen", l]),
            env_of(&[]),
        )
        .unwrap_err();
        assert!(e.contains("loopback"), "{l}: {e}");
    }
    for l in ["127.0.0.1:9999", "[::1]:9999"] {
        let Ok(Command::Run(c)) = parse(
            args(&["--role", "all", "--dev", "--listen", l]),
            env_of(&[]),
        ) else {
            panic!("{l}");
        };
        assert_eq!(c.listen.to_string(), l);
    }
}

#[test]
fn data_dir_needs_dev() {
    let e = parse(
        args(&["--role", "control", "--data-dir", "/tmp/d"]),
        env_of(TURSO),
    )
    .unwrap_err();
    assert!(e.contains("--dev"), "{e}");
    assert!(parse(args(&["--role", "all", "--dev=yes"]), env_of(&[])).is_err());
}

#[test]
fn debug_never_prints_the_token() {
    let Ok(Command::Run(c)) = parse(
        args(&["--role", "control"]),
        env_of(&[
            ("ARENA_CONTROL_DB_URL", "libsql://a.turso.io"),
            ("ARENA_CONTROL_DB_TOKEN", "tok-SECRET-xyz"),
        ]),
    ) else {
        panic!("expected run");
    };
    assert!(!format!("{c:?}").contains("tok-SECRET-xyz"));
}

#[test]
fn dev_ignores_a_bad_retry_env() {
    let Ok(Command::Run(c)) = parse(
        args(&["--role", "all", "--dev"]),
        env_of(&[("ARENA_CONTROL_DB_RETRY_SECS", "nope")]),
    ) else {
        panic!("--dev must ignore ARENA_CONTROL_DB_RETRY_SECS");
    };
    assert!(c.dev());
}

#[test]
fn the_command_sweeper_is_a_jobs_task() {
    assert_eq!(
        parse(
            args(&["--role", "jobs", "--task", "command-sweep"]),
            env_of(&[])
        ),
        Ok(Command::Job(JobConfig {
            task: Task::CommandSweep,
            dev_data_dir: None,
        }))
    );
    assert_eq!(Task::CommandSweep.as_str(), "command-sweep");
}

#[test]
fn the_offline_watch_is_a_jobs_task() {
    assert_eq!(
        parse(
            args(&["--role", "jobs", "--task", "offline-watch"]),
            env_of(&[])
        ),
        Ok(Command::Job(JobConfig {
            task: Task::OfflineWatch,
            dev_data_dir: None,
        }))
    );
    assert_eq!(Task::OfflineWatch.as_str(), "offline-watch");
}

#[test]
fn the_retention_sweep_is_a_jobs_task() {
    assert_eq!(
        parse(
            args(&["--role", "jobs", "--task", "retention-sweep"]),
            env_of(&[])
        ),
        Ok(Command::Job(JobConfig {
            task: Task::RetentionSweep,
            dev_data_dir: None,
        }))
    );
    assert_eq!(Task::RetentionSweep.as_str(), "retention-sweep");
}

#[test]
fn the_subscription_sweep_is_a_jobs_task() {
    assert_eq!(
        parse(
            args(&["--role", "jobs", "--task", "subscription-sweep"]),
            env_of(&[])
        ),
        Ok(Command::Job(JobConfig {
            task: Task::SubscriptionSweep,
            dev_data_dir: None,
        }))
    );
    assert_eq!(Task::SubscriptionSweep.as_str(), "subscription-sweep");
}

#[test]
fn jobs_run_one_task_and_read_no_env_at_parse_time() {
    assert_eq!(
        parse(
            args(&["--role", "jobs", "--task", "control-backup"]),
            env_of(&[])
        ),
        Ok(Command::Job(JobConfig {
            task: Task::ControlBackup,
            dev_data_dir: None,
        }))
    );
    assert_eq!(
        parse(
            args(&[
                "--role=jobs",
                "--task=control-backup",
                "--dev",
                "--data-dir",
                "/tmp/d"
            ]),
            env_of(&[])
        ),
        Ok(Command::Job(JobConfig {
            task: Task::ControlBackup,
            dev_data_dir: Some(PathBuf::from("/tmp/d")),
        }))
    );
}

#[test]
fn a_bad_task_combination_is_a_usage_error() {
    let e = parse(args(&["--role", "jobs", "--task", "vacuum"]), env_of(TURSO)).unwrap_err();
    assert!(e.contains("unknown --task vacuum"), "{e}");
    let e = parse(args(&["--role", "jobs"]), env_of(TURSO)).unwrap_err();
    assert!(e.contains("--task"), "{e}");
    for role in ["control", "all"] {
        let e = parse(
            args(&["--role", role, "--task", "control-backup"]),
            env_of(TURSO),
        )
        .unwrap_err();
        assert!(e.contains("--task needs --role jobs"), "{e}");
    }
    assert!(
        parse(
            args(&[
                "--role",
                "jobs",
                "--task",
                "control-backup",
                "--listen",
                "127.0.0.1:1"
            ]),
            env_of(TURSO)
        )
        .is_err()
    );
}

#[test]
fn restore_control_takes_one_source_and_one_decryption() {
    // The environment is read when the restore runs, not here.
    assert_eq!(
        parse(
            args(&[
                "ops",
                "restore-control",
                "--from-key",
                "gz_control/nightly/x.sql.age",
                "--identity",
                "/k"
            ]),
            env_of(&[])
        ),
        Ok(Command::RestoreControl(RestoreConfig {
            source: RestoreSource::Key("gz_control/nightly/x.sql.age".into()),
            decryption: Decryption::Identity(PathBuf::from("/k")),
            dev_data_dir: None,
        }))
    );
    assert_eq!(
        parse(
            args(&[
                "ops",
                "restore-control",
                "--from-file",
                "-",
                "--plaintext",
                "--dev",
                "--data-dir",
                "/d"
            ]),
            env_of(&[])
        ),
        Ok(Command::RestoreControl(RestoreConfig {
            source: RestoreSource::File(PathBuf::from("-")),
            decryption: Decryption::Plaintext,
            dev_data_dir: Some(PathBuf::from("/d")),
        }))
    );
    for bad in [
        &["ops", "restore-control", "--plaintext", "--dev"][..],
        &[
            "ops",
            "restore-control",
            "--from-file",
            "f",
            "--from-key",
            "k",
            "--plaintext",
            "--dev",
        ],
        &["ops", "restore-control", "--from-file", "f", "--dev"],
        &[
            "ops",
            "restore-control",
            "--from-file",
            "f",
            "--plaintext",
            "--identity",
            "i",
            "--dev",
        ],
        &[
            "ops",
            "restore-control",
            "--from-file",
            "f",
            "--plaintext",
            "--data-dir",
            "/d",
        ],
        &["ops", "restore-everything"],
        &["ops"],
    ] {
        assert!(parse(args(bad), env_of(TURSO)).is_err(), "{bad:?}");
    }
}
