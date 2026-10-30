//! The owner's remote experience against its targets (Story 8.42): Today's data age, command
//! delivery, the catch-up after an eight-hour outage and the R2 cost per branch, against the real
//! router and an in-memory data bucket on a simulated clock. The sizes are the story's; the
//! `ARENA_*` variables below move them (the release rig runs the full ones) and
//! `ARENA_REMOTE_OUT` names a folder the results are written to for `scripts/remote-report.mjs`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Instant;

use arena_cloud::auth::AuthState;
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
use arena_cloud::presign::{Presigner, PutCounts, R2_CLASS_A_PAISE_PER_MILLION};
use arena_cloud::store::{MemoryStore, ObjectStore, S3Config};
use arena_cloud::{Clock, app_with_auth, auth_state};
use arena_proto::licence::LicenceClaims;
use arena_proto::{BranchSummary, SUMMARY_SCHEMA, SummaryCatalogue, Tier};
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
        escrow_key: None,
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

    /// A branch on `tier` (a comp entitlement, as the internal test café has) with a Hub paired
    /// to it that has checked in under `epoch`.
    async fn paid_cafe(&self, tier: Tier, phone: &str, peer: &str, epoch: &str) -> Cafe {
        let a = self.cafe(phone, "Pixel Arena", peer).await;
        self.upgrade(&a.branch, tier, T0 + 90 * DAY).await;
        self.beat(&a, epoch, 0).await;
        a
    }

    /// The Hub checks in now.
    async fn beat(&self, c: &Cafe, epoch: &str, last_seq_uploaded: i64) -> Reply {
        let req = Request::builder()
            .method("POST")
            .uri("/v1/heartbeat")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", c.token))
            .body(Body::from(
                json!({"epoch": epoch, "version": "0.2.0", "schema_version": 12, "last_seq_uploaded": last_seq_uploaded})
                    .to_string(),
            ))
            .unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    async fn presign(&self, c: &Cafe, body: Value) -> Reply {
        let req = Request::builder()
            .method("POST")
            .uri("/v1/presign")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", c.token))
            .body(Body::from(body.to_string()))
            .unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    /// The branch summary the Hub built at `as_of`, landed in the bucket.
    async fn put_summary(&self, c: &Cafe, as_of: i64) {
        let s = BranchSummary {
            schema: SUMMARY_SCHEMA,
            branch_id: c.branch.clone(),
            as_of,
            business_day: "2026-10-30".into(),
            open: true,
            sessions_live: 3,
            revenue_so_far_minor: 125_000,
            currency: "INR".into(),
            cash_in_drawer_minor: 5_000,
            shifts: vec![],
            staff_on_shift: vec!["Asha".into()],
            attention: vec![],
            catalogue: SummaryCatalogue::default(),
            reminded_today: Vec::new(),
        };
        let key = format!(
            "tenants/{}/branches/{}/state/latest.json",
            c.tenant, c.branch
        );
        self.keys
            .put(&key, serde_json::to_vec(&s).unwrap())
            .await
            .unwrap();
    }

    /// Moves the cloud's clock on.
    fn advance(&self, ms: i64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
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

// ---- Sizes, results and the clock ----

fn env_i64(name: &str, default: i64) -> i64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Writes one result for `scripts/remote-report.mjs` when `ARENA_REMOTE_OUT` names a folder.
fn emit(name: &str, result: &Value) {
    if let Ok(dir) = std::env::var("ARENA_REMOTE_OUT") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join(format!("{name}.json")),
            serde_json::to_vec_pretty(result).unwrap(),
        )
        .unwrap();
    }
}

/// The value at the `pct` percentile (nearest rank) of `v`.
fn percentile(v: &mut [i64], pct: usize) -> i64 {
    v.sort_unstable();
    let rank = (v.len() * pct).div_ceil(100).max(1);
    v[rank - 1]
}

/// A small deterministic generator, so a run is the same every time.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
}

impl Harness {
    /// The owner's session outlives the simulated days (the app refreshes it in the background).
    fn keep_signed_in(&self) {
        let conn = rusqlite::Connection::open(self._dir.path().join("gz_control.db")).unwrap();
        let far = self.now.load(Ordering::SeqCst) + DAY;
        conn.execute(
            "UPDATE owner_sessions SET access_expires_at = ?1, refresh_expires_at = ?1",
            [far],
        )
        .unwrap();
    }
}

const TIERS: [(Tier, &str, &str); 2] = [
    (Tier::Basic, "basic", "+919876543210"),
    (Tier::Premium, "premium", "+919876543211"),
];

// ---- Today: first answer and the age of its figures ----

/// 1,000 scripted opens of Today on `tier`: the Hub builds its summary every cadence and the
/// file lands `lag_ms` later; the owner opens at scattered times, reads `live`, and reports the
/// age of what it showed. Returns the result for the report.
async fn script_opens(tier: Tier, label: &str, phone: &str, opens: i64, lag_ms: i64) -> Value {
    let h = Harness::new().await;
    let a = h.paid_cafe(tier, phone, "198.51.100.4", "1:HUBA").await;
    let cadence_s = i64::from(LicenceClaims::for_tier("t", "b", tier, 0).summary_interval_s);
    let period = cadence_s * 1000;
    let mut rng = Lcg(7);
    let (mut within, mut read_ms, mut ages) = (0_i64, Vec::new(), Vec::new());
    let mut landed = i64::MIN;
    let mut offset = 0_i64;
    for _ in 0..opens {
        // Far enough on for the 30 s cache to have lapsed, scattered across the cadence.
        offset += 31_000 + i64::try_from(rng.next() % 3_000_000).unwrap() % (2 * period);
        h.now.store(T0 + offset, Ordering::SeqCst);
        h.keep_signed_in();
        // The newest summary built on the cadence grid whose file has landed by now.
        let built = (offset - lag_ms).div_euclid(period) * period;
        if built != landed {
            landed = built;
            h.put_summary(&a, T0 + built).await;
        }
        let started = Instant::now();
        let r = h
            .call(
                "GET",
                &format!("/v1/branches/{}/live", a.branch),
                None,
                &a.access,
            )
            .await;
        read_ms.push(i64::try_from(started.elapsed().as_micros()).unwrap() / 1000);
        assert_eq!(r.status, 200, "{}", r.body);
        let age_ms = offset - (r.body["as_of"].as_i64().unwrap() - T0);
        ages.push(age_ms / 1000);
        within += i64::from(age_ms <= period);
        let report = h
            .call(
                "POST",
                &format!("/v1/branches/{}/data-age", a.branch),
                Some(json!({"age_s": age_ms / 1000})),
                &a.access,
            )
            .await;
        assert_eq!(report.status, 204, "{}", report.body);
    }
    let stats = h.st.data_age.stats(&a.tenant, &a.branch);
    let share = within as f64 * 100.0 / opens as f64;
    json!({
        "tier": label,
        "opens": opens,
        "cadence_s": cadence_s,
        "lag_ms": lag_ms,
        "within_cadence_pct": share,
        "p99_age_s": percentile(&mut ages, 99),
        "recorded_p99_age_s": stats.p99_s,
        "server_read_p95_ms": percentile(&mut read_ms, 95),
    })
}

#[tokio::test]
async fn todays_data_age_stays_inside_the_cadence_on_99_percent_of_opens() {
    let opens = env_i64("ARENA_TODAY_OPENS", 1_000);
    let lag_ms = env_i64("ARENA_TODAY_LAG_MS", 1_000);
    let budget_pct = env_i64("ARENA_TODAY_WITHIN_PCT", 99);
    let server_ms = env_i64("ARENA_TODAY_SERVER_MS", 500);
    let mut results = Vec::new();
    for (tier, label, phone) in TIERS {
        let r = script_opens(tier, label, phone, opens, lag_ms).await;
        let share = r["within_cadence_pct"].as_f64().unwrap();
        assert!(
            share >= budget_pct as f64,
            "{label}: {share}% of {opens} opens within the cadence\n{r}"
        );
        assert!(
            r["server_read_p95_ms"].as_i64().unwrap() <= server_ms,
            "{label}: the live read took too long\n{r}"
        );
        results.push(r);
    }
    emit(
        "today",
        &json!({"budget_within_pct": budget_pct, "tiers": results}),
    );
}

// ---- Owner commands: post to applied ----

const MINUTE_MS: i64 = 60_000;

/// 200 owner commands posted over about ten minutes to an online branch whose Hub fetches its
/// mailbox every `ARENA_COMMAND_POLL_S` (the Hub's `COMMAND_POLL_INTERVAL`, 60 s) and takes
/// `ARENA_COMMAND_APPLY_MS` to apply one; time runs in one-second ticks.
#[tokio::test]
async fn owner_commands_reach_applied_at_the_branch_in_under_two_minutes_at_p95() {
    let total = env_i64("ARENA_COMMANDS", 200);
    let poll_ms = env_i64("ARENA_COMMAND_POLL_S", 60) * 1000;
    let apply_ms = env_i64("ARENA_COMMAND_APPLY_MS", 500);
    let budget_ms = env_i64("ARENA_COMMAND_P95_MS", 2 * MINUTE_MS);
    let h = Harness::new().await;
    let a = h
        .paid_cafe(Tier::Basic, PHONE_A, "198.51.100.4", "3:HUBA")
        .await;
    let mut rng = Lcg(11);
    let phase =
        i64::try_from(rng.next() % u64::try_from(poll_ms / 1_000).unwrap()).unwrap() * 1_000;
    // Posts at about one every three seconds, scattered, in order.
    let mut at = 0_i64;
    let posts: Vec<i64> = (0..total)
        .map(|_| {
            at += 1_000 + i64::try_from(rng.next() % 4_000).unwrap();
            at
        })
        .collect();
    let mut posted: Vec<(i64, i64)> = Vec::new(); // (command id, posted at)
    let mut latencies: Vec<i64> = Vec::new();
    let (mut next_post, mut cursor, mut seq) = (0_usize, 0_i64, 0_i64);
    let mut t = 0_i64;
    while latencies.len() < usize::try_from(total).unwrap() {
        t += 1_000;
        assert!(t < at + 10 * poll_ms, "commands are still waiting at {t}");
        h.now.store(T0 + t, Ordering::SeqCst);
        h.keep_signed_in();
        while next_post < posts.len() && posts[next_post] <= t {
            let key = ulid::Ulid::from_parts(1_800_000_000_000, next_post as u128).to_string();
            let r = h
                .call(
                    "POST",
                    "/v1/commands",
                    Some(json!({
                        "branch_id": a.branch,
                        "idempotency_key": key,
                        "kind": "rate.upsert",
                        "payload": {"group_id": "g1", "price_per_hour_minor": 6000},
                    })),
                    &a.access,
                )
                .await;
            assert_eq!(r.status, 200, "{}", r.body);
            next_post += 1;
            posted.push((h.id_of(&a.branch, &key), t));
        }
        if t % poll_ms != phase {
            continue;
        }
        h.beat(&a, "3:HUBA", 0).await;
        loop {
            let page = h.hub_fetch(&a, cursor, "3:HUBA").await;
            assert_eq!(page.status, 200, "{}", page.body);
            let cmds = page.body["commands"].as_array().unwrap().clone();
            if cmds.is_empty() {
                break;
            }
            for c in cmds {
                let id = c["id"].as_i64().unwrap();
                cursor = id;
                seq += 1;
                let r = h
                    .hub_result(&a, id, json!({"epoch": "3:HUBA", "applied_seq": seq}))
                    .await;
                assert_eq!(r.body["status"], "applied", "{}", r.body);
                let (_, at_post) = posted.iter().find(|(pid, _)| *pid == id).unwrap();
                latencies.push(t + apply_ms - at_post);
            }
        }
    }
    let p95 = percentile(&mut latencies.clone(), 95);
    let max = *latencies.iter().max().unwrap();
    emit(
        "commands",
        &json!({"commands": total, "poll_s": poll_ms / 1000, "apply_ms": apply_ms, "p95_ms": p95, "max_ms": max, "budget_p95_ms": budget_ms}),
    );
    assert!(p95 < budget_ms, "p95 {p95} ms, max {max} ms");
    assert_eq!(h.rows(), total, "every command exactly once");
}

impl Harness {
    async fn hub_fetch(&self, c: &Cafe, after: i64, epoch: &str) -> Reply {
        let req = Request::builder()
            .method("GET")
            .uri(format!("/v1/commands?after={after}&epoch={epoch}"))
            .header("authorization", format!("Bearer {}", c.token))
            .body(Body::empty())
            .unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    async fn hub_result(&self, c: &Cafe, id: i64, body: Value) -> Reply {
        let req = Request::builder()
            .method("POST")
            .uri(format!("/v1/commands/{id}/result"))
            .header("authorization", format!("Bearer {}", c.token))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    fn id_of(&self, branch: &str, key: &str) -> i64 {
        rusqlite::Connection::open(self._dir.path().join("gz_control.db"))
            .unwrap()
            .query_row(
                "SELECT id FROM commands WHERE branch_id = ? AND idempotency_key = ?",
                [branch, key],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn rows(&self) -> i64 {
        rusqlite::Connection::open(self._dir.path().join("gz_control.db"))
            .unwrap()
            .query_row("SELECT COUNT(*) FROM commands", [], |r| r.get(0))
            .unwrap()
    }
}

// ---- The catch-up after a long outage ----

fn key_of(url: &str) -> String {
    let path = url.split('?').next().unwrap();
    path.strip_prefix("http://127.0.0.1:9000/arena-data/")
        .unwrap()
        .replace("%3A", ":")
}

/// The Hub comes back from `ARENA_OUTAGE_HOURS` with the changes of `ARENA_OUTAGE_ROWS` rows and
/// two business days of rollups to send, back to back in 5,000-row batches. The cloud takes every
/// link, the files land once under their keys even when a cycle is repeated, the next heartbeat
/// lists no hole and the whole catch-up fits `ARENA_CATCH_UP_MS` (10 minutes) on the cloud's clock.
#[tokio::test]
async fn a_branch_back_from_an_eight_hour_outage_catches_up_with_no_gap_and_no_duplicate() {
    let hours = env_i64("ARENA_OUTAGE_HOURS", 8);
    // 1,000 sessions and 500 orders at about eight changes each, over the 20 kiosks.
    let rows = env_i64("ARENA_OUTAGE_ROWS", 12_000);
    let budget_ms = env_i64("ARENA_CATCH_UP_MS", 10 * MINUTE_MS);
    let h = Harness::new().await;
    let a = h
        .paid_cafe(Tier::Basic, PHONE_A, "198.51.100.4", "3:HUBA")
        .await;
    let epoch = "3:HUBA";
    let prefix = format!("tenants/{}/branches/{}/", a.tenant, a.branch);

    // The WAN is down; the branch trades and nothing reaches the cloud.
    h.advance(hours * 60 * MINUTE_MS);
    let back = h.now.load(Ordering::SeqCst);
    let started = Instant::now();
    // A backlog goes back to back; a few seconds pass between two cycles.
    let step = 5_000_i64;
    let batch_max = i64::try_from(arena_store::BATCH_MAX_ROWS).unwrap();
    let mut first = 1;
    let mut puts = 0;
    while first <= rows {
        let last = (first + batch_max - 1).min(rows);
        h.advance(step);
        let r = h
            .presign(
                &a,
                json!({"epoch": epoch, "batch": {"first": first, "last": last}, "summary": last == rows}),
            )
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
        let key = key_of(r.body["batch_url"].as_str().unwrap());
        assert_eq!(
            key,
            format!("{prefix}changes/{epoch}/{first:012}-{last:012}.zst")
        );
        h.keys.put(&key, vec![1]).await.unwrap();
        puts += 1;
        if let Some(url) = r.body["summary_url"].as_str() {
            h.put_summary(&a, h.now.load(Ordering::SeqCst)).await;
            assert_eq!(key_of(url), format!("{prefix}state/latest.json"));
            puts += 1;
        }
        first = last + 1;
    }
    for date in ["2027-01-14", "2027-01-15"] {
        h.advance(step);
        let r = h
            .presign(&a, json!({"epoch": epoch, "kind": "rollup", "date": date}))
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
        let url = r
            .body
            .as_object()
            .and_then(|o| o.values().find_map(Value::as_str))
            .unwrap();
        assert_eq!(key_of(url), format!("{prefix}rollups/{date}.json"));
        h.keys.put(&key_of(url), vec![1]).await.unwrap();
        puts += 1;
    }

    // An interrupted cycle sends the first batch again: the same key, the same object.
    h.advance(step);
    let again = h
        .presign(
            &a,
            json!({"epoch": epoch, "batch": {"first": 1, "last": batch_max.min(rows)}}),
        )
        .await;
    assert_eq!(again.status, 200, "{}", again.body);
    h.keys
        .put(&key_of(again.body["batch_url"].as_str().unwrap()), vec![1])
        .await
        .unwrap();
    let batches = h
        .keys
        .keys()
        .into_iter()
        .filter(|k| k.contains("/changes/"))
        .count();
    assert_eq!(
        i64::try_from(batches).unwrap(),
        (rows + batch_max - 1) / batch_max,
        "one object per batch, however often it was sent"
    );

    // The heartbeat after the catch-up: nothing missing, nothing asked for.
    h.advance(step);
    let beat = h.beat(&a, epoch, rows).await;
    assert_eq!(beat.status, 200, "{}", beat.body);
    assert!(beat.body.get("resend").is_none(), "{}", beat.body);
    assert_eq!(h.st.gap_metrics.counts(&a.tenant, &a.branch).found, 0);

    let elapsed_ms =
        h.now.load(Ordering::SeqCst) - back + i64::try_from(started.elapsed().as_millis()).unwrap();
    emit(
        "catch_up",
        &json!({
            "outage_hours": hours,
            "rows": rows,
            "batches": batches,
            "puts": puts,
            "gaps_found": 0,
            "duplicate_objects": 0,
            "elapsed_ms": elapsed_ms,
            "budget_ms": budget_ms,
        }),
    );
    assert!(elapsed_ms <= budget_ms, "{elapsed_ms} ms");
}

// ---- R2 PUTs per branch ----

/// `days` of one branch's uploads on `tier` against the real presign path: every cycle asks for
/// a batch and the summary, every day for the rollup and the snapshot. Returns the counts.
async fn month_of_puts(tier: Tier, phone: &str, days: i64) -> PutCounts {
    let h = Harness::new().await;
    let epoch = "3:HUBA";
    let a = h.paid_cafe(tier, phone, "198.51.100.4", epoch).await;
    let cadence_ms = i64::from(LicenceClaims::for_tier("t", "b", tier, 0).upload_interval_s) * 1000;
    let mut seq = 0;
    for day in 1..=days {
        for _ in 0..DAY / cadence_ms {
            h.advance(cadence_ms);
            // A busy branch: every cycle has changes to send.
            let r = h
                .presign(
                    &a,
                    json!({"epoch": epoch, "batch": {"first": seq + 1, "last": seq + 50}, "summary": true}),
                )
                .await;
            assert_eq!(r.status, 200, "day {day}: {}", r.body);
            seq += 50;
        }
        let date = format!("2026-12-{day:02}");
        for kind in ["rollup", "snapshot"] {
            let r = h
                .presign(&a, json!({"epoch": epoch, "kind": kind, "date": date}))
                .await;
            assert_eq!(r.status, 200, "{kind} {date}: {}", r.body);
        }
    }
    h.st.put_metrics.counts(&a.tenant, &a.branch)
}

#[tokio::test]
async fn a_months_r2_puts_cost_each_branch_under_ten_rupees_on_average() {
    // The days are alike, so a few stand for the month; the release rig runs all 30.
    let days = env_i64("ARENA_COST_DAYS", 2);
    let price = u64::try_from(env_i64(
        "ARENA_R2_PUT_PAISE_PER_MILLION",
        i64::try_from(R2_CLASS_A_PAISE_PER_MILLION).unwrap(),
    ))
    .unwrap();
    let budget_paise = u64::try_from(env_i64("ARENA_COST_AVG_PAISE", 1_000)).unwrap();
    let mut tiers = Vec::new();
    let mut sum = 0;
    for (tier, label, phone) in TIERS {
        let c = month_of_puts(tier, phone, days).await;
        let paise = c.cost_paise(price * 30 / u64::try_from(days).unwrap());
        sum += paise;
        assert_eq!(c.refused, 0, "{label}: the hourly limit refused an upload");
        tiers.push(json!({
            "tier": label,
            "days_run": days,
            "puts_month": c.issued() * 30 / u64::try_from(days).unwrap(),
            "puts": c.issued(),
            "batch": c.batch,
            "summary": c.summary,
            "rollup": c.rollup,
            "snapshot": c.snapshot,
            "cost_paise": paise,
        }));
    }
    let average = sum / u64::try_from(TIERS.len()).unwrap();
    emit(
        "cost",
        &json!({"paise_per_million": price, "average_paise": average, "budget_paise": budget_paise, "tiers": tiers}),
    );
    assert!(average < budget_paise, "average {average} paise\n{tiers:?}");
}
