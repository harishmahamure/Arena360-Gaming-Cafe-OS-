//! `POST /v1/presign` (Story 8.1): the matrix of the spec, with a real PUT to MinIO when
//! `ARENA_TEST_S3_URL` is set.

#[path = "support/s3.rs"]
mod s3;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use arena_cloud::auth::AuthState;
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
use arena_cloud::presign::{Presigner, PutCounts};
use arena_cloud::store::S3Config;
use arena_cloud::{Clock, app_with_auth, auth_state};
use arena_proto::{LicenceClaims, Tier};
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
const HOUR: i64 = 60 * 60 * 1000;
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
    retry_after: Option<String>,
}

impl Reply {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or("")
    }
}

/// An owner's branch with a Hub paired to it.
struct Cafe {
    branch: String,
    tenant: String,
    hub: String,
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
    }
}

fn presigner() -> Presigner {
    Presigner::new(&S3Config {
        endpoint: "http://127.0.0.1:9000".into(),
        bucket: "arena-data".into(),
        access_key_id: "AKID-SECRET".into(),
        secret_access_key: "SAK-SECRET".into(),
    })
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
        assert!(st.presigner.set(presigner()).is_ok());
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

    async fn presign_with(&self, app: &axum::Router, token: &str, body: Value) -> Reply {
        let mut req = Request::builder()
            .method("POST")
            .uri("/v1/presign")
            .header("content-type", "application/json");
        if !token.is_empty() {
            req = req.header("authorization", format!("Bearer {token}"));
        }
        let req = req.body(Body::from(body.to_string())).unwrap();
        reply(app.clone().oneshot(req).await.unwrap()).await
    }

    async fn presign(&self, token: &str, body: Value) -> Reply {
        self.presign_with(&self.app, token, body).await
    }

    async fn beat(&self, token: &str, epoch: &str) -> Reply {
        let req = Request::builder()
            .method("POST")
            .uri("/v1/heartbeat")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(
                json!({"epoch": epoch, "version": "0.2.0", "schema_version": 12, "last_seq_uploaded": 0})
                    .to_string(),
            ))
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
        let token = r.body["token"].as_str().unwrap().to_owned();
        let hub = self
            .db
            .active_hub(&branch)
            .await
            .unwrap()
            .expect("a paired hub")
            .id;
        let tenant = self.db.branch(&branch).await.unwrap().unwrap().tenant_id;
        Cafe {
            branch,
            tenant,
            hub,
            token,
        }
    }

    /// A paid branch whose Hub has reported `fence` as its first epoch.
    async fn paid_cafe(&self, fence: Option<&str>) -> Cafe {
        let a = self.cafe(PHONE_A, "Pixel Arena", "198.51.100.4").await;
        self.upgrade(&a.branch, Tier::Basic, T0 + 30 * DAY).await;
        if let Some(epoch) = fence {
            assert_eq!(self.beat(&a.token, epoch).await.status, 200);
        }
        a
    }

    async fn upgrade(&self, branch: &str, tier: Tier, until: i64) {
        self.db
            .grant_comp(&CompGrant {
                branch_id: branch,
                tier,
                until,
                audit_id: &format!("audit-{branch}-{tier:?}-{until}"),
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

    fn counts(&self, c: &Cafe) -> PutCounts {
        self.st.put_metrics.counts(&c.tenant, &c.branch)
    }

    async fn audit_rows(&self) -> Vec<arena_store::control::AdminAuditRow> {
        self.db
            .admin_audit_rows()
            .await
            .unwrap()
            .into_iter()
            .filter(|r| r.action == "presign.forbidden")
            .collect()
    }
}

async fn reply(res: axum::response::Response) -> Reply {
    let status = res.status().as_u16();
    let retry_after = res
        .headers()
        .get(header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    Reply {
        status,
        body,
        retry_after,
    }
}

fn url_of<'a>(r: &'a Reply, field: &str) -> &'a str {
    r.body[field]
        .as_str()
        .unwrap_or_else(|| panic!("no {field} in {}", r.body))
}

/// The object key a link is for: its path without the bucket.
fn key_of(url: &str) -> String {
    let path = url.split('?').next().unwrap();
    let path = path
        .strip_prefix("http://127.0.0.1:9000/arena-data/")
        .unwrap();
    path.replace("%3A", ":")
}

fn assert_valid_600(url: &str) {
    assert!(url.contains("X-Amz-Expires=600"), "{url}");
    assert!(url.contains("X-Amz-Signature="), "{url}");
    assert!(!url.contains("SAK-SECRET"), "{url}");
}

#[tokio::test]
async fn a_paid_hub_gets_a_batch_link_and_a_summary_link_under_its_own_prefix() {
    let h = Harness::new().await;
    let a = h.paid_cafe(Some("3:HUBA")).await;
    let prefix = format!("tenants/{}/branches/{}/", a.tenant, a.branch);

    let r = h
        .presign(
            &a.token,
            json!({"epoch": "3:HUBA", "batch": {"first": 1, "last": 250}, "summary": true}),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let keys: Vec<&str> = r
        .body
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys.len(), 2, "{}", r.body);
    let (batch, summary) = (url_of(&r, "batch_url"), url_of(&r, "summary_url"));
    assert_eq!(
        key_of(batch),
        format!("{prefix}changes/3:HUBA/000000000001-000000000250.zst")
    );
    assert_eq!(key_of(summary), format!("{prefix}state/latest.json"));
    assert_valid_600(batch);
    assert_valid_600(summary);
    assert_eq!(
        h.counts(&a),
        PutCounts {
            batch: 1,
            summary: 1,
            ..PutCounts::default()
        }
    );

    // A batch alone.
    let r = h
        .presign(
            &a.token,
            json!({"epoch": "3:HUBA", "batch": {"first": 251, "last": 251}}),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(r.body.get("summary_url").is_none() && r.body.get("url").is_none());
    assert_eq!(h.counts(&a).batch, 2);
}

#[tokio::test]
async fn tenant_and_branch_in_the_body_are_ignored() {
    let h = Harness::new().await;
    let a = h.paid_cafe(None).await;
    let b = h.cafe(PHONE_B, "Other Arena", "198.51.100.5").await;
    h.upgrade(&b.branch, Tier::Basic, T0 + 30 * DAY).await;
    let r = h
        .presign(
            &a.token,
            json!({
                "epoch": "1:HUBA",
                "summary": true,
                "tenant_id": b.tenant,
                "branch_id": b.branch,
                "tenant": b.tenant,
                "branch": b.branch,
            }),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let key = key_of(url_of(&r, "summary_url"));
    assert_eq!(
        key,
        format!(
            "tenants/{}/branches/{}/state/latest.json",
            a.tenant, a.branch
        )
    );
    assert!(!key.contains(&b.branch) && !key.contains(&b.tenant));
    assert_eq!(h.counts(&b), PutCounts::default());
}

#[tokio::test]
async fn rollups_and_snapshots_are_dated_and_other_kinds_are_unsupported() {
    let h = Harness::new().await;
    let a = h.paid_cafe(Some("2:HUBA")).await;
    let prefix = format!("tenants/{}/branches/{}/", a.tenant, a.branch);
    let r = h
        .presign(
            &a.token,
            json!({"epoch": "2:HUBA", "kind": "rollup", "date": "2026-10-29"}),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body.as_object().unwrap().len(), 1);
    assert_eq!(
        key_of(url_of(&r, "url")),
        format!("{prefix}rollups/2026-10-29.json")
    );
    assert_valid_600(url_of(&r, "url"));
    let r = h
        .presign(
            &a.token,
            json!({"epoch": "2:HUBA", "kind": "snapshot", "date": "2026-02-28"}),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        key_of(url_of(&r, "url")),
        format!("{prefix}snapshots/2026-02-28.db.zst")
    );
    assert_eq!((h.counts(&a).rollup, h.counts(&a).snapshot), (1, 1));

    for kind in ["state", "changes", "", "ROLLUP"] {
        let r = h
            .presign(
                &a.token,
                json!({"epoch": "2:HUBA", "kind": kind, "date": "2026-10-29"}),
            )
            .await;
        assert_eq!((r.status, r.code()), (400, "UNSUPPORTED"), "{kind:?}");
    }
    assert_eq!(h.counts(&a).issued(), 2, "nothing more was issued");
}

#[tokio::test]
async fn a_key_component_with_path_characters_is_forbidden_audited_and_never_issued() {
    let h = Harness::new().await;
    let a = h.paid_cafe(Some("2:HUBA")).await;
    let bad = [
        json!({"epoch": "../2:HUBA", "summary": true}),
        json!({"epoch": "2:HUBA/../../x", "batch": {"first": 1, "last": 2}}),
        json!({"epoch": "2:HU\\BA", "summary": true}),
        json!({"epoch": "2:..", "summary": true}),
        json!({"epoch": "2:HUBA", "kind": "rollup", "date": "../../b/rollups/2026-10-29"}),
        json!({"epoch": "2:HUBA", "kind": "snapshot", "date": "2026-10-29/.."}),
        json!({"epoch": "2:HUBA", "kind": "snapshot", "date": "2026\\10\\29"}),
    ];
    for (i, body) in bad.iter().enumerate() {
        let r = h.presign(&a.token, body.clone()).await;
        assert_eq!((r.status, r.code()), (403, "FORBIDDEN"), "{body}");
        let rows = h.audit_rows().await;
        assert_eq!(rows.len(), i + 1, "{body}");
        let row = rows.last().unwrap();
        assert_eq!(
            (row.actor_kind.as_str(), row.actor_id.as_str()),
            ("system", a.hub.as_str())
        );
        assert_eq!(
            (row.target_kind.as_str(), row.target_id.as_str()),
            ("branch", a.branch.as_str())
        );
        let summary: Value = serde_json::from_str(&row.summary).unwrap();
        assert_eq!(summary["hub_id"], a.hub.as_str());
        assert!(
            !row.summary.contains("..") && !row.summary.contains('\\'),
            "the raw value is not stored: {}",
            row.summary
        );
    }
    assert_eq!(h.counts(&a), PutCounts::default());
}

#[tokio::test]
async fn a_stale_epoch_still_gets_its_batch_link_but_nothing_else() {
    let h = Harness::new().await;
    let a = h.paid_cafe(Some("10:HUBA")).await;

    // 9 < 10 numerically (not as text).
    let both = json!({"epoch": "9:HUBA", "batch": {"first": 1, "last": 5}, "summary": true});
    let r = h.presign(&a.token, both).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(r.body["batch_url"].is_string());
    assert!(r.body.get("summary_url").is_none());
    assert_eq!(r.body["summary_fenced"], true);
    assert_eq!(h.counts(&a).summary, 0);

    let r = h
        .presign(
            &a.token,
            json!({"epoch": "9:HUBA", "batch": {"first": 6, "last": 6}}),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(r.body.get("summary_fenced").is_none());

    let issued = h.counts(&a).issued();
    for body in [
        json!({"epoch": "9:HUBA", "summary": true}),
        json!({"epoch": "9:HUBA", "kind": "rollup", "date": "2026-10-29"}),
        json!({"epoch": "9:HUBA", "kind": "snapshot", "date": "2026-10-29"}),
        json!({"epoch": "10:HUB", "summary": true}),
    ] {
        let r = h.presign(&a.token, body.clone()).await;
        assert_eq!((r.status, r.code()), (409, "FENCED"), "{body}");
    }
    assert_eq!(h.counts(&a).issued(), issued);

    // The fence itself and anything above it pass.
    for epoch in ["10:HUBA", "10:HUBB", "11:A", "100:A"] {
        let r = h
            .presign(&a.token, json!({"epoch": epoch, "summary": true}))
            .await;
        assert_eq!(r.status, 200, "{epoch}: {}", r.body);
    }
}

#[tokio::test]
async fn a_free_branch_is_refused_before_anything_else() {
    let h = Harness::new().await;
    let a = h.cafe(PHONE_A, "Pixel Arena", "198.51.100.4").await;
    for body in [
        json!({"epoch": "1:HUBA", "summary": true}),
        json!({}),
        json!({"epoch": "../x", "summary": true}),
        json!({"epoch": "1:HUBA", "kind": "nope"}),
    ] {
        let r = h.presign(&a.token, body.clone()).await;
        assert_eq!(
            (r.status, r.code()),
            (403, "FEATURE_NOT_ENTITLED"),
            "{body}"
        );
    }
    assert!(h.audit_rows().await.is_empty(), "no audit for a Free Hub");
    assert_eq!(h.counts(&a), PutCounts::default());

    // A paid plan that has run out is Free again.
    h.upgrade(&a.branch, Tier::Basic, T0 + DAY).await;
    let ok = json!({"epoch": "1:HUBA", "summary": true});
    assert_eq!(h.presign(&a.token, ok.clone()).await.status, 200);
    h.now.fetch_add(2 * DAY, Ordering::SeqCst);
    let r = h.presign(&a.token, ok).await;
    assert_eq!((r.status, r.code()), (403, "FEATURE_NOT_ENTITLED"));
}

#[tokio::test]
async fn a_branch_over_its_hourly_limit_is_refused_with_retry_after_and_counted() {
    let h = Harness::new().await;
    let a = h.paid_cafe(Some("1:HUBA")).await;
    let interval = LicenceClaims::for_tier("t", "b", Tier::Basic, 0).upload_interval_s;
    let limit = 4 * 3600 / u64::from(interval).max(30);
    assert_eq!(limit, 48);
    let body = json!({"epoch": "1:HUBA", "summary": true});
    for i in 0..limit {
        let r = h.presign(&a.token, body.clone()).await;
        assert_eq!(r.status, 200, "call {i}: {}", r.body);
    }
    // A call that asks for two links is still one hit; refused calls issue nothing.
    let r = h.presign(&a.token, body.clone()).await;
    assert_eq!((r.status, r.code()), (429, "RATE_LIMITED"));
    let wait: i64 = r.retry_after.expect("Retry-After").parse().unwrap();
    assert!((1..=3601).contains(&wait), "{wait}");
    assert_eq!(h.counts(&a).refused, 1);
    assert_eq!(h.counts(&a).summary, limit);
    let r = h
        .presign(
            &a.token,
            json!({"epoch": "1:HUBA", "batch": {"first": 1, "last": 2}, "summary": true}),
        )
        .await;
    assert_eq!(r.status, 429);
    assert_eq!(h.counts(&a).refused, 2);
    assert_eq!(h.counts(&a).batch, 0);

    // Calls that fail validation or are fenced use no hit; another branch has its own.
    let b = h.cafe(PHONE_B, "Other Arena", "198.51.100.5").await;
    h.upgrade(&b.branch, Tier::Basic, T0 + 30 * DAY).await;
    assert_eq!(h.presign(&b.token, body.clone()).await.status, 200);

    // An hour later the window has emptied.
    h.now.fetch_add(HOUR + 2, Ordering::SeqCst);
    assert_eq!(h.presign(&a.token, body).await.status, 200);
}

#[tokio::test]
async fn bad_shapes_are_validation_failures_that_issue_and_count_nothing() {
    let h = Harness::new().await;
    let a = h.paid_cafe(Some("1:HUBA")).await;
    for (body, field) in [
        (json!({"epoch": "1:HUBA"}), "batch"),
        (json!({"epoch": "1:HUBA", "summary": false}), "batch"),
        (json!({}), "epoch"),
        (json!({"epoch": "HUBA", "summary": true}), "epoch"),
        (
            json!({"epoch": "1:HUBA", "batch": {"first": 9, "last": 8}}),
            "batch",
        ),
        (
            json!({"epoch": "1:HUBA", "batch": {"first": -1, "last": 8}}),
            "batch",
        ),
        (
            json!({"epoch": "1:HUBA", "batch": {"first": 1, "last": 1_000_000_000_000_i64}}),
            "batch",
        ),
        (json!({"epoch": "1:HUBA", "kind": "rollup"}), "date"),
        (
            json!({"epoch": "1:HUBA", "kind": "rollup", "date": "2026-13-40"}),
            "date",
        ),
        (
            json!({"epoch": "1:HUBA", "kind": "snapshot", "date": "29-10-2026"}),
            "date",
        ),
        (
            json!({"epoch": "1:HUBA", "kind": "rollup", "date": "2026-10-29", "summary": true}),
            "kind",
        ),
    ] {
        let r = h.presign(&a.token, body.clone()).await;
        assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"), "{body}");
        assert_eq!(r.body["field"], field, "{body}");
    }
    assert_eq!(h.counts(&a), PutCounts::default());
    assert!(h.audit_rows().await.is_empty());
    // The largest batch fits its twelve digits.
    let r = h
        .presign(
            &a.token,
            json!({"epoch": "1:HUBA", "batch": {"first": 999_999_999_998_i64, "last": 999_999_999_999_i64}}),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(key_of(url_of(&r, "batch_url")).ends_with("999999999998-999999999999.zst"));
}

#[tokio::test]
async fn only_a_valid_hub_token_gets_links() {
    let h = Harness::new().await;
    let a = h.paid_cafe(None).await;
    let body = json!({"epoch": "1:HUBA", "summary": true});
    let r = h.presign("", body.clone()).await;
    assert_eq!((r.status, r.code()), (401, "TOKEN_INVALID"));
    let r = h.presign("arena_hub_nope.x", body.clone()).await;
    assert_eq!((r.status, r.code()), (401, "TOKEN_INVALID"));
    h.revoke_hubs(&a.branch);
    let r = h.presign(&a.token, body).await;
    assert_eq!((r.status, r.code()), (401, "TOKEN_REVOKED"));
    assert_eq!(h.counts(&a), PutCounts::default());
}

#[tokio::test]
async fn without_a_data_bucket_the_cloud_says_it_is_unavailable() {
    let h = Harness::new().await;
    let a = h.paid_cafe(None).await;
    // The same database behind a state that never got a presigner.
    let bare = auth_state(Arc::clone(&h.db), &config(), Arc::new(|| T0)).unwrap();
    assert!(bare.presigner.get().is_none());
    let r = h
        .presign_with(
            &router(&bare),
            &a.token,
            json!({"epoch": "1:HUBA", "summary": true}),
        )
        .await;
    assert_eq!((r.status, r.code()), (503, "INTERNAL"));
}

#[tokio::test]
async fn the_player_key_backup_has_one_fixed_key_on_paid_tiers_and_follows_the_fence() {
    let h = Harness::new().await;
    let a = h.paid_cafe(Some("5:HUBA")).await;
    let prefix = format!("tenants/{}/branches/{}/", a.tenant, a.branch);
    let r = h
        .presign(&a.token, json!({"epoch": "5:HUBA", "kind": "keys"}))
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body.as_object().unwrap().len(), 1);
    assert_eq!(
        key_of(url_of(&r, "url")),
        format!("{prefix}keys/player-keys.bin")
    );
    assert_valid_600(url_of(&r, "url"));
    assert_eq!(h.counts(&a).keys, 1);

    // No date for it, and a Hub below the fence gets nothing.
    let r = h
        .presign(
            &a.token,
            json!({"epoch": "5:HUBA", "kind": "keys", "date": "2026-10-29"}),
        )
        .await;
    assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));
    let r = h
        .presign(&a.token, json!({"epoch": "4:HUBA", "kind": "keys"}))
        .await;
    assert_eq!((r.status, r.code()), (409, "FENCED"));
    assert_eq!(h.counts(&a).issued(), 1);
}

#[tokio::test]
async fn a_free_branch_gets_no_key_backup_link() {
    let h = Harness::new().await;
    let a = h.cafe(PHONE_A, "Pixel Arena", "198.51.100.4").await;
    let r = h
        .presign(&a.token, json!({"epoch": "1:HUBA", "kind": "keys"}))
        .await;
    assert_eq!((r.status, r.code()), (403, "FEATURE_NOT_ENTITLED"));
    assert_eq!(h.counts(&a), PutCounts::default());
}

/// A stand-in for an age file: what the cloud checks is its first line.
fn wrapped(extra: &str) -> String {
    use base64::Engine as _;
    let mut file = arena_proto::AGE_MAGIC.to_vec();
    file.extend_from_slice(extra.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(file)
}

impl Harness {
    async fn escrow(&self, token: &str, body: Value) -> Reply {
        let mut req = Request::builder()
            .method("POST")
            .uri("/v1/kek/escrow")
            .header("content-type", "application/json");
        if !token.is_empty() {
            req = req.header("authorization", format!("Bearer {token}"));
        }
        let req = req.body(Body::from(body.to_string())).unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    fn escrowed(&self) -> Vec<(String, String, String, i64)> {
        let conn = rusqlite::Connection::open(self.dir.path().join("gz_control.db")).unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT branch_id, hub_instance_id, wrapped_kek, created_at FROM kek_escrow \
                 ORDER BY branch_id",
            )
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    async fn escrow_audit(&self) -> Vec<arena_store::control::AdminAuditRow> {
        self.db
            .admin_audit_rows()
            .await
            .unwrap()
            .into_iter()
            .filter(|r| r.action == "kek.escrow.write")
            .collect()
    }
}

#[tokio::test]
async fn a_free_hub_escrows_its_branch_key_and_every_write_is_audited() {
    let h = Harness::new().await;
    let a = h.cafe(PHONE_A, "Pixel Arena", "198.51.100.4").await;
    let first = wrapped("-> X25519 first\n");
    let r = h.escrow(&a.token, json!({"wrapped_kek": first})).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body, json!({"stored": true}), "nothing comes back");
    assert_eq!(
        h.escrowed(),
        vec![(a.branch.clone(), "01HUBINSTANCE".into(), first.clone(), T0)]
    );

    // The same Hub escrowing a new key replaces its row; the audit keeps both writes.
    h.now.fetch_add(HOUR, Ordering::SeqCst);
    let second = wrapped("-> X25519 second\n");
    let r = h.escrow(&a.token, json!({"wrapped_kek": second})).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        h.escrowed(),
        vec![(
            a.branch.clone(),
            "01HUBINSTANCE".into(),
            second.clone(),
            T0 + HOUR
        )]
    );
    let audit = h.escrow_audit().await;
    assert_eq!(audit.len(), 2);
    for row in &audit {
        assert_eq!(
            (row.actor_kind.as_str(), row.actor_id.as_str()),
            ("system", a.hub.as_str())
        );
        assert_eq!(
            (row.target_kind.as_str(), row.target_id.as_str()),
            ("branch", a.branch.as_str())
        );
        assert!(
            !row.summary.contains(&first) && !row.summary.contains(&second),
            "the audit never holds the key: {}",
            row.summary
        );
    }
    assert_eq!(h.db.kek_escrow_count(&a.branch).await.unwrap(), 1);
}

#[tokio::test]
async fn only_a_wrapped_key_from_a_valid_hub_is_stored() {
    use base64::Engine as _;
    let h = Harness::new().await;
    let a = h.cafe(PHONE_A, "Pixel Arena", "198.51.100.4").await;
    let bare = base64::engine::general_purpose::STANDARD.encode([7u8; 32]);
    for body in [
        json!({"wrapped_kek": bare}),
        json!({"wrapped_kek": ""}),
        json!({"wrapped_kek": "not base64!"}),
        json!({"wrapped_kek": wrapped(&"x".repeat(2000))}),
    ] {
        let r = h.escrow(&a.token, body.clone()).await;
        assert_eq!(
            (r.status, r.code()),
            (422, "VALIDATION_FAILED"),
            "{body}: {}",
            r.body
        );
    }
    assert!(h.escrowed().is_empty());
    assert!(h.escrow_audit().await.is_empty());

    let ok = json!({"wrapped_kek": wrapped("-> X25519 k\n")});
    let r = h.escrow("", ok.clone()).await;
    assert_eq!((r.status, r.code()), (401, "TOKEN_INVALID"));
    h.revoke_hubs(&a.branch);
    let r = h.escrow(&a.token, ok).await;
    assert_eq!((r.status, r.code()), (401, "TOKEN_REVOKED"));
    assert!(h.escrowed().is_empty());
}

#[tokio::test]
async fn escrow_calls_are_rate_limited_per_branch() {
    let h = Harness::new().await;
    let a = h.cafe(PHONE_A, "Pixel Arena", "198.51.100.4").await;
    let ok = json!({"wrapped_kek": wrapped("-> X25519 k\n")});
    for _ in 0..12 {
        assert_eq!(h.escrow(&a.token, ok.clone()).await.status, 200);
    }
    let r = h.escrow(&a.token, ok).await;
    assert_eq!((r.status, r.code()), (429, "RATE_LIMITED"));
    assert!(r.retry_after.is_some());
}

/// A PUT of `body` to a presigned `http://host:port/path?query` URL: the status code.
fn put(url: &str, body: &[u8]) -> u16 {
    let rest = url.strip_prefix("http://").expect("an http URL");
    let (host, target) = rest.split_once('/').unwrap();
    let mut conn = TcpStream::connect(host).unwrap();
    write!(
        conn,
        "PUT /{target} HTTP/1.1\r\nHost: {host}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .unwrap();
    conn.write_all(body).unwrap();
    let mut raw = Vec::new();
    conn.read_to_end(&mut raw).unwrap();
    let head = String::from_utf8_lossy(&raw);
    head.split_whitespace().nth(1).unwrap().parse().unwrap()
}

#[tokio::test]
async fn a_put_to_each_link_succeeds_on_minio() {
    let Some(bucket) = s3::bucket().await else {
        return;
    };
    let h = Harness::new().await;
    let st = auth_state(Arc::clone(&h.db), &config(), {
        let tick = Arc::clone(&h.now);
        Arc::new(move || tick.load(Ordering::SeqCst))
    })
    .unwrap();
    assert!(st.presigner.set(Presigner::new(&bucket.cfg)).is_ok());
    let app = router(&st);
    let a = h.paid_cafe(Some("4:HUBA")).await;
    let prefix = format!("tenants/{}/branches/{}/", a.tenant, a.branch);

    let r = h
        .presign_with(
            &app,
            &a.token,
            json!({"epoch": "4:HUBA", "batch": {"first": 1, "last": 2}, "summary": true}),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(put(url_of(&r, "batch_url"), b"batch bytes"), 200);
    assert_eq!(put(url_of(&r, "summary_url"), b"{\"as_of\":1}"), 200);
    let r = h
        .presign_with(
            &app,
            &a.token,
            json!({"epoch": "4:HUBA", "kind": "rollup", "date": "2026-10-29"}),
        )
        .await;
    assert_eq!(put(url_of(&r, "url"), b"{}"), 200);
    let r = h
        .presign_with(
            &app,
            &a.token,
            json!({"epoch": "4:HUBA", "kind": "snapshot", "date": "2026-10-29"}),
        )
        .await;
    assert_eq!(put(url_of(&r, "url"), b"snap"), 200);

    use arena_cloud::store::ObjectStore;
    let mut keys = bucket.store.list(&prefix).await.unwrap();
    keys.sort();
    assert_eq!(
        keys,
        [
            format!("{prefix}changes/4:HUBA/000000000001-000000000002.zst"),
            format!("{prefix}rollups/2026-10-29.json"),
            format!("{prefix}snapshots/2026-10-29.db.zst"),
            format!("{prefix}state/latest.json"),
        ]
    );
    assert_eq!(
        bucket
            .store
            .get(&format!("{prefix}state/latest.json"))
            .await
            .unwrap(),
        b"{\"as_of\":1}"
    );

    // A link for one key does not write another.
    let r = h
        .presign_with(&app, &a.token, json!({"epoch": "4:HUBA", "summary": true}))
        .await;
    let url = url_of(&r, "summary_url").replace("latest.json", "other.json");
    assert_eq!(put(&url, b"x"), 403);
}
