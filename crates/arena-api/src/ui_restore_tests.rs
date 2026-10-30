//! Story 9.7: `POST /ui/backup/restore/check` and `POST /ui/backup/restore` over the loopback API.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use arena_core::rbac::ROLE_OWNER;
use arena_core::staff::hash_secret;
use arena_proto::{Actor, ActorKind, ExportHeader, IntentPayload, StaffUpsert, Tier};
use arena_store::{LATEST_SCHEMA_VERSION, Store};
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::ui_auth::tests::{Hub, T0, call, call_app, intent_as, token_for};

const CHECK: &str = "/ui/backup/restore/check";
const RESTORE: &str = "/ui/backup/restore";

/// A hub with an Owner (`owner` / `o-pass`, PIN 1111) and a router whose restart hook counts its
/// calls.
struct Fixture {
    hub: Hub,
    data: PathBuf,
    outside: tempfile::TempDir,
}

async fn fixture() -> Fixture {
    let hub = crate::ui_auth::tests::hub().await;
    let roles = Store::open_reader(&hub.path)
        .unwrap()
        .rows(arena_store::Table::Roles)
        .unwrap();
    let owner_role = roles
        .iter()
        .find(|r| r["name"] == arena_store::RowValue::Text(ROLE_OWNER.into()))
        .map(|r| match &r["role_id"] {
            arena_store::RowValue::Text(t) => t.clone(),
            other => panic!("{other:?}"),
        })
        .unwrap();
    hub.writer
        .submit_intent(intent_as(
            Actor {
                kind: ActorKind::Owner,
                id: "owner".into(),
            },
            IntentPayload::StaffUpsert(StaffUpsert {
                staff_id: None,
                login: "owner".into(),
                display_name: "Owner".into(),
                role_id: owner_role,
                password_hash: Some(hash_secret("o-pass").unwrap()),
                pin_hash: Some(hash_secret("1111").unwrap()),
                active: true,
                setup_code_hash: None,
                setup_expires_at: None,
            }),
            None,
        ))
        .await
        .unwrap();
    hub._status.send_modify(|s| s.tier = Some(Tier::Free));
    let data = hub.path.parent().unwrap().to_path_buf();
    Fixture {
        hub,
        data,
        outside: tempfile::tempdir().unwrap(),
    }
}

impl Fixture {
    async fn post(&self, uri: &str, token: &str, body: Value) -> (StatusCode, Value) {
        call_app(&self.hub.app, "POST", uri, Some(token), Some(body)).await
    }

    async fn token(&self, login: &str, password: &str) -> String {
        token_for(&self.hub, login, password).await
    }

    /// A good export of this branch outside the data folder, made a minute before `T0`.
    fn file(&self, name: &str) -> PathBuf {
        arena_store::write_export(&self.hub.path, self.outside.path(), name, T0 - 60_000)
            .unwrap()
            .path
    }

    fn data_listing(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(&self.data)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| !n.ends_with("-wal") && !n.ends_with("-shm"))
            .collect();
        v.sort();
        v
    }

    fn pending(&self) -> PathBuf {
        self.data.join("restore-pending")
    }
}

fn code(r: &(StatusCode, Value)) -> (StatusCode, &str) {
    (r.0, r.1["code"].as_str().unwrap_or(""))
}

fn relabel(file: &Path, edit: impl FnOnce(&mut ExportHeader)) {
    let bytes = std::fs::read(file).unwrap();
    let (mut header, at) = ExportHeader::decode(&bytes).unwrap();
    edit(&mut header);
    let mut out = header.encode();
    out.extend_from_slice(&bytes[at..]);
    std::fs::write(file, out).unwrap();
}

fn confirm(file: &Path, pin: &str, branch_code: &str) -> Value {
    json!({"path": file.to_string_lossy(), "pin": pin, "branch_code": branch_code})
}

#[tokio::test]
async fn the_owner_checks_a_good_file_and_sees_what_would_be_set_aside() {
    let f = fixture().await;
    let file = f.file("DEV01-2027-01-15-1325.a360");
    let owner = f.token("owner", "o-pass").await;
    let (status, reply) = f
        .post(CHECK, &owner, json!({"path": file.to_string_lossy()}))
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["branch_name"], "Dev Branch");
    assert_eq!(reply["branch_code"], "DEV01");
    assert_eq!(reply["file_name"], "DEV01-2027-01-15-1325.a360");
    assert_eq!(reply["exported_at"], T0 - 60_000);
    assert_eq!(reply["schema_version"], LATEST_SCHEMA_VERSION);
    assert!(reply["epoch"].as_str().is_some_and(|e| e.starts_with("1:")));
    assert!(reply["last_seq"].as_i64().unwrap() > 0);
    assert_eq!(reply["rows_set_aside"], 0);
    // 07:59 UTC is 13:29 in the branch's zone; the hub is on Free and holds no erasure.
    assert_eq!(
        reply["warning"],
        "Everything after 2027-01-15 13:29 on this PC will be set aside. Kiosks reconnect by themselves. Players erased after 2027-01-15 13:29 may reappear."
    );
    assert_eq!(reply["erased_may_reappear"], true);
    // Checking stages nothing and leaves the data folder as it was.
    assert!(!f.pending().exists());
    assert_eq!(f.hub.restarts.load(Ordering::SeqCst), 0);

    // On a paid tier the extra sentence is gone.
    f.hub._status.send_modify(|s| s.tier = Some(Tier::Basic));
    let (_, reply) = f
        .post(CHECK, &owner, json!({"path": file.to_string_lossy()}))
        .await;
    assert_eq!(reply["erased_may_reappear"], false);
    assert_eq!(
        reply["warning"],
        "Everything after 2027-01-15 13:29 on this PC will be set aside. Kiosks reconnect by themselves."
    );
}

#[tokio::test]
async fn only_the_owners_pin_opens_the_restore_for_anyone_else() {
    let f = fixture().await;
    let file = f.file("x.a360");
    let path = file.to_string_lossy().into_owned();
    for (login, password, own_pin) in [("manager", "m-pass", "2222"), ("counter", "c-pass", "3333")]
    {
        let token = f.token(login, password).await;
        // No PIN, and their own PIN: their role is not the Owner's.
        let r = f.post(CHECK, &token, json!({"path": path})).await;
        assert_eq!(
            code(&r),
            (StatusCode::FORBIDDEN, "PERMISSION_DENIED"),
            "{login}"
        );
        let r = f
            .post(CHECK, &token, json!({"path": path, "pin": own_pin}))
            .await;
        assert_eq!(
            code(&r),
            (StatusCode::FORBIDDEN, "PERMISSION_DENIED"),
            "{login}"
        );
        // A wrong PIN.
        let r = f
            .post(CHECK, &token, json!({"path": path, "pin": "9999"}))
            .await;
        assert_eq!(code(&r), (StatusCode::FORBIDDEN, "PIN_REJECTED"), "{login}");
        // The Owner's PIN opens the check in place.
        let r = f
            .post(CHECK, &token, json!({"path": path, "pin": "1111"}))
            .await;
        assert_eq!(r.0, StatusCode::OK, "{login}: {}", r.1);
        // Confirming with their own PIN is still refused.
        let r = f
            .post(RESTORE, &token, confirm(&file, own_pin, "DEV01"))
            .await;
        assert_eq!(
            code(&r),
            (StatusCode::FORBIDDEN, "PERMISSION_DENIED"),
            "{login}"
        );
    }
    assert!(!f.pending().exists());
    assert_eq!(f.hub.restarts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_manager_with_the_owners_pin_confirms_as_that_owner() {
    let f = fixture().await;
    let file = f.file("x.a360");
    let manager = f.token("manager", "m-pass").await;
    let (status, reply) = f
        .post(RESTORE, &manager, confirm(&file, "1111", "DEV01"))
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let marker: Value =
        serde_json::from_slice(&std::fs::read(f.pending().join("restore.json")).unwrap()).unwrap();
    let owner_id = Store::open_reader(&f.hub.path)
        .unwrap()
        .staff_with_role("owner")
        .unwrap()
        .unwrap()
        .0
        .staff_id;
    assert_eq!(marker["confirmed_by"], owner_id);
}

#[tokio::test]
async fn a_refused_file_is_named_and_nothing_is_staged() {
    let f = fixture().await;
    let owner = f.token("owner", "o-pass").await;
    let before = f.data_listing();

    let corrupt = f.file("corrupt.a360");
    let mut bytes = std::fs::read(&corrupt).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&corrupt, bytes).unwrap();
    let other = f.file("other.a360");
    relabel(&other, |h| h.branch_id = "01JOTHER".into());
    let newer = f.file("newer.a360");
    relabel(&newer, |h| h.schema_version = LATEST_SCHEMA_VERSION + 1);
    let junk = f.outside.path().join("junk.a360");
    std::fs::write(&junk, b"not an export").unwrap();

    let unprocessable = StatusCode::UNPROCESSABLE_ENTITY;
    for (file, want) in [
        (&corrupt, (unprocessable, "RESTORE_CORRUPT")),
        (&junk, (unprocessable, "RESTORE_CORRUPT")),
        (&other, (unprocessable, "RESTORE_WRONG_BRANCH")),
        (&newer, (unprocessable, "SCHEMA_TOO_NEW")),
    ] {
        let r = f
            .post(CHECK, &owner, json!({"path": file.to_string_lossy()}))
            .await;
        assert_eq!(code(&r), want, "check {file:?}");
        let r = f
            .post(RESTORE, &owner, confirm(file, "1111", "DEV01"))
            .await;
        assert_eq!(code(&r), want, "restore {file:?}");
    }
    // Not found, not absolute, not set.
    let gone = f.outside.path().join("gone.a360");
    let r = f
        .post(CHECK, &owner, json!({"path": gone.to_string_lossy()}))
        .await;
    assert_eq!(
        (r.0, r.1["field"].as_str(), r.1["message"].as_str()),
        (unprocessable, Some("path"), Some("not_found"))
    );
    let r = f
        .post(CHECK, &owner, json!({"path": "relative/x.a360"}))
        .await;
    assert_eq!(r.1["message"], "not_absolute");
    let r = f.post(CHECK, &owner, json!({"path": "  "})).await;
    assert_eq!(r.1["message"], "not_set");

    assert_eq!(f.data_listing(), before, "the data folder is as it was");
    assert_eq!(f.hub.restarts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_wrong_pin_or_branch_code_stages_nothing() {
    let f = fixture().await;
    let owner = f.token("owner", "o-pass").await;
    let file = f.file("x.a360");
    let before = f.data_listing();
    let r = f
        .post(RESTORE, &owner, confirm(&file, "9999", "DEV01"))
        .await;
    assert_eq!(code(&r), (StatusCode::FORBIDDEN, "PIN_REJECTED"));
    for typed in ["", "DEV02", "dev01"] {
        let r = f.post(RESTORE, &owner, confirm(&file, "1111", typed)).await;
        assert_eq!(
            code(&r),
            (StatusCode::UNPROCESSABLE_ENTITY, "VALIDATION_FAILED"),
            "{typed}"
        );
        assert_eq!(r.1["field"], "branch_code");
    }
    assert_eq!(f.data_listing(), before);
    assert!(!f.pending().exists());
    assert_eq!(f.hub.restarts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn too_many_wrong_pins_pause_the_attempts() {
    let f = fixture().await;
    let owner = f.token("owner", "o-pass").await;
    let file = f.file("x.a360");
    let mut last = (StatusCode::OK, Value::Null);
    for _ in 0..8 {
        last = f
            .post(RESTORE, &owner, confirm(&file, "9999", "DEV01"))
            .await;
        if last.1["code"] == "AUTH_LOCKED" {
            break;
        }
    }
    assert_eq!(code(&last).1, "AUTH_LOCKED", "{}", last.1);
    // Even the right PIN waits.
    let r = f
        .post(RESTORE, &owner, confirm(&file, "1111", "DEV01"))
        .await;
    assert_eq!(code(&r).1, "AUTH_LOCKED");
    assert!(!f.pending().exists());
}

#[tokio::test]
async fn the_owner_confirms_and_the_file_is_staged_before_the_hub_stops() {
    let f = fixture().await;
    let owner = f.token("owner", "o-pass").await;
    let file = f.file("x.a360");
    let changes_before = Store::open_reader(&f.hub.path)
        .unwrap()
        .changes()
        .unwrap()
        .len();

    let (status, reply) = f
        .post(RESTORE, &owner, confirm(&file, "1111", "DEV01"))
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply, json!({"restarting": true}));

    // Staged: the file and the marker. The live database is not touched yet.
    let pending = f.pending();
    assert_eq!(
        std::fs::read(pending.join("restore.a360")).unwrap(),
        std::fs::read(&file).unwrap()
    );
    let marker: Value =
        serde_json::from_slice(&std::fs::read(pending.join("restore.json")).unwrap()).unwrap();
    assert!(
        marker["confirmed_by"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    assert_eq!(marker["file_name"], "x.a360");
    assert_eq!(
        Store::open_reader(&f.hub.path)
            .unwrap()
            .changes()
            .unwrap()
            .len(),
        changes_before
    );

    // The Hub is asked to stop once the answer is out.
    for _ in 0..40 {
        if f.hub.restarts.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(f.hub.restarts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn the_android_hub_answers_unsupported() {
    let f = fixture().await;
    f.hub
        ._status
        .send_modify(|s| s.platform = arena_proto::HubPlatform::Android);
    let owner = f.token("owner", "o-pass").await;
    let file = f.file("x.a360");
    let r = f
        .post(CHECK, &owner, json!({"path": file.to_string_lossy()}))
        .await;
    assert_eq!(code(&r).1, "UNSUPPORTED", "{}", r.1);
    let r = f
        .post(RESTORE, &owner, confirm(&file, "1111", "DEV01"))
        .await;
    assert_eq!(code(&r).1, "UNSUPPORTED", "{}", r.1);
    assert!(!f.pending().exists());
    assert_eq!(f.hub.restarts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn the_direct_intent_route_does_not_take_ops_restore() {
    let f = fixture().await;
    let owner = f.token("owner", "o-pass").await;
    let file = f.file("x.a360");
    let (header, _) = ExportHeader::decode(&std::fs::read(&file).unwrap()).unwrap();
    let facts = arena_proto::OpsRestore {
        file_name: "x.a360".into(),
        header,
        old_epoch: "1:01JHUB".into(),
        new_epoch: "2:01JHUB".into(),
        rows_set_aside: 0,
        erasures_reapplied: 0,
        orphaned_file: "o.db".into(),
        restored_at: T0,
        by: None,
    };
    let r = call(
        &f.hub,
        "POST",
        "/ui/intent",
        Some(&owner),
        Some(json!({"intent": intent_as(
            Actor { kind: ActorKind::Staff, id: "x".into() },
            IntentPayload::OpsRestore(facts),
            None,
        )})),
    )
    .await;
    assert_eq!(code(&r).1, "UNSUPPORTED", "{}", r.1);
}
