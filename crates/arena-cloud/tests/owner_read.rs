//! The owner read API (Story 8.9): the matrix of the spec against an in-memory data bucket.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use arena_cloud::auth::AuthState;
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
use arena_cloud::presign::Presigner;
use arena_cloud::store::{MemoryStore, ObjectStore, S3Config, StoreCalls};
use arena_cloud::{Clock, app_with_auth, auth_state};
use arena_proto::{
    BranchSummary, RollupAmount, RollupByMethod, RollupByType, RollupCredit, RollupRevenue,
    RollupSessions, RollupStock, RollupV1, SUMMARY_SCHEMA, SummaryCatalogue, Tier,
};
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
    keys: Arc<MemoryStore>,
    _dir: tempfile::TempDir,
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
    token: String,
    access: String,
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
        let tenant = self.db.branch(&branch).await.unwrap().unwrap().tenant_id;
        Cafe {
            branch,
            tenant,
            token,
            access,
        }
    }

    /// A paid branch with a Hub paired to it.
    async fn paid_cafe(&self) -> Cafe {
        let a = self.cafe(PHONE_A, "Pixel Arena", "198.51.100.4").await;
        self.upgrade(&a.branch, Tier::Basic, T0 + 30 * DAY).await;
        a
    }

    /// Moves the cloud's clock on.
    fn advance(&self, ms: i64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }

    async fn get(&self, c: &Cafe, path: &str) -> Reply {
        self.call("GET", path, None, &c.access).await
    }

    /// The Hub checks in now (sets its `last_seen_at`).
    async fn beat(&self, c: &Cafe) {
        let req = Request::builder()
            .method("POST")
            .uri("/v1/heartbeat")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", c.token))
            .body(Body::from(
                json!({"epoch": "3:HUBA", "version": "0.2.0", "schema_version": 12, "last_seq_uploaded": 0})
                    .to_string(),
            ))
            .unwrap();
        let r = reply(self.app.clone().oneshot(req).await.unwrap()).await;
        assert_eq!(r.status, 200, "{}", r.body);
    }

    fn key(c: &Cafe, rel: &str) -> String {
        format!("tenants/{}/branches/{}/{rel}", c.tenant, c.branch)
    }

    async fn put_summary(&self, c: &Cafe, as_of: i64, revenue: i64) {
        let s = BranchSummary {
            schema: SUMMARY_SCHEMA,
            branch_id: c.branch.clone(),
            as_of,
            business_day: "2026-10-30".into(),
            open: true,
            sessions_live: 3,
            revenue_so_far_minor: revenue,
            currency: "INR".into(),
            cash_in_drawer_minor: 5_000,
            shifts: vec![],
            staff_on_shift: vec!["Asha".into()],
            attention: vec![],
            catalogue: SummaryCatalogue::default(),
        };
        self.keys
            .put(
                &Self::key(c, "state/latest.json"),
                serde_json::to_vec(&s).unwrap(),
            )
            .await
            .unwrap();
    }

    async fn put_rollup(&self, c: &Cafe, date: &str, schema: u32, total: i64) {
        self.put_rollup_in(c, date, schema, total, "INR").await;
    }

    async fn put_rollup_in(&self, c: &Cafe, date: &str, schema: u32, total: i64, currency: &str) {
        let r = RollupV1 {
            schema,
            branch_id: c.branch.clone(),
            business_day: date.into(),
            currency: currency.into(),
            revenue: RollupRevenue {
                total,
                by_type: RollupByType {
                    play: total,
                    ..RollupByType::default()
                },
                by_method: RollupByMethod {
                    cash: total,
                    ..RollupByMethod::default()
                },
            },
            refunds: RollupAmount {
                count: 1,
                amount: 10,
            },
            discounts: RollupAmount::default(),
            voids: RollupAmount::default(),
            sessions: RollupSessions {
                count: 2,
                minutes: 90,
            },
            shifts: vec![],
            stock: RollupStock::default(),
            credit: RollupCredit::default(),
            play_hours: vec![],
        };
        self.keys
            .put(
                &Self::key(c, &format!("rollups/{date}.json")),
                serde_json::to_vec(&r).unwrap(),
            )
            .await
            .unwrap();
    }

    fn calls(&self) -> StoreCalls {
        self.keys.calls()
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

fn live(c: &Cafe) -> String {
    format!("/v1/branches/{}/live", c.branch)
}

fn summary(c: &Cafe, query: &str) -> String {
    format!("/v1/branches/{}/summary?{query}", c.branch)
}

#[tokio::test]
async fn live_returns_the_summary_with_as_of_and_the_heartbeat_state() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    h.beat(&a).await;
    h.put_summary(&a, T0 - 5_000, 125_000).await;

    let r = h.get(&a, &live(&a)).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["branch_id"], a.branch.as_str());
    assert_eq!(r.body["as_of"], T0 - 5_000);
    assert_eq!(r.body["branch_online"], true);
    assert_eq!(r.body["last_heartbeat_at"], T0);
    assert_eq!(r.body["summary"]["revenue_so_far_minor"], 125_000);
    assert_eq!(r.body["summary"]["sessions_live"], 3);
    assert_eq!(r.body["currency"], "INR");

    // No heartbeat for three minutes and a second: offline, with the last time it was seen.
    h.advance(181_000);
    let r = h.get(&a, &live(&a)).await;
    assert_eq!(r.body["branch_online"], false);
    assert_eq!(r.body["last_heartbeat_at"], T0);
}

#[tokio::test]
async fn a_branch_with_no_summary_yet_answers_a_null_as_of() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let r = h.get(&a, &live(&a)).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(r.body["as_of"].is_null(), "{}", r.body);
    assert!(r.body["summary"].is_null());
    assert_eq!(r.body["branch_online"], false);
    assert!(r.body["last_heartbeat_at"].is_null());
}

#[tokio::test]
async fn live_is_served_from_memory_for_30_seconds_then_checked_against_the_etag() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    h.put_summary(&a, T0, 100).await;

    assert_eq!(h.get(&a, &live(&a)).await.body["as_of"], T0);
    assert_eq!(h.get(&a, &live(&a)).await.body["as_of"], T0);
    h.advance(29_000);
    assert_eq!(h.get(&a, &live(&a)).await.body["as_of"], T0);
    let c = h.calls();
    assert_eq!((c.heads, c.reads), (1, 1), "one check, one download");

    // Past 30 s the ETag is asked; an unchanged file is not downloaded again.
    h.advance(2_000);
    assert_eq!(h.get(&a, &live(&a)).await.body["as_of"], T0);
    let c = h.calls();
    assert_eq!((c.heads, c.reads), (2, 1));

    // A new file has a new ETag: downloaded once, then served from memory.
    h.put_summary(&a, T0 + 60_000, 900).await;
    h.advance(31_000);
    let r = h.get(&a, &live(&a)).await;
    assert_eq!(r.body["as_of"], T0 + 60_000);
    assert_eq!(r.body["summary"]["revenue_so_far_minor"], 900);
    let r = h.get(&a, &live(&a)).await;
    assert_eq!(r.body["as_of"], T0 + 60_000);
    let c = h.calls();
    assert_eq!((c.heads, c.reads), (3, 2));
}

#[tokio::test]
async fn a_range_is_summed_per_period_and_lists_the_missing_days() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    // Mon 26 .. Sun 1 Nov: the 28th is absent, the 29th is at a schema the cloud does not read.
    for (d, total) in [
        ("2026-10-26", 1_000),
        ("2026-10-27", 2_000),
        ("2026-10-30", 4_000),
        ("2026-10-31", 8_000),
        ("2026-11-01", 16_000),
    ] {
        h.put_rollup(&a, d, 1, total).await;
    }
    h.put_rollup(&a, "2026-10-29", 7, 999_999).await;

    let r = h
        .get(
            &a,
            &summary(&a, "from=2026-10-26&to=2026-11-01&group_by=day"),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["branch_id"], a.branch.as_str());
    assert_eq!(r.body["totals"]["revenue"]["total"], 31_000);
    assert_eq!(r.body["totals"]["revenue"]["by_type"]["play"], 31_000);
    assert_eq!(r.body["totals"]["revenue"]["by_method"]["cash"], 31_000);
    assert_eq!(r.body["totals"]["days"], 5);
    assert_eq!(r.body["totals"]["refunds"]["amount"], 50);
    assert_eq!(r.body["totals"]["sessions"]["minutes"], 450);
    assert_eq!(r.body["missing_days"], json!(["2026-10-28", "2026-10-29"]));
    let rows = r.body["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 7);
    assert_eq!(rows[0]["period"], "2026-10-26");
    assert_eq!(rows[0]["totals"]["revenue"]["total"], 1_000);
    assert_eq!(rows[2]["totals"]["days"], 0, "a missing day is a zero row");
    assert!(
        h.st.rollup_metrics
            .old_days(&a.tenant, &a.branch)
            .contains(&"2026-10-29".to_owned())
    );

    let r = h
        .get(
            &a,
            &summary(&a, "from=2026-10-26&to=2026-11-01&group_by=week"),
        )
        .await;
    let rows = r.body["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["period"], "2026-10-26");
    assert_eq!(rows[0]["totals"]["revenue"]["total"], 31_000);

    let r = h
        .get(
            &a,
            &summary(&a, "from=2026-10-26&to=2026-11-01&group_by=month"),
        )
        .await;
    let rows: Vec<(&str, i64)> = r.body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| {
            (
                x["period"].as_str().unwrap(),
                x["totals"]["revenue"]["total"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(rows, [("2026-10", 15_000), ("2026-11", 16_000)]);

    // group_by defaults to day.
    let r = h
        .get(&a, &summary(&a, "from=2026-10-26&to=2026-10-27"))
        .await;
    assert_eq!(r.body["group_by"], "day");
    assert_eq!(r.body["rows"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn a_regenerated_day_reads_again_and_a_day_in_another_currency_counts_as_missing() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    h.put_rollup(&a, "2026-10-26", 1, 100).await;
    h.put_rollup(&a, "2026-10-27", 0, 100).await;
    h.put_rollup_in(&a, "2026-10-28", 1, 500, "USD").await;
    let q = summary(&a, "from=2026-10-26&to=2026-10-28");
    let r = h.get(&a, &q).await;
    assert_eq!(r.body["totals"]["revenue"]["total"], 100);
    assert_eq!(r.body["missing_days"], json!(["2026-10-27", "2026-10-28"]));

    // The Hub regenerated the 27th at the current schema.
    h.put_rollup(&a, "2026-10-27", 1, 250).await;
    h.advance(31_000);
    let r = h.get(&a, &q).await;
    assert_eq!(r.body["totals"]["revenue"]["total"], 350);
    assert_eq!(r.body["missing_days"], json!(["2026-10-28"]));
    assert!(
        h.st.rollup_metrics
            .old_days(&a.tenant, &a.branch)
            .is_empty()
    );
}

#[tokio::test]
async fn month_files_are_cached_per_branch_and_only_changed_files_are_downloaded() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    for d in ["2026-10-26", "2026-10-27", "2026-11-02"] {
        h.put_rollup(&a, d, 1, 100).await;
    }
    let q = summary(&a, "from=2026-10-26&to=2026-11-02");
    assert_eq!(h.get(&a, &q).await.status, 200);
    let c = h.calls();
    assert_eq!((c.lists, c.reads), (2, 3), "one listing per month");

    h.advance(10_000);
    assert_eq!(h.get(&a, &q).await.status, 200);
    assert_eq!(h.calls(), c, "served from memory inside 30 s");

    h.advance(25_000);
    assert_eq!(h.get(&a, &q).await.body["totals"]["revenue"]["total"], 300);
    let c2 = h.calls();
    assert_eq!(
        (c2.lists, c2.reads),
        (4, 3),
        "re-listed, nothing downloaded"
    );

    h.put_rollup(&a, "2026-10-27", 1, 900).await;
    h.advance(31_000);
    assert_eq!(
        h.get(&a, &q).await.body["totals"]["revenue"]["total"],
        1_100
    );
    let c3 = h.calls();
    assert_eq!((c3.lists, c3.reads), (6, 4), "only the changed day");
}

#[tokio::test]
async fn all_has_one_entry_per_paid_branch_and_one_total_per_currency() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let r = h
        .call(
            "POST",
            "/v1/branches",
            Some(json!({"name": "Pixel Arena Two"})),
            &a.access,
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let b2 = r.body["id"].as_str().unwrap().to_owned();
    let r = h
        .call(
            "POST",
            "/v1/branches",
            Some(json!({"name": "Pixel Arena Free"})),
            &a.access,
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let free = r.body["id"].as_str().unwrap().to_owned();
    h.upgrade(&b2, Tier::Basic, T0 + 30 * DAY).await;
    let second = Cafe {
        branch: b2.clone(),
        tenant: a.tenant.clone(),
        token: String::new(),
        access: a.access.clone(),
    };
    rusqlite::Connection::open(h._dir.path().join("gz_control.db"))
        .unwrap()
        .execute(
            "UPDATE branches SET currency = 'AED' WHERE id = ?1",
            rusqlite::params![b2],
        )
        .unwrap();

    h.put_summary(&a, T0, 1_000).await;
    h.put_rollup(&a, "2026-10-26", 1, 1_000).await;
    h.put_rollup_in(&second, "2026-10-26", 1, 700, "AED").await;

    let r = h.get(&a, "/v1/branches/all/live").await;
    assert_eq!(r.status, 200, "{}", r.body);
    let branches = r.body["branches"].as_array().unwrap();
    assert_eq!(branches.len(), 2, "the Free branch is not listed");
    assert!(branches.iter().all(|b| b["branch_id"] != free.as_str()));
    let totals = r.body["totals"].as_array().unwrap();
    assert_eq!(totals.len(), 2);
    assert_eq!(totals[0]["currency"], "INR");
    assert_eq!(totals[0]["revenue_so_far_minor"], 1_000);
    assert_eq!(totals[0]["branches"], 1);
    assert_eq!(totals[1]["currency"], "AED");
    assert_eq!(totals[1]["branches"], 0, "no summary yet");

    let r = h
        .get(&a, "/v1/branches/all/summary?from=2026-10-26&to=2026-10-26")
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["branches"].as_array().unwrap().len(), 2);
    let totals = r.body["totals"].as_array().unwrap();
    assert_eq!(totals.len(), 2, "never summed across currencies");
    let by = |cur: &str| {
        totals
            .iter()
            .find(|t| t["currency"] == cur)
            .unwrap_or_else(|| panic!("no {cur} in {totals:?}"))
    };
    assert_eq!(by("INR")["totals"]["revenue"]["total"], 1_000);
    assert_eq!(by("AED")["totals"]["revenue"]["total"], 700);
}

#[tokio::test]
async fn another_cafes_branch_is_not_found_and_its_files_are_never_read() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    let b = h.cafe(PHONE_B, "Other Arena", "198.51.100.5").await;
    h.upgrade(&b.branch, Tier::Basic, T0 + 30 * DAY).await;
    h.put_summary(&b, T0, 777).await;
    h.put_rollup(&b, "2026-10-26", 1, 777).await;

    for path in [
        live(&b),
        summary(&b, "from=2026-10-26&to=2026-10-26"),
        "/v1/branches/nope/live".to_owned(),
        "/v1/branches/..%2F..%2Fx/live".to_owned(),
        "/v1/branches/%2E%2E/live".to_owned(),
    ] {
        let r = h.get(&a, &path).await;
        assert_eq!(r.status, 404, "{path}: {}", r.body);
        assert_eq!(r.code(), "NOT_FOUND", "{path}");
    }
    assert_eq!(h.calls().reads, 0, "nothing was fetched for a stranger");
    // The owner of that branch still reads it.
    assert_eq!(h.get(&b, &live(&b)).await.status, 200);
}

#[tokio::test]
async fn a_free_branch_is_not_entitled() {
    let h = Harness::new().await;
    let a = h.cafe(PHONE_A, "Pixel Arena", "198.51.100.4").await;
    h.put_summary(&a, T0, 1).await;
    for path in [live(&a), summary(&a, "from=2026-10-26&to=2026-10-26")] {
        let r = h.get(&a, &path).await;
        assert_eq!(r.status, 403, "{}", r.body);
        assert_eq!(r.code(), "FEATURE_NOT_ENTITLED");
    }
    let r = h.get(&a, "/v1/branches/all/live").await;
    assert_eq!(r.code(), "FEATURE_NOT_ENTITLED");
    assert_eq!(h.calls().reads, 0);
}

#[tokio::test]
async fn a_tenant_over_the_owner_api_limit_is_told_when_to_retry() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    h.put_summary(&a, T0, 1).await;
    for i in 0..arena_proto::OWNER_API_LIMIT_PER_MIN {
        let r = h.get(&a, &live(&a)).await;
        assert_eq!(r.status, 200, "call {i}: {}", r.body);
    }
    let r = h.get(&a, &live(&a)).await;
    assert_eq!(r.status, 429, "{}", r.body);
    assert_eq!(r.code(), "RATE_LIMITED");
    let wait: u64 = r.retry_after.expect("Retry-After").parse().unwrap();
    assert!((1..=61).contains(&wait), "{wait}");

    // Another tenant is not affected, and the window rolls on.
    let b = h.cafe(PHONE_B, "Other Arena", "198.51.100.5").await;
    h.upgrade(&b.branch, Tier::Basic, T0 + 30 * DAY).await;
    assert_eq!(h.get(&b, &live(&b)).await.status, 200);
    h.advance(61_000);
    assert_eq!(h.get(&a, &live(&a)).await.status, 200);
}

#[tokio::test]
async fn a_bad_range_is_a_validation_failure_and_a_missing_session_is_refused() {
    let h = Harness::new().await;
    let a = h.paid_cafe().await;
    for (q, field) in [
        ("", "from"),
        ("from=2026-10-26", "to"),
        ("from=2026-13-01&to=2026-13-02", "from"),
        ("from=2026-10-26&to=2026-02-31", "to"),
        ("from=2026-10-27&to=2026-10-26", "to"),
        ("from=2026-10-26&to=2026-10-27&group_by=year", "group_by"),
        ("from=2025-01-01&to=2026-10-27", "to"),
    ] {
        let r = h.get(&a, &summary(&a, q)).await;
        assert_eq!(r.status, 422, "{q}: {}", r.body);
        assert_eq!(r.code(), "VALIDATION_FAILED", "{q}");
        assert_eq!(r.body["field"], field, "{q}");
    }
    let r = h.call("GET", &live(&a), None, "").await;
    assert_eq!(r.status, 401, "{}", r.body);
    assert_eq!(h.calls().lists + h.calls().heads, 0);
}

#[tokio::test]
async fn without_a_data_bucket_the_read_is_unavailable() {
    let h = Harness::build(false).await;
    let a = h.paid_cafe().await;
    let r = h.get(&a, &live(&a)).await;
    assert_eq!(r.status, 503, "{}", r.body);
}
