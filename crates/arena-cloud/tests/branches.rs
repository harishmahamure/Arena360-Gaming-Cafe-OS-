use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use arena_cloud::auth::AuthState;
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
use arena_cloud::{Clock, app_with_auth, auth_state};
use arena_store::control::{ControlDb, LATEST_CONTROL_VERSION, NewBackupCode};
use axum::body::{Body, to_bytes};
use axum::http::{Request, header};
use serde_json::{Value, json};
use tokio::sync::watch;
use tower::ServiceExt;

const ORIGIN: &str = "https://start.arena360.cloud";
const PEPPER: &str = "test-pepper-0123456789abcdef0123456789";
const T0: i64 = 1_800_000_000_000;
const PHONE_A: &str = "+919876543210";
const PHONE_B: &str = "+919876543211";

struct Harness {
    app: axum::Router,
    db: Arc<ControlDb>,
    st: Arc<AuthState>,
    _dir: tempfile::TempDir,
}

struct Reply {
    status: u16,
    body: Value,
}

impl Reply {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or("")
    }

    fn field(&self) -> &str {
        self.body["field"].as_str().unwrap_or("")
    }
}

fn cafe() -> Value {
    json!({
        "cafe_name": "Pixel Arena Gaming",
        "city": "Pune",
        "country": "IN",
        "currency": "INR",
        "timezone": "Asia/Kolkata",
        "gst_registered": false,
    })
}

fn with(mut v: Value, key: &str, value: Value) -> Value {
    v[key] = value;
    v
}

impl Harness {
    async fn new() -> Self {
        Self::with_billing(false).await
    }

    async fn with_billing(billing_enabled: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = ControlDb::open_local(&dir.path().join("gz_control.db"))
            .await
            .unwrap();
        db.migrate(T0).await.unwrap();
        let db = Arc::new(db);
        let now = Arc::new(AtomicI64::new(T0));
        let clock: Clock = Arc::new(move || now.load(Ordering::SeqCst));
        let cfg = AuthConfig {
            pepper: PEPPER.as_bytes().to_vec(),
            web_origins: vec![ORIGIN.to_owned()],
            sms: SmsProviderConfig::Fake,
            sms_global_per_min: 10_000,
            mfa_key: MfaKey::from_bytes([0xa5; 32]),
            licence: arena_cloud::licence::LicenceSigner::dev(),
            billing_enabled,
            escrow_key: None,
        };
        let st = auth_state(Arc::clone(&db), &cfg, clock).unwrap();
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
            _dir: dir,
        }
    }

    async fn call(&self, method: &str, path: &str, body: Option<Value>, access: &str) -> Reply {
        let mut req = Request::builder().method(method).uri(path);
        if !access.is_empty() {
            req = req.header("cookie", format!("arena_access={access}"));
        }
        let req = match body {
            Some(b) => req
                .header("content-type", "application/json")
                .body(Body::from(b.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        Reply { status, body }
    }

    async fn sign_up(&self, phone: &str, cafe_name: &str) -> String {
        let s = self
            .call(
                "POST",
                "/v1/signup/start",
                Some(json!({
                    "name": "Asha Rao",
                    "phone": phone,
                    "email": format!("owner{}@example.in", &phone[phone.len() - 2..]),
                    "cafe_name": cafe_name,
                    "city": "Pune",
                })),
                "",
            )
            .await;
        assert_eq!(s.status, 202, "{}", s.body);
        let id = s.body["challenge_id"].as_str().unwrap().to_owned();
        let text = self
            .st
            .sms
            .fake()
            .unwrap()
            .sent()
            .last()
            .unwrap()
            .text
            .clone();
        let req = Request::builder()
            .method("POST")
            .uri("/v1/signup/verify")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"challenge_id": id, "code": &text[..6]}).to_string(),
            ))
            .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status().as_u16(), 200);
        res.headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .find_map(|c| {
                c.to_str()
                    .unwrap()
                    .strip_prefix("arena_access=")
                    .map(|v| v.split(';').next().unwrap().to_owned())
            })
            .unwrap()
    }

    async fn complete_setup(&self, phone: &str) -> String {
        let id = self.db.owner_by_phone(phone).await.unwrap().unwrap().id;
        assert!(
            self.db
                .set_owner_password(&id, "$argon2id$x", T0)
                .await
                .unwrap()
        );
        assert!(self.db.start_owner_mfa(&id, b"ct", b"n", T0).await.unwrap());
        let codes: Vec<NewBackupCode> = (0..10)
            .map(|i| NewBackupCode {
                id: format!("{id}-b{i}"),
                code_hash: format!("h{i}"),
            })
            .collect();
        assert!(
            self.db
                .confirm_owner_mfa(&id, b"n", 1, &codes, T0)
                .await
                .unwrap()
        );
        assert!(self.db.ack_backup_codes(&id, T0).await.unwrap());
        id
    }

    async fn owner(&self, phone: &str, cafe_name: &str) -> String {
        let access = self.sign_up(phone, cafe_name).await;
        self.complete_setup(phone).await;
        access
    }

    async fn cafe_done(&self, access: &str) -> String {
        let r = self
            .call("PUT", "/v1/wizard/cafe", Some(cafe()), access)
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
        r.body["branch"]["id"].as_str().unwrap().to_owned()
    }

    fn sql(&self, sql: &str) {
        let path = self._dir.path().join("gz_control.db");
        rusqlite::Connection::open(path)
            .unwrap()
            .execute_batch(sql)
            .unwrap();
    }
}

const PAID: &str = "INSERT INTO entitlements (branch_id, tenant_id, tier, source, issued_at, until, \
     updated_at) SELECT id, tenant_id, 'basic', 'comp', 1, 9999999999999, 1 FROM branches";
const HUB: &str = "INSERT INTO hubs (id, branch_id, platform, hub_instance_id, version, token_hash, \
     paired_at, last_seen_at) SELECT 'h1', id, 'windows', 'i', '1', 'x', 1, 5000 FROM branches \
     WHERE id = ";

fn commands(h: &Harness) -> Vec<(String, String, String)> {
    let c = rusqlite::Connection::open(h._dir.path().join("gz_control.db")).unwrap();
    let mut st = c
        .prepare("SELECT branch_id, kind, payload FROM commands ORDER BY id")
        .unwrap();
    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[tokio::test]
async fn the_list_shows_each_branch_with_its_hub_and_setup_state() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let none = h.call("GET", "/v1/branches", None, &a).await;
    assert_eq!((none.status, none.body.clone()), (200, json!([])));
    let id = h.cafe_done(&a).await;
    let r = h.call("GET", "/v1/branches", None, &a).await;
    assert_eq!(
        r.body,
        json!([{
            "id": id, "name": "Pixel Arena Gaming", "branch_code": "PAG", "timezone": "Asia/Kolkata",
            "tier": "free", "flags": [], "live": false, "live_at": null,
            "setup": {"steps_done": 1, "steps_total": 10, "next_step": "tier"},
            "hub": {"paired": false}, "waiting": false
        }])
    );
    h.sql(&format!("{HUB}'{id}'"));
    h.sql("UPDATE branches SET live_at = 9");
    let r = h.call("GET", "/v1/branches", None, &a).await;
    assert_eq!(
        r.body[0]["hub"],
        json!({"paired": true, "platform": "windows", "last_seen_at": 5000})
    );
    assert_eq!(
        (r.body[0]["live"].clone(), r.body[0]["live_at"].clone()),
        (json!(true), json!(9))
    );
    assert_eq!(h.call("GET", "/v1/branches", None, "").await.status, 401);
}

#[tokio::test]
async fn an_unpaired_branch_is_edited_in_place_and_sends_no_command() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let id = h.cafe_done(&a).await;
    let r = h
        .call(
            "PATCH",
            &format!("/v1/branches/{id}"),
            Some(json!({"name": " Pixel Two ", "timezone": "Asia/Dubai"})),
            &a,
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        (
            r.body["name"].as_str(),
            r.body["timezone"].as_str(),
            r.body["waiting"].as_bool()
        ),
        (Some("Pixel Two"), Some("Asia/Dubai"), Some(false))
    );
    assert!(commands(&h).is_empty());
}

#[tokio::test]
async fn a_paired_branch_queues_one_settings_command_per_change_and_waits() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let id = h.cafe_done(&a).await;
    h.sql(&format!("{HUB}'{id}'"));
    let path = format!("/v1/branches/{id}");
    let r = h
        .call("PATCH", &path, Some(json!({"timezone": "Asia/Dubai"})), &a)
        .await;
    assert_eq!((r.status, r.body["waiting"].clone()), (200, json!(true)));
    let same = h
        .call("PATCH", &path, Some(json!({"timezone": "Asia/Dubai"})), &a)
        .await;
    assert_eq!(same.status, 200);
    let both = h
        .call(
            "PATCH",
            &path,
            Some(json!({"name": "Renamed", "timezone": "Asia/Kolkata"})),
            &a,
        )
        .await;
    assert_eq!(both.status, 200);
    let got = commands(&h);
    assert_eq!(got.len(), 3);
    assert!(got.iter().all(|c| c.0 == id && c.1 == "settings.set"));
    assert_eq!(
        serde_json::from_str::<Value>(&got[0].2).unwrap(),
        json!({"key": "branch.tz", "value": "Asia/Dubai"})
    );
    assert_eq!(
        serde_json::from_str::<Value>(&got[1].2).unwrap(),
        json!({"key": "branch.name", "value": "Renamed"})
    );
}

#[tokio::test]
async fn bad_edits_name_their_field_and_another_owners_branch_is_forbidden() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let b = h.owner(PHONE_B, "Level Up").await;
    let id = h.cafe_done(&a).await;
    let path = format!("/v1/branches/{id}");
    for (body, field) in [
        (json!({"name": "  "}), "name"),
        (json!({"name": "x".repeat(81)}), "name"),
        (json!({"timezone": "Mars/Base"}), "timezone"),
    ] {
        let r = h.call("PATCH", &path, Some(body), &a).await;
        assert_eq!((r.status, r.field()), (422, field));
    }
    let r = h
        .call("PATCH", &path, Some(json!({"name": "Stolen"})), &b)
        .await;
    assert_eq!((r.status, r.code()), (403, "FORBIDDEN"));
    let list = h.call("GET", "/v1/branches", None, &a).await;
    assert_eq!(list.body[0]["name"], "Pixel Arena Gaming");
}

#[tokio::test]
async fn adding_a_branch_is_locked_on_free_and_opens_step_one_when_paid() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let first = h.cafe_done(&a).await;
    let locked = h
        .call("POST", "/v1/branches", Some(json!({"name": "Second"})), &a)
        .await;
    assert_eq!(
        (locked.status, locked.code()),
        (403, "FEATURE_NOT_ENTITLED")
    );
    h.sql(PAID);
    let bad = h
        .call("POST", "/v1/branches", Some(json!({"name": " "})), &a)
        .await;
    assert_eq!((bad.status, bad.field()), (422, "name"));
    let r = h
        .call("POST", "/v1/branches", Some(json!({"name": "Second"})), &a)
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let id = r.body["id"].as_str().unwrap().to_owned();
    assert_ne!(id, first);
    assert_eq!(
        (r.body["tier"].as_str(), r.body["live"].as_bool()),
        (Some("free"), Some(false))
    );
    assert_ne!(r.body["branch_code"], "PAG");
    let w = h
        .call("GET", &format!("/v1/wizard?branch_id={id}"), None, &a)
        .await;
    assert_eq!(
        (w.body["current"].as_str(), w.body["branch"]["id"].as_str()),
        (Some("cafe"), Some(id.as_str()))
    );
    let saved = h
        .call(
            "PUT",
            "/v1/wizard/cafe",
            Some(with(
                with(cafe(), "cafe_name", json!("Second Pune")),
                "branch_id",
                json!(id),
            )),
            &a,
        )
        .await;
    assert_eq!(saved.status, 200, "{}", saved.body);
    assert_eq!(saved.body["current"], "tier");
    let list = h.call("GET", "/v1/branches", None, &a).await;
    assert_eq!(list.body[0]["name"], "Pixel Arena Gaming");
    assert_eq!(list.body[1]["name"], "Second Pune");
    let own = h.call("GET", "/v1/wizard", None, &a).await;
    assert_eq!(own.body["tenant"]["name"], "Pixel Arena Gaming");
}

#[tokio::test]
async fn another_owner_cannot_open_a_branch_wizard_by_id() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let b = h.owner(PHONE_B, "Level Up").await;
    let id = h.cafe_done(&a).await;
    h.cafe_done(&b).await;
    let r = h
        .call("GET", &format!("/v1/wizard?branch_id={id}"), None, &b)
        .await;
    assert_eq!((r.status, r.code()), (403, "FORBIDDEN"));
    let r = h
        .call(
            "PUT",
            "/v1/wizard/cafe",
            Some(with(cafe(), "branch_id", json!(id))),
            &b,
        )
        .await;
    assert_eq!((r.status, r.code()), (403, "FORBIDDEN"));
}
