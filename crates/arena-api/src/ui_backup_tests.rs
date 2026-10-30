//! Story 9.4: `POST /ui/backup/export` over the loopback API.

use std::path::Path;

use arena_proto::{
    Actor, ActorKind, BackupExport, ExportHeader, IntentPayload, SettingsSet, export_file_name,
};
use arena_store::Store;
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::ui_auth::tests::{Hub, T0, call, hub, intent_as, token_for};

const URL: &str = "/ui/backup/export";
const KEY_DIR: &str = "system.backup_dir";
const KEY_LAST: &str = "backup.last_export";

fn setting(hub: &Hub, key: &str) -> (String, i64) {
    let s = Store::open_reader(&hub.path)
        .unwrap()
        .settings()
        .unwrap()
        .into_iter()
        .find(|s| s.key == key)
        .unwrap();
    (s.value, s.rev)
}

/// Points the backup location at `value` the way the Hub stores it (an owner command skips the
/// probe, so a location that is already gone can be set).
async fn locate(hub: &Hub, value: &str) {
    let (_, rev) = setting(hub, KEY_DIR);
    hub.writer
        .submit_intent(intent_as(
            Actor {
                kind: ActorKind::Owner,
                id: "owner".into(),
            },
            IntentPayload::SettingsSet(SettingsSet {
                key: KEY_DIR.into(),
                value: value.into(),
            }),
            Some(rev),
        ))
        .await
        .unwrap();
}

fn changes(hub: &Hub) -> Vec<arena_store::ChangeRecord> {
    Store::open_reader(&hub.path).unwrap().changes().unwrap()
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

async fn export(hub: &Hub, token: &str, ticket: Option<&str>) -> (StatusCode, Value) {
    let body = ticket.map(|t| json!({ "elevation_ticket": t }));
    call(hub, "POST", URL, Some(token), body).await
}

async fn ticket_for(hub: &Hub, token: &str, kind: &str, pin: &str) -> String {
    let (status, ok) = call(
        hub,
        "POST",
        "/ui/elevate",
        Some(token),
        Some(json!({"items": [{"intent_kind": kind}], "pin": pin})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{ok}");
    ok["ticket"].as_str().unwrap().to_owned()
}

fn refused(r: &(StatusCode, Value), status: StatusCode, code: &str) {
    assert_eq!((r.0, r.1["code"].as_str()), (status, Some(code)), "{}", r.1);
}

fn invalid_folder(r: &(StatusCode, Value), reason: &str) {
    refused(r, StatusCode::UNPROCESSABLE_ENTITY, "VALIDATION_FAILED");
    assert_eq!(r.1["field"], "folder", "{}", r.1);
    assert_eq!(r.1["message"], reason, "{}", r.1);
}

#[tokio::test]
async fn a_manager_exports_and_the_file_and_the_record_agree() {
    let hub = hub().await;
    let dir = tempfile::tempdir().unwrap();
    locate(&hub, &dir.path().to_string_lossy()).await;
    let token = token_for(&hub, "manager", "m-pass").await;
    let before = changes(&hub).len();

    let (status, reply) = export(&hub, &token, None).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let facts: BackupExport = serde_json::from_value(reply.clone()).unwrap();
    // T0 is 2027-01-15 08:00 UTC, 13:30 in the branch's Asia/Kolkata.
    assert_eq!(facts.file_name, "DEV01-2027-01-15-1330.a360");
    assert_eq!(
        facts.file_name,
        export_file_name("DEV01", T0, "Asia/Kolkata")
    );
    assert_eq!(facts.exported_at, T0);
    assert_eq!(
        names(dir.path()),
        vec![facts.file_name.clone()],
        "no part file stays"
    );

    let bytes = std::fs::read(dir.path().join(&facts.file_name)).unwrap();
    assert_eq!(i64::try_from(bytes.len()).unwrap(), facts.bytes);
    let (header, _) = ExportHeader::decode(&bytes).unwrap();
    assert_eq!(
        (
            header.branch_code.as_str(),
            header.epoch.as_str(),
            header.last_seq
        ),
        ("DEV01", facts.epoch.as_str(), facts.last_seq)
    );
    // The file stands for the point just before the record of it.
    let rows = changes(&hub);
    assert_eq!(rows.len(), before + 1);
    assert_eq!(rows[before - 1].seq, facts.last_seq);
    assert_eq!(rows[before].actor_id, hub.manager_id);
    assert_eq!(rows[before].elevated_by_id, None);
    assert_eq!(
        BackupExport::parse(&setting(&hub, KEY_LAST).0),
        Some(facts),
        "backup.last_export holds the facts"
    );
}

#[tokio::test]
async fn no_location_is_not_set_and_nothing_is_written() {
    let hub = hub().await;
    let token = token_for(&hub, "manager", "m-pass").await;
    let before = changes(&hub).len();
    invalid_folder(&export(&hub, &token, None).await, "not_set");
    assert_eq!(changes(&hub).len(), before);
    assert_eq!(setting(&hub, KEY_LAST).0, "");
}

#[tokio::test]
async fn a_removed_drive_a_file_and_a_tree_uri_name_their_cause_and_leave_nothing() {
    let hub = hub().await;
    let token = token_for(&hub, "manager", "m-pass").await;
    let root = tempfile::tempdir().unwrap();

    let usb = root.path().join("usb");
    std::fs::create_dir(&usb).unwrap();
    locate(&hub, &usb.to_string_lossy()).await;
    std::fs::remove_dir(&usb).unwrap();
    invalid_folder(&export(&hub, &token, None).await, "not_found");

    let file = root.path().join("a-file");
    std::fs::write(&file, b"x").unwrap();
    locate(&hub, &file.to_string_lossy()).await;
    invalid_folder(&export(&hub, &token, None).await, "not_dir");

    locate(
        &hub,
        "content://com.android.externalstorage.documents/tree/primary%3ABackups",
    )
    .await;
    invalid_folder(&export(&hub, &token, None).await, "unsupported");

    assert_eq!(names(root.path()), vec!["a-file"]);
    assert_eq!(setting(&hub, KEY_LAST).0, "");
}

#[tokio::test]
async fn a_counter_without_a_ticket_is_refused_before_any_file_work() {
    let hub = hub().await;
    let token = token_for(&hub, "counter", "c-pass").await;
    // With no location set, the answer is still the permission, not the folder.
    refused(
        &export(&hub, &token, None).await,
        StatusCode::FORBIDDEN,
        "PERMISSION_DENIED",
    );
    let dir = tempfile::tempdir().unwrap();
    locate(&hub, &dir.path().to_string_lossy()).await;
    let before = changes(&hub).len();
    refused(
        &export(&hub, &token, None).await,
        StatusCode::FORBIDDEN,
        "PERMISSION_DENIED",
    );
    assert!(names(dir.path()).is_empty());
    assert_eq!(changes(&hub).len(), before);
}

#[tokio::test]
async fn a_counter_with_a_managers_pin_exports_once_and_the_audit_names_both() {
    let hub = hub().await;
    let dir = tempfile::tempdir().unwrap();
    locate(&hub, &dir.path().to_string_lossy()).await;
    let token = token_for(&hub, "counter", "c-pass").await;

    let t = ticket_for(&hub, &token, "backup.export", "2222").await;
    let (status, reply) = export(&hub, &token, Some(&t)).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let last = changes(&hub).pop().unwrap();
    assert_eq!(last.actor_id, hub.counter_id);
    assert_eq!(
        last.elevated_by_id.as_deref(),
        Some(hub.manager_id.as_str())
    );
    let name = reply["file_name"].as_str().unwrap().to_owned();
    assert_eq!(names(dir.path()), vec![name.clone()]);

    // The ticket was spent: a second tap is refused before it touches the first file.
    let before = changes(&hub).len();
    refused(
        &export(&hub, &token, Some(&t)).await,
        StatusCode::FORBIDDEN,
        "PIN_REQUIRED",
    );
    assert_eq!(names(dir.path()), vec![name]);
    assert_eq!(changes(&hub).len(), before);
}

#[tokio::test]
async fn a_junk_ticket_or_one_for_another_kind_does_not_export() {
    let hub = hub().await;
    let dir = tempfile::tempdir().unwrap();
    locate(&hub, &dir.path().to_string_lossy()).await;
    let token = token_for(&hub, "counter", "c-pass").await;
    refused(
        &export(&hub, &token, Some("junk")).await,
        StatusCode::FORBIDDEN,
        "PIN_REQUIRED",
    );
    let other = ticket_for(&hub, &token, "product.upsert", "2222").await;
    refused(
        &export(&hub, &token, Some(&other)).await,
        StatusCode::FORBIDDEN,
        "PIN_REQUIRED",
    );
    assert!(names(dir.path()).is_empty());
}

#[tokio::test]
async fn the_direct_intent_route_does_not_take_a_backup_export() {
    let hub = hub().await;
    let token = token_for(&hub, "manager", "m-pass").await;
    let facts = BackupExport {
        file_name: "x.a360".into(),
        bytes: 1,
        epoch: String::new(),
        last_seq: 0,
        exported_at: T0,
    };
    let (status, body) = call(
        &hub,
        "POST",
        "/ui/intent",
        Some(&token),
        Some(json!({"intent": intent_as(
            Actor { kind: ActorKind::Staff, id: "x".into() },
            IntentPayload::BackupExport(facts),
            None,
        )})),
    )
    .await;
    assert_ne!(status, StatusCode::OK, "{body}");
    assert_eq!(body["code"], "UNSUPPORTED");
    assert_eq!(setting(&hub, KEY_LAST).0, "");
}

#[tokio::test]
async fn a_request_with_no_session_is_unauthorised() {
    let hub = hub().await;
    let (status, _) = call(&hub, "POST", URL, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_export_the_intent_refuses_is_deleted() {
    let hub = hub().await;
    let dir = tempfile::tempdir().unwrap();
    locate(&hub, &dir.path().to_string_lossy()).await;
    let token = token_for(&hub, "manager", "m-pass").await;
    // A branch whose record row is missing refuses the intent after the file exists.
    hub.writer
        .submit(|tx| {
            tx.execute_batch("DELETE FROM settings WHERE key = 'backup.last_export'")
                .map_err(arena_store::db_error)
        })
        .await
        .unwrap();
    let before = changes(&hub).len();
    let (status, body) = export(&hub, &token, None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "VALIDATION_FAILED");
    assert!(names(dir.path()).is_empty(), "the file was deleted");
    assert_eq!(changes(&hub).len(), before);
}

#[tokio::test]
async fn a_refused_export_keeps_an_earlier_file_it_replaced() {
    let hub = hub().await;
    let dir = tempfile::tempdir().unwrap();
    locate(&hub, &dir.path().to_string_lossy()).await;
    let token = token_for(&hub, "manager", "m-pass").await;
    let (status, body) = export(&hub, &token, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let kept = names(dir.path());
    assert_eq!(kept.len(), 1);
    hub.writer
        .submit(|tx| {
            tx.execute_batch("DELETE FROM settings WHERE key = 'backup.last_export'")
                .map_err(arena_store::db_error)
        })
        .await
        .unwrap();
    // Same clock minute: the second export replaces the first file, then the intent is refused.
    let (status, _) = export(&hub, &token, None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(names(dir.path()), kept, "the earlier export is not deleted");
}
