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
const GSTIN: &str = "27AAPFU0939F1ZV";

struct Harness {
    app: axum::Router,
    db: Arc<ControlDb>,
    st: Arc<AuthState>,
    _dir: tempfile::TempDir,
}

struct Reply {
    status: u16,
    headers: axum::http::HeaderMap,
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
            billing_enabled: false,
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
        let headers = res.headers().clone();
        let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
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

    async fn counts(&self) -> (usize, usize) {
        let path = self._dir.path().join("gz_control.db");
        let c = rusqlite::Connection::open(path).unwrap();
        let n = |t: &str| -> usize {
            c.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap() as usize
        };
        (n("tenants"), n("branches"))
    }

    async fn branch_row(&self, id: &str) -> (String, String, i64) {
        let path = self._dir.path().join("gz_control.db");
        let c = rusqlite::Connection::open(path).unwrap();
        c.query_row(
            "SELECT name, branch_code, updated_at FROM branches WHERE id = ?",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
    }
}

fn states(body: &Value) -> Vec<(String, String, bool)> {
    body["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["key"].as_str().unwrap().to_owned(),
                s["state"].as_str().unwrap().to_owned(),
                s["can_skip"].as_bool().unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn a_new_owner_starts_at_cafe_with_every_step_pending() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let r = h.call("GET", "/v1/wizard", None, &a).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["current"], "cafe");
    assert!(r.body.get("tenant").is_none() && r.body.get("branch").is_none());
    let s = states(&r.body);
    assert_eq!(
        s.iter().map(|(k, _, _)| k.as_str()).collect::<Vec<_>>(),
        [
            "cafe",
            "tier",
            "install_hub",
            "devices",
            "rates",
            "products",
            "staff",
            "printer_payments",
            "test_session",
            "go_live"
        ]
    );
    assert!(s.iter().all(|(_, state, _)| state == "pending"));
    assert_eq!(
        s.iter()
            .filter(|(_, _, skip)| *skip)
            .map(|(k, _, _)| k.as_str())
            .collect::<Vec<_>>(),
        ["products", "staff", "printer_payments"]
    );
}

#[tokio::test]
async fn an_owner_without_finished_setup_is_refused() {
    let h = Harness::new().await;
    let a = h.sign_up(PHONE_A, "Pixel Arena Gaming").await;
    for (method, path, body) in [
        ("GET", "/v1/wizard", None),
        ("PUT", "/v1/wizard/cafe", Some(cafe())),
        ("PUT", "/v1/branches/x/code", Some(json!({"code": "ABC"}))),
    ] {
        let r = h.call(method, path, body, &a).await;
        assert_eq!((r.status, r.code()), (403, "SETUP_INCOMPLETE"), "{path}");
    }
    let r = h.call("GET", "/v1/wizard", None, "").await;
    assert_eq!((r.status, r.code()), (401, "AUTH_FAILED"));
    assert_eq!(h.counts().await, (0, 0));
}

#[tokio::test]
async fn step_one_creates_the_tenant_branch_and_progress() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let r = h.call("PUT", "/v1/wizard/cafe", Some(cafe()), &a).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["current"], "tier");
    let s = states(&r.body);
    assert_eq!(s[0].1, "done");
    assert!(s[1..].iter().all(|(_, state, _)| state == "pending"));
    let b = &r.body["branch"];
    assert_eq!(b["branch_code"], "PAG");
    assert_eq!(b["code_editable"], true);
    assert_eq!(b["country"], "IN");
    assert_eq!(b["currency"], "INR");
    assert_eq!(b["timezone"], "Asia/Kolkata");
    assert_eq!(b["gst_registered"], false);
    assert_eq!(b["gstin"], Value::Null);
    assert_eq!(r.body["tenant"]["name"], "Pixel Arena Gaming");
    assert_eq!(h.counts().await, (1, 1));
    let id = b["id"].as_str().unwrap();
    let progress = h.db.onboarding_progress(id).await.unwrap();
    assert_eq!(progress.len(), 10);
    assert_eq!(progress[0].completed_at, Some(T0));

    let g = h.call("GET", "/v1/wizard", None, &a).await;
    assert_eq!(g.body, r.body);
}

#[tokio::test]
async fn gstin_rules() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let gst = with(cafe(), "gst_registered", json!(true));
    let mut bad = GSTIN.to_owned();
    bad.replace_range(14..15, "A");
    for body in [
        gst.clone(),
        with(gst.clone(), "gstin", json!("")),
        with(gst.clone(), "gstin", json!(bad)),
        with(gst.clone(), "gstin", json!("27AAPFU0939F1Z")),
        with(cafe(), "gstin", json!(GSTIN)),
    ] {
        let r = h
            .call("PUT", "/v1/wizard/cafe", Some(body.clone()), &a)
            .await;
        assert_eq!(
            (r.status, r.code(), r.field()),
            (422, "VALIDATION_FAILED", "gstin"),
            "{body}"
        );
    }
    assert_eq!(h.counts().await, (0, 0));
    let r = h
        .call(
            "PUT",
            "/v1/wizard/cafe",
            Some(with(gst, "gstin", json!(" 27aapfu0939f1zv "))),
            &a,
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["branch"]["gstin"], GSTIN);
    assert_eq!(r.body["branch"]["gst_registered"], true);
}

#[tokio::test]
async fn unknown_country_currency_and_timezone_name_their_field() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    for (key, value) in [
        ("timezone", json!("Mars/Base")),
        ("timezone", json!("US/Eastern")),
        ("timezone", json!("Etc/GMT+5")),
        ("timezone", json!("Factory")),
        ("currency", json!("XYZ")),
        ("country", json!("ZZ")),
        ("cafe_name", json!("  ")),
        ("city", json!("")),
    ] {
        let r = h
            .call("PUT", "/v1/wizard/cafe", Some(with(cafe(), key, value)), &a)
            .await;
        assert_eq!(
            (r.status, r.code(), r.field()),
            (422, "VALIDATION_FAILED", key)
        );
    }
    let r = h
        .call(
            "PUT",
            "/v1/wizard/cafe",
            Some(with(
                with(cafe(), "country", json!("AE")),
                "gst_registered",
                json!(true),
            )),
            &a,
        )
        .await;
    assert_eq!((r.status, r.field()), (422, "gst_registered"));
    assert_eq!(h.counts().await, (0, 0));
    let r = h
        .call(
            "PUT",
            "/v1/wizard/cafe",
            Some(with(
                with(cafe(), "country", json!("AE")),
                "timezone",
                json!("Europe/London"),
            )),
            &a,
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["branch"]["timezone"], "Europe/London");
}

#[tokio::test]
async fn editing_step_one_updates_in_place() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let first = h.call("PUT", "/v1/wizard/cafe", Some(cafe()), &a).await;
    let id = first.body["branch"]["id"].as_str().unwrap().to_owned();
    let edit = json!({
        "cafe_name": "Pixel Arena",
        "city": "Mumbai",
        "country": "IN",
        "currency": "USD",
        "timezone": "Asia/Kolkata",
        "gst_registered": true,
        "gstin": GSTIN,
    });
    let r = h.call("PUT", "/v1/wizard/cafe", Some(edit), &a).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["current"], "tier");
    assert_eq!(states(&r.body)[0].1, "done");
    assert_eq!(r.body["branch"]["id"], id.as_str());
    assert_eq!(r.body["branch"]["city"], "Mumbai");
    assert_eq!(r.body["branch"]["currency"], "USD");
    assert_eq!(r.body["branch"]["branch_code"], "PAG");
    assert_eq!(r.body["tenant"]["name"], "Pixel Arena");
    assert_eq!(r.body["tenant"]["id"], first.body["tenant"]["id"]);
    assert_eq!(h.counts().await, (1, 1));
    assert_eq!(
        h.db.onboarding_progress(&id).await.unwrap()[0].completed_at,
        Some(T0)
    );
}

#[tokio::test]
async fn the_branch_code_can_be_edited_until_locked() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let r = h.call("PUT", "/v1/wizard/cafe", Some(cafe()), &a).await;
    let id = r.body["branch"]["id"].as_str().unwrap().to_owned();
    let path = format!("/v1/branches/{id}/code");

    let ok = h
        .call("PUT", &path, Some(json!({"code": "ARENA1"})), &a)
        .await;
    assert_eq!(ok.status, 200, "{}", ok.body);
    assert_eq!(ok.body, json!({"branch_code": "ARENA1"}));
    let lower = h
        .call("PUT", &path, Some(json!({"code": "pune1"})), &a)
        .await;
    assert_eq!(lower.body, json!({"branch_code": "PUNE1"}));

    for bad in ["ab", "ABCDEFG", "AB-1", ""] {
        let r = h.call("PUT", &path, Some(json!({ "code": bad })), &a).await;
        assert_eq!(
            (r.status, r.code(), r.field()),
            (422, "VALIDATION_FAILED", "code"),
            "{bad}"
        );
    }

    let c = rusqlite::Connection::open(h._dir.path().join("gz_control.db")).unwrap();
    let tenant: String = c
        .query_row("SELECT tenant_id FROM branches WHERE id = ?", [&id], |r| {
            r.get(0)
        })
        .unwrap();
    c.execute(
        "INSERT INTO branches (id, tenant_id, name, city, country, currency, timezone, \
         gst_registered, gstin, branch_code, code_locked_at, created_at, updated_at) \
         VALUES ('second', ?, 'Second', 'Pune', 'IN', 'INR', 'Asia/Kolkata', 0, NULL, 'SEC', \
         NULL, 9000000000000, 9000000000000)",
        [&tenant],
    )
    .unwrap();
    let dup = h.call("PUT", &path, Some(json!({"code": "SEC"})), &a).await;
    assert_eq!(
        (dup.status, dup.code(), dup.field()),
        (409, "BRANCH_CODE_TAKEN", "code")
    );
    assert_eq!(h.branch_row(&id).await.1, "PUNE1");

    assert!(h.db.lock_branch_code(&id, T0 + 5).await.unwrap());
    let locked = h
        .call("PUT", &path, Some(json!({"code": "NEW1"})), &a)
        .await;
    assert_eq!((locked.status, locked.code()), (409, "ALREADY_SET"));
    assert_eq!(h.branch_row(&id).await.1, "PUNE1");
    let w = h.call("GET", "/v1/wizard", None, &a).await;
    assert_eq!(w.body["branch"]["code_editable"], false);
}

#[tokio::test]
async fn another_owners_branch_or_an_unknown_one_is_forbidden_and_untouched() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let b = h.owner(PHONE_B, "Level Up").await;
    let ra = h.call("PUT", "/v1/wizard/cafe", Some(cafe()), &a).await;
    let a_branch = ra.body["branch"]["id"].as_str().unwrap().to_owned();
    let before = h.branch_row(&a_branch).await;

    let wb = h.call("GET", "/v1/wizard", None, &b).await;
    assert_eq!(wb.body["current"], "cafe");
    assert!(wb.body.get("branch").is_none());

    for target in [a_branch.as_str(), "01JUNKNOWNBRANCH000000000000"] {
        let r = h
            .call(
                "PUT",
                &format!("/v1/branches/{target}/code"),
                Some(json!({"code": "STOLEN"})),
                &b,
            )
            .await;
        assert_eq!((r.status, r.code()), (403, "FORBIDDEN"), "{target}");
        let r = h
            .call(
                "PUT",
                &format!("/v1/branches/{target}/code"),
                Some(json!({"code": "x"})),
                &b,
            )
            .await;
        assert_eq!((r.status, r.code()), (403, "FORBIDDEN"), "{target}");
    }
    assert_eq!(h.branch_row(&a_branch).await, before);
    assert_eq!(h.counts().await, (1, 1));

    let rb = h
        .call(
            "PUT",
            "/v1/wizard/cafe",
            Some(with(cafe(), "cafe_name", json!("Level Up"))),
            &b,
        )
        .await;
    assert_eq!(rb.status, 200, "{}", rb.body);
    assert_ne!(rb.body["branch"]["id"], a_branch.as_str());
    assert_ne!(rb.body["tenant"]["id"], ra.body["tenant"]["id"]);
    assert_eq!(rb.body["branch"]["branch_code"], "LEV");
    assert_eq!(h.branch_row(&a_branch).await, before);
    assert_eq!(h.counts().await, (2, 2));

    let ignored = h
        .call(
            "PUT",
            "/v1/wizard/cafe",
            Some(with(cafe(), "tenant_id", ra.body["tenant"]["id"].clone())),
            &b,
        )
        .await;
    assert_eq!(ignored.status, 200, "{}", ignored.body);
    assert_eq!(ignored.body["tenant"]["id"], rb.body["tenant"]["id"]);
    assert_eq!(h.branch_row(&a_branch).await, before);
}

#[tokio::test]
async fn the_session_carries_the_signup_cafe_and_city() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let r = h.call("GET", "/v1/session", None, &a).await;
    assert_eq!(r.body["owner"]["cafe_name"], "Pixel Arena Gaming");
    assert_eq!(r.body["owner"]["city"], "Pune");
}

#[tokio::test]
async fn meta_lists_countries_and_timezones() {
    let h = Harness::new().await;
    let c = h.call("GET", "/v1/meta/countries", None, "").await;
    assert_eq!(c.status, 200);
    let list = c.body.as_array().unwrap();
    assert_eq!(list.len(), 9);
    assert_eq!(
        list[0],
        json!({"code": "IN", "name": "India", "currency": "INR", "timezone": "Asia/Kolkata", "gst": true})
    );
    let t = h.call("GET", "/v1/meta/timezones", None, "").await;
    let zones = t.body.as_array().unwrap();
    assert!(zones.contains(&json!("Asia/Kolkata")) && zones.contains(&json!("UTC")));
    assert!(!zones.contains(&json!("Mars/Base")));
    assert!(!zones.contains(&json!("US/Eastern")));
    for r in [&c, &t] {
        assert_eq!(
            r.headers.get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=86400"
        );
    }
}

#[tokio::test]
async fn two_concurrent_first_saves_make_one_tenant_and_branch() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let (r1, r2) = tokio::join!(
        h.call("PUT", "/v1/wizard/cafe", Some(cafe()), &a),
        h.call(
            "PUT",
            "/v1/wizard/cafe",
            Some(with(cafe(), "city", json!("Mumbai"))),
            &a
        ),
    );
    assert_eq!(
        (r1.status, r2.status),
        (200, 200),
        "{} {}",
        r1.body,
        r2.body
    );
    assert_eq!(h.counts().await, (1, 1));
    assert_eq!(r1.body["branch"]["id"], r2.body["branch"]["id"]);
    let id = r1.body["branch"]["id"].as_str().unwrap();
    assert_eq!(h.db.onboarding_progress(id).await.unwrap().len(), 10);
}

#[tokio::test]
async fn a_failed_progress_insert_leaves_no_tenant_or_branch() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let c = rusqlite::Connection::open(h._dir.path().join("gz_control.db")).unwrap();
    c.execute_batch(
        "CREATE TRIGGER fail_progress BEFORE INSERT ON onboarding_progress \
         WHEN NEW.step = 'go_live' BEGIN SELECT RAISE(ABORT, 'injected'); END;",
    )
    .unwrap();
    let r = h.call("PUT", "/v1/wizard/cafe", Some(cafe()), &a).await;
    assert_eq!((r.status, r.code()), (500, "INTERNAL"));
    assert_eq!(h.counts().await, (0, 0));
    let progress: i64 = c
        .query_row("SELECT COUNT(*) FROM onboarding_progress", [], |r| r.get(0))
        .unwrap();
    assert_eq!(progress, 0);
    c.execute_batch("DROP TRIGGER fail_progress").unwrap();
    let r = h.call("PUT", "/v1/wizard/cafe", Some(cafe()), &a).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(h.counts().await, (1, 1));
}

#[tokio::test]
async fn a_tenant_without_a_branch_gets_its_first_branch() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let owner = h.db.owner_by_phone(PHONE_A).await.unwrap().unwrap().id;
    let c = rusqlite::Connection::open(h._dir.path().join("gz_control.db")).unwrap();
    c.execute(
        "INSERT INTO tenants (id, owner_id, name, created_at) VALUES ('t-orphan', ?, 'Old', 1)",
        [&owner],
    )
    .unwrap();
    let w = h.call("GET", "/v1/wizard", None, &a).await;
    assert_eq!(w.body["current"], "cafe");
    let r = h.call("PUT", "/v1/wizard/cafe", Some(cafe()), &a).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["tenant"]["id"], "t-orphan");
    assert_eq!(r.body["tenant"]["name"], "Pixel Arena Gaming");
    assert_eq!(r.body["current"], "tier");
    assert_eq!(r.body["branch"]["branch_code"], "PAG");
    assert_eq!(h.counts().await, (1, 1));
    let again = h.call("PUT", "/v1/wizard/cafe", Some(cafe()), &a).await;
    assert_eq!(again.status, 200);
    assert_eq!(h.counts().await, (1, 1));
}

#[tokio::test]
async fn a_name_without_latin_letters_takes_the_city_then_brx() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let r = h
        .call(
            "PUT",
            "/v1/wizard/cafe",
            Some(with(cafe(), "cafe_name", json!("गेमिंग अड्डा"))),
            &a,
        )
        .await;
    assert_eq!(r.body["branch"]["branch_code"], "PUN");
    let b = h.owner(PHONE_B, "Level Up").await;
    let r = h
        .call(
            "PUT",
            "/v1/wizard/cafe",
            Some(with(
                with(cafe(), "cafe_name", json!("गेमिंग अड्डा")),
                "city",
                json!("पुणे"),
            )),
            &b,
        )
        .await;
    assert_eq!(r.body["branch"]["branch_code"], "BRX");
}

#[test]
fn gstin_checksum_vectors() {
    use arena_cloud::gstin::valid_gstin;
    for g in [
        "27AAPFU0939F1ZV",
        "29AAGCB7383J1Z4",
        "24AAACC1206D1ZM",
        "27AAACR5055K1Z7",
        "09AAACH7409R1ZZ",
    ] {
        assert!(valid_gstin(g), "{g}");
        let mut wrong = g.to_owned();
        let last = if g.ends_with('0') { "1" } else { "0" };
        wrong.replace_range(14..15, last);
        assert!(!valid_gstin(&wrong), "{wrong}");
    }
}

#[test]
fn branch_code_derivation_and_clashes() {
    use arena_cloud::branch_code::{assign, derive};
    assert_eq!(derive("Pixel Arena Gaming", "Pune"), "PAG");
    assert_eq!(derive("Arena", "Pune"), "ARE");
    assert_eq!(derive("Go", "Pune"), "GOX");
    assert_eq!(derive("गेमिंग", "Pune"), "PUN");
    assert_eq!(derive("गेमिंग", "पुणे"), "BRX");
    assert_eq!(
        assign("गेमिंग", "पुणे", &["BRX".to_owned()]).as_deref(),
        Some("BR2")
    );
    assert_eq!(
        assign("Pixel Arena Gaming", "Pune", &["PAG".to_owned()]).as_deref(),
        Some("PA2")
    );
    assert_eq!(
        assign(
            "Pixel Arena Gaming",
            "Pune",
            &["PAG".to_owned(), "PA2".to_owned()]
        )
        .as_deref(),
        Some("PA3")
    );
}
