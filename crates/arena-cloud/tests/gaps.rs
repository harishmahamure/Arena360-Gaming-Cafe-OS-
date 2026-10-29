//! Upload gap detection by heartbeat (Story 8.3): the matrix of the spec against an in-memory
//! listing of the data bucket.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use arena_cloud::auth::AuthState;
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::gaps::GapCounts;
use arena_cloud::health::Reachability;
use arena_cloud::presign::Presigner;
use arena_cloud::store::{MemoryStore, ObjectStore, S3Config};
use arena_cloud::{Clock, app_with_auth, auth_state};
use arena_proto::Tier;
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

struct Harness {
    app: axum::Router,
    db: Arc<ControlDb>,
    st: Arc<AuthState>,
    now: Arc<AtomicI64>,
    keys: Arc<MemoryStore>,
    _dir: tempfile::TempDir,
}

struct Reply {
    status: u16,
    body: Value,
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

fn presigner(keys: &Arc<MemoryStore>) -> Presigner {
    Presigner::new(&S3Config {
        endpoint: "http://127.0.0.1:9000".into(),
        bucket: "arena-data".into(),
        access_key_id: "AKID-SECRET".into(),
        secret_access_key: "SAK-SECRET".into(),
    })
    .with_memory_listing(Arc::clone(keys))
}

/// A heartbeat body: the Hub says it uploaded `epoch` through `last`.
fn hb(epoch: &str, last: i64) -> Value {
    json!({"epoch": epoch, "version": "0.2.0", "schema_version": 12, "last_seq_uploaded": last})
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
        Self::build(true).await
    }

    async fn build(with_bucket: bool) -> Self {
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
        let keys = Arc::new(MemoryStore::new());
        if with_bucket {
            assert!(st.presigner.set(presigner(&keys)).is_ok());
        }
        let app = router(&st);
        Self {
            app,
            db,
            st,
            now,
            keys,
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
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    async fn beat(&self, token: &str, body: Value) -> Reply {
        let req = Request::builder()
            .method("POST")
            .uri("/v1/heartbeat")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(body.to_string()))
            .unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    /// A heartbeat at `epoch` that says the Hub uploaded through `last`.
    async fn beat_at(&self, c: &Cafe, epoch: &str, last: i64) -> Reply {
        self.beat(&c.token, hb(epoch, last)).await
    }

    /// Puts an empty object for every batch in `ranges`.
    async fn hold(&self, c: &Cafe, epoch: &str, ranges: &[(i64, i64)]) {
        for (first, last) in ranges {
            self.keys
                .put(
                    &format!(
                        "tenants/{}/branches/{}/changes/{epoch}/{first:012}-{last:012}.zst",
                        c.tenant, c.branch
                    ),
                    vec![1],
                )
                .await
                .unwrap();
        }
    }

    fn gaps(&self, c: &Cafe) -> GapCounts {
        self.st.gap_metrics.counts(&c.tenant, &c.branch)
    }

    async fn view(&self, c: &Cafe) -> arena_store::control::HubUploadView {
        self.db.hub_upload_view(&c.hub).await.unwrap()
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

    /// A paid branch with a Hub paired to it.
    async fn paid_cafe(&self) -> Cafe {
        let a = self.cafe(PHONE_A, "Pixel Arena", "198.51.100.4").await;
        self.upgrade(&a.branch, Tier::Basic, T0 + 30 * DAY).await;
        a
    }

    /// Lets the heartbeat rate limit (one per 5 s per Hub) pass.
    fn tick(&self) {
        self.now.fetch_add(10_000, Ordering::SeqCst);
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

const E: &str = "3:HUBA";

fn resend_of(r: &Reply) -> Option<(i64, i64)> {
    assert_eq!(r.status, 200, "{}", r.body);
    r.body.get("resend").map(|v| {
        (
            v["from"].as_i64().expect("from"),
            v["to"].as_i64().expect("to"),
        )
    })
}

#[tokio::test]
async fn a_heartbeat_that_agrees_with_the_cached_view_lists_nothing() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    h.hold(&a, E, &[(1, 10)]).await;
    assert_eq!(resend_of(&h.beat_at(&a, E, 10).await), None);
    let v = h.view(&a).await;
    assert_eq!((v.epoch_seen.as_deref(), v.seq_seen), (Some(E), Some(10)));

    // The bucket loses the batch, but the Hub's position is unchanged: no listing, no resend.
    h.keys
        .delete(&format!(
            "tenants/{}/branches/{}/changes/{E}/000000000001-000000000010.zst",
            a.tenant, a.branch
        ))
        .await
        .unwrap();
    h.tick();
    assert_eq!(resend_of(&h.beat_at(&a, E, 10).await), None);
    assert_eq!(h.gaps(&a), GapCounts::default());

    // A new position is checked again, and now the hole shows.
    h.tick();
    assert_eq!(resend_of(&h.beat_at(&a, E, 12).await), Some((1, 12)));
}

#[tokio::test]
async fn the_first_hole_is_asked_for_until_it_is_filled_and_counts_once() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    h.hold(&a, E, &[(1, 5000), (10_001, 12_000)]).await;
    assert_eq!(
        resend_of(&h.beat_at(&a, E, 12_000).await),
        Some((5001, 10_000))
    );
    let v = h.view(&a).await;
    assert_eq!(
        v.seq_seen, None,
        "the view is left so the next beat checks again"
    );
    assert_eq!(v.gaps_found, 1);
    assert_eq!(h.gaps(&a).found, 1);

    h.tick();
    assert_eq!(
        resend_of(&h.beat_at(&a, E, 12_000).await),
        Some((5001, 10_000))
    );
    assert_eq!(h.gaps(&a).found, 1, "the same run is not counted again");

    h.hold(&a, E, &[(5001, 10_000)]).await;
    h.tick();
    assert_eq!(resend_of(&h.beat_at(&a, E, 12_000).await), None);
    let v = h.view(&a).await;
    assert_eq!(
        (v.epoch_seen.as_deref(), v.seq_seen),
        (Some(E), Some(12_000))
    );
    assert_eq!(v.resend, None);
}

#[tokio::test]
async fn a_lost_last_batch_is_a_gap_at_the_tail() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    h.hold(&a, E, &[(1, 5000)]).await;
    assert_eq!(resend_of(&h.beat_at(&a, E, 7000).await), Some((5001, 7000)));
}

#[tokio::test]
async fn a_forked_epoch_starts_after_its_parent() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    h.hold(&a, E, &[(41, 100)]).await;
    let mut body = hb(E, 100);
    body["fork_seq"] = json!(40);
    assert_eq!(resend_of(&h.beat(&a.token, body).await), None);

    // Without the fork point the cloud would ask for rows the epoch never had.
    let other = "4:HUBA";
    h.hold(&a, other, &[(41, 100)]).await;
    h.tick();
    assert_eq!(resend_of(&h.beat_at(&a, other, 100).await), Some((1, 40)));
}

#[tokio::test]
async fn a_run_the_hub_no_longer_holds_is_recorded_counted_and_not_asked_for_again() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    h.hold(&a, E, &[(1, 5000), (10_001, 12_000)]).await;
    assert_eq!(
        resend_of(&h.beat_at(&a, E, 12_000).await),
        Some((5001, 10_000))
    );

    h.tick();
    let mut body = hb(E, 12_000);
    body["resend_unavailable"] = json!({"epoch": E, "from": 5001, "to": 10_000});
    assert_eq!(resend_of(&h.beat(&a.token, body).await), None);
    let v = h.view(&a).await;
    let u = v.unavailable.expect("recorded on the branch");
    assert_eq!((u.epoch.as_str(), u.from, u.to), (E, 5001, 10_000));
    assert_eq!(v.seq_seen, Some(12_000), "the listing stops");
    assert_eq!(
        h.gaps(&a),
        GapCounts {
            found: 1,
            unavailable: 1
        }
    );

    // The same report again is not counted twice, and a later hole is still found.
    h.tick();
    let mut body = hb(E, 12_000);
    body["resend_unavailable"] = json!({"epoch": E, "from": 5001, "to": 10_000});
    assert_eq!(resend_of(&h.beat(&a.token, body).await), None);
    assert_eq!(h.gaps(&a).unavailable, 1);
    h.tick();
    assert_eq!(
        resend_of(&h.beat_at(&a, E, 15_000).await),
        Some((12_001, 15_000))
    );
}

#[tokio::test]
async fn an_unavailable_report_for_another_epoch_or_a_bad_range_is_ignored() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    for bad in [
        json!({"epoch": "2:OLD", "from": 1, "to": 5}),
        json!({"epoch": E, "from": 9, "to": 5}),
        json!({"epoch": E, "from": 0, "to": 5}),
    ] {
        h.tick();
        let mut body = hb(E, 5);
        body["resend_unavailable"] = bad;
        assert_eq!(resend_of(&h.beat(&a.token, body).await), Some((1, 5)));
    }
    assert_eq!(h.view(&a).await.unavailable, None);
    assert_eq!(h.gaps(&a).unavailable, 0);
}

#[tokio::test]
async fn a_free_branch_is_not_checked_or_recorded() {
    let h = Harness::new().await;
    let a = h.cafe(PHONE_A, "Pixel Arena", "198.51.100.4").await;
    let mut body = hb(E, 5000);
    body["last_upload_at"] = json!(777);
    assert_eq!(resend_of(&h.beat(&a.token, body).await), None);
    assert_eq!(h.view(&a).await, Default::default());
    assert_eq!(h.gaps(&a), GapCounts::default());
}

#[tokio::test]
async fn the_hubs_own_progress_times_are_kept_on_the_view() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let mut body = hb(E, 0);
    body["last_upload_at"] = json!(777);
    body["last_summary_at"] = json!(888);
    body["last_rollup_date"] = json!("2026-10-29");
    assert_eq!(resend_of(&h.beat(&a.token, body).await), None);
    let v = h.view(&a).await;
    assert_eq!(
        (
            v.last_upload_at,
            v.last_summary_at,
            v.last_rollup_date.as_deref()
        ),
        (Some(777), Some(888), Some("2026-10-29"))
    );
    // A heartbeat without them keeps what is known.
    h.tick();
    assert_eq!(resend_of(&h.beat_at(&a, E, 0).await), None);
    assert_eq!(h.view(&a).await.last_upload_at, Some(777));
}

#[tokio::test]
async fn nonsense_positions_and_epochs_never_fail_the_heartbeat_or_list_anything() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    for (epoch, last) in [
        ("3:HUBA", -1),
        ("3:HUBA", 1_000_000_000_000_000),
        ("01JEPOCH", 5),
    ] {
        h.tick();
        assert_eq!(
            resend_of(&h.beat_at(&a, epoch, last).await),
            None,
            "{epoch} {last}"
        );
    }
    assert_eq!(h.view(&a).await.seq_seen, None);
}

#[tokio::test]
async fn without_a_data_bucket_the_heartbeat_still_answers_and_nothing_is_asked_for() {
    let h = Harness::build(false).await;
    let a = h.paid_cafe().await;
    assert_eq!(resend_of(&h.beat_at(&a, E, 9000).await), None);
    assert_eq!(h.view(&a).await.seq_seen, None);
}
