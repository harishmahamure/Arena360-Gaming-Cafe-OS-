//! Owner alerts (Story 8.33): raising once per alert id, web push and WhatsApp, the subscription
//! and settings routes and the tenant scope, over the router with the recorded fakes.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use arena_cloud::auth::AuthState;
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
use arena_cloud::messaging::{Behaviour, Channel, FakeNotifier, FakeSms, Messenger, Notifier};
use arena_cloud::push::{FakePush, PushOutcome, WebPush};
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
const PHONE_B: &str = "+919876543211";
/// The public key and auth secret of RFC 8291's example browser.
const P256DH: &str =
    "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4";
const AUTH: &str = "BTBZMqHH6r4Tts7J_aSIgg";

struct Reply {
    status: u16,
    body: Value,
}

impl Reply {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or("")
    }
}

struct Cafe {
    access: String,
    branch: String,
    owner: String,
}

struct Harness {
    app: axum::Router,
    db: Arc<ControlDb>,
    st: Arc<AuthState>,
    now: Arc<AtomicI64>,
    dir: tempfile::TempDir,
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
        Self::build(true).await
    }

    /// No VAPID key: the state has no push sender.
    async fn without_push() -> Self {
        Self::build(false).await
    }

    async fn build(with_push: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = ControlDb::open_local(&dir.path().join("gz_control.db"))
            .await
            .unwrap();
        db.migrate(T0).await.unwrap();
        let db = Arc::new(db);
        let now = Arc::new(AtomicI64::new(T0));
        let tick = Arc::clone(&now);
        let clock: Clock = Arc::new(move || tick.load(Ordering::SeqCst));
        let st = if with_push {
            auth_state(Arc::clone(&db), &config(), clock).unwrap()
        } else {
            let st = Arc::new(AuthState::new(
                Arc::clone(&db),
                Messenger::Fake(FakeSms::new(Arc::clone(&clock))),
                Arc::clone(&clock),
                &config(),
            ));
            let _ = st
                .notifier
                .set(Notifier::Fake(FakeNotifier::new(Arc::clone(&clock))));
            st
        };
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

    fn fake_push(&self) -> &FakePush {
        self.st
            .push
            .get()
            .and_then(WebPush::fake)
            .expect("the recorded fake push")
    }

    fn fake_wa(&self) -> &FakeNotifier {
        self.st
            .notifier
            .get()
            .and_then(Notifier::fake)
            .expect("the recorded fake")
    }

    fn sql<T: rusqlite::types::FromSql>(&self, sql: &str) -> T {
        rusqlite::Connection::open(self.dir.path().join("gz_control.db"))
            .unwrap()
            .query_row(sql, [], |r| r.get(0))
            .unwrap()
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

    /// A signed-up owner with one branch; `tier` is its comp plan (`None`: Free).
    async fn cafe(&self, phone: &str, name: &str, tier: Option<Tier>) -> Cafe {
        let access = self.sign_up(phone, name).await;
        let owner = self.complete_setup(phone).await;
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
        if let Some(tier) = tier {
            self.db
                .grant_comp(&CompGrant {
                    branch_id: &branch,
                    tier,
                    until: T0 + 400 * DAY,
                    audit_id: &format!("audit-{branch}"),
                    operator: "ops",
                    reason: "beta cafe",
                    details_json: "{}",
                    now: T0,
                })
                .await
                .unwrap();
        }
        Cafe {
            access,
            branch,
            owner,
        }
    }

    async fn paid(&self) -> Cafe {
        self.cafe(PHONE_A, "Pixel Arena", Some(Tier::Basic)).await
    }

    /// Pairs a Hub to the café's branch and returns its device token.
    async fn pair_hub(&self, cafe: &Cafe) -> String {
        let r = self
            .call(
                "POST",
                "/v1/wizard/install-hub/code",
                Some(json!({})),
                &cafe.access,
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
        req.extensions_mut().insert(ConnectInfo(SocketAddr::new(
            "198.51.100.4".parse().unwrap(),
            40000,
        )));
        let r = reply(self.app.clone().oneshot(req).await.unwrap()).await;
        assert_eq!(r.status, 200, "{}", r.body);
        r.body["token"].as_str().unwrap().to_owned()
    }

    /// What a Hub's drain posts for an owner alert.
    async fn hub_alert(&self, token: &str, outbox_id: &str, kind: &str, params: Value) -> Reply {
        let body = json!({
            "outbox_id": outbox_id,
            "kind": kind,
            "purpose": "owner_alert",
            "consent_policy_version": "owner",
            "recipient": "",
            "params": params,
        });
        let req = Request::builder()
            .method("POST")
            .uri("/v1/notify")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(body.to_string()))
            .unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    async fn subscribe(&self, cafe: &Cafe, endpoint: &str) -> Reply {
        self.call(
            "POST",
            "/v1/alerts/push",
            Some(json!({"endpoint": endpoint, "keys": {"p256dh": P256DH, "auth": AUTH}})),
            &cafe.access,
        )
        .await
    }

    async fn raise(&self, branch: &str, id: Option<&str>, kind: &str, params: Value) -> Reply {
        let mut body = json!({"branch_id": branch, "kind": kind, "params": params});
        if let Some(id) = id {
            body["id"] = json!(id);
        }
        self.call("POST", "/v1/dev/owner-alerts", Some(body), "")
            .await
    }

    async fn low_stock(&self, branch: &str, id: &str) -> Reply {
        self.raise(
            branch,
            Some(id),
            "low_stock",
            json!({"product": "Cola", "qty": "3"}),
        )
        .await
    }

    async fn alerts(&self, cafe: &Cafe) -> Value {
        let r = self.call("GET", "/v1/alerts", None, &cafe.access).await;
        assert_eq!(r.status, 200, "{}", r.body);
        r.body
    }

    fn alert_rows(&self) -> i64 {
        self.sql("SELECT COUNT(*) FROM owner_alerts")
    }

    fn subscription_rows(&self) -> i64 {
        self.sql("SELECT COUNT(*) FROM push_subscriptions")
    }

    fn ledger_rows(&self) -> i64 {
        self.sql("SELECT COUNT(*) FROM send_ledger WHERE kind = 'owner_alert'")
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

fn endpoint(n: u32) -> String {
    format!("https://push.example/send/{n}")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_raise_is_listed_pushed_to_each_browser_and_sent_once_on_whatsapp() {
    let h = Harness::new().await;
    let cafe = h.paid().await;
    assert_eq!(h.subscribe(&cafe, &endpoint(1)).await.status, 204);
    assert_eq!(h.subscribe(&cafe, &endpoint(2)).await.status, 204);

    let r = h.low_stock(&cafe.branch, "alert-1").await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body, json!({"id": "alert-1", "created": true}));

    assert_eq!(h.alert_rows(), 1);
    let pushes = h.fake_push().sent();
    assert_eq!(pushes.len(), 2);
    let mut endpoints: Vec<_> = pushes.iter().map(|p| p.endpoint.as_str()).collect();
    endpoints.sort_unstable();
    assert_eq!(endpoints, [endpoint(1), endpoint(2)]);
    let payload: Value = serde_json::from_str(&pushes[0].payload).unwrap();
    assert_eq!(
        payload,
        json!({
            "title": "Pixel Arena",
            "body": "Cola is running low: 3 left.",
            "url": format!("/reports?tab=stock&branch={}", cafe.branch),
            "tag": "alert-1",
        })
    );
    let wa = h.fake_wa().sent();
    assert_eq!(wa.len(), 1);
    assert_eq!(wa[0].to, PHONE_A);
    assert_eq!(wa[0].outbox_id, "alert-1.wa");
    assert_eq!(wa[0].template_id, "arena360_owner_alert_v1");
    assert_eq!(
        wa[0].params,
        ["Pixel Arena", "Cola is running low: 3 left."]
    );
    assert_eq!(h.ledger_rows(), 1);
    let kind: String =
        h.sql("SELECT kind || '/' || purpose || '/' || consent_policy_version FROM send_ledger WHERE outbox_id = 'alert-1.wa'");
    assert_eq!(kind, "owner_alert/owner_alert/owner");
    let used: i64 =
        h.sql("SELECT COUNT(*) FROM push_subscriptions WHERE last_success_at IS NOT NULL");
    assert_eq!(used, 2);

    let list = h.alerts(&cafe).await;
    let a = &list["alerts"][0];
    assert_eq!(a["id"], "alert-1");
    assert_eq!(a["branch_id"], cafe.branch.as_str());
    assert_eq!(a["branch_name"], "Pixel Arena");
    assert_eq!(a["kind"], "low_stock");
    assert_eq!(a["text"], "Cola is running low: 3 left.");
    assert_eq!(a["created_at"], T0);
    assert!(list.get("next_before").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repeat_with_one_id_adds_no_row_and_sends_nothing_twice() {
    let h = Harness::new().await;
    let cafe = h.paid().await;
    h.subscribe(&cafe, &endpoint(1)).await;
    h.subscribe(&cafe, &endpoint(2)).await;
    assert_eq!(h.low_stock(&cafe.branch, "alert-1").await.status, 200);
    let again = h.low_stock(&cafe.branch, "alert-1").await;
    assert_eq!(again.body, json!({"id": "alert-1", "created": false}));

    assert_eq!(h.alert_rows(), 1);
    assert_eq!(h.fake_push().sent().len(), 2);
    assert_eq!(h.fake_wa().sent().len(), 1);
    assert_eq!(h.ledger_rows(), 1);

    // A browser that subscribed since gets only its own push on a repeat.
    h.subscribe(&cafe, &endpoint(3)).await;
    assert_eq!(h.low_stock(&cafe.branch, "alert-1").await.status, 200);
    assert_eq!(h.fake_push().sent().len(), 3);
    assert_eq!(h.fake_wa().sent().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_gone_subscription_is_deleted_and_the_other_still_gets_its_push() {
    let h = Harness::new().await;
    let cafe = h.paid().await;
    h.subscribe(&cafe, &endpoint(1)).await;
    h.subscribe(&cafe, &endpoint(2)).await;
    h.fake_push().set_outcome(&endpoint(1), PushOutcome::Gone);

    assert_eq!(h.low_stock(&cafe.branch, "alert-1").await.status, 200);
    assert_eq!(h.subscription_rows(), 1);
    let sent = h.fake_push().sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].endpoint, endpoint(2));
    assert_eq!(h.fake_wa().sent().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn another_failure_keeps_the_subscription_and_is_not_retried() {
    let h = Harness::new().await;
    let cafe = h.paid().await;
    h.subscribe(&cafe, &endpoint(1)).await;
    h.fake_push()
        .set_outcome(&endpoint(1), PushOutcome::Failed("PUSH_HTTP_500".into()));
    assert_eq!(h.low_stock(&cafe.branch, "alert-1").await.status, 200);
    assert_eq!(h.subscription_rows(), 1);
    let used: i64 =
        h.sql("SELECT COUNT(*) FROM push_subscriptions WHERE last_success_at IS NOT NULL");
    assert_eq!(used, 0);

    h.fake_push()
        .set_outcome(&endpoint(1), PushOutcome::Delivered);
    assert_eq!(h.low_stock(&cafe.branch, "alert-1").await.status, 200);
    assert!(h.fake_push().sent().is_empty(), "claimed, so not retried");
}

#[tokio::test(flavor = "multi_thread")]
async fn whatsapp_off_leaves_push_and_the_list_only() {
    let h = Harness::new().await;
    let cafe = h.paid().await;
    h.subscribe(&cafe, &endpoint(1)).await;
    let r = h
        .call(
            "PATCH",
            "/v1/alerts/settings",
            Some(json!({"whatsapp": false})),
            &cafe.access,
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["whatsapp"], false);

    assert_eq!(h.low_stock(&cafe.branch, "alert-1").await.status, 200);
    assert_eq!(h.fake_push().sent().len(), 1);
    assert!(h.fake_wa().sent().is_empty());
    assert_eq!(h.ledger_rows(), 0);
    assert_eq!(h.alerts(&cafe).await["alerts"].as_array().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_whatsapp_does_not_fail_the_raise_and_push_still_goes() {
    let h = Harness::new().await;
    let cafe = h.paid().await;
    h.subscribe(&cafe, &endpoint(1)).await;
    h.fake_wa()
        .set_behaviour(Channel::Whatsapp, Behaviour::Uncertain);
    assert_eq!(h.low_stock(&cafe.branch, "alert-1").await.status, 200);
    assert_eq!(h.fake_push().sent().len(), 1);
    assert_eq!(h.ledger_rows(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bad_fixture_writes_nothing() {
    let h = Harness::new().await;
    let cafe = h.paid().await;
    h.subscribe(&cafe, &endpoint(1)).await;
    let unknown = h.raise(&cafe.branch, None, "volcano", json!({})).await;
    assert_eq!((unknown.status, unknown.code()), (422, "VALIDATION_FAILED"));
    let missing = h
        .raise(&cafe.branch, None, "low_stock", json!({"product": "Cola"}))
        .await;
    assert_eq!((missing.status, missing.code()), (422, "VALIDATION_FAILED"));
    assert_eq!(missing.body["field"], "qty");
    let phone = h
        .raise(
            &cafe.branch,
            None,
            "large_refund",
            json!({"amount": "Rs 5", "phone": PHONE_B}),
        )
        .await;
    assert_eq!((phone.status, phone.code()), (422, "VALIDATION_FAILED"));
    let id = h
        .raise(&cafe.branch, Some("bad id!"), "test", json!({}))
        .await;
    assert_eq!((id.status, id.code()), (422, "VALIDATION_FAILED"));

    assert_eq!(h.alert_rows(), 0);
    assert!(h.fake_push().sent().is_empty());
    assert!(h.fake_wa().sent().is_empty());
    assert_eq!(h.ledger_rows(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_branch_is_not_found_and_a_free_branch_is_not_entitled() {
    let h = Harness::new().await;
    let free = h.cafe(PHONE_A, "Pixel Arena", None).await;
    let r = h.raise("01NOSUCHBRANCH", None, "test", json!({})).await;
    assert_eq!((r.status, r.code()), (404, "NOT_FOUND"));
    let r = h.raise(&free.branch, None, "test", json!({})).await;
    assert_eq!((r.status, r.code()), (403, "FEATURE_NOT_ENTITLED"));
    assert_eq!(h.alert_rows(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_alert_of_another_cafe_is_not_listed_and_its_id_is_not_reusable() {
    let h = Harness::new().await;
    let a = h.paid().await;
    let b = h.cafe(PHONE_B, "Neon Den", Some(Tier::Basic)).await;
    assert_eq!(h.low_stock(&a.branch, "alert-a").await.status, 200);
    assert_eq!(h.low_stock(&b.branch, "alert-b").await.status, 200);

    let ids = |list: &Value| -> Vec<String> {
        list["alerts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["id"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(ids(&h.alerts(&a).await), ["alert-a"]);
    assert_eq!(ids(&h.alerts(&b).await), ["alert-b"]);

    let r = h.low_stock(&b.branch, "alert-a").await;
    assert_eq!((r.status, r.code()), (409, "IDEMPOTENCY_CONFLICT"));
    assert_eq!(h.alert_rows(), 2);
    assert_eq!(h.fake_wa().sent().len(), 2);
    let r = h.call("GET", "/v1/alerts", None, "").await;
    assert_eq!(r.status, 401);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_list_is_newest_first_in_pages_and_drops_alerts_over_90_days_old() {
    let h = Harness::new().await;
    let cafe = h.paid().await;
    // An alert from 91 days ago, with a push delivered for it.
    h.subscribe(&cafe, &endpoint(1)).await;
    h.now.store(T0 - 91 * DAY, Ordering::SeqCst);
    assert_eq!(h.low_stock(&cafe.branch, "old").await.status, 200);
    let delivered: i64 =
        h.sql("SELECT COUNT(*) FROM owner_alert_deliveries WHERE alert_id = 'old'");
    assert_eq!(delivered, 1);
    for (i, id) in ["a1", "a2", "a3"].iter().enumerate() {
        h.now.store(T0 + (i as i64) * 1000, Ordering::SeqCst);
        assert_eq!(h.low_stock(&cafe.branch, id).await.status, 200);
    }
    let r = h
        .call("GET", "/v1/alerts?limit=2", None, &cafe.access)
        .await;
    let first: Vec<_> = r.body["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert_eq!(first, ["a3", "a2"]);
    let next = r.body["next_before"].as_str().unwrap().to_owned();
    let r = h
        .call(
            "GET",
            &format!("/v1/alerts?limit=2&before={next}"),
            None,
            &cafe.access,
        )
        .await;
    assert_eq!(r.body["alerts"].as_array().unwrap().len(), 1);
    assert_eq!(r.body["alerts"][0]["id"], "a1");
    assert!(r.body.get("next_before").is_none());
    let bad = h
        .call("GET", "/v1/alerts?before=zzz", None, &cafe.access)
        .await;
    assert_eq!((bad.status, bad.code()), (422, "VALIDATION_FAILED"));

    // The 91-day-old alert was never listed, and the raise right after it deleted it with its
    // delivery.
    assert_eq!(h.alert_rows(), 3);
    assert_eq!(h.low_stock(&cafe.branch, "a4").await.status, 200);
    assert_eq!(h.alert_rows(), 4);
    let gone: i64 = h.sql("SELECT COUNT(*) FROM owner_alerts WHERE id = 'old'");
    assert_eq!(gone, 0);
    let left: i64 = h.sql("SELECT COUNT(*) FROM owner_alert_deliveries WHERE alert_id = 'old'");
    assert_eq!(left, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_subscription_is_an_upsert_with_https_and_a_limit_of_ten() {
    let h = Harness::new().await;
    let cafe = h.paid().await;
    assert_eq!(h.subscribe(&cafe, &endpoint(1)).await.status, 204);
    assert_eq!(h.subscribe(&cafe, &endpoint(1)).await.status, 204);
    assert_eq!(h.subscription_rows(), 1);
    let ua: String = h.sql("SELECT user_agent FROM push_subscriptions");
    assert_eq!(ua, "");

    let r = h.subscribe(&cafe, "http://push.example/send/9").await;
    assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));
    assert_eq!(r.body["field"], "endpoint");
    let r = h.subscribe(&cafe, "https://127.0.0.1/send/9").await;
    assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));
    let r = h
        .call(
            "POST",
            "/v1/alerts/push",
            Some(json!({"endpoint": endpoint(5), "keys": {"p256dh": "AAAA", "auth": AUTH}})),
            &cafe.access,
        )
        .await;
    assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));
    assert_eq!(r.body["field"], "keys.p256dh");

    for n in 2..=10 {
        assert_eq!(h.subscribe(&cafe, &endpoint(n)).await.status, 204, "{n}");
    }
    let r = h.subscribe(&cafe, &endpoint(11)).await;
    assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));
    assert_eq!(h.subscription_rows(), 10);
    // A known endpoint can still refresh at the limit.
    assert_eq!(h.subscribe(&cafe, &endpoint(4)).await.status, 204);

    let s = h
        .call("GET", "/v1/alerts/settings", None, &cafe.access)
        .await;
    assert_eq!(s.status, 200);
    assert_eq!(s.body["subscriptions"], 10);
    assert_eq!(s.body["whatsapp"], true);
    assert_eq!(
        s.body["vapid_public_key"].as_str().unwrap(),
        h.st.push.get().unwrap().public_key()
    );

    let r = h
        .call(
            "POST",
            "/v1/alerts/push/remove",
            Some(json!({"endpoint": endpoint(4)})),
            &cafe.access,
        )
        .await;
    assert_eq!(r.status, 204);
    assert_eq!(h.subscription_rows(), 9);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_user_agent_is_stored_and_another_owner_cannot_remove_a_subscription() {
    let h = Harness::new().await;
    let a = h.paid().await;
    let b = h.cafe(PHONE_B, "Neon Den", Some(Tier::Basic)).await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/alerts/push")
        .header("content-type", "application/json")
        .header("cookie", format!("arena_access={}", a.access))
        .header("user-agent", "Mozilla/5.0 (Linux; Android 14) Chrome/130")
        .body(Body::from(
            json!({"endpoint": endpoint(1), "keys": {"p256dh": P256DH, "auth": AUTH}}).to_string(),
        ))
        .unwrap();
    assert_eq!(
        reply(h.app.clone().oneshot(req).await.unwrap())
            .await
            .status,
        204
    );
    let ua: String = h.sql("SELECT user_agent FROM push_subscriptions");
    assert_eq!(ua, "Mozilla/5.0 (Linux; Android 14) Chrome/130");
    let owner: String = h.sql("SELECT owner_user_id FROM push_subscriptions");
    assert_eq!(owner, a.owner);

    let r = h
        .call(
            "POST",
            "/v1/alerts/push/remove",
            Some(json!({"endpoint": endpoint(1)})),
            &b.access,
        )
        .await;
    assert_eq!(r.status, 204);
    assert_eq!(
        h.subscription_rows(),
        1,
        "another owner's remove changes nothing"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_vapid_key_push_is_unavailable_but_alerts_still_list_and_send() {
    let h = Harness::without_push().await;
    let cafe = h.paid().await;
    let r = h.subscribe(&cafe, &endpoint(1)).await;
    assert_eq!(r.status, 503);
    let r = h
        .call(
            "POST",
            "/v1/alerts/push/remove",
            Some(json!({"endpoint": endpoint(1)})),
            &cafe.access,
        )
        .await;
    assert_eq!(r.status, 503);
    let s = h
        .call("GET", "/v1/alerts/settings", None, &cafe.access)
        .await;
    assert_eq!(s.status, 200);
    assert!(s.body["vapid_public_key"].is_null());

    assert_eq!(h.low_stock(&cafe.branch, "alert-1").await.status, 200);
    assert_eq!(h.alerts(&cafe).await["alerts"].as_array().unwrap().len(), 1);
    assert_eq!(h.fake_wa().sent().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn alerts_hold_no_phone_name_or_email() {
    let h = Harness::new().await;
    let cafe = h.paid().await;
    let r = h
        .raise(
            &cafe.branch,
            Some("alert-1"),
            "credit_overdue",
            json!({"amount": "Rs 400", "days": "9", "player_id": "01J9PLAYER"}),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let raw: String =
        h.sql("SELECT id || tenant_id || branch_id || kind || params FROM owner_alerts");
    for secret in [PHONE_A, "owner10@example.in", "Asha"] {
        assert!(!raw.contains(secret), "{secret}");
    }
    let list = h.alerts(&cafe).await;
    assert_eq!(
        list["alerts"][0]["text"],
        "Credit of Rs 400 is overdue by 9 days."
    );
    assert_eq!(
        list["alerts"][0]["url"],
        format!("/reports?tab=credit&branch={}", cafe.branch)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_alert_routes_need_a_finished_account() {
    let h = Harness::new().await;
    let access = h.sign_up(PHONE_A, "Pixel Arena").await;
    for (method, path) in [("GET", "/v1/alerts"), ("GET", "/v1/alerts/settings")] {
        let r = h.call(method, path, None, &access).await;
        assert_eq!((r.status, r.code()), (403, "SETUP_INCOMPLETE"), "{path}");
    }
    let r = h
        .call(
            "PATCH",
            "/v1/alerts/settings",
            Some(json!({"whatsapp": false})),
            &access,
        )
        .await;
    assert_eq!((r.status, r.code()), (403, "SETUP_INCOMPLETE"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hub_post_raises_one_alert_under_the_outbox_id_and_a_repeat_adds_none() {
    let h = Harness::new().await;
    let cafe = h.paid().await;
    let token = h.pair_hub(&cafe).await;
    assert_eq!(h.subscribe(&cafe, &endpoint(1)).await.status, 204);
    let id = "01J9OUTBOXCASHVARIANCE000";

    let r = h
        .hub_alert(
            &token,
            id,
            "cash_variance",
            json!({"amount": "₹200.00", "shift": "Asha", "reason": "counting error"}),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["outbox_id"], id);
    assert_eq!(r.body["state"], "sent");
    assert_eq!(r.body["channel"], "app");

    let list = h.alerts(&cafe).await;
    assert_eq!(list["alerts"].as_array().unwrap().len(), 1);
    let a = &list["alerts"][0];
    assert_eq!(a["id"], id);
    assert_eq!(a["kind"], "cash_variance");
    assert_eq!(
        a["text"],
        "Cash is off by ₹200.00 at the close of the Asha shift (counting error)."
    );
    assert_eq!(h.fake_push().sent().len(), 1);
    assert_eq!(h.fake_wa().sent().len(), 1);

    // The drain posts it again (a lost reply): the same answer, nothing more is raised or sent.
    let again = h
        .hub_alert(
            &token,
            id,
            "cash_variance",
            json!({"amount": "₹200.00", "shift": "Asha", "reason": "counting error"}),
        )
        .await;
    assert_eq!((again.status, &again.body), (200, &r.body));
    assert_eq!(h.alert_rows(), 1);
    assert_eq!(h.fake_push().sent().len(), 1);
    assert_eq!(h.fake_wa().sent().len(), 1);
    // The alert carries no phone, name or email of anyone.
    let raw: String =
        h.sql("SELECT id || tenant_id || branch_id || kind || params FROM owner_alerts");
    assert!(!raw.contains(PHONE_A) && !raw.contains("owner10@example.in"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hub_may_raise_only_its_six_kinds_with_the_right_parameters() {
    let h = Harness::new().await;
    let cafe = h.paid().await;
    let token = h.pair_hub(&cafe).await;
    let kinds = [
        ("cash_variance", json!({"amount": "Rs 5", "shift": "Asha"})),
        ("low_stock", json!({"product": "Cola", "qty": "3"})),
        (
            "credit_overdue",
            json!({"amount": "Rs 5", "days": "15", "player_id": "01J9PLAYER"}),
        ),
        ("large_refund", json!({"amount": "Rs 5"})),
        ("large_discount", json!({"amount": "Rs 5"})),
        ("standby_forced", json!({"manager": "Asha"})),
    ];
    for (n, (kind, params)) in kinds.iter().enumerate() {
        let r = h
            .hub_alert(&token, &format!("OUTBOX{n}"), kind, params.clone())
            .await;
        assert_eq!(r.status, 200, "{kind}: {}", r.body);
    }
    assert_eq!(h.alert_rows(), 6);

    let bad = [
        // The cloud's own kinds, and ones nobody knows.
        ("test", json!({})),
        ("branch_offline", json!({"minutes": "20"})),
        (
            "invoice_ready",
            json!({"number": "A360/2026-27/000001", "amount": "Rs 1180.00"}),
        ),
        ("owner_alert", json!({})),
        ("nope", json!({})),
        // A missing, an unknown, an empty and a personal parameter.
        ("low_stock", json!({"product": "Cola"})),
        (
            "low_stock",
            json!({"product": "Cola", "qty": "3", "phone": PHONE_A}),
        ),
        ("large_refund", json!({"amount": " "})),
        ("large_refund", json!({"amount": "x".repeat(41)})),
        ("standby_forced", json!({})),
    ];
    for (n, (kind, params)) in bad.into_iter().enumerate() {
        let r = h
            .hub_alert(&token, &format!("BAD{n}"), kind, params.clone())
            .await;
        assert_eq!(
            (r.status, r.code()),
            (422, "VALIDATION_FAILED"),
            "{kind} {params}"
        );
    }
    assert_eq!(h.alert_rows(), 6);
    let r = h
        .hub_alert(&token, "a/b", "large_refund", json!({"amount": "Rs 5"}))
        .await;
    assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));
    assert_eq!(h.alert_rows(), 6);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_free_branch_hub_raises_nothing() {
    let h = Harness::new().await;
    let free = h.cafe(PHONE_B, "Free Arena", None).await;
    let token = h.pair_hub(&free).await;
    let r = h
        .hub_alert(&token, "OUT1", "large_refund", json!({"amount": "Rs 5"}))
        .await;
    assert_eq!((r.status, r.code()), (403, "FEATURE_NOT_ENTITLED"));
    assert_eq!(h.alert_rows(), 0);
    assert!(h.fake_wa().sent().is_empty());
}

// ---- Story 8.35: the offline watch -------------------------------------------------------------

mod offline_watch {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::time::Duration;

    use arena_cloud::backup::AlertTarget;
    use arena_cloud::offline_watch::{WatchOutcome, watch_once};

    use super::*;

    const MINUTE: i64 = 60_000;
    const HOUR: i64 = 60 * MINUTE;
    /// Asia/Kolkata is UTC+05:30 all year.
    const IST: i64 = 19_800_000;
    const EPOCH: &str = "1:01HUBEPOCH";

    /// Every day 09:00 to 23:00.
    fn daily_hours() -> String {
        let day = r#"{"open_min":540,"close_min":1380}"#;
        format!("[{}]", [day; 7].join(","))
    }

    /// Midnight in Kolkata on the day after `T0`: every time in these tests is after `T0`.
    fn day0() -> i64 {
        (T0 + IST).div_euclid(DAY) * DAY - IST + DAY
    }

    fn opening() -> Value {
        json!({"opening_hours": daily_hours(), "timezone": "Asia/Kolkata"})
    }

    fn target(webhook: Option<&str>) -> AlertTarget {
        AlertTarget {
            webhook: webhook.map(str::to_owned),
            env_name: "staging".into(),
            heartbeat: None,
        }
    }

    impl Harness {
        /// A heartbeat from `token` at cloud time `at`; `extra` is merged into the body.
        async fn beat(&self, token: &str, at: i64, extra: Value) -> Reply {
            self.now.store(at, Ordering::SeqCst);
            let mut body = json!({
                "epoch": EPOCH,
                "version": "0.2.0",
                "schema_version": 12,
                "last_seq_uploaded": 0,
            });
            for (k, v) in extra.as_object().unwrap() {
                body[k] = v.clone();
            }
            let req = Request::builder()
                .method("POST")
                .uri("/v1/heartbeat")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(body.to_string()))
                .unwrap();
            reply(self.app.clone().oneshot(req).await.unwrap()).await
        }

        /// Runs the watch once at cloud time `at`.
        async fn watch(&self, at: i64) -> WatchOutcome {
            self.watch_with(at, &target(None)).await
        }

        async fn watch_with(&self, at: i64, target: &AlertTarget) -> WatchOutcome {
            self.now.store(at, Ordering::SeqCst);
            watch_once(&self.st, target).await.unwrap()
        }

        fn exec(&self, sql: &str) {
            rusqlite::Connection::open(self.dir.path().join("gz_control.db"))
                .unwrap()
                .execute_batch(sql)
                .unwrap();
        }

        fn column(&self, sql: &str) -> Vec<String> {
            let conn = rusqlite::Connection::open(self.dir.path().join("gz_control.db")).unwrap();
            let mut stmt = conn.prepare(sql).unwrap();
            stmt.query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        }

        /// The kinds of the alerts raised, oldest first.
        fn kinds(&self) -> Vec<String> {
            self.column("SELECT kind FROM owner_alerts ORDER BY created_at, id")
        }

        /// A paid café with a paired Hub.
        async fn watched(&self) -> (Cafe, String) {
            let cafe = self.paid().await;
            let token = self.pair_hub(&cafe).await;
            (cafe, token)
        }

        /// Adds `n` more paid branches with a live Hub, hours cached and `last_seen_at` at
        /// `seen`, to the café's tenant. Returns their ids.
        async fn estate(&self, cafe: &Cafe, n: usize, seen: i64) -> Vec<String> {
            let mut ids = Vec::new();
            for i in 0..n {
                let id = format!("EXTRA{i:03}");
                self.exec(&format!(
                    "INSERT INTO branches (id, tenant_id, name, city, country, currency, timezone, \
                         gst_registered, gstin, branch_code, code_locked_at, created_at, updated_at) \
                     SELECT '{id}', tenant_id, 'Extra {i}', city, country, currency, timezone, 0, NULL, \
                         'X{i:03}', NULL, created_at, updated_at FROM branches WHERE id = '{b}';
                     INSERT INTO entitlements (branch_id, tenant_id, tier, source, addons_json, \
                         issued_at, until, updated_at) \
                     SELECT '{id}', tenant_id, 'basic', 'comp', '[]', {T0}, {until}, {T0} \
                         FROM branches WHERE id = '{id}';
                     INSERT INTO hubs (id, branch_id, platform, hub_instance_id, version, token_hash, \
                         paired_at, last_seen_at) \
                     VALUES ('HUB{id}', '{id}', 'android', 'inst{i}', '0.2.0', 'x', {T0}, {seen});",
                    b = cafe.branch,
                    until = T0 + 400 * DAY,
                ));
                self.db
                    .record_branch_opening(&id, &daily_hours(), "Asia/Kolkata", T0)
                    .await
                    .unwrap();
                ids.push(id);
            }
            ids
        }

        fn silence(&self, branch: &str, seen: i64) {
            self.exec(&format!(
                "UPDATE hubs SET last_seen_at = {seen} WHERE branch_id = '{branch}'"
            ));
        }
    }

    /// A local HTTP listener standing in for the alert webhook: answers 200 and hands over each
    /// request's body.
    struct Webhook {
        url: String,
        requests: mpsc::Receiver<String>,
    }

    impl Webhook {
        fn start() -> Self {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = l.local_addr().unwrap().port();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                for stream in l.incoming() {
                    let Ok(mut s) = stream else { continue };
                    let mut r = BufReader::new(s.try_clone().unwrap());
                    let mut len = 0usize;
                    loop {
                        let mut line = String::new();
                        if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                            len = v.trim().parse().unwrap_or(0);
                        }
                    }
                    let mut body = vec![0; len];
                    let _ = r.read_exact(&mut body);
                    let _ = write!(
                        s,
                        "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                    let _ = tx.send(String::from_utf8_lossy(&body).into_owned());
                }
            });
            Self {
                url: format!("http://127.0.0.1:{port}/hook"),
                requests: rx,
            }
        }

        fn next(&self) -> Option<String> {
            self.requests.recv_timeout(Duration::from_secs(5)).ok()
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_paid_heartbeat_caches_the_hours_and_a_free_one_stores_none() {
        let h = Harness::new().await;
        let (cafe, token) = h.watched().await;
        let day = day0();
        let r = h
            .beat(&token, day + 10 * HOUR, json!({"opening": opening()}))
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
        let cached = h.db.branch_opening(&cafe.branch).await.unwrap().unwrap();
        assert_eq!(cached, (daily_hours(), "Asia/Kolkata".to_owned()));
        let written: i64 = h.sql("SELECT updated_at FROM branch_opening");
        assert_eq!(written, day + 10 * HOUR);

        // The same block again writes nothing.
        h.beat(&token, day + 11 * HOUR, json!({"opening": opening()}))
            .await;
        assert_eq!(
            h.sql::<i64>("SELECT updated_at FROM branch_opening"),
            written
        );

        let free = h.cafe(PHONE_B, "Free Arena", None).await;
        let free_token = h.pair_hub(&free).await;
        let r = h
            .beat(&free_token, day + 12 * HOUR, json!({"opening": opening()}))
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
        assert_eq!(h.db.branch_opening(&free.branch).await.unwrap(), None);
        assert_eq!(h.sql::<i64>("SELECT COUNT(*) FROM branch_opening"), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_bad_hours_block_is_ignored_and_never_fails_the_heartbeat() {
        let h = Harness::new().await;
        let (cafe, token) = h.watched().await;
        let day = day0();
        let long = format!("{}{}", daily_hours(), " ".repeat(1024));
        let bad = [
            json!({"opening_hours": "[]", "timezone": "Asia/Kolkata"}),
            json!({"opening_hours": daily_hours(), "timezone": "Mars/Base"}),
            json!({"opening_hours": long, "timezone": "Asia/Kolkata"}),
        ];
        for (n, block) in bad.iter().enumerate() {
            let r = h
                .beat(
                    &token,
                    day + (10 + n as i64) * HOUR,
                    json!({"opening": block}),
                )
                .await;
            assert_eq!(r.status, 200, "{}", r.body);
        }
        assert_eq!(h.db.branch_opening(&cafe.branch).await.unwrap(), None);

        h.beat(&token, day + 14 * HOUR, json!({"opening": opening()}))
            .await;
        let other = json!({"opening_hours": "nope", "timezone": "Asia/Kolkata"});
        let r = h
            .beat(&token, day + 15 * HOUR, json!({"opening": other}))
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
        assert_eq!(
            h.db.branch_opening(&cafe.branch).await.unwrap().unwrap().0,
            daily_hours()
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_offline_branch_in_hours_is_alerted_once() {
        let h = Harness::new().await;
        let (cafe, token) = h.watched().await;
        h.subscribe(&cafe, &endpoint(1)).await;
        let day = day0();
        let seen = day + 11 * HOUR;
        h.beat(&token, seen, json!({"opening": opening()})).await;

        // 25 minutes of silence is not enough.
        assert_eq!(h.watch(seen + 25 * MINUTE).await.offline, 0);
        assert_eq!(h.alert_rows(), 0);

        let out = h.watch(seen + 35 * MINUTE).await;
        assert_eq!((out.offline, out.watched, out.held), (1, 1, None));
        assert_eq!(h.kinds(), ["branch_offline"]);
        let id: String = h.sql("SELECT id FROM owner_alerts");
        assert_eq!(id, format!("off-{}-{seen}", cafe.branch));
        let wa = h.fake_wa().sent();
        assert_eq!(wa.len(), 1);
        assert_eq!(
            wa[0].params[1],
            "The branch has been offline for 35 minutes."
        );
        let pushes = h.fake_push().sent();
        assert_eq!(pushes.len(), 1);
        let payload: Value = serde_json::from_str(&pushes[0].payload).unwrap();
        assert_eq!(payload["url"], format!("/#today-{}", cafe.branch));
        assert_eq!(h.sql::<i64>("SELECT offline_key FROM branch_watch"), seen);

        // A rerun, now or later in the same silence, adds nothing.
        assert_eq!(
            h.watch(seen + 40 * MINUTE).await,
            WatchOutcome {
                watched: 1,
                ..Default::default()
            }
        );
        h.watch(seen + 3 * HOUR).await;
        assert_eq!(h.alert_rows(), 1);
        assert_eq!(h.fake_wa().sent().len(), 1);
        assert_eq!(h.fake_push().sent().len(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_branch_silent_all_night_is_alerted_half_an_hour_after_it_opens() {
        let h = Harness::new().await;
        let (_cafe, token) = h.watched().await;
        let day = day0();
        h.beat(&token, day + 22 * HOUR, json!({"opening": opening()}))
            .await;

        // Closed from 23:00 to 09:00: nothing, however long it is silent.
        assert_eq!(h.watch(day + 23 * HOUR + 10 * MINUTE).await.offline, 0);
        assert_eq!(h.watch(day + DAY + 8 * HOUR + 59 * MINUTE).await.offline, 0);
        // Open, but not yet 30 minutes since it opened.
        assert_eq!(h.watch(day + DAY + 9 * HOUR + 20 * MINUTE).await.offline, 0);
        assert_eq!(h.alert_rows(), 0);

        assert_eq!(h.watch(day + DAY + 9 * HOUR + 30 * MINUTE).await.offline, 1);
        let wa = h.fake_wa().sent();
        assert_eq!(
            wa[0].params[1],
            "The branch has been offline for 30 minutes."
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_branch_that_reports_but_does_not_upload_is_alerted_per_stall() {
        let h = Harness::new().await;
        let (cafe, token) = h.watched().await;
        let day = day0();
        let stale = day + 11 * HOUR;
        let up = |last_upload_at: i64, summary: i64| {
            json!({
                "opening": opening(),
                "last_upload_at": last_upload_at,
                "last_summary_at": summary,
            })
        };
        h.beat(
            &token,
            day + 11 * HOUR + 35 * MINUTE,
            up(stale, stale - 10 * MINUTE),
        )
        .await;

        let out = h.watch(day + 11 * HOUR + 35 * MINUTE).await;
        assert_eq!((out.offline, out.not_uploading), (0, 1));
        assert_eq!(h.kinds(), ["branch_not_uploading"]);
        let id: String = h.sql("SELECT id FROM owner_alerts");
        assert_eq!(id, format!("up-{}-{stale}", cafe.branch));
        assert_eq!(
            h.fake_wa().sent()[0].params[1],
            "The branch is online but has not backed up for 35 minutes."
        );
        assert_eq!(h.sql::<i64>("SELECT upload_key FROM branch_watch"), stale);
        assert_eq!(
            h.watch(day + 11 * HOUR + 36 * MINUTE).await.not_uploading,
            0
        );
        assert_eq!(h.alert_rows(), 1);

        // An upload ends the episode and clears its key.
        let fresh = day + 11 * HOUR + 50 * MINUTE;
        h.beat(&token, fresh + MINUTE, up(fresh, fresh)).await;
        assert_eq!(h.watch(fresh + MINUTE).await.not_uploading, 0);
        assert_eq!(
            h.sql::<Option<i64>>("SELECT upload_key FROM branch_watch"),
            None
        );

        // The next stall is a new alert.
        let later = fresh + 40 * MINUTE;
        h.beat(&token, later, up(fresh, fresh)).await;
        assert_eq!(h.watch(later).await.not_uploading, 1);
        assert_eq!(h.kinds(), ["branch_not_uploading", "branch_not_uploading"]);
        assert_eq!(h.sql::<i64>("SELECT upload_key FROM branch_watch"), fresh);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_return_adds_one_push_only_entry_and_a_new_outage_alerts_again() {
        let h = Harness::new().await;
        let (cafe, token) = h.watched().await;
        h.subscribe(&cafe, &endpoint(1)).await;
        let day = day0();
        let seen = day + 11 * HOUR;
        h.beat(&token, seen, json!({"opening": opening()})).await;
        assert_eq!(h.watch(seen + 35 * MINUTE).await.offline, 1);
        assert_eq!(
            (h.fake_wa().sent().len(), h.fake_push().sent().len()),
            (1, 1)
        );

        let back = seen + 50 * MINUTE;
        h.beat(&token, back, json!({})).await;
        let out = h.watch(back).await;
        assert_eq!((out.online, out.offline), (1, 0));
        assert_eq!(h.kinds(), ["branch_offline", "branch_online"]);
        let id: String = h.sql("SELECT id FROM owner_alerts WHERE kind = 'branch_online'");
        assert_eq!(id, format!("on-{}-{seen}", cafe.branch));
        // Push went out; WhatsApp did not.
        assert_eq!(h.fake_push().sent().len(), 2);
        let body: Value = serde_json::from_str(&h.fake_push().sent()[1].payload).unwrap();
        assert_eq!(body["body"], "The branch is back online after 50 minutes.");
        assert_eq!(h.fake_wa().sent().len(), 1);
        assert_eq!(h.ledger_rows(), 1);
        assert_eq!(
            h.sql::<Option<i64>>("SELECT offline_key FROM branch_watch"),
            None
        );
        assert_eq!(h.watch(back + MINUTE).await.online, 0);
        assert_eq!(h.alert_rows(), 2);

        // A new outage is a new episode.
        assert_eq!(h.watch(back + 35 * MINUTE).await.offline, 1);
        assert_eq!(h.alert_rows(), 3);
        assert_eq!(h.fake_wa().sent().len(), 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn free_branches_and_branches_without_hours_are_never_alerted() {
        let h = Harness::new().await;
        let day = day0();
        let free = h.cafe(PHONE_B, "Free Arena", None).await;
        let free_token = h.pair_hub(&free).await;
        h.beat(&free_token, day + 11 * HOUR, json!({"opening": opening()}))
            .await;

        let (_paid, token) = h.watched().await;
        // The paid Hub never sends its hours.
        h.beat(&token, day + 11 * HOUR + 10 * MINUTE, json!({}))
            .await;

        let out = h.watch(day + 13 * HOUR).await;
        assert_eq!(
            out,
            WatchOutcome {
                watched: 1,
                ..Default::default()
            }
        );
        assert_eq!(h.alert_rows(), 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_mass_outage_is_held_and_pages_the_on_call() {
        let h = Harness::new().await;
        let (cafe, _token) = h.watched().await;
        let day = day0();
        let now = day + 12 * HOUR;
        h.db.record_branch_opening(&cafe.branch, &daily_hours(), "Asia/Kolkata", T0)
            .await
            .unwrap();
        let extra = h.estate(&cafe, 9, now - MINUTE).await;
        // Five of ten are silent.
        h.silence(&cafe.branch, day + 11 * HOUR);
        for id in &extra[..4] {
            h.silence(id, day + 11 * HOUR);
        }
        let hook = Webhook::start();
        let out = h.watch_with(now, &target(Some(&hook.url))).await;
        assert_eq!((out.watched, out.held), (10, Some(5)));
        assert_eq!((out.offline, out.failed), (0, 0));
        assert_eq!(h.alert_rows(), 0);
        assert_eq!(h.sql::<i64>("SELECT COUNT(*) FROM branch_watch"), 0);
        let page: Value = serde_json::from_str(&hook.next().expect("a page")).unwrap();
        assert_eq!(
            page["text"],
            "arena-cloud staging: task offline-watch held 5 branch alerts because many branches look offline at once. See the job's logs."
        );
        // The next run looks again, and pages again while it holds.
        let out = h
            .watch_with(now + 5 * MINUTE, &target(Some(&hook.url)))
            .await;
        assert_eq!(out.held, Some(5));
        assert!(hook.next().is_some());

        // Two come back: three would alert, below the floor, so they are raised.
        h.silence(&extra[0], now + 5 * MINUTE);
        h.silence(&extra[1], now + 5 * MINUTE);
        let out = h
            .watch_with(now + 6 * MINUTE, &target(Some(&hook.url)))
            .await;
        assert_eq!((out.held, out.offline), (None, 3));
        assert_eq!(h.alert_rows(), 3);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_share_of_exactly_twenty_percent_is_not_held() {
        let h = Harness::new().await;
        let (cafe, _token) = h.watched().await;
        let day = day0();
        let now = day + 12 * HOUR;
        h.db.record_branch_opening(&cafe.branch, &daily_hours(), "Asia/Kolkata", T0)
            .await
            .unwrap();
        let extra = h.estate(&cafe, 24, now - MINUTE).await;
        h.silence(&cafe.branch, day + 11 * HOUR);
        // Five of 25 is exactly 20 percent: not more, so not held.
        for id in &extra[..4] {
            h.silence(id, day + 11 * HOUR);
        }
        let out = h.watch(now).await;
        assert_eq!((out.held, out.offline), (None, 5));
        assert_eq!(h.alert_rows(), 5);
        // A sixth is raised on its own: the five before it are already alerted.
        h.silence(&extra[4], day + 11 * HOUR);
        let out = h.watch(now + MINUTE).await;
        assert_eq!((out.held, out.offline), (None, 1));
        assert_eq!(h.alert_rows(), 6);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn one_branch_failing_to_raise_does_not_stop_the_others_or_record_its_key() {
        let h = Harness::new().await;
        let (cafe, _token) = h.watched().await;
        let day = day0();
        let now = day + 12 * HOUR;
        h.db.record_branch_opening(&cafe.branch, &daily_hours(), "Asia/Kolkata", T0)
            .await
            .unwrap();
        let extra = h.estate(&cafe, 2, now - MINUTE).await;
        let seen = day + 11 * HOUR;
        h.silence(&cafe.branch, seen);
        h.silence(&extra[0], seen);
        // The id the first branch would use is already taken by another branch's alert.
        h.exec(&format!(
            "INSERT INTO owner_alerts (id, tenant_id, branch_id, kind, params, created_at) \
             SELECT 'off-{b}-{seen}', tenant_id, '{x}', 'branch_offline', '{{\"minutes\":\"1\"}}', {T0} \
             FROM branches WHERE id = '{x}'",
            b = cafe.branch,
            x = extra[1],
        ));
        let out = h.watch(now).await;
        assert_eq!((out.offline, out.failed), (1, 1));
        assert_eq!(h.sql::<i64>("SELECT COUNT(*) FROM branch_watch"), 1);
        let keyed: String = h.sql("SELECT branch_id FROM branch_watch");
        assert_eq!(keyed, extra[0]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_new_kinds_are_not_raised_by_a_hub() {
        let h = Harness::new().await;
        let (_cafe, token) = h.watched().await;
        for (n, kind) in ["branch_not_uploading", "branch_online"].iter().enumerate() {
            let r = h
                .hub_alert(&token, &format!("NEW{n}"), kind, json!({"minutes": "20"}))
                .await;
            assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"), "{kind}");
        }
        assert_eq!(h.alert_rows(), 0);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_forced_standby_alert_names_the_manager_and_reaches_the_owner() {
    let h = Harness::new().await;
    let cafe = h.paid().await;
    h.subscribe(&cafe, &endpoint(1)).await;
    let token = h.pair_hub(&cafe).await;
    let r = h
        .hub_alert(
            &token,
            "FORCE1",
            "standby_forced",
            json!({"manager": "Asha"}),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(h.alert_rows(), 1);
    let sent = h.fake_push().sent();
    assert_eq!(sent.len(), 1);
    // The same post again raises nothing more.
    let again = h
        .hub_alert(
            &token,
            "FORCE1",
            "standby_forced",
            json!({"manager": "Asha"}),
        )
        .await;
    assert_eq!(again.status, 200);
    assert_eq!(h.alert_rows(), 1);
}
