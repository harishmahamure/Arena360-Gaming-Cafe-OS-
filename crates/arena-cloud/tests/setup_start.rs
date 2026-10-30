//! A second branch starts from the tenant's existing setup (Story 5.38).

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use arena_cloud::auth::AuthState;
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
use arena_cloud::templates::TemplateStore;
use arena_cloud::{Clock, app_with_auth, auth_state};
use arena_proto::{SETUP_LINK_TTL_MS, Tier};
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
    access: String,
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
        let r = self.pair(&access, &branch, peer).await;
        assert_eq!(r.status, 200, "{}", r.body);
        Cafe {
            access,
            branch,
            token: r.body["token"].as_str().unwrap().to_owned(),
        }
    }

    /// Issues a pairing code for `branch` and pairs an Android Hub with it.
    async fn pair(&self, access: &str, branch: &str, peer: &str) -> Reply {
        let r = self
            .call(
                "POST",
                "/v1/wizard/install-hub/code",
                Some(json!({"branch_id": branch})),
                access,
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
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    /// Adds a branch to the owner's tenant (the first must be paid) and fills in its café step.
    async fn add_branch(&self, access: &str, name: &str) -> String {
        let r = self
            .call("POST", "/v1/branches", Some(json!({"name": name})), access)
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
        let id = r.body["id"].as_str().unwrap().to_owned();
        let r = self
            .call(
                "PUT",
                "/v1/wizard/cafe",
                Some(json!({
                    "branch_id": id,
                    "cafe_name": name,
                    "city": "Pune",
                    "country": "IN",
                    "currency": "INR",
                    "timezone": "Asia/Kolkata",
                    "gst_registered": false,
                })),
                access,
            )
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
        let r = self
            .call(
                "PUT",
                "/v1/wizard/tier",
                Some(json!({"tier": "free", "branch_id": id})),
                access,
            )
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
        id
    }

    async fn upload(&self, c: &Cafe, body: &str) {
        let req = Request::builder()
            .method("PUT")
            .uri(format!("/v1/branches/{}/setup-template", c.branch))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", c.token))
            .body(Body::from(body.to_owned()))
            .unwrap();
        let r = reply(self.app.clone().oneshot(req).await.unwrap()).await;
        assert_eq!(r.status, 200, "{}", r.body);
    }

    async fn heartbeat(&self, token: &str) -> Reply {
        let req = Request::builder()
            .method("POST")
            .uri("/v1/heartbeat")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(
                json!({"epoch": "01JEPOCH", "version": "0.1.0", "schema_version": 1, "last_seq_uploaded": 0}).to_string(),
            ))
            .unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    async fn outcome(&self, token: &str, branch: &str, body: Value) -> Reply {
        let req = Request::builder()
            .method("POST")
            .uri(format!("/v1/branches/{branch}/setup-template/outcome"))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(body.to_string()))
            .unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    async fn download(&self, path: &str) -> (u16, Vec<u8>) {
        let req = Request::builder()
            .method("GET")
            .uri(path)
            .body(Body::empty())
            .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        (
            status,
            to_bytes(res.into_body(), 1 << 20).await.unwrap().to_vec(),
        )
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

    fn lapse(&self, branch: &str) {
        rusqlite::Connection::open(self.dir.path().join("gz_control.db"))
            .unwrap()
            .execute(
                "UPDATE entitlements SET until = ?1 WHERE branch_id = ?2",
                rusqlite::params![T0 - 1, branch],
            )
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
const FILE: &str = r#"{"format":"arena360-setup","version":1,"currency":"INR","groups":[]}"#;

/// A paid first branch with a stored file, and a second branch (Free, unpaired) of the same owner.
async fn two_branches(h: &Harness) -> (Cafe, String) {
    let a = h.cafe(PHONE_A, "Pixel Arena", "198.51.100.4").await;
    h.upgrade(&a.branch, T0 + 30 * DAY).await;
    h.upload(&a, FILE).await;
    let b = h.add_branch(&a.access, "Second Pune").await;
    (a, b)
}

async fn view(h: &Harness, c: &Cafe, branch: &str) -> Reply {
    h.call(
        "GET",
        &format!("/v1/branches/{branch}/setup-start"),
        None,
        &c.access,
    )
    .await
}

async fn choose(h: &Harness, c: &Cafe, branch: &str, body: Value) -> Reply {
    h.call(
        "PUT",
        &format!("/v1/branches/{branch}/setup-start"),
        Some(body),
        &c.access,
    )
    .await
}

async fn retry(h: &Harness, c: &Cafe, branch: &str) -> Reply {
    h.call(
        "POST",
        &format!("/v1/branches/{branch}/setup-start/retry"),
        None,
        &c.access,
    )
    .await
}

#[tokio::test]
async fn a_second_branch_is_offered_the_paid_branch_that_has_a_stored_file_on_by_default() {
    let h = Harness::new().await;
    let a = h.cafe(PHONE_A, "Pixel Arena", "198.51.100.4").await;
    h.upgrade(&a.branch, T0 + 30 * DAY).await;
    let b = h.add_branch(&a.access, "Second Pune").await;

    // Paid, but nothing stored yet: the wizard explains the file path.
    let v = view(&h, &a, &b).await;
    assert_eq!(v.status, 200, "{}", v.body);
    assert_eq!(v.body["sources"], json!([]));
    assert_eq!(v.body["enabled"], false);
    assert_eq!(v.body["selected"], Value::Null);
    assert_eq!(v.body["other_branches"], 1);

    h.upload(&a, FILE).await;
    let v = view(&h, &a, &b).await;
    assert_eq!(v.body["enabled"], true);
    assert_eq!(v.body["selected"], json!(a.branch));
    assert_eq!(v.body["sources"][0]["branch_id"], json!(a.branch));
    assert_eq!(v.body["sources"][0]["name"], "Pixel Arena");
    assert_eq!(v.body["sources"][0]["uploaded_at"], T0);
    assert_eq!(v.body["status"], Value::Null);

    // The first branch itself has no other branch to start from.
    let own = view(&h, &a, &a.branch).await;
    assert_eq!(own.body["sources"], json!([]));
    assert_eq!(own.body["other_branches"], 1);

    // A lapsed plan takes the branch out of the picker.
    h.lapse(&a.branch);
    let v = view(&h, &a, &b).await;
    assert_eq!(v.body["sources"], json!([]));
    assert_eq!(v.body["enabled"], false);
}

#[tokio::test]
async fn pairing_carries_a_short_lived_link_to_the_chosen_file() {
    let h = Harness::new().await;
    let (a, b) = two_branches(&h).await;

    let paired = h.pair(&a.access, &b, "198.51.100.6").await;
    assert_eq!(paired.status, 200, "{}", paired.body);
    let link = &paired.body["bootstrap"]["template"];
    let path = link["path"].as_str().expect("a link").to_owned();
    assert!(path.starts_with("/v1/setup-template/"), "{path}");
    assert_eq!(link["version"], 1);
    assert_eq!(link["bytes"], FILE.len());
    assert_eq!(link["expires_at"], T0 + SETUP_LINK_TTL_MS);
    let secret = paired.body["token"].as_str().unwrap();
    assert!(
        !path.contains(&secret[secret.len() - 12..]),
        "the link holds no hub secret"
    );

    let (status, body) = h.download(&path).await;
    assert_eq!(status, 200);
    assert_eq!(body, FILE.as_bytes());

    let v = view(&h, &a, &b).await;
    assert_eq!(v.body["status"], "pending");
    assert_eq!(v.body["selected"], json!(a.branch));

    // The same link works until it expires, then never again.
    h.now.fetch_add(SETUP_LINK_TTL_MS - 1, Ordering::SeqCst);
    assert_eq!(h.download(&path).await.0, 200);
    h.now.fetch_add(1, Ordering::SeqCst);
    let (status, body) = h.download(&path).await;
    assert_eq!(status, 404);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["code"],
        "NOT_FOUND"
    );
}

#[tokio::test]
async fn a_link_that_is_forged_or_whose_file_changed_leads_nowhere() {
    let h = Harness::new().await;
    let (a, b) = two_branches(&h).await;
    let paired = h.pair(&a.access, &b, "198.51.100.6").await;
    let path = paired.body["bootstrap"]["template"]["path"]
        .as_str()
        .unwrap()
        .to_owned();

    // A flipped signature, a stretched expiry and a bare id.
    let mut forged = path.clone();
    let last = forged.pop().unwrap();
    forged.push(if last == '0' { '1' } else { '0' });
    assert_eq!(h.download(&forged).await.0, 404);
    let parts: Vec<&str> = path.rsplit('/').next().unwrap().split('.').collect();
    let stretched = format!(
        "/v1/setup-template/{}.{}.{}.{}",
        parts[0],
        T0 + 10 * DAY,
        parts[2],
        parts[3]
    );
    assert_eq!(h.download(&stretched).await.0, 404);
    let bare = format!("/v1/setup-template/{}", a.branch);
    assert_eq!(h.download(&bare).await.0, 404);

    // The source uploads a newer file: the old link no longer matches what it described.
    h.upload(
        &a,
        r#"{"format":"arena360-setup","version":1,"note":"newer"}"#,
    )
    .await;
    assert_eq!(h.download(&path).await.0, 404);
}

#[tokio::test]
async fn turning_it_off_pairs_without_a_link_and_a_bad_source_is_refused() {
    let h = Harness::new().await;
    let (a, b) = two_branches(&h).await;

    for bad in [
        json!({"enabled": true, "source_branch_id": b}),
        json!({"enabled": true, "source_branch_id": "01JNOSUCHBRANCH"}),
    ] {
        let r = choose(&h, &a, &b, bad).await;
        assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));
        assert_eq!(r.body["field"], "source_branch_id");
    }
    let off = choose(&h, &a, &b, json!({"enabled": false})).await;
    assert_eq!(off.status, 200, "{}", off.body);
    assert_eq!(off.body["enabled"], false);
    assert_eq!(off.body["selected"], Value::Null);
    assert_eq!(
        off.body["sources"].as_array().unwrap().len(),
        1,
        "it can be turned back on"
    );

    let paired = h.pair(&a.access, &b, "198.51.100.6").await;
    assert_eq!(paired.status, 200, "{}", paired.body);
    assert!(paired.body["bootstrap"].get("template").is_none());

    // Another owner cannot read or change it.
    let other = h.cafe(PHONE_B, "Other Arena", "198.51.100.5").await;
    let r = view(&h, &other, &b).await;
    assert_eq!((r.status, r.code()), (403, "FORBIDDEN"));
    let r = choose(&h, &other, &b, json!({"enabled": true})).await;
    assert_eq!((r.status, r.code()), (403, "FORBIDDEN"));
}

#[tokio::test]
async fn a_failed_try_shows_in_the_wizard_and_retry_sends_one_fresh_link_with_the_next_heartbeat() {
    let h = Harness::new().await;
    let (a, b) = two_branches(&h).await;
    let paired = h.pair(&a.access, &b, "198.51.100.6").await;
    let token = paired.body["token"].as_str().unwrap().to_owned();

    // The Hub could not download it.
    let r = h
        .outcome(
            &token,
            &b,
            json!({"applied": false, "message": "  The download failed.  "}),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let v = view(&h, &a, &b).await;
    assert_eq!(v.body["status"], "failed");
    assert_eq!(v.body["error"], "The download failed.");

    // Nothing is sent until the owner asks again.
    h.now.fetch_add(60_000, Ordering::SeqCst);
    let beat = h.heartbeat(&token).await;
    assert_eq!(beat.status, 200, "{}", beat.body);
    assert!(beat.body.get("setup_template").is_none());

    let again = retry(&h, &a, &b).await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(again.body["status"], "retry");

    h.now.fetch_add(60_000, Ordering::SeqCst);
    let beat = h.heartbeat(&token).await;
    let link = &beat.body["setup_template"];
    let path = link["path"].as_str().expect("a fresh link").to_owned();
    assert_eq!(link["expires_at"], T0 + 120_000 + SETUP_LINK_TTL_MS);
    assert_eq!(h.download(&path).await.1, FILE.as_bytes());
    assert_eq!(view(&h, &a, &b).await.body["status"], "pending");

    h.now.fetch_add(60_000, Ordering::SeqCst);
    let next = h.heartbeat(&token).await;
    assert!(
        next.body.get("setup_template").is_none(),
        "one heartbeat wins the link"
    );

    // Applied; a Retry now changes nothing.
    let r = h.outcome(&token, &b, json!({"applied": true})).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let done = retry(&h, &a, &b).await;
    assert_eq!(done.body["status"], "applied");
    assert_eq!(done.body["error"], Value::Null);
}

#[tokio::test]
async fn an_outcome_is_accepted_only_from_the_branch_s_own_hub() {
    let h = Harness::new().await;
    let (a, b) = two_branches(&h).await;
    let paired = h.pair(&a.access, &b, "198.51.100.6").await;
    let token = paired.body["token"].as_str().unwrap().to_owned();

    // The first branch's Hub cannot report for the second, and a bad token is refused.
    let r = h.outcome(&a.token, &b, json!({"applied": true})).await;
    assert_eq!((r.status, r.code()), (403, "FORBIDDEN"));
    let r = h
        .outcome("arena_hub_nope.x", &b, json!({"applied": true}))
        .await;
    assert_eq!((r.status, r.code()), (401, "TOKEN_INVALID"));
    assert_eq!(view(&h, &a, &b).await.body["status"], "pending");

    // A long message is cut, a blank one gets the standard words.
    let long = json!({"applied": false, "message": "x".repeat(500)});
    assert_eq!(h.outcome(&token, &b, long).await.status, 200);
    let error = view(&h, &a, &b).await.body["error"].clone();
    assert_eq!(error.as_str().unwrap().chars().count(), 200);
    h.outcome(&token, &b, json!({"applied": false, "message": "  "}))
        .await;
    assert_eq!(
        view(&h, &a, &b).await.body["error"],
        "The Hub could not set this up."
    );

    // A revoked Hub reports nothing.
    h.revoke_hubs(&b);
    let r = h.outcome(&token, &b, json!({"applied": true})).await;
    assert_eq!((r.status, r.code()), (401, "TOKEN_REVOKED"));
    assert_eq!(view(&h, &a, &b).await.body["status"], "failed");
}

#[tokio::test]
async fn choosing_it_after_the_hub_paired_queues_the_file_for_the_next_heartbeat() {
    let h = Harness::new().await;
    let a = h.cafe(PHONE_A, "Pixel Arena", "198.51.100.4").await;
    h.upgrade(&a.branch, T0 + 30 * DAY).await;
    let b = h.add_branch(&a.access, "Second Pune").await;
    let paired = h.pair(&a.access, &b, "198.51.100.6").await;
    assert!(
        paired.body["bootstrap"].get("template").is_none(),
        "no stored file yet"
    );
    let token = paired.body["token"].as_str().unwrap().to_owned();

    h.upload(&a, FILE).await;
    let on = choose(&h, &a, &b, json!({"enabled": true})).await;
    assert_eq!(on.status, 200, "{}", on.body);
    assert_eq!(on.body["status"], "retry");

    h.now.fetch_add(60_000, Ordering::SeqCst);
    let beat = h.heartbeat(&token).await;
    assert!(
        beat.body["setup_template"]["path"].is_string(),
        "{}",
        beat.body
    );

    // Choosing the same branch again does not queue it twice.
    let same = choose(
        &h,
        &a,
        &b,
        json!({"enabled": true, "source_branch_id": a.branch}),
    )
    .await;
    assert_eq!(same.body["status"], "pending");
}
