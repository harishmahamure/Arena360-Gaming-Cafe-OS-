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
    // The record adds who made it and the list of the exports before this one.
    let kept = BackupExport::parse(&setting(&hub, KEY_LAST).0).expect("a record");
    assert!(kept.by.is_some(), "the staff member is named");
    assert_eq!(
        BackupExport {
            by: None,
            recent: Vec::new(),
            ..kept
        },
        facts,
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
        ..Default::default()
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

// Story 9.6: `POST /ui/backup/export-everything`.

const EVERYTHING: &str = "/ui/backup/export-everything";

async fn everything(
    hub: &Hub,
    token: &str,
    range: (&str, &str),
    ticket: Option<&str>,
) -> (StatusCode, Value) {
    let mut body = json!({"from": range.0, "to": range.1});
    if let Some(t) = ticket {
        body["elevation_ticket"] = json!(t);
    }
    call(hub, "POST", EVERYTHING, Some(token), Some(body)).await
}

#[tokio::test]
async fn a_manager_exports_everything_into_one_folder_on_a_free_hub() {
    // The test Hub is on the Free tier: no tier lock stands in the way.
    let hub = hub().await;
    let dir = tempfile::tempdir().unwrap();
    locate(&hub, &dir.path().to_string_lossy()).await;
    let token = token_for(&hub, "manager", "m-pass").await;
    let before = changes(&hub).len();

    let (status, reply) = everything(&hub, &token, ("2027-01-01", "2027-01-15"), None).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let done: arena_proto::EverythingExported = serde_json::from_value(reply).unwrap();
    assert_eq!(
        done.folder_name,
        "DEV01-everything-2027-01-01_2027-01-15-2027-01-15-1330"
    );
    assert_eq!(names(dir.path()), vec![done.folder_name.clone()]);
    let inside = names(&dir.path().join(&done.folder_name));
    assert!(inside.contains(&"README.txt".to_owned()));
    assert!(inside.contains(&done.export.file_name));
    assert!(done.reports.iter().all(|r| inside.contains(r)));
    assert!(
        done.reports
            .iter()
            .any(|r| r.starts_with("DEV01_daily-sales_2027-01-01_2027-01-15"))
    );

    // The database file is recorded like any export, by the staff member.
    assert_eq!(changes(&hub).len(), before + 1);
    let kept = BackupExport::parse(&setting(&hub, KEY_LAST).0).expect("a record");
    assert_eq!(kept.file_name, done.export.file_name);
    assert!(kept.by.is_some());
    assert_eq!(
        changes(&hub).pop().unwrap().actor_id,
        hub.manager_id,
        "the audit names the manager"
    );
}

#[tokio::test]
async fn a_bad_range_or_location_is_refused_with_its_field_and_nothing_is_written() {
    let hub = hub().await;
    let token = token_for(&hub, "manager", "m-pass").await;
    // No location yet.
    invalid_folder(
        &everything(&hub, &token, ("2027-01-01", "2027-01-02"), None).await,
        "not_set",
    );
    let dir = tempfile::tempdir().unwrap();
    locate(&hub, &dir.path().to_string_lossy()).await;
    let before = changes(&hub).len();
    for (from, to, field) in [
        ("2027-01-15", "2027-01-01", "to"),
        ("2027-01-01", "2027-12-31", "to"),
        ("x", "2027-01-01", "from"),
    ] {
        let r = everything(&hub, &token, (from, to), None).await;
        refused(&r, StatusCode::UNPROCESSABLE_ENTITY, "VALIDATION_FAILED");
        assert_eq!(r.1["field"], field, "{}", r.1);
    }
    let (status, body) = call(&hub, "POST", EVERYTHING, Some(&token), Some(json!({}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(names(dir.path()).is_empty());
    assert_eq!(changes(&hub).len(), before);
    assert_eq!(setting(&hub, KEY_LAST).0, "");
}

#[tokio::test]
async fn a_counter_needs_a_managers_pin_for_export_everything() {
    let hub = hub().await;
    let dir = tempfile::tempdir().unwrap();
    locate(&hub, &dir.path().to_string_lossy()).await;
    let token = token_for(&hub, "counter", "c-pass").await;
    refused(
        &everything(&hub, &token, ("2027-01-01", "2027-01-02"), None).await,
        StatusCode::FORBIDDEN,
        "PERMISSION_DENIED",
    );
    assert!(names(dir.path()).is_empty());

    let t = ticket_for(&hub, &token, "backup.export", "2222").await;
    let (status, reply) = everything(&hub, &token, ("2027-01-01", "2027-01-02"), Some(&t)).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let last = changes(&hub).pop().unwrap();
    assert_eq!(last.actor_id, hub.counter_id);
    assert_eq!(
        last.elevated_by_id.as_deref(),
        Some(hub.manager_id.as_str())
    );
    // The ticket was spent.
    refused(
        &everything(&hub, &token, ("2027-01-01", "2027-01-02"), Some(&t)).await,
        StatusCode::FORBIDDEN,
        "PIN_REQUIRED",
    );
    assert_eq!(names(dir.path()).len(), 1);
}

#[tokio::test]
async fn an_everything_export_the_intent_refuses_leaves_no_folder() {
    let hub = hub().await;
    let dir = tempfile::tempdir().unwrap();
    locate(&hub, &dir.path().to_string_lossy()).await;
    let token = token_for(&hub, "manager", "m-pass").await;
    hub.writer
        .submit(|tx| {
            tx.execute_batch("DELETE FROM settings WHERE key = 'backup.last_export'")
                .map_err(arena_store::db_error)
        })
        .await
        .unwrap();
    let (status, body) = everything(&hub, &token, ("2027-01-01", "2027-01-02"), None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(names(dir.path()).is_empty(), "the folder was deleted");
}

#[tokio::test]
async fn export_everything_needs_a_session() {
    let hub = hub().await;
    let (status, _) = call(&hub, "POST", EVERYTHING, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

const GST_URL: &str = "/ui/backup/gst-archive";

#[tokio::test]
async fn the_gst_archive_list_names_each_year_saved_newest_first() {
    let hub = hub().await;
    let token = token_for(&hub, "manager", "m-pass").await;
    let (status, empty) = call(&hub, "GET", GST_URL, Some(&token), None).await;
    assert_eq!((status, empty), (StatusCode::OK, json!({ "archives": [] })));

    for (fy, invoices) in [("FY2025-26", 7), ("FY2026-27", 9)] {
        hub.writer
            .publish_gst_archive(arena_proto::GstArchivePublish {
                fy: fy.into(),
                key: format!("gst-archive/{fy}.zst"),
                sha256: "ab".repeat(32),
                bytes: 321,
                invoices,
                credit_notes: 2,
                orphaned: vec!["A/1".into(), "A/2".into()],
                built_at: T0,
            })
            .await
            .unwrap();
    }
    let (status, list) = call(&hub, "GET", GST_URL, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let years = list["archives"].as_array().unwrap();
    assert_eq!(years.len(), 2);
    assert_eq!(
        (years[0]["fy"].as_str(), years[0]["label"].as_str()),
        (Some("FY2026-27"), Some("26-27"))
    );
    assert_eq!(years[1]["label"], "25-26");
    assert_eq!(
        (
            years[0]["invoices"].clone(),
            years[0]["credit_notes"].clone(),
            years[0]["orphaned"].clone()
        ),
        (json!(9), json!(2), json!(2)),
        "the list carries counts, not the invoice numbers"
    );
    assert!(years[0].get("sha256").is_none());
}

#[tokio::test]
async fn the_gst_archive_list_needs_a_session() {
    let hub = hub().await;
    let (status, _) = call(&hub, "GET", GST_URL, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_gst_archive_list_needs_the_report_permission() {
    let hub = hub().await;
    let token = token_for(&hub, "counter", "c-pass").await;
    let (status, body) = call(&hub, "GET", GST_URL, Some(&token), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "PERMISSION_DENIED");
}

const STATUS_URL: &str = "/ui/backup/status";

#[tokio::test]
async fn a_free_hub_answers_no_cloud_backup() {
    let hub = hub().await;
    let token = token_for(&hub, "counter", "c-pass").await;
    let (status, body) = call(&hub, "GET", STATUS_URL, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["cloud"], false);
    assert_eq!(body["upload_interval_s"], 0);
    assert_eq!(body["last_upload_at"], json!(null));
    hub._status
        .send_modify(|s| s.tier = Some(arena_proto::Tier::Free));
    let (_, body) = call(&hub, "GET", STATUS_URL, Some(&token), None).await;
    assert_eq!(body["cloud"], false, "an explicit Free is the same");
}

#[tokio::test]
async fn a_paid_hub_reads_its_bookkeeping_for_any_signed_in_member() {
    let hub = hub().await;
    hub._status
        .send_modify(|s| s.tier = Some(arena_proto::Tier::Basic));
    hub.writer
        .record_upload_error(Some("CLOUD_UNREACHABLE".into()))
        .await
        .unwrap();
    hub.writer.record_upload("0:A".into(), 3).await.unwrap();
    let token = token_for(&hub, "counter", "c-pass").await;
    let (status, body) = call(&hub, "GET", STATUS_URL, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["cloud"], true);
    assert_eq!(body["upload_interval_s"], 900);
    assert_eq!(body["last_upload_at"], json!(T0));
    assert_eq!(body["as_of"], json!(T0));
    assert_eq!(body["first_backup_pending"], false);
    assert_eq!(body["error"]["source"], "upload");
    assert_eq!(body["error"]["code"], "CLOUD_UNREACHABLE");
}

#[tokio::test]
async fn the_backup_status_needs_a_session() {
    let hub = hub().await;
    let (status, _) = call(&hub, "GET", STATUS_URL, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_newest_error_wins_and_a_paid_hub_with_no_snapshot_is_first_backup_pending() {
    let hub = hub().await;
    hub._status
        .send_modify(|s| s.tier = Some(arena_proto::Tier::Premium));
    hub.writer
        .record_first_snapshot_pending(Some("DISK_FULL".into()))
        .await
        .unwrap();
    hub.clock.advance(5_000);
    hub.writer
        .record_upload_error(Some("CLOUD_UNREACHABLE".into()))
        .await
        .unwrap();
    let token = token_for(&hub, "manager", "m-pass").await;
    let (_, body) = call(&hub, "GET", STATUS_URL, Some(&token), None).await;
    assert_eq!(body["first_backup_pending"], true);
    assert_eq!(body["last_upload_at"], json!(null));
    assert_eq!(body["error"]["source"], "upload");
    assert_eq!(body["error"]["at"], json!(T0 + 5_000));
    hub.writer.record_upload_error(None).await.unwrap();
    let (_, body) = call(&hub, "GET", STATUS_URL, Some(&token), None).await;
    assert_eq!(body["error"]["source"], "first_snapshot");
    assert_eq!(body["error"]["code"], "DISK_FULL");
}

// Story 9.23: what a promote or restore set aside.

const ASIDE_URL: &str = "/ui/backup/set-aside";
const ASIDE_EXPORT: &str = "/ui/backup/set-aside/export";

/// A copy of the Hub's database in `orphaned/` with one invoice the live one lacks.
fn orphan(hub: &Hub) -> std::path::PathBuf {
    let dir = hub.path.parent().unwrap().join("orphaned");
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("4-abc-20270115T133000.db");
    arena_store::DbConn::open(&hub.path)
        .unwrap()
        .execute("VACUUM INTO ?1", [target.to_str().unwrap()])
        .unwrap();
    arena_store::DbConn::open(&target)
        .unwrap()
        .execute(
            "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy, rev,
                                 created_at, updated_at, settled_at, kind, invoice_no, tax_minor,
                                 tax_breakup)
             VALUES ('O1', 'settled', 11800, 11800, '2027-01-10', 'FY2026-27', 0, 5, 5, ?1,
                     'pos_sale', 'INV/26-27/0042', 0, '[]')",
            [T0],
        )
        .unwrap();
    target
}

#[tokio::test]
async fn the_set_aside_list_shows_the_orphaned_invoices_and_needs_the_report_permission() {
    let hub = hub().await;
    let manager = token_for(&hub, "manager", "m-pass").await;
    let (status, empty) = call(&hub, "GET", ASIDE_URL, Some(&manager), None).await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert_eq!(empty["forks"], json!([]));
    assert_eq!(empty["databases"], json!([]));
    assert_eq!(empty["invoice_total"], 0);

    orphan(&hub);
    let (status, list) = call(&hub, "GET", ASIDE_URL, Some(&manager), None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["databases"][0]["file"], "4-abc-20270115T133000.db");
    assert_eq!(list["databases"][0]["invoices"], 1);
    assert_eq!(list["databases"][0]["unreadable"], false);
    assert_eq!(list["invoices"][0]["invoice_no"], "INV/26-27/0042");
    assert_eq!(list["invoices"][0]["amount_minor"], 11_800);
    assert_eq!(list["invoices_truncated"], false);

    let (status, _) = call(&hub, "GET", ASIDE_URL, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let counter = token_for(&hub, "counter", "c-pass").await;
    let (status, body) = call(&hub, "GET", ASIDE_URL, Some(&counter), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "PERMISSION_DENIED");
}

#[tokio::test]
async fn export_list_writes_a_csv_to_the_backup_location() {
    let hub = hub().await;
    let out = tempfile::tempdir().unwrap();
    locate(&hub, out.path().to_str().unwrap()).await;
    orphan(&hub);
    let manager = token_for(&hub, "manager", "m-pass").await;
    let before = changes(&hub).len();

    let (status, done) = call(&hub, "POST", ASIDE_EXPORT, Some(&manager), None).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    let name = done["file_name"].as_str().unwrap();
    assert!(
        name.contains("-set-aside-") && name.ends_with(".csv"),
        "{name}"
    );
    assert_eq!(names(out.path()), vec![name.to_owned()]);
    let csv = std::fs::read_to_string(out.path().join(name)).unwrap();
    assert!(csv.contains("INV/26-27/0042,118.00,"), "{csv}");
    assert_eq!(
        changes(&hub).len(),
        before,
        "a copy of what the Hub holds records nothing"
    );

    let counter = token_for(&hub, "counter", "c-pass").await;
    let (status, body) = call(&hub, "POST", ASIDE_EXPORT, Some(&counter), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, _) = call(&hub, "POST", ASIDE_EXPORT, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn export_list_says_why_the_location_cannot_take_it() {
    let hub = hub().await;
    let manager = token_for(&hub, "manager", "m-pass").await;
    let (status, body) = call(&hub, "POST", ASIDE_EXPORT, Some(&manager), None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["field"], "folder");
    assert_eq!(body["message"], "not_set");
}
