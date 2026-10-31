//! The nightly `gz_control` backup and its restore (Story 2.3): the task through the library
//! with an in-memory store and a local webhook listener, the binary's exit codes and alerts,
//! `ops restore-control`, and an end-to-end round trip through MinIO and sqld (gated).

#[path = "support/s3.rs"]
mod s3;
#[path = "../../arena-store/tests/support/sqld.rs"]
mod sqld;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use age::secrecy::ExposeSecret;
use arena_cloud::backup::{
    self, AlertTarget, BACKUP_SUFFIX, KEEP, NIGHTLY_PREFIX, apply_retention, code, object_key,
    run_backup,
};
use arena_cloud::store::{MemoryStore, ObjectStore, StoreError};
use arena_store::control::ControlDb;

const S3_SECRET: &str = "s3-SECRET-do-not-log-0123456789";
const HOOK_SECRET: &str = "T000/B000/hook-SECRET-path";
/// 2026-10-24T02:30:00Z
const NOW: i64 = 1_792_809_000_000;
const LATEST: u32 = arena_store::control::LATEST_CONTROL_VERSION;

// ---- fixtures -------------------------------------------------------------------------------

const FIXTURE: &str = "
    CREATE TABLE members (id INTEGER PRIMARY KEY, phone TEXT NOT NULL, note TEXT, pic BLOB);
    CREATE TABLE licences (id INTEGER PRIMARY KEY, owner_id INTEGER, tier TEXT, exp REAL);
    CREATE INDEX licences_owner ON licences (owner_id);
    INSERT INTO members VALUES (1, '+919800000001', 'it''s' || char(10) || 'two lines ✓', X'0102');
    INSERT INTO members VALUES (2, '+919800000002', NULL, NULL);
    WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 1500)
        INSERT INTO licences SELECT x, 1 + x % 2, 'free', x * 1.5 FROM c;
";

fn expected_counts() -> BTreeMap<String, u64> {
    // The fixture's tables, plus every migration's (empty) tables.
    [
        ("admin_audit", 0),
        ("ad_cursor", 0),
        ("ad_impression_daily", 0),
        ("ad_request_daily", 0),
        ("billing_bands", 4),
        ("billing_events", 0),
        ("billing_invoices", 0),
        ("branch_fence", 0),
        ("branch_opening", 0),
        ("branch_retention", 0),
        ("branch_setup_status", 0),
        ("branch_template_choice", 0),
        ("branch_tunnels", 0),
        ("branch_watch", 0),
        ("branches", 0),
        ("builder_jobs", 0),
        ("catalog_exe_signatures", 0),
        ("catalog_games", 0),
        ("catalog_media", 0),
        ("catalog_state", 1),
        ("commands", 0),
        ("entitlements", 0),
        ("hubs", 0),
        ("kek_escrow", 0),
        ("key_requests", 0),
        ("licences", 1500),
        ("members", 2),
        ("message_templates", 6),
        ("onboarding_progress", 0),
        ("ops_audit", 0),
        ("ops_tasks", 0),
        ("otp_challenges", 0),
        ("owner_alert_deliveries", 0),
        ("owner_alert_settings", 0),
        ("owner_alerts", 0),
        ("owner_backup_codes", 0),
        ("owner_devices", 0),
        ("owner_mfa", 0),
        ("owner_sessions", 0),
        ("owner_setup_codes", 0),
        ("owners", 0),
        ("pairing_codes", 0),
        ("price_book_items", 40),
        ("price_books", 1),
        ("push_subscriptions", 0),
        ("rate_limits", 0),
        ("send_ledger", 0),
        (
            "schema_migrations",
            u64::from(arena_store::control::LATEST_CONTROL_VERSION),
        ),
        ("signin_challenges", 0),
        ("staff_invites", 0),
        ("subscription_history", 0),
        ("subscriptions", 0),
        ("tenants", 0),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect()
}

/// A local gz_control at `<dir>/gz_control.db` (the `--dev --data-dir` file), migrated and
/// seeded.
async fn seeded_local(dir: &Path) -> ControlDb {
    let path = dir.join("gz_control.db");
    let db = ControlDb::open_local(&path).await.unwrap();
    db.migrate(1_000).await.unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(FIXTURE)
        .unwrap();
    db
}

fn keypair() -> (age::x25519::Identity, age::x25519::Recipient) {
    let id = age::x25519::Identity::generate();
    let r = id.to_public();
    (id, r)
}

fn write_identity(dir: &Path, id: &age::x25519::Identity) -> PathBuf {
    let path = dir.join("backup.agekey");
    std::fs::write(&path, format!("{}\n", id.to_string().expose_secret())).unwrap();
    path
}

fn target(webhook: Option<&str>) -> AlertTarget {
    AlertTarget {
        webhook: webhook.map(str::to_owned),
        env_name: "staging".into(),
        heartbeat: None,
    }
}

/// A store whose writes fail.
struct DownStore;

impl ObjectStore for DownStore {
    async fn put(&self, _: &str, _: Vec<u8>) -> Result<(), StoreError> {
        Err(StoreError(
            "put: dispatch failure: connection refused".into(),
        ))
    }
    async fn get(&self, _: &str) -> Result<Vec<u8>, StoreError> {
        Err(StoreError("down".into()))
    }
    async fn list(&self, _: &str) -> Result<Vec<String>, StoreError> {
        Err(StoreError("down".into()))
    }
    async fn delete(&self, _: &str) -> Result<(), StoreError> {
        Err(StoreError("down".into()))
    }
}

/// A local HTTP listener standing in for the alert webhook (or the heartbeat, or S3): answers
/// every request with `status` and hands over each request's head and body.
struct Webhook {
    url: String,
    port: u16,
    requests: mpsc::Receiver<(String, String)>,
}

impl Webhook {
    fn start(status: u16) -> Self {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for stream in l.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut len = 0usize;
                let mut head = String::new();
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    head.push_str(&line);
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0; len];
                let _ = r.read_exact(&mut body);
                let _ = write!(
                    s,
                    "HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = tx.send((head, String::from_utf8_lossy(&body).into_owned()));
            }
        });
        Self {
            url: format!("http://127.0.0.1:{port}/services/{HOOK_SECRET}"),
            port,
            requests: rx,
        }
    }

    /// The next request's body.
    fn next(&self) -> Option<String> {
        self.next_request().map(|(_, body)| body)
    }

    /// The next request's head (request line and headers) and body.
    fn next_request(&self) -> Option<(String, String)> {
        self.requests.recv_timeout(Duration::from_secs(15)).ok()
    }
}

// ---- the task through the library -----------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_nightly_run_writes_an_encrypted_object_that_restores_to_the_same_counts() {
    let dir = tempfile::tempdir().unwrap();
    let db = seeded_local(dir.path()).await;
    let (id, r) = keypair();
    let store = MemoryStore::new();
    let out = run_backup(&db, &store, &[r], NOW, &target(None), backup::RUN_TIMEOUT)
        .await
        .unwrap();
    assert_eq!(out.key, "gz_control/nightly/2026-10-24T023000Z.sql.age");
    assert_eq!(out.key, object_key(NOW));
    assert_eq!(store.keys(), vec![out.key.clone()]);
    assert_eq!(out.tables, expected_counts());
    assert!(out.deleted.is_empty());

    let sealed = store.get(&out.key).await.unwrap();
    assert!(sealed.starts_with(b"age-encryption.org/v1\n"));
    assert!(!String::from_utf8_lossy(&sealed).contains("+919800000001"));
    let plain = backup::decrypt(&sealed, &[Box::new(id)]).unwrap();
    let fresh = ControlDb::open_local(&dir.path().join("restored.db"))
        .await
        .unwrap();
    let report = fresh
        .restore(std::str::from_utf8(&plain).unwrap())
        .await
        .unwrap();
    assert_eq!(report.tables, expected_counts());
    assert_eq!(
        report.schema_version,
        arena_store::control::LATEST_CONTROL_VERSION
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn retention_keeps_the_newest_30_including_the_new_one() {
    let dir = tempfile::tempdir().unwrap();
    let db = seeded_local(dir.path()).await;
    let (_, r) = keypair();
    let store = MemoryStore::new();
    let day = 86_400_000;
    // 31 older nightly backups, plus objects retention must never touch.
    for i in 1..=31 {
        store
            .put(&object_key(NOW - i * day), vec![1])
            .await
            .unwrap();
    }
    store
        .put(
            &format!("{NIGHTLY_PREFIX}manual-copy{BACKUP_SUFFIX}"),
            vec![2],
        )
        .await
        .unwrap();
    store
        .put("gz_control/other/x.sql.age", vec![3])
        .await
        .unwrap();
    let out = run_backup(&db, &store, &[r], NOW, &target(None), backup::RUN_TIMEOUT)
        .await
        .unwrap();
    assert_eq!(
        out.deleted,
        vec![object_key(NOW - 31 * day), object_key(NOW - 30 * day)]
    );
    let nightly: Vec<String> = store
        .keys()
        .into_iter()
        .filter(|k| backup::is_backup_key(k))
        .collect();
    assert_eq!(nightly.len(), KEEP);
    assert_eq!(nightly.last(), Some(&out.key));
    assert!(nightly.contains(&object_key(NOW - 29 * day)));
    assert_eq!(store.keys().len(), KEEP + 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn retention_never_deletes_the_object_just_written() {
    let store = MemoryStore::new();
    let day = 86_400_000;
    for i in 1..=30 {
        store
            .put(&object_key(NOW + i * day), vec![1])
            .await
            .unwrap();
    }
    // A clock that went backwards: the new key sorts first.
    let just = object_key(NOW);
    store.put(&just, vec![1]).await.unwrap();
    assert!(
        apply_retention(&store, &just, KEEP)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(store.keys().contains(&just));
    // Not listed yet (eventual consistency): still counted among the newest, never deleted.
    let unlisted = object_key(NOW + 40 * day);
    let deleted = apply_retention(&store, &unlisted, KEEP).await.unwrap();
    assert_eq!(deleted, vec![object_key(NOW), object_key(NOW + day)]);
    assert_eq!(store.keys().len(), KEEP - 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_store_outage_fails_with_its_code_and_posts_the_alert() {
    let dir = tempfile::tempdir().unwrap();
    let db = seeded_local(dir.path()).await;
    let (_, r) = keypair();
    let hook = Webhook::start(200);
    let err = run_backup(
        &db,
        &DownStore,
        &[r],
        NOW,
        &target(Some(&hook.url)),
        backup::RUN_TIMEOUT,
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, code::UPLOAD_FAILED);
    let body: serde_json::Value = serde_json::from_str(&hook.next().unwrap()).unwrap();
    let text = body["text"].as_str().unwrap();
    for want in ["staging", "control-backup", "BACKUP_UPLOAD_FAILED"] {
        assert!(text.contains(want), "{text}");
    }
    assert!(
        !text.contains("connection refused"),
        "no detail in the alert: {text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_database_outage_fails_with_its_code_and_posts_the_alert() {
    let db = ControlDb::connect("http://127.0.0.1:9", "tok-SECRET")
        .await
        .unwrap();
    let (_, r) = keypair();
    let hook = Webhook::start(200);
    let store = MemoryStore::new();
    let err = run_backup(
        &db,
        &store,
        &[r],
        NOW,
        &target(Some(&hook.url)),
        backup::RUN_TIMEOUT,
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, code::DUMP_FAILED);
    assert!(!err.detail.contains("tok-SECRET"), "{}", err.detail);
    assert!(hook.next().unwrap().contains("BACKUP_DUMP_FAILED"));
    assert!(store.keys().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_or_missing_webhook_still_reports_the_failure() {
    let dir = tempfile::tempdir().unwrap();
    let db = seeded_local(dir.path()).await;
    let (_, r) = keypair();
    let broken = Webhook::start(500);
    let err = run_backup(
        &db,
        &DownStore,
        std::slice::from_ref(&r),
        NOW,
        &target(Some(&broken.url)),
        backup::RUN_TIMEOUT,
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, code::UPLOAD_FAILED);
    assert!(broken.next().is_some());
    let err = run_backup(
        &db,
        &DownStore,
        &[r],
        NOW,
        &target(None),
        backup::RUN_TIMEOUT,
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, code::UPLOAD_FAILED);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hung_run_times_out_with_its_own_code() {
    let dir = tempfile::tempdir().unwrap();
    let db = seeded_local(dir.path()).await;
    let (_, r) = keypair();
    let err = run_backup(
        &db,
        &MemoryStore::new(),
        &[r],
        NOW,
        &target(None),
        Duration::ZERO,
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, code::TIMEOUT);
}

#[test]
fn recipients_are_public_keys_and_errors_never_echo_them() {
    let (id, r) = keypair();
    let (_, r2) = keypair();
    assert_eq!(
        backup::parse_recipients(&format!(" {r} ,{r2},"))
            .unwrap()
            .len(),
        2
    );
    let secret = id.to_string().expose_secret().to_owned();
    for bad in [secret.as_str(), "", "age1nope", "ssh-ed25519 AAAA"] {
        let e = backup::parse_recipients(bad).unwrap_err();
        assert!(e.contains("ARENA_BACKUP_AGE_RECIPIENTS"), "{e}");
        if !bad.is_empty() {
            assert!(!e.contains(bad), "{e}");
        }
    }
}

#[test]
fn a_wrong_identity_cannot_decrypt() {
    let (_, r) = keypair();
    let (other, _) = keypair();
    let sealed = backup::encrypt(b"-- arena-control-dump v1\n", &[r]).unwrap();
    assert!(backup::decrypt(&sealed, &[Box::new(other)]).is_err());
}

// ---- the binary -----------------------------------------------------------------------------

fn cloud() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_arena-cloud"));
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("ARENA_") {
            c.env_remove(k);
        }
    }
    c.env("RUST_LOG", "info");
    c
}

fn run(c: &mut Command) -> Output {
    c.stdin(Stdio::null()).output().unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn closed_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
fn task_usage_errors_exit_64() {
    for a in [
        &["--role", "jobs", "--task", "vacuum"][..],
        &["--role", "jobs"],
        &["--role", "control", "--task", "control-backup"],
        &["--role", "all", "--dev", "--task", "control-backup"],
    ] {
        let o = run(cloud().args(a));
        assert_eq!(o.status.code(), Some(64), "{a:?}: {}", text(&o));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_job_binary_exits_1_and_alerts_when_the_store_is_down() {
    let dir = tempfile::tempdir().unwrap();
    seeded_local(dir.path()).await;
    let (_, r) = keypair();
    let hook = Webhook::start(200);
    let url = hook.url.clone();
    let data = dir.path().to_owned();
    let o = tokio::task::spawn_blocking(move || {
        run(cloud()
            .args([
                "--role",
                "jobs",
                "--task",
                "control-backup",
                "--dev",
                "--data-dir",
            ])
            .arg(&data)
            .env(
                "ARENA_BACKUP_S3_ENDPOINT",
                format!("http://127.0.0.1:{}", closed_port()),
            )
            .env("ARENA_BACKUP_S3_BUCKET", "b")
            .env("ARENA_BACKUP_S3_ACCESS_KEY_ID", "id")
            .env("ARENA_BACKUP_S3_SECRET_ACCESS_KEY", S3_SECRET)
            .env("ARENA_BACKUP_AGE_RECIPIENTS", r.to_string())
            .env("ARENA_ALERT_WEBHOOK_URL", url)
            .env("ARENA_ENV", "staging"))
    })
    .await
    .unwrap();
    let out = text(&o);
    assert_eq!(o.status.code(), Some(1), "{out}");
    assert!(out.contains("BACKUP_UPLOAD_FAILED"), "{out}");
    assert!(out.contains("alert sent"), "{out}");
    for secret in [S3_SECRET, HOOK_SECRET] {
        assert!(!out.contains(secret), "{out}");
    }
    let alert = hook.next().unwrap();
    assert!(
        alert.contains("staging") && alert.contains("BACKUP_UPLOAD_FAILED"),
        "{alert}"
    );
    for line in String::from_utf8_lossy(&o.stdout).lines() {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|_| panic!("not JSON: {line}"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bad_environment_is_a_failure_that_alerts_without_echoing_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let (id, _) = keypair();
    let private = id.to_string().expose_secret().to_owned();
    let hook = Webhook::start(200);
    let url = hook.url.clone();
    let data = dir.path().to_owned();
    let pasted = private.clone();
    let o = tokio::task::spawn_blocking(move || {
        run(cloud()
            .args([
                "--role",
                "jobs",
                "--task",
                "control-backup",
                "--dev",
                "--data-dir",
            ])
            .arg(&data)
            .env("ARENA_BACKUP_S3_ENDPOINT", "http://127.0.0.1:9")
            .env("ARENA_BACKUP_S3_BUCKET", "b")
            .env("ARENA_BACKUP_S3_ACCESS_KEY_ID", "id")
            .env("ARENA_BACKUP_S3_SECRET_ACCESS_KEY", S3_SECRET)
            // A private key pasted where the public one belongs.
            .env("ARENA_BACKUP_AGE_RECIPIENTS", &pasted)
            .env("ARENA_ALERT_WEBHOOK_URL", url))
    })
    .await
    .unwrap();
    let out = text(&o);
    assert_eq!(o.status.code(), Some(1), "{out}");
    assert!(out.contains("BACKUP_CONFIG_INVALID"), "{out}");
    assert!(!out.contains(&private) && !out.contains(S3_SECRET), "{out}");
    let alert = hook.next().unwrap();
    assert!(
        alert.contains("BACKUP_CONFIG_INVALID") && alert.contains("unknown"),
        "{alert}"
    );
    assert!(!alert.contains(&private));

    // Without a webhook the failure is only logged.
    let o = run(cloud()
        .args([
            "--role",
            "jobs",
            "--task",
            "control-backup",
            "--dev",
            "--data-dir",
        ])
        .arg(dir.path()));
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("ARENA_ALERT_WEBHOOK_URL"), "{}", text(&o));
}

/// Writes `dump` encrypted to `r` into `<dir>/<name>`.
fn sealed_file(dir: &Path, name: &str, dump: &str, r: &age::x25519::Recipient) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(
        &path,
        backup::encrypt(dump.as_bytes(), std::slice::from_ref(r)).unwrap(),
    )
    .unwrap();
    path
}

fn restore_cmd(source: &Path, identity: &Path, data_dir: &Path) -> Command {
    let mut c = cloud();
    c.args(["ops", "restore-control", "--from-file"])
        .arg(source)
        .arg("--identity")
        .arg(identity)
        .args(["--dev", "--data-dir"])
        .arg(data_dir);
    c
}

fn local_tables(data_dir: &Path) -> Vec<String> {
    let path = data_dir.join("gz_control.db");
    if !path.exists() {
        return vec![];
    }
    let c = rusqlite::Connection::open(path).unwrap();
    let mut st = c
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap();
    st.query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn restore_control_from_a_file_verifies_counts_and_refuses_bad_input() {
    let dir = tempfile::tempdir().unwrap();
    let src = tempfile::tempdir().unwrap();
    let dump = seeded_local(src.path()).await.dump(NOW).await.unwrap();
    let (id, r) = keypair();
    let identity = write_identity(dir.path(), &id);
    let good = sealed_file(dir.path(), "good.sql.age", &dump, &r);

    // Restore ok, counts printed.
    let target = dir.path().join("target");
    let o = run(&mut restore_cmd(&good, &identity, &target));
    let out = text(&o);
    assert_eq!(o.status.code(), Some(0), "{out}");
    assert!(
        out.contains(&format!(
            "schema version {}",
            arena_store::control::LATEST_CONTROL_VERSION
        )),
        "{out}"
    );
    for (table, n) in expected_counts() {
        assert!(
            out.lines()
                .any(|l| l.split_whitespace().collect::<Vec<_>>()
                    == [table.as_str(), &n.to_string()]),
            "{table} {n}: {out}"
        );
    }
    // Restore non-empty: the same target again.
    let o = run(&mut restore_cmd(&good, &identity, &target));
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        text(&o).matches("RESTORE_TARGET_NOT_EMPTY").count(),
        1,
        "the code once: {}",
        text(&o)
    );

    // Wrong identity: nothing written.
    let (other, _) = keypair();
    let other_dir = tempfile::tempdir().unwrap();
    let wrong = write_identity(other_dir.path(), &other);
    let empty = dir.path().join("wrong-identity");
    let o = run(&mut restore_cmd(&good, &wrong, &empty));
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("RESTORE_DECRYPT_FAILED"), "{}", text(&o));
    assert!(local_tables(&empty).is_empty());

    // Count mismatch: a tampered manifest names the table.
    let tampered = sealed_file(
        dir.path(),
        "tampered.sql.age",
        &dump.replacen("\"members\":2", "\"members\":3", 1),
        &r,
    );
    let o = run(&mut restore_cmd(
        &tampered,
        &identity,
        &dir.path().join("t2"),
    ));
    assert_eq!(o.status.code(), Some(1));
    assert!(
        text(&o).contains("RESTORE_COUNT_MISMATCH") && text(&o).contains("members"),
        "{}",
        text(&o)
    );

    // Newer dump: SCHEMA_TOO_NEW, nothing written.
    let newer = sealed_file(
        dir.path(),
        "newer.sql.age",
        &dump.replacen(
            &format!("\"schema_version\":{LATEST}"),
            &format!("\"schema_version\":{}", LATEST + 1),
            1,
        ),
        &r,
    );
    let t3 = dir.path().join("t3");
    let o = run(&mut restore_cmd(&newer, &identity, &t3));
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("SCHEMA_TOO_NEW"), "{}", text(&o));
    assert!(local_tables(&t3).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn restore_control_reads_a_plaintext_dump_from_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let src = tempfile::tempdir().unwrap();
    let dump = seeded_local(src.path()).await.dump(NOW).await.unwrap();
    let target = dir.path().join("target");
    let mut child = cloud()
        .args([
            "ops",
            "restore-control",
            "--from-file",
            "-",
            "--plaintext",
            "--dev",
            "--data-dir",
        ])
        .arg(&target)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(dump.as_bytes())
        .unwrap();
    let o = child.wait_with_output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    assert_eq!(
        local_tables(&target),
        vec![
            "ad_cursor",
            "ad_impression_daily",
            "ad_request_daily",
            "admin_audit",
            "billing_bands",
            "billing_events",
            "billing_invoices",
            "branch_fence",
            "branch_opening",
            "branch_retention",
            "branch_setup_status",
            "branch_template_choice",
            "branch_tunnels",
            "branch_watch",
            "branches",
            "builder_jobs",
            "catalog_exe_signatures",
            "catalog_games",
            "catalog_media",
            "catalog_state",
            "commands",
            "entitlements",
            "hubs",
            "kek_escrow",
            "key_requests",
            "licences",
            "members",
            "message_templates",
            "onboarding_progress",
            "ops_audit",
            "ops_tasks",
            "otp_challenges",
            "owner_alert_deliveries",
            "owner_alert_settings",
            "owner_alerts",
            "owner_backup_codes",
            "owner_devices",
            "owner_mfa",
            "owner_sessions",
            "owner_setup_codes",
            "owners",
            "pairing_codes",
            "price_book_items",
            "price_books",
            "push_subscriptions",
            "rate_limits",
            "schema_migrations",
            "send_ledger",
            "signin_challenges",
            "staff_invites",
            "subscription_history",
            "subscriptions",
            "tenants"
        ]
    );
}

// ---- end to end: sqld + MinIO ---------------------------------------------------------------

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn healthz(port: u16) -> Option<serde_json::Value> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        s,
        "GET /healthz HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut buf = String::new();
    s.read_to_string(&mut buf).ok()?;
    serde_json::from_str(buf.split_once("\r\n\r\n")?.1).ok()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backup_through_minio_restores_into_a_fresh_sqld_and_control_starts_on_it() {
    let Some(t) = sqld::fresh().await else { return };
    let Some(bucket) = s3::bucket().await else {
        return;
    };

    // Source: migrated and seeded on sqld.
    let src = ControlDb::connect(&t.url, "").await.unwrap();
    src.migrate(1_000).await.unwrap();
    sqld::exec(&t.url, FIXTURE).await;
    let source_rows = sqld::migration_rows(&t.url).await;
    let source_version = src.schema_version().await.unwrap();

    let dir = tempfile::tempdir().unwrap();
    let (id, r) = keypair();
    let identity = write_identity(dir.path(), &id);
    let s3_env = |c: &mut Command| {
        c.env("ARENA_BACKUP_S3_ENDPOINT", &bucket.cfg.endpoint)
            .env("ARENA_BACKUP_S3_BUCKET", &bucket.cfg.bucket)
            .env("ARENA_BACKUP_S3_ACCESS_KEY_ID", &bucket.cfg.access_key_id)
            .env(
                "ARENA_BACKUP_S3_SECRET_ACCESS_KEY",
                &bucket.cfg.secret_access_key,
            );
    };

    // The nightly task.
    let mut job = cloud();
    job.args(["--role", "jobs", "--task", "control-backup"])
        .env("ARENA_CONTROL_DB_URL", &t.url)
        .env("ARENA_BACKUP_AGE_RECIPIENTS", r.to_string())
        .env("ARENA_ENV", "test");
    s3_env(&mut job);
    let o = tokio::task::spawn_blocking(move || run(&mut job))
        .await
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    let keys = bucket.store.list(NIGHTLY_PREFIX).await.unwrap();
    assert_eq!(keys.len(), 1, "{keys:?}");
    assert!(backup::is_backup_key(&keys[0]), "{keys:?}");

    // A fresh (emptied) sqld, restored from the bucket.
    sqld::wipe(&t.url).await;
    assert!(sqld::tables(&t.url).await.is_empty());
    let mut restore = cloud();
    restore
        .args([
            "ops",
            "restore-control",
            "--from-key",
            &keys[0],
            "--identity",
        ])
        .arg(&identity)
        .env("ARENA_CONTROL_DB_URL", &t.url);
    s3_env(&mut restore);
    let o = tokio::task::spawn_blocking(move || run(&mut restore))
        .await
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    let restored = ControlDb::connect(&t.url, "").await.unwrap();
    let dump = restored.dump(0).await.unwrap();
    let manifest = arena_store::control::read_manifest(&dump).unwrap();
    assert_eq!(manifest.tables, expected_counts());
    assert_eq!(sqld::migration_rows(&t.url).await, source_rows);

    // The control role starts on it at the source's version, applying nothing.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let _server = Running(
        cloud()
            .args([
                "--role",
                "control",
                "--listen",
                &format!("127.0.0.1:{port}"),
            ])
            .env("ARENA_CONTROL_DB_URL", &t.url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    let health = loop {
        if let Some(h) = healthz(port) {
            break h;
        }
        assert!(Instant::now() < deadline, "the control role did not start");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert_eq!(health["migration_version"], u64::from(source_version));
    assert_eq!(sqld::migration_rows(&t.url).await, source_rows);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_s3_store_puts_gets_lists_and_deletes_against_minio() {
    let Some(bucket) = s3::bucket().await else {
        return;
    };
    let s = &bucket.store;
    s.put("a/1", b"one".to_vec()).await.unwrap();
    s.put("a/2", vec![0, 255]).await.unwrap();
    s.put("b/1", vec![]).await.unwrap();
    assert_eq!(s.get("a/2").await.unwrap(), vec![0, 255]);
    let mut keys = s.list("a/").await.unwrap();
    keys.sort();
    assert_eq!(keys, vec!["a/1", "a/2"]);
    s.delete("a/1").await.unwrap();
    assert_eq!(s.list("a/").await.unwrap(), vec!["a/2"]);
    assert!(s.get("a/1").await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_errors_never_carry_the_credentials() {
    let Some(bucket) = s3::bucket().await else {
        return;
    };
    let mut cfg = bucket.cfg.clone();
    cfg.access_key_id = "AKID-not-a-real-key-0123".into();
    cfg.secret_access_key = S3_SECRET.into();
    let err = arena_cloud::store::S3Store::new(&cfg)
        .put("x", vec![1])
        .await
        .unwrap_err()
        .to_string();
    assert!(!err.contains(S3_SECRET), "{err}");
    assert!(!err.contains("AKID-not-a-real-key-0123"), "{err}");
}

/// A store whose writes succeed but whose listing fails (retention cannot run).
struct NoListStore(MemoryStore);

impl ObjectStore for NoListStore {
    async fn put(&self, k: &str, b: Vec<u8>) -> Result<(), StoreError> {
        self.0.put(k, b).await
    }
    async fn get(&self, k: &str) -> Result<Vec<u8>, StoreError> {
        self.0.get(k).await
    }
    async fn list(&self, _: &str) -> Result<Vec<String>, StoreError> {
        Err(StoreError("list: access denied".into()))
    }
    async fn delete(&self, k: &str) -> Result<(), StoreError> {
        self.0.delete(k).await
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_retention_failure_after_the_put_is_its_own_failure() {
    let dir = tempfile::tempdir().unwrap();
    let db = seeded_local(dir.path()).await;
    let (_, r) = keypair();
    let hook = Webhook::start(200);
    let store = NoListStore(MemoryStore::new());
    let err = run_backup(
        &db,
        &store,
        &[r],
        NOW,
        &target(Some(&hook.url)),
        backup::RUN_TIMEOUT,
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, code::RETENTION_FAILED);
    // The backup itself was written.
    assert_eq!(store.0.keys(), vec![object_key(NOW)]);
    assert!(hook.next().unwrap().contains("BACKUP_RETENTION_FAILED"));
}

#[test]
fn either_of_two_recipients_decrypts() {
    let (first, r1) = keypair();
    let (second, r2) = keypair();
    let sealed = backup::encrypt(b"-- arena-control-dump v1\n", &[r1, r2]).unwrap();
    for id in [first, second] {
        assert_eq!(
            backup::decrypt(&sealed, &[Box::new(id)]).unwrap(),
            b"-- arena-control-dump v1\n"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_successful_run_pings_the_heartbeat_and_a_failed_ping_keeps_success() {
    let dir = tempfile::tempdir().unwrap();
    let db = seeded_local(dir.path()).await;
    let (_, r) = keypair();
    let beat = Webhook::start(200);
    let mut t = target(None);
    t.heartbeat = Some(beat.url.clone());
    let once = std::slice::from_ref(&r);
    run_backup(&db, &MemoryStore::new(), once, NOW, &t, backup::RUN_TIMEOUT)
        .await
        .unwrap();
    let (head, _) = beat.next_request().unwrap();
    assert!(head.starts_with("GET /services/"), "{head}");

    // A heartbeat that answers 500, or nobody listening: still a successful backup.
    let broken = Webhook::start(500);
    t.heartbeat = Some(broken.url.clone());
    run_backup(&db, &MemoryStore::new(), once, NOW, &t, backup::RUN_TIMEOUT)
        .await
        .unwrap();
    assert!(broken.next_request().is_some());
    t.heartbeat = Some(format!("http://127.0.0.1:{}/x", closed_port()));
    run_backup(&db, &MemoryStore::new(), once, NOW, &t, backup::RUN_TIMEOUT)
        .await
        .unwrap();

    // A failed run never pings.
    let silent = Webhook::start(200);
    t.heartbeat = Some(silent.url.clone());
    run_backup(&db, &DownStore, once, NOW, &t, backup::RUN_TIMEOUT)
        .await
        .unwrap_err();
    assert!(
        silent
            .requests
            .recv_timeout(Duration::from_millis(500))
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn puts_are_path_style_without_checksum_trailers() {
    // R2 rejects the SDK's default CRC trailers (MinIO accepts them), so check the wire.
    let rec = Webhook::start(200);
    let store = arena_cloud::store::S3Store::new(&arena_cloud::store::S3Config {
        endpoint: format!("http://127.0.0.1:{}", rec.port),
        bucket: "bkt".into(),
        access_key_id: "id".into(),
        secret_access_key: "secret".into(),
    });
    store
        .put("gz_control/nightly/x.sql.age", vec![7; 2048])
        .await
        .unwrap();
    let (head, body) = rec.next_request().unwrap();
    let mut lines = head.lines();
    let request = lines.next().unwrap_or_default();
    // Path-style: the bucket is the first path segment (the SDK adds `?x-id=PutObject`).
    let path = request
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default();
    assert!(request.starts_with("PUT "), "{head}");
    assert_eq!(path, "/bkt/gz_control/nightly/x.sql.age", "{head}");
    assert!(
        head.to_ascii_lowercase().contains("\nhost: 127.0.0.1:"),
        "{head}"
    );
    for h in lines.map(str::to_ascii_lowercase) {
        assert!(!h.starts_with("x-amz-trailer"), "{h}");
        assert!(!h.starts_with("x-amz-checksum-"), "{h}");
        assert!(
            !(h.starts_with("content-encoding") && h.contains("aws-chunked")),
            "{h}"
        );
        assert!(
            !(h.starts_with("transfer-encoding") && h.contains("chunked")),
            "{h}"
        );
    }
    assert_eq!(body.len(), 2048);
}

#[test]
fn restore_without_a_target_database_exits_1_with_a_code() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("dump.sql");
    std::fs::write(&f, "-- arena-control-dump v1\n").unwrap();
    let o = run(cloud()
        .args(["ops", "restore-control", "--from-file"])
        .arg(&f)
        .arg("--plaintext"));
    let out = text(&o);
    assert_eq!(o.status.code(), Some(1), "{out}");
    assert!(out.contains("RESTORE_CONFIG_INVALID"), "{out}");
    assert!(out.contains("ARENA_CONTROL_DB_URL"), "{out}");
}
