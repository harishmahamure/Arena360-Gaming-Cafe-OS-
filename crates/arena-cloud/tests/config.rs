//! `--role`, `--listen` and the `gz_control` environment.

use std::collections::HashMap;
use std::ffi::OsString;
use std::time::Duration;

use arena_cloud::config::{Command, DEFAULT_RETRY_WINDOW, Role, parse};

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
        c.db_url,
        "libsql://gz-control-arena.aws-ap-south-1.turso.io"
    );
    assert_eq!(c.db_token, "tok");
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
fn other_roles_are_not_available_yet() {
    for role in ["builder", "command", "jobs", "all"] {
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
        assert_eq!(c.db_token, "");
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
    assert_eq!(c.db_url, "libsql://a.turso.io");
    assert_eq!(c.db_token, "tok");
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
