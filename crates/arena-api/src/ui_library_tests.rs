//! Story 6.3: `GET /ui/library` and the library intents through `POST /ui/intent`.

use arena_proto::{
    Actor, ActorKind, CatalogGame, CatalogManifest, IntentPayload, KioskLibraryEntry,
};
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::ui_auth::tests::{Hub, T0, call, hub, intent_as, token_for};

fn who() -> Actor {
    Actor {
        kind: ActorKind::Staff,
        id: "x".into(),
    }
}

fn entry(name: &str, path: &str) -> arena_proto::LibraryUpsert {
    arena_proto::LibraryUpsert {
        id: None,
        entry_kind: "game".into(),
        name: name.into(),
        launcher: "steam".into(),
        executable_path: path.into(),
        launch_uri: String::new(),
        launch_args: String::new(),
        category: String::new(),
        tags: Vec::new(),
        age_rating: String::new(),
        enabled: true,
        catalog_id: None,
    }
}

fn body(u: arena_proto::LibraryUpsert, base_rev: Option<i64>, ticket: Option<&str>) -> Value {
    let mut b = json!({ "intent": intent_as(who(), IntentPayload::LibraryUpsert(u), base_rev) });
    if let Some(t) = ticket {
        b["elevation_ticket"] = json!(t);
    }
    b
}

async fn post(hub: &Hub, token: &str, b: Value) -> (StatusCode, Value) {
    call(hub, "POST", "/ui/intent", Some(token), Some(b)).await
}

async fn list(hub: &Hub, token: &str) -> Value {
    let (status, v) = call(hub, "GET", "/ui/library", Some(token), None).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    v
}

fn game(rev: u64, name: &str, age_rating: Option<&str>) -> CatalogGame {
    CatalogGame {
        id: "01JCATALOG".into(),
        rev,
        deleted: false,
        name: Some(name.into()),
        launcher: Some("Steam".into()),
        category: Some("Shooter".into()),
        age_rating: age_rating.map(Into::into),
        signatures: vec![],
        media: vec![],
    }
}

fn catalogue(hub: &Hub, version: u64, g: CatalogGame) {
    hub.media
        .apply_manifest(&CatalogManifest {
            catalog_version: version,
            games: vec![g],
        })
        .unwrap();
}

#[tokio::test]
async fn the_library_list_needs_a_session_and_shows_every_live_entry() {
    let hub = hub().await;
    let (status, _) = call(&hub, "GET", "/ui/library", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let counter = token_for(&hub, "counter", "c-pass").await;
    assert_eq!(list(&hub, &counter).await, json!({"entries": []}));
    let manager = token_for(&hub, "manager", "m-pass").await;
    let mut off = entry("Zed", r"C:\Games\zed.exe");
    off.enabled = false;
    off.tags = vec!["a".into()];
    for u in [off, entry("Alpha", r"C:\Games\alpha.exe")] {
        let (status, b) = post(&hub, &manager, body(u, None, None)).await;
        assert_eq!(status, StatusCode::OK, "{b}");
    }
    let view = list(&hub, &counter).await;
    let names: Vec<_> = view["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["name"].as_str().unwrap(), e["enabled"].as_bool().unwrap()))
        .collect();
    assert_eq!(names, [("Alpha", true), ("Zed", false)]);
    assert_eq!(view["entries"][1]["tags"], json!(["a"]));
    assert_eq!(view["entries"][0]["rev"], 1);
    assert_eq!(view["entries"][0]["kind"], "game");
    // The kiosk setting carries the enabled one only.
    let (_, settings) = call(&hub, "GET", "/ui/settings", Some(&manager), None).await;
    let lib = settings
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == "kiosk.library")
        .unwrap();
    let on: Vec<KioskLibraryEntry> = serde_json::from_str(lib["value"].as_str().unwrap()).unwrap();
    assert_eq!(
        on.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        ["Alpha"]
    );
}

#[tokio::test]
async fn a_counter_needs_a_manager_pin_and_the_edit_is_audited_with_both() {
    let hub = hub().await;
    let counter = token_for(&hub, "counter", "c-pass").await;
    let (status, denied) = post(
        &hub,
        &counter,
        body(entry("A", r"C:\Games\a.exe"), None, None),
    )
    .await;
    assert_eq!(
        (status, denied["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("PERMISSION_DENIED"))
    );
    let (status, elevated) = call(
        &hub,
        "POST",
        "/ui/elevate",
        Some(&counter),
        Some(json!({"items": [{"intent_kind": "library.upsert"}], "pin": "2222"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{elevated}");
    let (status, done) = post(
        &hub,
        &counter,
        body(
            entry("A", r"C:\Games\a.exe"),
            None,
            elevated["ticket"].as_str(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    let rows = arena_store::Store::open_reader(&hub.path)
        .unwrap()
        .changes()
        .unwrap();
    let change = rows
        .iter()
        .rev()
        .find(|c| c.tbl == "library_entries")
        .unwrap();
    assert_eq!(change.actor_id, hub.counter_id);
    assert_eq!(
        change.elevated_by_id.as_deref(),
        Some(hub.manager_id.as_str())
    );
}

#[tokio::test]
async fn field_errors_name_the_field_and_a_stale_edit_is_stale() {
    let hub = hub().await;
    let manager = token_for(&hub, "manager", "m-pass").await;
    let (status, applied) = post(
        &hub,
        &manager,
        body(entry("A", r"C:\Games\a.exe"), None, None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    for (u, field) in [
        (entry("", r"C:\Games\b.exe"), "name"),
        (entry("B", ""), "executable_path"),
        (entry("B", r"C:\Games\A.exe"), "executable_path"),
        (entry("B", r"C:\Users\p\g.exe"), "executable_path"),
    ] {
        let (status, e) = post(&hub, &manager, body(u, None, None)).await;
        assert_eq!(
            (e["code"].as_str(), e["field"].as_str()),
            (Some("VALIDATION_FAILED"), Some(field)),
            "{status} {e}"
        );
        assert!(status.is_client_error(), "{status}");
    }
    let view = list(&hub, &manager).await;
    assert_eq!(
        view["entries"].as_array().unwrap().len(),
        1,
        "nothing else written"
    );
    let id = view["entries"][0]["id"].as_str().unwrap().to_owned();
    let mut edit = entry("A", r"C:\Games\a.exe");
    edit.id = Some(id);
    edit.category = "Action".into();
    let (status, e) = post(&hub, &manager, body(edit.clone(), Some(5), None)).await;
    assert_eq!(
        (status, e["code"].as_str()),
        (StatusCode::CONFLICT, Some("STALE")),
        "{e}"
    );
    let (status, e) = post(&hub, &manager, body(edit, Some(1), None)).await;
    assert_eq!(status, StatusCode::OK, "{e}");
    assert_eq!(
        list(&hub, &manager).await["entries"][0]["category"],
        "Action"
    );
}

#[tokio::test]
async fn a_linked_entry_takes_the_catalogues_fields_and_an_unknown_id_is_refused() {
    let hub = hub().await;
    let manager = token_for(&hub, "manager", "m-pass").await;
    let mut linked = entry("My name", r"C:\Games\cs2.exe");
    linked.catalog_id = Some("01JCATALOG".into());
    linked.launcher = "none".into();
    // The catalogue is empty: the id is unknown.
    let (_, e) = post(&hub, &manager, body(linked.clone(), None, None)).await;
    assert_eq!(
        (e["code"].as_str(), e["field"].as_str()),
        (Some("VALIDATION_FAILED"), Some("catalog_id")),
        "{e}"
    );
    catalogue(&hub, 1, game(1, "Counter-Strike 2", Some("18+")));
    let mut unknown = linked.clone();
    unknown.catalog_id = Some("01JNOPE".into());
    let (_, e) = post(&hub, &manager, body(unknown, None, None)).await;
    assert_eq!(e["field"].as_str(), Some("catalog_id"), "{e}");
    assert_eq!(
        list(&hub, &manager).await,
        json!({"entries": []}),
        "nothing written"
    );
    linked.category = "FPS".into();
    let (status, e) = post(&hub, &manager, body(linked.clone(), None, None)).await;
    assert_eq!(status, StatusCode::OK, "{e}");
    let view = list(&hub, &manager).await;
    let e = &view["entries"][0];
    assert_eq!(
        (
            e["name"].as_str(),
            e["launcher"].as_str(),
            e["age_rating"].as_str()
        ),
        (Some("Counter-Strike 2"), Some("steam"), Some("18+")),
        "name, launcher and age rating are the catalogue's"
    );
    assert_eq!(e["category"], "FPS");
    assert_eq!(e["catalog_id"], "01JCATALOG");
    // Category and tags are editable, whatever name the client sends.
    let mut edit = linked.clone();
    edit.id = Some(e["id"].as_str().unwrap().to_owned());
    edit.name = "Renamed by a client".into();
    edit.tags = vec!["esports".into()];
    let (status, done) = post(&hub, &manager, body(edit, Some(1), None)).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    let view = list(&hub, &manager).await;
    assert_eq!(view["entries"][0]["name"], "Counter-Strike 2");
    assert_eq!(view["entries"][0]["tags"], json!(["esports"]));
    // The list shows the catalogue's current values after a rename there; the stored entry is
    // not rewritten, and it can still be edited.
    catalogue(&hub, 2, game(2, "CS2", None));
    let view = list(&hub, &manager).await;
    assert_eq!(view["entries"][0]["name"], "CS2");
    assert_eq!(view["entries"][0]["age_rating"], "");
    let mut again = linked;
    again.id = Some(view["entries"][0]["id"].as_str().unwrap().to_owned());
    again.category = "Shooter".into();
    let (status, done) = post(&hub, &manager, body(again, Some(2), None)).await;
    assert_eq!(status, StatusCode::OK, "{done}");
}

// ---- Story 6.4: `GET /ui/library/scans` ----

async fn register_pc(hub: &Hub, label: &str) -> String {
    hub.writer
        .submit_intent(intent_as(
            Actor {
                kind: ActorKind::Owner,
                id: "owner".into(),
            },
            IntentPayload::DeviceRegister(arena_proto::DeviceRegister::new(
                arena_proto::ClientClass::Paired,
                label,
                arena_core::devices::hash_token_secret("s").unwrap(),
            )),
            None,
        ))
        .await
        .unwrap()
        .changes[0]
        .row_id
        .clone()
}

async fn report_scan(hub: &Hub, device_id: &str, at: i64, paths: &[&str]) {
    let i = intent_as(
        Actor {
            kind: ActorKind::Device,
            id: device_id.into(),
        },
        IntentPayload::KioskScanReported(arena_proto::KioskScanReported {
            scanned_at: at,
            entries: paths
                .iter()
                .map(|p| arena_proto::ScanEntry {
                    path: (*p).into(),
                    file_name: "g.exe".into(),
                    product_name: String::new(),
                    publisher: String::new(),
                    launcher: "none".into(),
                })
                .collect(),
            launchers: vec![arena_proto::ScanLauncher {
                launcher: "steam".into(),
                path: r"C:\Program Files (x86)\Steam".into(),
            }],
            skipped: 1,
        }),
        None,
    );
    hub.writer.submit_intent(i).await.unwrap();
}

#[tokio::test]
async fn the_scan_list_shows_every_paired_pc_and_null_for_one_never_scanned() {
    let hub = hub().await;
    let (status, _) = call(&hub, "GET", "/ui/library/scans", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let counter = token_for(&hub, "counter", "c-pass").await;
    let (status, v) = call(&hub, "GET", "/ui/library/scans", Some(&counter), None).await;
    assert_eq!((status, &v), (StatusCode::OK, &json!({"devices": []})));

    let b = register_pc(&hub, "PC-B").await;
    let a = register_pc(&hub, "PC-A").await;
    report_scan(
        &hub,
        &a,
        T0 - 2_000,
        &[r"C:\Games\1.exe", r"C:\Games\2.exe"],
    )
    .await;
    let (status, v) = call(&hub, "GET", "/ui/library/scans", Some(&counter), None).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let devices = v["devices"].as_array().unwrap();
    assert_eq!(devices.len(), 2);
    assert_eq!(
        (
            devices[0]["device_id"].as_str(),
            devices[0]["name"].as_str()
        ),
        (Some(a.as_str()), Some("PC-A"))
    );
    assert_eq!(devices[0]["scanned_at"], T0 - 2_000);
    assert_eq!(devices[0]["entry_count"], 2);
    assert_eq!(devices[0]["skipped"], 1);
    assert_eq!(
        devices[0]["launchers"],
        json!([{"launcher": "steam", "path": r"C:\Program Files (x86)\Steam"}])
    );
    assert_eq!(devices[1]["device_id"], b.as_str());
    for field in ["scanned_at", "entry_count", "skipped", "launchers"] {
        assert!(devices[1][field].is_null(), "{field}");
    }

    // A newer report replaces the PC's row.
    report_scan(&hub, &a, T0 - 1_000, &[r"C:\Games\3.exe"]).await;
    let (_, v) = call(&hub, "GET", "/ui/library/scans", Some(&counter), None).await;
    assert_eq!(v["devices"][0]["scanned_at"], T0 - 1_000);
    assert_eq!(v["devices"][0]["entry_count"], 1);
}
