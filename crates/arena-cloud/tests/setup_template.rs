//! `PUT /v1/branches/{id}/setup-template` (Story 5.37).

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use arena_cloud::auth::AuthState;
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
use arena_cloud::templates::{TemplateStore, file_key, meta_key};
use arena_cloud::{Clock, app_with_auth, auth_state};
use arena_proto::{SETUP_MAX_BYTES, Tier};
use arena_store::control::{CompGrant, ControlDb, LATEST_CONTROL_VERSION, NewBackupCode};
use axum::body::{Body, to_bytes};
use axum::extract::connect_info::ConnectInfo;
use axum::http::{Request, header};
use serde_json::{Value, json};
use tokio::sync::watch;
use tower::ServiceExt;

const ORIGIN: &str = "https://start.arena360.cloud";
const PEPPER: &str = "test-pepper-0123456789abcdef0123456789";
const T0: i64 = 1_800_000_000_000;
const DAY: i64 = 24 * 60 * 60 * 1000;
const PHONE_A: &str = "+919876543210";
const PHONE_B: &str = "+919876543211";

struct Harness {
    app: axum::Router,
    db: Arc<ControlDb>,
    st: Arc<AuthState>,
    now: Arc<AtomicI64>,
    dir: tempfile::TempDir,
}

struct Reply {
    status: u16,
    body: Value,
}

impl Reply {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or("")
    }
}

/// An owner's branch with a Hub paired to it.
struct Cafe {
    branch: String,
    token: String,
}

fn config() -> AuthConfig {
    AuthConfig {
        pepper: PEPPER.as_bytes().to_vec(),
        web_origins: vec![ORIGIN.to_owned()],
        sms: SmsProviderConfig::Fake,
        sms_global_per_min: 10_000,
        mfa_key: MfaKey::from_bytes([0xa5; 32]),
        licence: arena_cloud::licence::LicenceSigner::dev(),
        billing_enabled: false,
        escrow_key: None,
    }
}

fn router(st: &Arc<AuthState>) -> axum::Router {
    let (_tx, rx) = watch::channel(Reachability {
        reachable: true,
        checked_at: None,
    });
    app_with_auth(
        Role::All,
        true,
        rx,
        LATEST_CONTROL_VERSION,
        Some(Arc::clone(st)),
    )
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
        let tick = Arc::clone(&now);
        let clock: Clock = Arc::new(move || tick.load(Ordering::SeqCst));
        let st = auth_state(Arc::clone(&db), &config(), clock).unwrap();
        assert!(st.templates.set(TemplateStore::memory()).is_ok());
        let app = router(&st);
        Self {
            app,
            db,
            st,
            now,
            dir,
        }
    }

    fn templates(&self) -> &TemplateStore {
        self.st.templates.get().unwrap()
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
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    async fn put(&self, branch: &str, token: &str, body: Vec<u8>) -> Reply {
        let req = Request::builder()
            .method("PUT")
            .uri(format!("/v1/branches/{branch}/setup-template"))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(body))
            .unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
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

    async fn complete_setup(&self, phone: &str) {
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
    }

    /// A signed-up owner whose branch has a Hub paired (`peer` is the Hub's address).
    async fn cafe(&self, phone: &str, name: &str, peer: &str) -> Cafe {
        let access = self.sign_up(phone, name).await;
        self.complete_setup(phone).await;
        let r = self
            .call(
                "PUT",
                "/v1/wizard/cafe",
                Some(json!({
                    "cafe_name": name,
                    "city": "Pune",
                    "country": "IN",
                    "currency": "INR",
                    "timezone": "Asia/Kolkata",
                    "gst_registered": false,
                })),
                &access,
            )
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
        let branch = r.body["branch"]["id"].as_str().unwrap().to_owned();
        let r = self
            .call(
                "PUT",
                "/v1/wizard/tier",
                Some(json!({"tier": "free"})),
                &access,
            )
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
        let r = self
            .call(
                "POST",
                "/v1/wizard/install-hub/code",
                Some(json!({})),
                &access,
            )
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
        let code = r.body["code"].as_str().unwrap().to_owned();
        let mut req = Request::builder()
            .method("POST")
            .uri("/v1/pair")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "code": code,
                    "platform": "android",
                    "hub_instance_id": "01HUBINSTANCE",
                    "version": "0.1.0",
                })
                .to_string(),
            ))
            .unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::new(peer.parse().unwrap(), 40000)));
        let r = reply(self.app.clone().oneshot(req).await.unwrap()).await;
        assert_eq!(r.status, 200, "{}", r.body);
        Cafe {
            branch,
            token: r.body["token"].as_str().unwrap().to_owned(),
        }
    }

    async fn upgrade(&self, branch: &str, until: i64) {
        self.db
            .grant_comp(&CompGrant {
                branch_id: branch,
                tier: Tier::Basic,
                until,
                audit_id: &format!("audit-{branch}"),
                operator: "ops",
                reason: "beta cafe",
                details_json: "{}",
                now: T0,
            })
            .await
            .unwrap();
    }

    fn revoke_hubs(&self, branch: &str) {
        rusqlite::Connection::open(self.dir.path().join("gz_control.db"))
            .unwrap()
            .execute(
                "UPDATE hubs SET revoked_at = ?1 WHERE branch_id = ?2",
                rusqlite::params![T0, branch],
            )
            .unwrap();
    }
}

async fn reply(res: axum::response::Response) -> Reply {
    let status = res.status().as_u16();
    let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    Reply { status, body }
}

fn file(note: &str) -> Vec<u8> {
    format!(r#"{{"format":"arena360-setup","version":1,"currency":"INR","note":"{note}"}}"#)
        .into_bytes()
}

#[tokio::test]
async fn a_paid_branch_stores_its_template_with_its_version_and_upload_time() {
    let h = Harness::new().await;
    let a = h.cafe(PHONE_A, "Pixel Arena", "198.51.100.4").await;
    h.upgrade(&a.branch, T0 + 30 * DAY).await;

    // Bigger than the 64 KiB every other route takes.
    let big = file(&"x".repeat(200 * 1024));
    let r = h.put(&a.branch, &a.token, big.clone()).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["version"], 1);
    assert_eq!(r.body["bytes"], big.len());
    assert_eq!(r.body["uploaded_at"], T0);
    let stored = h.templates().get(&file_key(&a.branch)).await.unwrap();
    assert_eq!(stored, big, "the file is kept byte for byte");
    let described = h.templates().describe(&a.branch).await.unwrap().unwrap();
    assert_eq!(serde_json::to_value(&described).unwrap(), r.body);
    assert_eq!(
        h.templates().list("branches/").await.unwrap().len(),
        2,
        "a file and its description"
    );

    // A later upload replaces both.
    h.now.fetch_add(60_000, Ordering::SeqCst);
    let newer = file("newer");
    let r = h.put(&a.branch, &a.token, newer.clone()).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["uploaded_at"], T0 + 60_000);
    assert_eq!(
        h.templates().get(&file_key(&a.branch)).await.unwrap(),
        newer
    );
    let meta = h.templates().get(&meta_key(&a.branch)).await.unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&meta).unwrap(), r.body);
}

#[tokio::test]
async fn a_free_branch_cannot_upload_and_nothing_is_stored() {
    let h = Harness::new().await;
    let a = h.cafe(PHONE_A, "Pixel Arena", "198.51.100.4").await;
    let r = h.put(&a.branch, &a.token, file("free")).await;
    assert_eq!((r.status, r.code()), (403, "FEATURE_NOT_ENTITLED"));
    assert!(h.templates().list("branches/").await.unwrap().is_empty());

    // A paid plan that has run out is Free again.
    h.upgrade(&a.branch, T0 + DAY).await;
    assert_eq!(h.put(&a.branch, &a.token, file("paid")).await.status, 200);
    h.now.fetch_add(2 * DAY, Ordering::SeqCst);
    let r = h.put(&a.branch, &a.token, file("lapsed")).await;
    assert_eq!((r.status, r.code()), (403, "FEATURE_NOT_ENTITLED"));
    assert_eq!(
        h.templates().get(&file_key(&a.branch)).await.unwrap(),
        file("paid")
    );
}

#[tokio::test]
async fn refused_uploads_leave_the_stored_file_unchanged() {
    let h = Harness::new().await;
    let a = h.cafe(PHONE_A, "Pixel Arena", "198.51.100.4").await;
    let b = h.cafe(PHONE_B, "Other Arena", "198.51.100.5").await;
    h.upgrade(&a.branch, T0 + 30 * DAY).await;
    h.upgrade(&b.branch, T0 + 30 * DAY).await;
    assert_eq!(h.put(&a.branch, &a.token, file("kept")).await.status, 200);
    let kept = file("kept");
    let unchanged = async || {
        assert_eq!(h.templates().get(&file_key(&a.branch)).await.unwrap(), kept);
    };

    // Over 1 MB.
    let over = vec![b' '; SETUP_MAX_BYTES + 1];
    let r = h.put(&a.branch, &a.token, over).await;
    assert_eq!((r.status, r.code()), (413, "PAYLOAD_TOO_LARGE"));
    unchanged().await;
    // Exactly 1 MB is fine.
    let mut exact = file("");
    exact.extend(vec![b' '; SETUP_MAX_BYTES - exact.len()]);
    assert_eq!(exact.len(), SETUP_MAX_BYTES);
    assert_eq!(h.put(&b.branch, &b.token, exact).await.status, 200);

    // Another branch's id (of another tenant), with the Hub's own token, either way round.
    let r = h.put(&b.branch, &a.token, file("mine")).await;
    assert_eq!((r.status, r.code()), (403, "FORBIDDEN"));
    unchanged().await;
    let r = h.put(&a.branch, &b.token, file("theirs")).await;
    assert_eq!((r.status, r.code()), (403, "FORBIDDEN"));
    unchanged().await;

    // A token that is wrong.
    let r = h.put(&a.branch, "arena_hub_nope.x", file("x")).await;
    assert_eq!((r.status, r.code()), (401, "TOKEN_INVALID"));
    unchanged().await;

    // Not a setup file.
    for bad in [&b"hello"[..], br#"{"format":"x","version":1}"#, b"{}"] {
        let r = h.put(&a.branch, &a.token, bad.to_vec()).await;
        assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));
        assert_eq!(r.body["field"], "file");
    }
    unchanged().await;

    // A revoked token.
    h.revoke_hubs(&a.branch);
    let r = h.put(&a.branch, &a.token, file("revoked")).await;
    assert_eq!((r.status, r.code()), (401, "TOKEN_REVOKED"));
    unchanged().await;
}

#[tokio::test]
async fn without_an_object_store_the_cloud_says_it_is_unavailable() {
    let h = Harness::new().await;
    let a = h.cafe(PHONE_A, "Pixel Arena", "198.51.100.4").await;
    h.upgrade(&a.branch, T0 + 30 * DAY).await;
    // The same database behind a state that never got a store.
    let bare = auth_state(Arc::clone(&h.db), &config(), Arc::new(|| T0)).unwrap();
    assert!(bare.templates.get().is_none());
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/v1/branches/{}/setup-template", a.branch))
        .header("authorization", format!("Bearer {}", a.token))
        .body(Body::from(file("x")))
        .unwrap();
    let r = reply(router(&bare).oneshot(req).await.unwrap()).await;
    assert_eq!((r.status, r.code()), (503, "INTERNAL"));
}
