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
        installs: Vec::new(),
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

// ---- Story 6.5: "Found on PCs" ----

const COVER: &[u8] = b"\x89PNG-cover";

fn cover_sha() -> String {
    "ab".repeat(32)
}

/// A catalogue holding one game (`01JCATALOG`) with a signature for `g.exe` and a stored cover.
fn catalogue_with_cover(hub: &Hub) {
    hub.media
        .apply_manifest(&CatalogManifest {
            catalog_version: 1,
            games: vec![CatalogGame {
                id: "01JCATALOG".into(),
                rev: 1,
                deleted: false,
                name: Some("Great Game".into()),
                launcher: Some("Steam".into()),
                category: Some("Shooter".into()),
                age_rating: Some("16+".into()),
                signatures: vec![arena_proto::CatalogSignature {
                    exe_name: "g.exe".into(),
                    product_name: "Great".into(),
                    publisher: String::new(),
                    sha256: None,
                }],
                media: vec![arena_proto::CatalogMedia {
                    kind: "cover".into(),
                    sha256: cover_sha(),
                    bytes: COVER.len() as u64,
                    url: "https://cdn.example/catalog/cover.png".into(),
                }],
            }],
        })
        .unwrap();
    let file = hub.media.pending().unwrap().pop().unwrap();
    std::fs::write(hub.media.part_path(&file.sha256), COVER).unwrap();
    hub.media.commit(&file, T0).unwrap();
}

fn found(path: &str, file: &str, product: &str) -> arena_proto::ScanEntry {
    arena_proto::ScanEntry {
        path: path.into(),
        file_name: file.into(),
        product_name: product.into(),
        publisher: "Maker".into(),
        launcher: "none".into(),
    }
}

/// Reports a scan for `device` and records its catalogue matches, as the LAN layer does.
async fn scan_and_match(hub: &Hub, device: &str, at: i64, entries: Vec<arena_proto::ScanEntry>) {
    let matches =
        arena_core::game_proposals::match_scan(&entries, &hub.media.signatures().unwrap());
    let report = IntentPayload::KioskScanReported(arena_proto::KioskScanReported {
        scanned_at: at,
        entries,
        launchers: Vec::new(),
        skipped: 0,
    });
    let device_actor = Actor {
        kind: ActorKind::Device,
        id: device.into(),
    };
    hub.writer
        .submit_intent(intent_as(device_actor, report, None))
        .await
        .unwrap();
    let system = Actor {
        kind: ActorKind::System,
        id: "hub-lan".into(),
    };
    let matched = IntentPayload::GameMatched(arena_proto::GameMatched {
        device_id: device.into(),
        scanned_at: at,
        matches,
    });
    hub.writer
        .submit_intent(intent_as(system, matched, None))
        .await
        .unwrap();
}

async fn proposals(hub: &Hub, token: &str) -> Value {
    let (status, v) = call(hub, "GET", "/ui/library/proposals", Some(token), None).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    v
}

async fn raw_get(hub: &Hub, uri: &str, token: &str) -> axum::response::Response {
    use tower::ServiceExt;
    let req = axum::http::Request::builder()
        .method("GET")
        .uri(uri)
        .header(axum::http::header::HOST, "127.0.0.1:7409")
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
        .body(axum::body::Body::empty())
        .unwrap();
    hub.app.clone().oneshot(req).await.unwrap()
}

#[tokio::test]
async fn found_on_pcs_lists_matched_games_with_cover_and_unmatched_ones_by_name() {
    let hub = hub().await;
    catalogue_with_cover(&hub);
    let (status, _) = call(&hub, "GET", "/ui/library/proposals", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let counter = token_for(&hub, "counter", "c-pass").await;
    assert_eq!(
        proposals(&hub, &counter).await,
        json!({"proposals": [], "dismissed": 0})
    );

    let a = register_pc(&hub, "PC-A").await;
    let b = register_pc(&hub, "PC-B").await;
    scan_and_match(
        &hub,
        &a,
        T0 - 2_000,
        vec![
            found(r"C:\Games\g.exe", "g.exe", "Great"),
            found(r"C:\Tools\tool.exe", "tool.exe", "Handy Tool"),
        ],
    )
    .await;
    scan_and_match(
        &hub,
        &b,
        T0 - 1_000,
        vec![found(r"D:\Stuff\g.exe", "g.exe", "great")],
    )
    .await;
    let v = proposals(&hub, &counter).await;
    let list = v["proposals"].as_array().unwrap();
    assert_eq!(list.len(), 2, "{v}");
    let great = &list[0];
    assert_eq!(great["name"], "Great Game");
    assert_eq!(great["key"], "game:01JCATALOG");
    assert_eq!(great["catalog_id"], "01JCATALOG");
    assert_eq!(great["cover_sha256"], cover_sha());
    assert_eq!(great["launcher"], "steam");
    assert_eq!(great["category"], "Shooter");
    assert_eq!(great["age_rating"], "16+");
    assert_eq!(great["pc_count"], 2);
    let paths: Vec<_> = great["devices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| (d["label"].as_str().unwrap(), d["path"].as_str().unwrap()))
        .collect();
    assert_eq!(
        paths,
        [("PC-A", r"C:\Games\g.exe"), ("PC-B", r"D:\Stuff\g.exe")]
    );
    let tool = &list[1];
    assert_eq!(tool["name"], "Handy Tool");
    assert_eq!(tool["catalog_id"], Value::Null);
    assert_eq!(tool["cover_sha256"], Value::Null);
    assert_eq!(tool["pc_count"], 1);
    assert_eq!(tool["key"], "exe:tool.exe|handy tool|maker");
}

#[tokio::test]
async fn approving_a_proposal_removes_it_and_records_the_install_paths() {
    let hub = hub().await;
    catalogue_with_cover(&hub);
    let manager = token_for(&hub, "manager", "m-pass").await;
    let a = register_pc(&hub, "PC-A").await;
    let b = register_pc(&hub, "PC-B").await;
    let (pa, pb) = (r"C:\Games\g.exe", r"D:\Stuff\g.exe");
    scan_and_match(&hub, &a, T0 - 2_000, vec![found(pa, "g.exe", "Great")]).await;
    scan_and_match(&hub, &b, T0 - 1_000, vec![found(pb, "g.exe", "Great")]).await;

    let mut approve = entry("ignored", pa);
    approve.catalog_id = Some("01JCATALOG".into());
    approve.installs = [(&a, pa), (&b, pb)]
        .into_iter()
        .map(|(d, p)| arena_proto::LibraryInstall {
            device_id: d.clone(),
            path: p.into(),
            match_key: "game:01JCATALOG".into(),
        })
        .collect();
    let (status, done) = post(&hub, &manager, body(approve, None, None)).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    let view = list(&hub, &manager).await;
    // The catalogue's name and launcher replaced what the client sent.
    assert_eq!(view["entries"][0]["name"], "Great Game");
    assert_eq!(view["entries"][0]["launcher"], "steam");
    let installs = view["entries"][0]["installs"].as_array().unwrap();
    assert_eq!(installs.len(), 2);
    assert_eq!(installs[0]["device_id"], a.as_str());
    assert_eq!(installs[1]["path"], pb);
    assert_eq!(
        proposals(&hub, &manager).await,
        json!({"proposals": [], "dismissed": 0}),
        "an approved game is not listed again"
    );
}

#[tokio::test]
async fn dismissing_hides_a_proposal_until_cleared_and_a_counter_needs_a_pin() {
    let hub = hub().await;
    let counter = token_for(&hub, "counter", "c-pass").await;
    let a = register_pc(&hub, "PC-A").await;
    scan_and_match(
        &hub,
        &a,
        T0 - 2_000,
        vec![found(r"C:\Tools\tool.exe", "tool.exe", "Handy Tool")],
    )
    .await;
    let key = "exe:tool.exe|handy tool|maker";
    let dismiss = |ticket: Option<&str>| {
        let payload = IntentPayload::LibraryDismiss(arena_proto::LibraryDismiss {
            keys: vec![key.into()],
        });
        let mut b = json!({ "intent": intent_as(who(), payload, None) });
        if let Some(t) = ticket {
            b["elevation_ticket"] = json!(t);
        }
        b
    };
    let (status, denied) = post(&hub, &counter, dismiss(None)).await;
    assert_eq!(
        (status, denied["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("PERMISSION_DENIED"))
    );
    let (status, elevated) = call(
        &hub,
        "POST",
        "/ui/elevate",
        Some(&counter),
        Some(json!({"items": [{"intent_kind": "library.dismiss"}], "pin": "2222"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{elevated}");
    let (status, done) = post(&hub, &counter, dismiss(elevated["ticket"].as_str())).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(
        proposals(&hub, &counter).await,
        json!({"proposals": [], "dismissed": 1})
    );
    // A later scan reporting the same executable does not bring it back.
    scan_and_match(
        &hub,
        &a,
        T0 - 1_000,
        vec![found(r"D:\Elsewhere\TOOL.exe", "TOOL.exe", "handy tool")],
    )
    .await;
    assert_eq!(proposals(&hub, &counter).await["proposals"], json!([]));

    let manager = token_for(&hub, "manager", "m-pass").await;
    let clear = json!({ "intent": intent_as(
        who(),
        IntentPayload::LibraryDismissClear(arena_proto::LibraryDismissClear {}),
        None,
    ) });
    let (status, done) = post(&hub, &manager, clear).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    let v = proposals(&hub, &manager).await;
    assert_eq!(v["dismissed"], 0);
    assert_eq!(v["proposals"].as_array().unwrap().len(), 1);
    assert_eq!(
        v["proposals"][0]["devices"][0]["path"],
        r"D:\Elsewhere\TOOL.exe"
    );
}

#[tokio::test]
async fn a_stored_cover_is_served_to_a_signed_in_staff_member_and_nothing_else_is() {
    let hub = hub().await;
    catalogue_with_cover(&hub);
    let path = format!("/ui/library/cover/{}", cover_sha());
    let (status, _) = call(&hub, "GET", &path, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let counter = token_for(&hub, "counter", "c-pass").await;
    let resp = raw_get(&hub, &path, &counter).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "image/png");
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    assert_eq!(&bytes[..], COVER);
    let unknown = format!("/ui/library/cover/{}", "cd".repeat(32));
    for missing in ["/ui/library/cover/nothex", unknown.as_str()] {
        let resp = raw_get(&hub, missing, &counter).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{missing}");
    }
}
