//! The global game catalogue (Story 6.1): the import, `admin_audit` and
//! `GET /v1/catalog/manifest`.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use arena_cloud::auth::AuthState;
use arena_cloud::catalog::{self, ImportSummary};
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
use arena_cloud::passwords::hash_secret;
use arena_cloud::templates::TemplateStore;
use arena_cloud::{Clock, app_with_auth, auth_state};
use arena_store::control::{ControlDb, LATEST_CONTROL_VERSION, NewAdminAudit};
use axum::body::{Body, to_bytes};
use axum::http::Request;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use tower::ServiceExt;

const T0: i64 = 1_800_000_000_000;
const CDN: &str = "https://cdn.arena360.cloud";
const SECRET: &str = "s3cret-for-hub-one";

struct Harness {
    app: axum::Router,
    db: Arc<ControlDb>,
    st: Arc<AuthState>,
    now: Arc<AtomicI64>,
    dir: tempfile::TempDir,
}

struct Reply {
    status: u16,
    headers: axum::http::HeaderMap,
    body: Value,
}

fn config() -> AuthConfig {
    AuthConfig {
        pepper: b"test-pepper-0123456789abcdef0123456789".to_vec(),
        web_origins: vec!["https://start.arena360.cloud".to_owned()],
        sms: SmsProviderConfig::Fake,
        sms_global_per_min: 10_000,
        mfa_key: MfaKey::from_bytes([0xa5; 32]),
        licence: arena_cloud::licence::LicenceSigner::dev(),
        billing_enabled: false,
    }
}

impl Harness {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("gz_control.db");
        let db = ControlDb::open_local(&db_path).await.unwrap();
        db.migrate(T0).await.unwrap();
        let hash = hash_secret(SECRET).unwrap();
        rusqlite::Connection::open(&db_path)
            .unwrap()
            .execute_batch(&format!(
                "PRAGMA foreign_keys = OFF;
                 INSERT INTO tenants (id, owner_id, name, created_at) VALUES ('t1', 'o1', 'Pixel', 1);
                 INSERT INTO branches (id, tenant_id, name, city, country, currency, timezone,
                   gst_registered, gstin, branch_code, code_locked_at, created_at, updated_at)
                   VALUES ('b1', 't1', 'Pixel Arena', 'Pune', 'IN', 'INR', 'Asia/Kolkata', 0, NULL,
                   'PAG', NULL, 1, 1);
                 INSERT INTO hubs (id, branch_id, platform, hub_instance_id, version, token_hash,
                   paired_at, last_seen_at) VALUES ('h1', 'b1', 'windows', 'i', '1', '{hash}', 1, 5);"
            ))
            .unwrap();
        let db = Arc::new(db);
        let now = Arc::new(AtomicI64::new(T0));
        let tick = Arc::clone(&now);
        let clock: Clock = Arc::new(move || tick.load(Ordering::SeqCst));
        let st = auth_state(Arc::clone(&db), &config(), clock).unwrap();
        assert!(st.catalog_cdn.set(CDN.to_owned()).is_ok());
        let (_tx, rx) = watch::channel(Reachability {
            reachable: true,
            checked_at: None,
        });
        let app = app_with_auth(
            Role::All,
            true,
            rx,
            LATEST_CONTROL_VERSION,
            Some(Arc::clone(&st)),
        );
        Self {
            app,
            db,
            st,
            now,
            dir,
        }
    }

    fn sql(&self, sql: &str) {
        rusqlite::Connection::open(self.dir.path().join("gz_control.db"))
            .unwrap()
            .execute_batch(sql)
            .unwrap();
    }

    async fn get(&self, path: &str, token: Option<&str>, etag: Option<&str>) -> Reply {
        let mut req = Request::builder().method("GET").uri(path);
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        if let Some(e) = etag {
            req = req.header("if-none-match", e);
        }
        let res = self
            .app
            .clone()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = res.status().as_u16();
        let headers = res.headers().clone();
        let bytes = to_bytes(res.into_body(), 1 << 24).await.unwrap();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        Reply {
            status,
            headers,
            body,
        }
    }

    async fn manifest(&self, since: i64) -> Reply {
        self.get(
            &format!("/v1/catalog/manifest?since={since}"),
            Some(&token()),
            None,
        )
        .await
    }

    /// Imports `file` (named by `name`, holding `text`) with media under `dir`.
    async fn import(
        &self,
        store: &TemplateStore,
        name: &str,
        text: &str,
        dir: &Path,
    ) -> ImportSummary {
        let games = catalog::parse_import(name, text).unwrap();
        let games = catalog::resolve(games, dir).unwrap();
        self.now.fetch_add(1000, Ordering::SeqCst);
        catalog::import(
            &self.db,
            Some(store),
            name,
            &games,
            "harish",
            self.now.load(Ordering::SeqCst),
        )
        .await
        .unwrap()
    }
}

fn token() -> String {
    format!("hub_h1.{SECRET}")
}

fn hash_of(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

const COVER: &[u8] = b"\xff\xd8\xff cover bytes";
const VIDEO: &[u8] = b"video bytes of a trailer";

fn media_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("dota.jpg"), COVER).unwrap();
    std::fs::write(dir.path().join("dota.mp4"), VIDEO).unwrap();
    dir
}

fn list(published: bool, flash: bool) -> String {
    json!({"games": [
        {
            "name": "Dota 2", "launcher": "Steam", "category": "MOBA", "age_rating": "PEGI 12",
            "published": published, "cover": "dota.jpg", "video": "dota.mp4",
            "flash_screened": flash,
            "signatures": [
                {"exe": "dota2.exe", "product": "Dota 2", "publisher": "Valve"},
                {"exe": "dota2_launcher.exe", "publisher": "Valve", "sha256": "ab".repeat(32)}
            ]
        },
        {"name": "Valorant", "launcher": "Riot", "category": "Shooter", "published": true}
    ]})
    .to_string()
}

#[tokio::test]
async fn admin_audit_can_only_be_appended() {
    let h = Harness::new().await;
    h.db.append_admin_audit(&NewAdminAudit {
        id: "a1",
        at: T0,
        actor_kind: "admin",
        actor_id: "harish",
        action: "catalog.import",
        target_kind: "catalog",
        target_id: "global",
        reason: Some("seed"),
        summary: "{}",
        request_id: "r1",
    })
    .await
    .unwrap();
    let conn = rusqlite::Connection::open(h.dir.path().join("gz_control.db")).unwrap();
    let update = conn
        .execute("UPDATE admin_audit SET action = 'x'", [])
        .unwrap_err();
    assert!(update.to_string().contains("append-only"), "{update}");
    let delete = conn.execute("DELETE FROM admin_audit", []).unwrap_err();
    assert!(delete.to_string().contains("append-only"), "{delete}");
    let rows = h.db.admin_audit_rows().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].action.as_str(), rows[0].reason.as_deref()),
        ("catalog.import", Some("seed"))
    );
    let bad_actor = conn.execute(
        "INSERT INTO admin_audit VALUES ('a2', 1, 'player', 'x', 'a', 't', 't', NULL, '{}', 'r')",
        [],
    );
    assert!(bad_actor.is_err(), "only admin and system may act");
}

#[tokio::test]
async fn an_import_is_idempotent_and_uploads_each_media_file_once() {
    let h = Harness::new().await;
    let store = TemplateStore::memory();
    let media = media_dir();
    let first = h
        .import(&store, "games.json", &list(true, false), media.path())
        .await;
    assert_eq!(
        (first.created, first.updated, first.unchanged, first.version),
        (2, 0, 0, 1)
    );
    assert_eq!(first.media_uploaded, 2);
    let TemplateStore::Memory(mem) = &store else {
        unreachable!()
    };
    let mut want = vec![
        format!("catalog/{}.jpg", hash_of(COVER)),
        format!("catalog/{}.mp4", hash_of(VIDEO)),
    ];
    want.sort();
    assert_eq!(mem.keys(), want);

    let again = h
        .import(&store, "games.json", &list(true, false), media.path())
        .await;
    assert_eq!(
        (again.created, again.updated, again.unchanged, again.version),
        (0, 0, 2, 1),
        "an identical file changes nothing and moves no version"
    );
    assert_eq!(again.media_uploaded, 0);
    let audit = h.db.admin_audit_rows().await.unwrap();
    assert_eq!(audit.len(), 2);
    assert_eq!(
        (audit[0].actor_kind.as_str(), audit[0].action.as_str()),
        ("admin", "catalog.import")
    );
    assert_eq!(audit[0].actor_id, "harish");
    let summary: Value = serde_json::from_str(&audit[0].summary).unwrap();
    assert_eq!(
        summary,
        json!({"file": "games.json", "games": 2, "created": 2, "updated": 0, "unchanged": 0,
               "media_uploaded": 2})
    );

    // The same cover under another game is the same object: nothing new is uploaded for it.
    std::fs::write(media.path().join("copy.jpg"), COVER).unwrap();
    let other = json!([{"name": "Counter-Strike 2", "launcher": "Steam", "category": "Shooter",
        "cover": "copy.jpg"}])
    .to_string();
    let third = h.import(&store, "more.json", &other, media.path()).await;
    assert_eq!(
        (third.created, third.media_uploaded, third.media_present),
        (1, 0, 1)
    );
    assert_eq!(third.version, 2);
}

#[tokio::test]
async fn the_manifest_serves_published_changes_and_keeps_unscreened_videos_out() {
    let h = Harness::new().await;
    let store = TemplateStore::memory();
    let media = media_dir();
    h.import(&store, "games.json", &list(true, false), media.path())
        .await;

    let r = h.manifest(0).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["catalog_version"], 1);
    assert_eq!(r.headers["etag"], "\"0-1\"");
    let games = r.body["games"].as_array().unwrap();
    assert_eq!(games.len(), 2);
    let dota = games.iter().find(|g| g["name"] == "Dota 2").unwrap();
    assert_eq!(dota["launcher"], "Steam");
    assert_eq!(dota["age_rating"], "PEGI 12");
    assert_eq!(dota["rev"], 1);
    assert_eq!(dota["signatures"].as_array().unwrap().len(), 2);
    assert_eq!(dota["signatures"][0]["exe_name"], "dota2.exe");
    assert!(dota["signatures"][0].get("sha256").is_none());
    assert_eq!(dota["signatures"][1]["sha256"], "ab".repeat(32));
    assert_eq!(
        dota["media"],
        json!([{
            "kind": "cover", "sha256": hash_of(COVER), "bytes": COVER.len(),
            "url": format!("{CDN}/catalog/{}.jpg", hash_of(COVER)),
        }]),
        "the cover is listed and the unscreened video never is"
    );
    let body = r.body.to_string();
    assert!(!body.contains(&hash_of(VIDEO)), "{body}");

    // Nothing changed: 304, by version and by ETag.
    let r = h.manifest(1).await;
    assert_eq!(
        (r.status, r.headers["etag"].to_str().unwrap()),
        (304, "\"1-1\"")
    );
    let r = h
        .get(
            "/v1/catalog/manifest?since=0",
            Some(&token()),
            Some("\"0-1\""),
        )
        .await;
    assert_eq!(r.status, 304);

    // Screening the video publishes it, and only that game comes back.
    h.import(&store, "games.json", &list(true, true), media.path())
        .await;
    let r = h.manifest(1).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.body["catalog_version"], 2);
    let games = r.body["games"].as_array().unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0]["rev"], 2);
    let kinds: Vec<&str> = games[0]["media"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["cover", "video"]);

    // Unpublishing is a tombstone for a Hub that has the game, and nothing for one that has not.
    h.import(&store, "games.json", &list(false, true), media.path())
        .await;
    let r = h.manifest(2).await;
    assert_eq!(r.body["catalog_version"], 3);
    let id = games[0]["id"].clone();
    assert_eq!(
        r.body["games"],
        json!([{"id": id, "rev": 3, "deleted": true}])
    );
    let r = h.manifest(0).await;
    let names: Vec<&str> = r.body["games"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Valorant"]);

    // A deleted game is a tombstone too.
    h.sql(
        "UPDATE catalog_state SET version = 4;
           UPDATE catalog_games SET deleted_at = 1, changed_version = 4 WHERE name = 'Valorant'",
    );
    let r = h.manifest(3).await;
    assert_eq!(r.body["games"][0]["deleted"], true);
    assert!(r.body["games"][0].get("name").is_none());
}

#[tokio::test]
async fn a_changed_game_with_nothing_published_does_not_move_the_version() {
    let h = Harness::new().await;
    let store = TemplateStore::memory();
    let media = media_dir();
    let draft = json!([{"name": "Draft", "launcher": "Steam", "category": "Racing",
        "published": false}])
    .to_string();
    let r = h.import(&store, "d.json", &draft, media.path()).await;
    assert_eq!((r.created, r.version), (1, 0));
    let edited = draft.replace("Racing", "Sports");
    let r = h.import(&store, "d.json", &edited, media.path()).await;
    assert_eq!((r.updated, r.version), (1, 0));
    assert_eq!(h.manifest(0).await.status, 304, "nothing is published yet");
}

#[tokio::test]
async fn csv_lists_import_like_json_lists() {
    let h = Harness::new().await;
    let store = TemplateStore::memory();
    let media = media_dir();
    let csv = "name,launcher,category,age_rating,exe,product,publisher,cover\n\
               Dota 2,Steam,MOBA,PEGI 12,dota2.exe,Dota 2,Valve,dota.jpg\n\
               Dota 2,Steam,,,dota2_launcher.exe,,Valve,\n";
    let r = h.import(&store, "games.csv", csv, media.path()).await;
    assert_eq!((r.created, r.media_uploaded), (1, 1));
    let m = h.manifest(0).await;
    assert_eq!(
        m.body["games"][0]["signatures"].as_array().unwrap().len(),
        2
    );
    let again = h.import(&store, "games.csv", csv, media.path()).await;
    assert_eq!((again.unchanged, again.version), (1, 1));
}

#[tokio::test]
async fn an_unusable_file_stops_before_anything_is_written() {
    let media = media_dir();
    let missing =
        json!([{"name": "A", "launcher": "S", "category": "C", "cover": "nope.jpg"}]).to_string();
    let games = catalog::parse_import("a.json", &missing).unwrap();
    assert!(catalog::resolve(games, media.path()).is_err());
    let gif =
        json!([{"name": "A", "launcher": "S", "category": "C", "cover": "a.gif"}]).to_string();
    let games = catalog::parse_import("a.json", &gif).unwrap();
    assert!(
        catalog::resolve(games, media.path())
            .unwrap_err()
            .contains("must be jpg")
    );
}

#[tokio::test]
async fn the_manifest_refuses_a_missing_or_revoked_token_and_a_tenant_over_its_limit() {
    let h = Harness::new().await;
    let store = TemplateStore::memory();
    h.import(&store, "g.json", &list(true, true), media_dir().path())
        .await;

    let none = h.get("/v1/catalog/manifest", None, None).await;
    assert_eq!(
        (none.status, none.body["code"].as_str()),
        (401, Some("TOKEN_INVALID"))
    );
    let wrong = h
        .get("/v1/catalog/manifest", Some("hub_h1.wrong-secret"), None)
        .await;
    assert_eq!(wrong.status, 401);
    assert!(wrong.body.get("games").is_none());

    // Within the limit, then over it: 429 with Retry-After and no catalogue data.
    for i in 0..30 {
        let r = h.manifest(0).await;
        assert_eq!(r.status, 200, "call {i}");
    }
    let limited = h.manifest(0).await;
    assert_eq!(
        (limited.status, limited.body["code"].as_str()),
        (429, Some("RATE_LIMITED"))
    );
    assert!(limited.headers.contains_key("retry-after"));
    assert!(limited.body.get("games").is_none());
    h.now.fetch_add(61_000, Ordering::SeqCst);
    assert_eq!(h.manifest(0).await.status, 200);

    h.sql("UPDATE hubs SET revoked_at = 9");
    h.st.hub_tokens.lock().unwrap().clear();
    let revoked = h.manifest(0).await;
    assert_eq!(
        (revoked.status, revoked.body["code"].as_str()),
        (403, Some("FORBIDDEN"))
    );
    assert!(revoked.body.get("games").is_none());
}
