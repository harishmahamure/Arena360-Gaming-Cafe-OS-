use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use std::net::SocketAddr;

use arena_cloud::auth::AuthState;
use arena_cloud::billing::{
    BillingProvider, FakeRazorpay, PaymentInfo, PaymentOrigin, RemoteSubscription,
};
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
use arena_cloud::reconcile::reconcile;
use arena_cloud::webhook::{
    FAKE_WEBHOOK_SECRET, ReviewError, TASK_REVIEW, WebhookSecret, approve_review,
};
use arena_cloud::{Clock, app_with_auth, auth_state};
use arena_proto::SubscriptionState;
use arena_proto::licence::{DEV_LICENCE_VERIFY_KEYS, verify};
use arena_store::control::{ControlDb, LATEST_CONTROL_VERSION, NewBackupCode};
use axum::body::{Body, to_bytes};
use axum::extract::connect_info::ConnectInfo;
use axum::http::{Request, header};
use serde_json::{Value, json};
use tokio::sync::watch;
use tower::ServiceExt;

const ORIGIN: &str = "https://start.arena360.cloud";
const PEPPER: &str = "test-pepper-0123456789abcdef0123456789";
const T0: i64 = 1_800_000_000_000;
const PHONE_A: &str = "+919876543210";

struct Harness {
    app: axum::Router,
    db: Arc<ControlDb>,
    st: Arc<AuthState>,
    /// The cloud's clock, in epoch milliseconds.
    clock: Arc<AtomicI64>,
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

impl Harness {
    async fn with_billing(billing_enabled: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = ControlDb::open_local(&dir.path().join("gz_control.db"))
            .await
            .unwrap();
        db.migrate(T0).await.unwrap();
        let db = Arc::new(db);
        let now = Arc::new(AtomicI64::new(T0));
        let ticking = Arc::clone(&now);
        let clock: Clock = Arc::new(move || ticking.load(Ordering::SeqCst));
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
            clock: now,
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

const HUB_IP: &str = "198.51.100.4";

impl Harness {
    fn fake(&self) -> &FakeRazorpay {
        match self.st.billing.get() {
            Some(BillingProvider::Fake(f)) => f,
            _ => panic!("the test harness uses the fake billing provider"),
        }
    }

    async fn subscribe(&self, branch: &str, access: &str, body: Value) -> Reply {
        self.call(
            "POST",
            &format!("/v1/branches/{branch}/subscription"),
            Some(body),
            access,
        )
        .await
    }

    /// An owner with a saved café and a `pending` Basic monthly subscription (Razorpay id
    /// `sub_fake_001`); billing on.
    async fn pending() -> (Self, String, String, String) {
        Self::pending_with(json!({"tier": "basic", "cycle": "monthly", "billing_country": "IN"}))
            .await
    }

    async fn pending_with(body: Value) -> (Self, String, String, String) {
        let h = Self::with_billing(true).await;
        let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
        let branch = h.cafe_done(&a).await;
        let r = h.subscribe(&branch, &a, body).await;
        assert_eq!(r.status, 201, "{}", r.body);
        let id = r.body["subscription_id"].as_str().unwrap().to_owned();
        (h, a, branch, id)
    }

    async fn deliver(&self, body: &str, signature: Option<&str>, event_id: Option<&str>) -> Reply {
        let mut req = Request::builder()
            .method("POST")
            .uri("/v1/webhooks/razorpay")
            .header("content-type", "application/json");
        if let Some(s) = signature {
            req = req.header("x-razorpay-signature", s);
        }
        if let Some(e) = event_id {
            req = req.header("x-razorpay-event-id", e);
        }
        let res = self
            .app
            .clone()
            .oneshot(req.body(Body::from(body.to_owned())).unwrap())
            .await
            .unwrap();
        let status = res.status().as_u16();
        let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        Reply { status, body }
    }

    /// Delivers `body` signed with the secret the harness knows, as event `event_id`.
    async fn send(&self, event_id: &str, body: &Value) -> Reply {
        let text = body.to_string();
        let sig = WebhookSecret::new(FAKE_WEBHOOK_SECRET)
            .unwrap()
            .sign(text.as_bytes());
        self.deliver(&text, Some(&sig), Some(event_id)).await
    }

    async fn only(&self, branch: &str) -> arena_store::control::Subscription {
        let rows = self.db.subscriptions_of_branch(branch).await.unwrap();
        assert_eq!(rows.len(), 1);
        rows.into_iter().next().unwrap()
    }

    fn count(&self, sql: &str) -> i64 {
        let path = self._dir.path().join("gz_control.db");
        rusqlite::Connection::open(path)
            .unwrap()
            .query_row(sql, [], |r| r.get(0))
            .unwrap()
    }

    async fn choose_free(&self, access: &str) {
        let r = self
            .call(
                "PUT",
                "/v1/wizard/tier",
                Some(json!({"tier": "free"})),
                access,
            )
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
    }

    async fn hub_token(&self, access: &str) -> String {
        let r = self
            .call(
                "POST",
                "/v1/wizard/install-hub/code",
                Some(json!({})),
                access,
            )
            .await;
        assert_eq!(r.status, 200, "{}", r.body);
        let mut req = Request::builder()
            .method("POST")
            .uri("/v1/pair")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "code": r.body["code"],
                    "platform": "android",
                    "hub_instance_id": "01HUBINSTANCE",
                    "version": "0.1.0",
                })
                .to_string(),
            ))
            .unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::new(HUB_IP.parse().unwrap(), 40000)));
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(status, 200, "{v}");
        v["token"].as_str().unwrap().to_owned()
    }

    async fn heartbeat(&self, token: &str) -> Reply {
        let req = Request::builder()
            .method("POST")
            .uri("/v1/heartbeat")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(
                json!({
                    "epoch": "3:HUBA",
                    "version": "0.2.0",
                    "schema_version": 12,
                    "last_seq_uploaded": 0,
                })
                .to_string(),
            ))
            .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        Reply {
            status,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }
}

/// A Razorpay event body. `card_international: Some(..)` adds a card payment.
fn event(kind: &str, rid: &str, end_s: i64, card_international: Option<bool>) -> Value {
    let mut payload = json!({
        "subscription": {"entity": {"id": rid, "status": "active", "current_end": end_s}},
    });
    if let Some(intl) = card_international {
        payload["payment"] = json!({"entity": {
            "id": "pay_1", "method": "card", "card": {"international": intl}
        }});
    }
    json!({"event": kind, "created_at": 1_800_000_100, "payload": payload})
}

const PERIOD_END: i64 = 1_802_592_000;

#[tokio::test]
async fn a_bad_signature_is_refused_counted_and_writes_nothing() {
    let (h, _a, _branch, _id) = Harness::pending().await;
    let body = event(
        "subscription.charged",
        "sub_fake_001",
        PERIOD_END,
        Some(false),
    )
    .to_string();
    let good = WebhookSecret::new(FAKE_WEBHOOK_SECRET)
        .unwrap()
        .sign(body.as_bytes());
    let other = WebhookSecret::new("whsec_other")
        .unwrap()
        .sign(body.as_bytes());
    let tampered = body.replace("sub_fake_001", "sub_fake_002");
    for (text, sig) in [
        (body.as_str(), Some(other.as_str())),
        (body.as_str(), None),
        (body.as_str(), Some("")),
        (body.as_str(), Some("not hex")),
        (tampered.as_str(), Some(good.as_str())),
    ] {
        let r = h.deliver(text, sig, Some("evt_bad")).await;
        assert_eq!((r.status, r.code()), (400, "BAD_REQUEST"), "{}", r.body);
    }
    assert_eq!(h.st.webhook_metrics.counts().failures, 5);
    assert_eq!(h.count("SELECT COUNT(*) FROM billing_events"), 0);
    assert_eq!(h.count("SELECT COUNT(*) FROM subscription_history"), 0);
    assert_eq!(
        h.count("SELECT COUNT(*) FROM entitlements WHERE source = 'paid'"),
        0
    );
}

#[tokio::test]
async fn a_signed_body_that_is_not_an_event_is_refused_before_anything_is_written() {
    let (h, _a, _branch, _id) = Harness::pending().await;
    let secret = WebhookSecret::new(FAKE_WEBHOOK_SECRET).unwrap();
    let junk = "not json";
    let r = h
        .deliver(junk, Some(&secret.sign(junk.as_bytes())), Some("evt_1"))
        .await;
    assert_eq!((r.status, r.code()), (400, "BAD_REQUEST"));
    let ok = event("subscription.charged", "sub_fake_001", PERIOD_END, None).to_string();
    let r = h
        .deliver(&ok, Some(&secret.sign(ok.as_bytes())), None)
        .await;
    assert_eq!((r.status, r.code()), (400, "BAD_REQUEST"), "no event id");
    assert_eq!(h.st.webhook_metrics.counts().failures, 2);
    assert_eq!(h.count("SELECT COUNT(*) FROM billing_events"), 0);
}

#[tokio::test]
async fn authenticated_then_charged_activates_and_issues_the_entitlement() {
    let (h, a, branch, id) = Harness::pending_with(json!({
        "tier": "basic", "cycle": "monthly", "billing_country": "IN",
        "add_ons": {"devices_20": 2, "tunnel": 1},
    }))
    .await;
    h.choose_free(&a).await;
    let free = h.db.entitlement(&branch).await.unwrap().unwrap();
    assert_eq!(free.source.as_str(), "free");

    let r = h
        .send(
            "evt_auth",
            &event(
                "subscription.authenticated",
                "sub_fake_001",
                PERIOD_END,
                None,
            ),
        )
        .await;
    assert_eq!(
        (r.status, r.body["status"].as_str()),
        (200, Some("recorded"))
    );
    assert_eq!(h.only(&branch).await.state, SubscriptionState::Pending);
    let still = h.db.entitlement(&branch).await.unwrap().unwrap();
    assert_eq!(still.source.as_str(), "free");

    let r = h
        .send(
            "evt_charge",
            &event(
                "subscription.charged",
                "sub_fake_001",
                PERIOD_END,
                Some(false),
            ),
        )
        .await;
    assert_eq!(
        (r.status, r.body["status"].as_str()),
        (200, Some("applied")),
        "{}",
        r.body
    );

    let s = h.only(&branch).await;
    assert_eq!(s.id, id);
    assert_eq!(s.state, SubscriptionState::Active);
    assert_eq!(s.current_period_end, Some(PERIOD_END * 1000));
    assert_eq!(s.first_paid_at, Some(1_800_000_100_000));
    assert_eq!(s.payment_country.as_deref(), Some("IN"));
    assert_eq!(s.mandate_method.map(|m| m.as_str()), Some("card"));
    let e = h.db.entitlement(&branch).await.unwrap().unwrap();
    assert_eq!((e.tier.as_str(), e.source.as_str()), ("basic", "paid"));
    assert_eq!(e.addons, ["devices_20:2", "tunnel"]);
    assert_eq!(e.until, None);

    let history = h.db.subscription_history(&s.id).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(
        (
            history[0].old_state.as_str(),
            history[0].new_state.as_str(),
            history[0].cause.as_str()
        ),
        ("pending", "active", "evt_charge")
    );
    assert_eq!(history[0].cloud_time, T0);
    let events = h.db.billing_events().await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].razorpay_event_id, "evt_auth");
    assert_eq!(events[0].applied_at, None);
    assert_eq!(events[1].kind, "subscription.charged");
    assert_eq!(events[1].source, "webhook");
    assert_eq!(events[1].subscription_id.as_deref(), Some(s.id.as_str()));
    assert!(events[1].applied_at.is_some());
    assert_eq!(events[1].payload_hash.len(), 64);
    let counts = h.st.webhook_metrics.counts();
    assert_eq!((counts.failures, counts.handled), (0, 2));
    // The wizard's plan step is done once the entitlement exists.
    assert_eq!(
        h.count("SELECT COUNT(*) FROM onboarding_progress WHERE step = 'tier' AND state = 'done'"),
        1
    );
}

#[tokio::test]
async fn charged_then_authenticated_ends_in_the_same_place() {
    let (h, _a, branch, _id) = Harness::pending().await;
    let r = h
        .send(
            "evt_charge",
            &event(
                "subscription.charged",
                "sub_fake_001",
                PERIOD_END,
                Some(false),
            ),
        )
        .await;
    assert_eq!(r.body["status"], "applied");
    let r = h
        .send(
            "evt_auth",
            &event(
                "subscription.authenticated",
                "sub_fake_001",
                PERIOD_END,
                None,
            ),
        )
        .await;
    assert_eq!(r.body["status"], "recorded");
    let s = h.only(&branch).await;
    assert_eq!(s.state, SubscriptionState::Active);
    assert!(s.first_paid_at.is_some());
    assert_eq!(h.db.subscription_history(&s.id).await.unwrap().len(), 1);
    let e = h.db.entitlement(&branch).await.unwrap().unwrap();
    assert_eq!(e.source.as_str(), "paid");
}

#[tokio::test]
async fn activated_alone_activates_and_a_later_charge_dates_the_first_payment() {
    let (h, _a, branch, _id) = Harness::pending().await;
    let r = h
        .send(
            "evt_act",
            &event("subscription.activated", "sub_fake_001", PERIOD_END, None),
        )
        .await;
    assert_eq!(r.body["status"], "applied");
    let s = h.only(&branch).await;
    assert_eq!(
        (s.state, s.first_paid_at),
        (SubscriptionState::Active, None)
    );
    let r = h
        .send(
            "evt_charge",
            &event(
                "subscription.charged",
                "sub_fake_001",
                PERIOD_END,
                Some(false),
            ),
        )
        .await;
    assert_eq!(r.body["status"], "applied");
    let s = h.only(&branch).await;
    assert_eq!(s.state, SubscriptionState::Active);
    assert_eq!(s.first_paid_at, Some(1_800_000_100_000));
    assert_eq!(s.current_period_end, Some(PERIOD_END * 1000));
}

#[tokio::test]
async fn a_repeated_event_id_applies_nothing() {
    let (h, _a, branch, _id) = Harness::pending().await;
    let body = event(
        "subscription.charged",
        "sub_fake_001",
        PERIOD_END,
        Some(false),
    );
    assert_eq!(h.send("evt_1", &body).await.body["status"], "applied");
    let before = h.only(&branch).await;
    let r = h.send("evt_1", &body).await;
    assert_eq!(
        (r.status, r.body["status"].as_str()),
        (200, Some("duplicate"))
    );
    // The same id with a different body is still the same event.
    let other = event(
        "subscription.charged",
        "sub_fake_001",
        PERIOD_END + 86_400,
        Some(false),
    );
    assert_eq!(h.send("evt_1", &other).await.body["status"], "duplicate");
    assert_eq!(h.only(&branch).await, before);
    assert_eq!(h.count("SELECT COUNT(*) FROM billing_events"), 1);
    assert_eq!(h.count("SELECT COUNT(*) FROM subscription_history"), 1);
    assert_eq!(h.st.webhook_metrics.counts().duplicates, 2);
}

#[tokio::test]
async fn the_same_event_delivered_twice_at_once_applies_once() {
    let (h, _a, branch, _id) = Harness::pending().await;
    let body = event(
        "subscription.charged",
        "sub_fake_001",
        PERIOD_END,
        Some(false),
    );
    let (a, b) = tokio::join!(h.send("evt_race", &body), h.send("evt_race", &body));
    let mut statuses = [
        a.body["status"].as_str().unwrap().to_owned(),
        b.body["status"].as_str().unwrap().to_owned(),
    ];
    statuses.sort();
    assert_eq!(statuses, ["applied", "duplicate"]);
    assert_eq!(h.count("SELECT COUNT(*) FROM billing_events"), 1);
    assert_eq!(h.count("SELECT COUNT(*) FROM subscription_history"), 1);
    assert_eq!(h.only(&branch).await.state, SubscriptionState::Active);
}

#[tokio::test]
async fn a_paid_entitlement_replaces_a_comp_one() {
    let (h, _a, branch, _id) = Harness::pending().await;
    let g =
        h.db.grant_comp(&arena_store::control::CompGrant {
            branch_id: &branch,
            tier: arena_proto::Tier::Premium,
            until: T0 + 30 * 86_400_000,
            audit_id: "audit-1",
            operator: "ops",
            reason: "beta cafe",
            details_json: "{}",
            now: T0,
        })
        .await
        .unwrap();
    assert!(matches!(
        g,
        arena_store::control::GrantOutcome::Granted { .. }
    ));
    h.send(
        "evt_1",
        &event(
            "subscription.charged",
            "sub_fake_001",
            PERIOD_END,
            Some(false),
        ),
    )
    .await;
    let e = h.db.entitlement(&branch).await.unwrap().unwrap();
    assert_eq!(
        (e.tier.as_str(), e.source.as_str(), e.until),
        ("basic", "paid", None)
    );
}

#[tokio::test]
async fn an_event_for_a_subscription_we_do_not_know_is_recorded_and_changes_nothing() {
    let (h, _a, branch, _id) = Harness::pending().await;
    let r = h
        .send(
            "evt_x",
            &event(
                "subscription.charged",
                "sub_unknown",
                PERIOD_END,
                Some(false),
            ),
        )
        .await;
    assert_eq!(
        (r.status, r.body["status"].as_str()),
        (200, Some("unmatched"))
    );
    assert_eq!(h.only(&branch).await.state, SubscriptionState::Pending);
    let events = h.db.billing_events().await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        (events[0].subscription_id.clone(), events[0].applied_at),
        (None, None)
    );
    let r = h
        .send("evt_y", &json!({"event": "payment.failed", "payload": {}}))
        .await;
    assert_eq!(r.body["status"], "unmatched");
}

#[tokio::test]
async fn a_first_payment_from_another_country_is_held_until_an_operator_clears_it() {
    let (h, a, branch, id) = Harness::pending().await;
    let r = h
        .send(
            "evt_charge",
            &event(
                "subscription.charged",
                "sub_fake_001",
                PERIOD_END,
                Some(true),
            ),
        )
        .await;
    assert_eq!(
        (r.status, r.body["status"].as_str()),
        (200, Some("applied"))
    );
    let s = h.only(&branch).await;
    assert_eq!(s.state, SubscriptionState::PendingReview);
    assert_eq!(s.payment_country.as_deref(), Some("INTL"));
    assert!(s.first_paid_at.is_some(), "the money was taken");
    assert_eq!(s.current_period_end, Some(PERIOD_END * 1000));
    assert_eq!(
        h.count("SELECT COUNT(*) FROM entitlements WHERE source = 'paid'"),
        0
    );

    let tasks = h.db.open_ops_tasks(TASK_REVIEW).await.unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].target, id);
    assert!(tasks[0].details_json.contains(&branch));

    let status = h
        .call(
            "GET",
            &format!("/v1/branches/{branch}/subscription"),
            None,
            &a,
        )
        .await;
    assert_eq!(status.status, 200, "{}", status.body);
    assert_eq!(status.body["state"], "pending_review");
    assert_eq!(
        status.body["message"],
        "We're checking your payment \u{2014} this usually takes a working day"
    );

    // A renewal-shaped event does not unhold it.
    let r = h
        .send(
            "evt_more",
            &event(
                "subscription.charged",
                "sub_fake_001",
                PERIOD_END + 86_400,
                Some(false),
            ),
        )
        .await;
    assert_eq!(r.body["status"], "recorded");
    assert_eq!(
        h.only(&branch).await.state,
        SubscriptionState::PendingReview
    );

    let cleared = approve_review(
        &h.db,
        &id,
        "asha",
        "bank confirmed the card is Indian",
        T0 + 5,
    )
    .await
    .unwrap();
    assert_eq!(cleared.state, SubscriptionState::Active);
    let e = h.db.entitlement(&branch).await.unwrap().unwrap();
    assert_eq!((e.tier.as_str(), e.source.as_str()), ("basic", "paid"));
    assert_eq!(e.issued_at, T0 + 5);
    let audit = h.db.ops_audit_for(&id).await.unwrap();
    assert_eq!(audit.len(), 1);
    assert_eq!(
        (
            audit[0].operator.as_str(),
            audit[0].action.as_str(),
            audit[0].reason.as_str()
        ),
        (
            "asha",
            "subscription_review",
            "bank confirmed the card is Indian"
        )
    );
    assert!(h.db.open_ops_tasks(TASK_REVIEW).await.unwrap().is_empty());
    let history = h.db.subscription_history(&id).await.unwrap();
    assert_eq!(
        history
            .iter()
            .map(|r| (r.old_state.as_str(), r.new_state.as_str(), r.cause.as_str()))
            .collect::<Vec<_>>(),
        [
            ("pending", "pending_review", "evt_charge"),
            ("pending_review", "active", "ops:review:asha"),
        ]
    );
    let status = h
        .call(
            "GET",
            &format!("/v1/branches/{branch}/subscription"),
            None,
            &a,
        )
        .await;
    assert_eq!(status.body["state"], "active");
    assert!(status.body.get("message").is_none());

    assert_eq!(
        approve_review(&h.db, &id, "asha", "again", T0 + 6)
            .await
            .unwrap_err(),
        ReviewError::NotInReview(SubscriptionState::Active)
    );
    assert_eq!(
        approve_review(&h.db, "nope", "asha", "x", T0 + 6)
            .await
            .unwrap_err(),
        ReviewError::Unknown("nope".to_owned())
    );
}

#[tokio::test]
async fn a_renewal_moves_the_period_forward_and_an_older_event_never_moves_it_back() {
    let (h, _a, branch, id) = Harness::pending().await;
    h.send(
        "evt_1",
        &event(
            "subscription.charged",
            "sub_fake_001",
            PERIOD_END,
            Some(false),
        ),
    )
    .await;
    // The renewal falls into grace (set directly: lapse handling is a later story).
    h.sql(&format!(
        "UPDATE subscriptions SET state = 'grace', grace_until = {} WHERE id = '{id}'",
        T0 + 7 * 86_400_000
    ));
    let month = 30 * 86_400;
    let r = h
        .send(
            "evt_2",
            &event(
                "subscription.charged",
                "sub_fake_001",
                PERIOD_END + month,
                Some(false),
            ),
        )
        .await;
    assert_eq!(r.body["status"], "applied");
    let s = h.only(&branch).await;
    assert_eq!(s.state, SubscriptionState::Active);
    assert_eq!(s.current_period_end, Some((PERIOD_END + month) * 1000));
    assert_eq!(s.grace_until, None);

    // Events for earlier periods arrive late.
    for (n, end) in [("evt_old", PERIOD_END), ("evt_older", PERIOD_END - month)] {
        let r = h
            .send(
                n,
                &event("subscription.charged", "sub_fake_001", end, Some(false)),
            )
            .await;
        assert_eq!(r.body["status"], "recorded");
    }
    assert_eq!(
        h.only(&branch).await.current_period_end,
        Some((PERIOD_END + month) * 1000)
    );
    let history = h.db.subscription_history(&id).await.unwrap();
    assert_eq!(
        history
            .iter()
            .map(|r| (r.old_state.as_str(), r.new_state.as_str()))
            .collect::<Vec<_>>(),
        [("pending", "active"), ("grace", "active")]
    );
    // An older renewal never lifts a subscription out of grace either.
    h.sql(&format!(
        "UPDATE subscriptions SET state = 'grace', grace_until = 1 WHERE id = '{id}'"
    ));
    let r = h
        .send(
            "evt_old_in_grace",
            &event(
                "subscription.charged",
                "sub_fake_001",
                PERIOD_END,
                Some(false),
            ),
        )
        .await;
    assert_eq!(r.body["status"], "recorded");
    assert_eq!(h.only(&branch).await.state, SubscriptionState::Grace);
}

fn remote(status: &str, paid: u32, origin: PaymentOrigin) -> RemoteSubscription {
    RemoteSubscription {
        status: status.to_owned(),
        current_end_ms: Some(PERIOD_END * 1000),
        paid_count: paid,
        payment: (paid > 0).then_some(PaymentInfo {
            method: Some(arena_proto::MandateMethod::Card),
            origin,
            detail: None,
        }),
    }
}

fn india() -> PaymentOrigin {
    PaymentOrigin::Country("IN".to_owned())
}

#[tokio::test]
async fn the_reconcile_applies_what_the_webhook_never_delivered_through_the_same_path() {
    let (h, _a, branch, id) = Harness::pending().await;
    let provider = h.st.billing.get().unwrap();
    // Nothing known for it yet, or only authenticated: nothing moves.
    let out = reconcile(&h.db, provider, T0).await.unwrap();
    assert_eq!((out.checked, out.applied), (0, 0));
    h.fake().set_remote(
        "sub_fake_001",
        remote("authenticated", 0, PaymentOrigin::Unknown),
    );
    let out = reconcile(&h.db, provider, T0 + 1).await.unwrap();
    assert_eq!((out.checked, out.applied), (1, 0));
    assert_eq!(h.only(&branch).await.state, SubscriptionState::Pending);

    h.fake()
        .set_remote("sub_fake_001", remote("active", 1, india()));
    let out = reconcile(&h.db, provider, T0 + 2).await.unwrap();
    assert_eq!((out.checked, out.applied, out.failed), (1, 1, 0));
    let s = h.only(&branch).await;
    assert_eq!(s.state, SubscriptionState::Active);
    assert_eq!(s.current_period_end, Some(PERIOD_END * 1000));
    assert!(s.first_paid_at.is_some());
    let e = h.db.entitlement(&branch).await.unwrap().unwrap();
    assert_eq!(e.source.as_str(), "paid");
    let history = h.db.subscription_history(&id).await.unwrap();
    assert_eq!(history.len(), 1);
    assert!(
        history[0]
            .cause
            .starts_with("reconcile:sub_fake_001:active:1:")
    );
    let events = h.db.billing_events().await.unwrap();
    assert!(
        events
            .iter()
            .any(|e| e.source == "reconcile" && e.applied_at.is_some())
    );

    // Active rows are not read again, and a webhook for the same state afterwards changes nothing.
    let out = reconcile(&h.db, provider, T0 + 3).await.unwrap();
    assert_eq!(out.checked, 0);
    let r = h
        .send(
            "evt_late",
            &event(
                "subscription.charged",
                "sub_fake_001",
                PERIOD_END,
                Some(false),
            ),
        )
        .await;
    assert_eq!(r.body["status"], "recorded");
    assert_eq!(h.db.subscription_history(&id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn the_reconcile_holds_a_foreign_first_payment_and_catches_a_missed_renewal() {
    let (h, _a, branch, _id) = Harness::pending().await;
    let provider = h.st.billing.get().unwrap();
    h.fake().set_remote(
        "sub_fake_001",
        remote("active", 1, PaymentOrigin::International),
    );
    let out = reconcile(&h.db, provider, T0).await.unwrap();
    assert_eq!(out.applied, 1);
    assert_eq!(
        h.only(&branch).await.state,
        SubscriptionState::PendingReview
    );
    assert_eq!(h.db.open_ops_tasks(TASK_REVIEW).await.unwrap().len(), 1);
    let out = reconcile(&h.db, provider, T0 + 1).await.unwrap();
    assert_eq!(out.checked, 0, "pending_review is not reconciled");

    let (h, _a, branch, id) = Harness::pending().await;
    let provider = h.st.billing.get().unwrap();
    h.send(
        "evt_1",
        &event(
            "subscription.charged",
            "sub_fake_001",
            PERIOD_END,
            Some(false),
        ),
    )
    .await;
    h.sql(&format!(
        "UPDATE subscriptions SET state = 'renewal_needs_approval' WHERE id = '{id}'"
    ));
    let mut renewed = remote("active", 2, india());
    renewed.current_end_ms = Some((PERIOD_END + 30 * 86_400) * 1000);
    h.fake().set_remote("sub_fake_001", renewed);
    let out = reconcile(&h.db, provider, T0 + 2).await.unwrap();
    assert_eq!((out.checked, out.applied), (1, 1));
    let s = h.only(&branch).await;
    assert_eq!(s.state, SubscriptionState::Active);
    assert_eq!(
        s.current_period_end,
        Some((PERIOD_END + 30 * 86_400) * 1000)
    );
}

#[tokio::test]
async fn a_provider_failure_leaves_the_subscription_for_the_next_reconcile() {
    let (h, _a, branch, _id) = Harness::pending().await;
    let provider = h.st.billing.get().unwrap();
    h.fake()
        .set_remote("sub_fake_001", remote("active", 1, india()));
    h.fake().fail_subscriptions(true);
    let out = reconcile(&h.db, provider, T0).await.unwrap();
    assert_eq!((out.checked, out.failed, out.applied), (0, 1, 0));
    assert_eq!(h.only(&branch).await.state, SubscriptionState::Pending);
    h.fake().fail_subscriptions(false);
    let out = reconcile(&h.db, provider, T0 + 1).await.unwrap();
    assert_eq!(out.applied, 1);
}

#[tokio::test]
async fn the_next_heartbeat_carries_the_paid_licence_and_no_command_is_posted() {
    let (h, a, branch, _id) = Harness::pending_with(json!({
        "tier": "basic", "cycle": "monthly", "billing_country": "IN",
        "add_ons": {"devices_20": 2, "tunnel": 1},
    }))
    .await;
    h.choose_free(&a).await;
    let token = h.hub_token(&a).await;
    let claims = |r: &Reply| {
        verify(
            r.body["licence"].as_str().unwrap(),
            DEV_LICENCE_VERIFY_KEYS,
            T0,
        )
        .unwrap()
    };
    let before = h.heartbeat(&token).await;
    assert_eq!(before.status, 200, "{}", before.body);
    assert_eq!(claims(&before).tier, arena_proto::Tier::Free);
    assert_eq!(claims(&before).cycle, None);

    h.send(
        "evt_1",
        &event(
            "subscription.charged",
            "sub_fake_001",
            PERIOD_END,
            Some(false),
        ),
    )
    .await;
    // The heartbeat limit is one per 5 s per Hub and the harness clock is fixed.
    h.sql("DELETE FROM rate_limits");
    let after = h.heartbeat(&token).await;
    assert_eq!(after.status, 200, "{}", after.body);
    let c = claims(&after);
    assert_eq!(c.tier, arena_proto::Tier::Basic);
    assert_eq!(c.limits.devices, 60 + 40);
    // Basic with the Tunnel add-on: the Basic flags plus the tunnel and live view (Story 10.6).
    for flag in [
        "owner_app",
        "kiosk_orders",
        "kitchen",
        "standby",
        "tunnel",
        "live_view",
    ] {
        assert!(c.flags.iter().any(|f| f == flag), "{flag}");
    }
    assert_eq!(c.cycle, Some(arena_proto::BillingCycle::Monthly));
    assert_eq!(
        (c.limits.users, c.limits.branches),
        (arena_proto::UNLIMITED, arena_proto::UNLIMITED)
    );
    assert_eq!((c.upload_interval_s, c.summary_interval_s), (900, 900));
    assert_eq!(c.retention_days, 365);
    assert_eq!(
        h.count(&format!(
            "SELECT COUNT(*) FROM commands WHERE branch_id = '{branch}'"
        )),
        0
    );
}

#[tokio::test]
async fn the_ops_command_approves_a_held_payment_and_refuses_anything_else() {
    use arena_cloud::config::SubscriptionReviewConfig;
    use arena_cloud::ops::{OpsError, subscription_review};

    let (h, _a, branch, id) = Harness::pending().await;
    let cfg = |sub: &str, operator: Option<&str>| SubscriptionReviewConfig {
        subscription_id: sub.to_owned(),
        operator: operator.map(str::to_owned),
        reason: "card confirmed Indian".to_owned(),
        dev_data_dir: Some(h._dir.path().to_path_buf()),
    };
    let no_env = |_: &str| None;
    // Still `pending`: nothing to review.
    let err = subscription_review(&cfg(&id, Some("asha")), no_env, T0)
        .await
        .unwrap_err();
    assert_eq!(
        err,
        OpsError::NotInReview {
            subscription_id: id.clone(),
            state: "pending"
        }
    );
    let err = subscription_review(&cfg("nope", Some("asha")), no_env, T0)
        .await
        .unwrap_err();
    assert_eq!(err, OpsError::UnknownSubscription("nope".to_owned()));
    h.send(
        "evt_charge",
        &event(
            "subscription.charged",
            "sub_fake_001",
            PERIOD_END,
            Some(true),
        ),
    )
    .await;
    // No operator named and none in the environment.
    let err = subscription_review(&cfg(&id, None), no_env, T0)
        .await
        .unwrap_err();
    assert!(matches!(err, OpsError::Config(_)), "{err}");
    assert_eq!(
        h.only(&branch).await.state,
        SubscriptionState::PendingReview
    );
    // The environment's user is the operator when `--operator` is not given.
    let env = |k: &str| (k == "USER").then(|| std::ffi::OsString::from("sam"));
    let lines = subscription_review(&cfg(&id, None), env, T0 + 1)
        .await
        .unwrap();
    assert!(lines[0].contains("is active"), "{lines:?}");
    assert_eq!(h.only(&branch).await.state, SubscriptionState::Active);
    assert_eq!(h.db.ops_audit_for(&id).await.unwrap()[0].operator, "sam");
}

#[tokio::test]
async fn an_applied_event_wakes_the_branchs_wizard_stream_and_a_repeat_does_not() {
    let (h, _a, branch, _id) = Harness::pending().await;
    let mut rx = h.st.wizard_tx.subscribe();

    // Only recorded: the plan step did not move, so nobody is woken.
    let r = h
        .send(
            "evt_auth",
            &event(
                "subscription.authenticated",
                "sub_fake_001",
                PERIOD_END,
                None,
            ),
        )
        .await;
    assert_eq!(r.body["status"], "recorded");
    assert!(rx.try_recv().is_err());

    let charged = event(
        "subscription.charged",
        "sub_fake_001",
        PERIOD_END,
        Some(false),
    );
    let r = h.send("evt_charged", &charged).await;
    assert_eq!(r.body["status"], "applied");
    assert_eq!(rx.try_recv().unwrap(), branch);

    let r = h.send("evt_charged", &charged).await;
    assert_eq!(r.body["status"], "duplicate");
    assert!(rx.try_recv().is_err());
}

// Story 10.7: every charge gets one immutable GST tax invoice.

use arena_cloud::invoice::{self, render_once};
use arena_cloud::store::MemoryStore;
use arena_store::control::{EventOutcome, EventRecord, Invoice, NewInvoice, SubscriptionChange};

impl Harness {
    async fn charge(&self, event_id: &str, end_s: i64) -> Reply {
        self.send(
            event_id,
            &event("subscription.charged", "sub_fake_001", end_s, Some(false)),
        )
        .await
    }

    async fn invoices(&self) -> Vec<Invoice> {
        self.db.invoices_to_render(1000).await.unwrap()
    }

    fn alerts(&self) -> i64 {
        self.count("SELECT COUNT(*) FROM owner_alerts WHERE kind = 'invoice_ready'")
    }
}

fn lines_of(inv: &Invoice) -> Vec<invoice::Line> {
    serde_json::from_str(&inv.lines_json).unwrap()
}

#[tokio::test]
async fn a_first_charge_issues_the_numbered_invoice_priced_from_the_locked_book() {
    let (h, _a, branch, _id) = Harness::pending_with(json!({
        "tier": "basic", "cycle": "monthly", "billing_country": "IN",
        "add_ons": {"devices_20": 2, "tunnel": 1},
    }))
    .await;
    // Not a charge: no invoice.
    h.send(
        "evt_auth",
        &event(
            "subscription.authenticated",
            "sub_fake_001",
            PERIOD_END,
            None,
        ),
    )
    .await;
    h.send(
        "evt_act",
        &event("subscription.activated", "sub_fake_001", PERIOD_END, None),
    )
    .await;
    assert!(h.invoices().await.is_empty());

    let r = h.charge("evt_charge", PERIOD_END).await;
    assert_eq!(r.body["status"], "applied", "{}", r.body);
    let sub = h.only(&branch).await;
    let quote = arena_cloud::subscription::quote_for(&h.db, &sub)
        .await
        .unwrap();

    let all = h.invoices().await;
    assert_eq!(all.len(), 1);
    let issued = h.db.invoices_for_event("evt_charge").await.unwrap();
    assert_eq!(issued, all);
    let inv = &issued[0];
    // T0 is 15 Jan 2027: financial year 2026-27.
    assert_eq!(inv.number, "A360/2026-27/000001");
    assert_eq!(
        (inv.fy.as_str(), inv.kind.as_str(), inv.seq),
        ("2026-27", "invoice", 1)
    );
    assert_eq!(h.db.invoice(&inv.number).await.unwrap().as_ref(), Some(inv));
    assert_eq!(
        (inv.subtotal_minor, inv.total_minor),
        (quote.amount_minor, quote.total_minor)
    );
    // The branch has no GSTIN: place of supply is the seller's state, CGST + SGST.
    assert_eq!(inv.cgst_minor + inv.sgst_minor, quote.tax_minor);
    assert_eq!(inv.igst_minor, 0);
    assert!(inv.cgst_minor <= inv.sgst_minor);
    assert_eq!(inv.buyer_gstin, None);
    assert_eq!(inv.buyer_state_code.as_deref(), Some("27"));
    assert_eq!((inv.band.as_str(), inv.currency.as_str()), ("IN", "INR"));
    assert_eq!(inv.sac, "998314");
    assert_eq!(inv.buyer_name, "Pixel Arena Gaming");
    assert_eq!(inv.subscription_id, sub.id);
    assert_eq!(inv.branch_id, branch);
    assert_eq!(inv.issued_at, T0);
    assert_eq!(inv.period_end, PERIOD_END * 1000);
    assert!(inv.period_start < inv.period_end);
    assert_eq!(inv.seller_gstin, invoice::Seller::dev().gstin);
    assert_eq!((inv.pdf_key.as_deref(), inv.notified_at), (None, None));
    // One line per item, add-ons with their quantity.
    let lines = lines_of(inv);
    assert_eq!(
        lines.iter().map(|l| l.quantity).collect::<Vec<_>>(),
        [1, 2, 1]
    );
    assert_eq!(
        lines.iter().map(|l| l.amount_minor).sum::<i64>(),
        quote.amount_minor
    );
}

#[tokio::test]
async fn the_buyers_gstin_state_picks_cgst_sgst_or_igst() {
    let (h, _a, branch, _id) = Harness::pending().await;
    h.sql(&format!(
        "UPDATE branches SET gst_registered = 1, gstin = '27AAPFU0939F1ZV' WHERE id = '{branch}'"
    ));
    h.charge("evt_1", PERIOD_END).await;
    let same = h.db.invoices_for_event("evt_1").await.unwrap().remove(0);
    assert_eq!(same.buyer_gstin.as_deref(), Some("27AAPFU0939F1ZV"));
    assert_eq!(same.buyer_state_code.as_deref(), Some("27"));
    assert_eq!(same.igst_minor, 0);
    assert_eq!(
        same.cgst_minor + same.sgst_minor,
        same.total_minor - same.subtotal_minor
    );
    assert!(same.cgst_minor > 0);

    // The owner moves the registration to Karnataka before the next renewal.
    h.sql(&format!(
        "UPDATE branches SET gstin = '29AAGCB7383J1Z4' WHERE id = '{branch}'"
    ));
    h.charge("evt_2", PERIOD_END + 30 * 86_400).await;
    let other = h.db.invoices_for_event("evt_2").await.unwrap().remove(0);
    assert_eq!(other.number, "A360/2026-27/000002");
    assert_eq!(other.buyer_state_code.as_deref(), Some("29"));
    assert_eq!((other.cgst_minor, other.sgst_minor), (0, 0));
    assert_eq!(other.igst_minor, same.cgst_minor + same.sgst_minor);
    assert_eq!(other.total_minor, same.total_minor);
}

#[tokio::test]
async fn an_export_band_invoice_has_no_gst_and_says_so() {
    let h = Harness::with_billing(true).await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let branch = h.cafe_done(&a).await;
    h.sql(
        "UPDATE branches SET country = 'MY'; \
         UPDATE billing_bands SET enabled = 1 WHERE band = 'MY'",
    );
    let r = h
        .subscribe(
            &branch,
            &a,
            json!({"tier": "basic", "cycle": "monthly", "billing_country": "MY"}),
        )
        .await;
    assert_eq!(r.status, 201, "{}", r.body);
    let r = h
        .send(
            "evt_my",
            &event(
                "subscription.charged",
                "sub_fake_001",
                PERIOD_END,
                Some(true),
            ),
        )
        .await;
    assert_eq!(r.body["status"], "applied", "{}", r.body);
    let inv = h.db.invoices_for_event("evt_my").await.unwrap().remove(0);
    assert_eq!((inv.band.as_str(), inv.currency.as_str()), ("MY", "MYR"));
    assert_eq!((inv.cgst_minor, inv.sgst_minor, inv.igst_minor), (0, 0, 0));
    assert_eq!(inv.total_minor, inv.subtotal_minor);
    assert_eq!(
        inv.note.as_deref(),
        Some("Export of services - zero-rated under LUT")
    );
    assert_eq!((inv.buyer_gstin, inv.buyer_state_code), (None, None));
}

#[tokio::test]
async fn a_repeat_a_reconcile_and_an_older_event_issue_nothing_more() {
    let (h, _a, branch, _id) = Harness::pending().await;
    let r = h.charge("evt_1", PERIOD_END).await;
    assert_eq!(r.body["status"], "applied");
    let r = h.charge("evt_1", PERIOD_END).await;
    assert_eq!(r.body["status"], "duplicate");
    // The same period under another event id (and the reconcile's view of it): not newer.
    let r = h.charge("evt_1b", PERIOD_END).await;
    assert_eq!(r.body["status"], "recorded");
    let provider = h.st.billing.get().unwrap();
    h.fake()
        .set_remote("sub_fake_001", remote("active", 1, india()));
    h.sql(&format!(
        "UPDATE subscriptions SET state = 'grace', grace_until = 1 WHERE branch_id = '{branch}'"
    ));
    let out = reconcile(&h.db, provider, T0 + 1).await.unwrap();
    assert_eq!((out.checked, out.applied), (1, 0));
    assert_eq!(h.invoices().await.len(), 1);

    // A later period is the next number, with no gap.
    let r = h.charge("evt_2", PERIOD_END + 30 * 86_400).await;
    assert_eq!(r.body["status"], "applied");
    let numbers: Vec<String> = h.invoices().await.into_iter().map(|i| i.number).collect();
    assert_eq!(numbers, ["A360/2026-27/000001", "A360/2026-27/000002"]);
}

#[tokio::test]
async fn a_charge_after_an_activation_is_invoiced_once_and_so_is_a_reconcile_first_charge() {
    let (h, _a, _branch, _id) = Harness::pending().await;
    h.send(
        "evt_act",
        &event("subscription.activated", "sub_fake_001", PERIOD_END, None),
    )
    .await;
    assert!(h.invoices().await.is_empty());
    h.charge("evt_charge", PERIOD_END).await;
    h.charge("evt_charge_again", PERIOD_END).await;
    assert_eq!(h.invoices().await.len(), 1);

    let (h, _a, _branch, _id) = Harness::pending().await;
    let provider = h.st.billing.get().unwrap();
    h.fake()
        .set_remote("sub_fake_001", remote("active", 1, india()));
    reconcile(&h.db, provider, T0).await.unwrap();
    assert_eq!(h.invoices().await.len(), 1);
    // The webhook for the same payment, arriving late, adds nothing.
    h.charge("evt_late", PERIOD_END).await;
    assert_eq!(h.invoices().await.len(), 1);
}

#[tokio::test]
async fn a_price_the_book_lacks_records_nothing_and_fails_the_call_so_it_is_retried() {
    let (h, _a, branch, _id) = Harness::pending_with(json!({
        "tier": "basic", "cycle": "monthly", "billing_country": "IN",
        "add_ons": {"tunnel": 1},
    }))
    .await;
    h.sql(
        "DROP TRIGGER price_book_items_no_delete; \
         DELETE FROM price_book_items WHERE item = 'addon_tunnel' AND band = 'IN' \
         AND cycle = 'monthly'",
    );
    let r = h.charge("evt_1", PERIOD_END).await;
    assert_eq!(r.status, 500, "{}", r.body);
    assert_eq!(h.count("SELECT COUNT(*) FROM billing_events"), 0);
    assert_eq!(h.count("SELECT COUNT(*) FROM billing_invoices"), 0);
    assert_eq!(h.only(&branch).await.state, SubscriptionState::Pending);
    assert_eq!(
        h.count("SELECT COUNT(*) FROM entitlements WHERE source = 'paid'"),
        0
    );
    assert_eq!(h.st.webhook_metrics.counts().failures, 1);
}

#[tokio::test]
async fn an_issued_invoice_cannot_change_except_for_its_pdf_and_notice_marks() {
    let (h, _a, _branch, _id) = Harness::pending().await;
    h.charge("evt_1", PERIOD_END).await;
    let path = h._dir.path().join("gz_control.db");
    let conn = rusqlite::Connection::open(path).unwrap();
    for sql in [
        "UPDATE billing_invoices SET total_minor = 1",
        "UPDATE billing_invoices SET number = 'A360/2026-27/000009'",
        "UPDATE billing_invoices SET subtotal_minor = 1",
        "UPDATE billing_invoices SET buyer_gstin = '29AAGCB7383J1Z4'",
        "UPDATE billing_invoices SET buyer_name = 'x'",
        "UPDATE billing_invoices SET cgst_minor = 0",
        "DELETE FROM billing_invoices",
    ] {
        assert!(conn.execute(sql, []).is_err(), "{sql}");
    }
    // The number is unique.
    let dup = "INSERT INTO billing_invoices (id, number, fy, kind, seq, tenant_id, branch_id, \
        subscription_id, billing_event_id, issued_at, period_start, period_end, band, currency, \
        lines_json, subtotal_minor, total_minor, sac, buyer_name, seller_name, seller_address, \
        seller_gstin) SELECT 'dup', number, fy, kind, seq + 1, tenant_id, branch_id, \
        subscription_id, billing_event_id, issued_at, period_start, period_end, band, currency, \
        lines_json, subtotal_minor, total_minor, sac, buyer_name, seller_name, seller_address, \
        seller_gstin FROM billing_invoices";
    assert!(conn.execute(dup, []).is_err());
    conn.execute(
        "UPDATE billing_invoices SET pdf_key = 'k', pdf_at = 1, notified_at = 2",
        [],
    )
    .unwrap();
    assert!(
        h.invoices().await.is_empty(),
        "finished invoices are not listed again"
    );
    let inv = h.db.invoices_for_event("evt_1").await.unwrap().remove(0);
    assert_eq!(inv.pdf_key.as_deref(), Some("k"));
    assert_eq!(h.count("SELECT COUNT(*) FROM billing_invoices"), 1);
}

fn reissue(of: &Invoice, id: &str, fy: &str) -> NewInvoice {
    NewInvoice {
        id: id.to_owned(),
        fy: fy.to_owned(),
        tenant_id: of.tenant_id.clone(),
        branch_id: of.branch_id.clone(),
        subscription_id: of.subscription_id.clone(),
        issued_at: of.issued_at,
        period_start: of.period_start,
        period_end: of.period_end,
        band: of.band.clone(),
        currency: of.currency.clone(),
        lines_json: of.lines_json.clone(),
        subtotal_minor: of.subtotal_minor,
        cgst_minor: of.cgst_minor,
        sgst_minor: of.sgst_minor,
        igst_minor: of.igst_minor,
        total_minor: of.total_minor,
        sac: of.sac.clone(),
        buyer_name: of.buyer_name.clone(),
        buyer_gstin: of.buyer_gstin.clone(),
        buyer_state_code: of.buyer_state_code.clone(),
        seller_name: of.seller_name.clone(),
        seller_address: of.seller_address.clone(),
        seller_gstin: of.seller_gstin.clone(),
        note: of.note.clone(),
    }
}

#[tokio::test]
async fn the_sequence_restarts_each_financial_year_and_a_failed_issue_leaves_no_gap() {
    let (h, _a, branch, _id) = Harness::pending().await;
    h.charge("evt_1", PERIOD_END).await;
    let first = h.invoices().await.remove(0);
    assert_eq!(first.number, "A360/2026-27/000001");

    let steps = [
        ("2027-28", "A360/2027-28/000001", "rec_a", "hist_a"),
        ("2026-27", "A360/2026-27/000002", "rec_b", "hist_b"),
        ("2027-28", "A360/2027-28/000002", "rec_c", "hist_c"),
    ];
    for (n, (fy, want, rec, hist)) in steps.into_iter().enumerate() {
        let sub = h.only(&branch).await;
        let change = |rev: i64, history_id: &'static str| SubscriptionChange {
            subscription_id: &sub.id,
            expect_rev: rev,
            old_state: sub.state,
            new_state: sub.state,
            period_end: None,
            first_paid_at: None,
            payment_country: None,
            mandate_method: None,
            mandate_detail: None,
            entitlement: None,
            history_id,
            cause: "test",
            ops_task: None,
            audit: None,
            close_task_kind: None,
            plan_change: None,
            lifecycle: Default::default(),
        };
        let event_id = format!("evt_fy_{n}");
        let record = |id: &'static str| EventRecord {
            id,
            razorpay_event_id: &event_id,
            kind: "subscription.charged",
            subscription_id: Some(&sub.id),
            payload_hash: "h",
            source: "webhook",
            received_at: T0,
        };
        // A stale revision writes nothing, not even an invoice number.
        let stale =
            h.db.apply_billing_event(
                &record("rec_stale"),
                Some(&change(sub.rev - 1, "hist_stale")),
                Some(&reissue(&first, "inv_stale", fy)),
                T0,
            )
            .await
            .unwrap();
        assert_eq!(stale, EventOutcome::Conflict);
        let out =
            h.db.apply_billing_event(
                &record(rec),
                Some(&change(sub.rev, hist)),
                Some(&reissue(&first, &format!("inv_{n}"), fy)),
                T0,
            )
            .await
            .unwrap();
        assert_eq!(out, EventOutcome::Recorded);
        let issued = h.db.invoices_for_event(&event_id).await.unwrap();
        assert_eq!(issued[0].number, want);
    }
    assert_eq!(h.count("SELECT COUNT(*) FROM billing_invoices"), 4);
}

#[tokio::test]
async fn the_jobs_task_writes_one_pdf_and_raises_one_alert_however_often_it_runs() {
    let (h, _a, _branch, _id) = Harness::pending().await;
    let store = MemoryStore::new();
    let out = render_once(&h.st, &store).await.unwrap();
    assert_eq!((out.rendered, out.notified), (0, 0));

    h.charge("evt_1", PERIOD_END).await;
    let out = render_once(&h.st, &store).await.unwrap();
    assert_eq!((out.rendered, out.notified, out.failed), (1, 1, 0));
    let key = "billing/invoices/2026-27/A360-2026-27-000001.pdf";
    assert_eq!(store.keys(), [key]);
    let (pdf, _) = store.read(key).unwrap();
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.starts_with("%PDF-1.4") && text.contains("A360/2026-27/000001"));
    assert_eq!(h.alerts(), 1);
    let inv = h.db.invoice("A360/2026-27/000001").await.unwrap().unwrap();
    assert_eq!(inv.pdf_key.as_deref(), Some(key));
    assert_eq!((inv.pdf_at, inv.notified_at), (Some(T0), Some(T0)));
    // The alert says what it is, with the number and the amount.
    let row =
        h.db.owner_alerts(&inv.tenant_id, 0, None, 10)
            .await
            .unwrap()
            .remove(0);
    assert_eq!(row.kind, "invoice_ready");
    assert!(row.params.contains("A360/2026-27/000001") && row.params.contains("Rs "));

    for _ in 0..2 {
        let out = render_once(&h.st, &store).await.unwrap();
        assert_eq!((out.rendered, out.notified, out.failed), (0, 0, 0));
    }
    assert_eq!(store.keys().len(), 1);
    assert_eq!(h.alerts(), 1);

    // A renewal is its own invoice, PDF and alert.
    h.charge("evt_2", PERIOD_END + 30 * 86_400).await;
    let out = render_once(&h.st, &store).await.unwrap();
    assert_eq!((out.rendered, out.notified), (1, 1));
    assert_eq!(store.keys().len(), 2);
    assert_eq!(h.alerts(), 2);
}

#[tokio::test]
async fn a_payment_held_for_review_is_invoiced_and_the_owner_hears_once_it_is_cleared() {
    let (h, _a, branch, id) = Harness::pending().await;
    h.send(
        "evt_charge",
        &event(
            "subscription.charged",
            "sub_fake_001",
            PERIOD_END,
            Some(true),
        ),
    )
    .await;
    assert_eq!(
        h.only(&branch).await.state,
        SubscriptionState::PendingReview
    );
    assert_eq!(h.invoices().await.len(), 1, "the money was taken");
    let store = MemoryStore::new();
    let out = render_once(&h.st, &store).await.unwrap();
    // The PDF is written; the branch is not entitled yet, so there is nobody to tell.
    assert_eq!((out.rendered, out.notified, out.failed), (1, 0, 1));
    assert_eq!(h.alerts(), 0);
    approve_review(&h.db, &id, "sam", "checked", T0 + 1)
        .await
        .unwrap();
    let out = render_once(&h.st, &store).await.unwrap();
    assert_eq!((out.rendered, out.notified, out.failed), (0, 1, 0));
    assert_eq!(h.alerts(), 1);
}

#[tokio::test]
async fn a_store_that_fails_leaves_the_invoice_for_the_next_run() {
    use arena_cloud::store::{ObjectStore, StoreError};
    struct Down;
    impl ObjectStore for Down {
        async fn put(&self, _: &str, _: Vec<u8>) -> Result<(), StoreError> {
            Err(StoreError("down".to_owned()))
        }
        async fn get(&self, _: &str) -> Result<Vec<u8>, StoreError> {
            Err(StoreError("down".to_owned()))
        }
        async fn list(&self, _: &str) -> Result<Vec<String>, StoreError> {
            Ok(Vec::new())
        }
        async fn delete(&self, _: &str) -> Result<(), StoreError> {
            Ok(())
        }
    }
    let (h, _a, _branch, _id) = Harness::pending().await;
    h.charge("evt_1", PERIOD_END).await;
    let out = render_once(&h.st, &Down).await.unwrap();
    assert_eq!((out.rendered, out.notified, out.failed), (0, 0, 1));
    assert_eq!(h.alerts(), 0);
    let out = render_once(&h.st, &MemoryStore::new()).await.unwrap();
    assert_eq!((out.rendered, out.notified, out.failed), (1, 1, 0));
}

// ---- Account -> Subscription (Story 10.8) ----

const PHONE_B: &str = "+919812345678";

/// A charge paid by `payment` (a Razorpay payment entity).
fn paid_event(rid: &str, end_s: i64, payment: Value) -> Value {
    json!({
        "event": "subscription.charged",
        "created_at": 1_800_000_100,
        "payload": {
            "subscription": {"entity": {"id": rid, "status": "active", "current_end": end_s}},
            "payment": {"entity": payment},
        },
    })
}

fn card(last4: &str) -> Value {
    json!({"id": "pay_1", "method": "card", "card": {"international": false, "last4": last4}})
}

impl Harness {
    async fn plans(&self, access: &str, query: &str) -> Reply {
        self.call(
            "GET",
            &format!("/v1/account/subscription{query}"),
            None,
            access,
        )
        .await
    }

    async fn invoice_list(&self, access: &str, query: &str) -> Reply {
        self.call("GET", &format!("/v1/account/invoices{query}"), None, access)
            .await
    }

    fn with_presigner(&self) {
        use arena_cloud::presign::Presigner;
        use arena_cloud::store::S3Config;
        let p = Presigner::new(&S3Config {
            endpoint: "http://127.0.0.1:9000".into(),
            bucket: "arena-data".into(),
            access_key_id: "AKID-SECRET".into(),
            secret_access_key: "SAK-SECRET".into(),
        });
        assert!(self.st.presigner.set(p).is_ok());
    }
}

#[tokio::test]
async fn a_free_branch_reads_as_free_with_nothing_to_pay() {
    let h = Harness::with_billing(true).await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let branch = h.cafe_done(&a).await;
    h.choose_free(&a).await;
    let r = h.plans(&a, "").await;
    assert_eq!(r.status, 200, "{}", r.body);
    let b = &r.body["branches"];
    assert_eq!(b.as_array().unwrap().len(), 1);
    assert_eq!(b[0]["branch_id"], branch.as_str());
    assert_eq!(b[0]["branch_name"], "Pixel Arena Gaming");
    assert_eq!(
        (b[0]["state"].as_str(), b[0]["tier"].as_str()),
        (Some("free"), Some("free"))
    );
    assert_eq!(b[0]["prices"], json!([]));
    assert!(b[0].get("payment_method").is_none() && b[0].get("cycle").is_none());
}

#[tokio::test]
async fn a_paid_branch_shows_its_plan_add_ons_renewal_prices_and_payment_method() {
    let (h, a, branch, _id) = Harness::pending_with(json!({
        "tier": "basic", "cycle": "monthly", "billing_country": "IN",
        "add_ons": {"devices_20": 2, "tunnel": 1},
    }))
    .await;
    // Before the payment lands it is waiting for it.
    let r = h.plans(&a, "").await;
    assert_eq!(r.body["branches"][0]["state"], "pending");

    let r = h
        .send(
            "evt_c",
            &paid_event("sub_fake_001", PERIOD_END, card("4242")),
        )
        .await;
    assert_eq!(r.body["status"], "applied", "{}", r.body);
    let sub = h.only(&branch).await;
    assert_eq!(sub.mandate_detail.as_deref(), Some("4242"));

    let r = h.plans(&a, &format!("?branch={branch}")).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let p = &r.body["branches"][0];
    assert_eq!(
        (p["state"].as_str(), p["tier"].as_str()),
        (Some("active"), Some("basic"))
    );
    assert_eq!(
        (
            p["cycle"].as_str(),
            p["band"].as_str(),
            p["currency"].as_str()
        ),
        (Some("monthly"), Some("IN"), Some("INR"))
    );
    assert_eq!(
        p["add_ons"],
        json!({"devices_20": 2, "tunnel": 1, "owner_login": 0})
    );
    assert_eq!(p["current_period_end"], PERIOD_END * 1000);
    assert_eq!(p["gst_percent"], 18);
    assert_eq!(
        p["payment_method"],
        json!({"method": "card", "detail": "4242"})
    );
    // Both cycles come from the subscription's own book; the current one equals what it renews at.
    let quote = arena_cloud::subscription::quote_for(&h.db, &sub)
        .await
        .unwrap();
    let prices = p["prices"].as_array().unwrap();
    assert_eq!(prices.len(), 2);
    assert_eq!(prices[0]["cycle"], "monthly");
    assert_eq!(prices[0]["amount_minor"], quote.amount_minor);
    assert_eq!(prices[0]["tax_minor"], quote.tax_minor);
    assert_eq!(prices[0]["total_minor"], quote.total_minor);
    assert_eq!(prices[1]["cycle"], "yearly");
    assert_eq!(
        prices[1]["tax_minor"].as_i64().unwrap() * 100 / 18 / 100,
        prices[1]["amount_minor"].as_i64().unwrap() / 100,
        "GST is 18% of the yearly amount"
    );
}

#[tokio::test]
async fn a_bank_mandate_names_its_bank_and_a_upi_one_names_nothing() {
    let (h, a, branch, _id) = Harness::pending().await;
    let mandate = json!({"id": "pay_2", "method": "emandate", "bank": "HDFC"});
    h.send("evt_1", &paid_event("sub_fake_001", PERIOD_END, mandate))
        .await;
    let r = h.plans(&a, "").await;
    assert_eq!(
        r.body["branches"][0]["payment_method"],
        json!({"method": "emandate", "detail": "HDFC"})
    );
    // The renewal is paid by UPI: the earlier bank's name must not stay on the new method.
    let upi = json!({"id": "pay_3", "method": "upi", "vpa": "someone@okbank"});
    h.send(
        "evt_2",
        &paid_event("sub_fake_001", PERIOD_END + 30 * 86_400, upi),
    )
    .await;
    let r = h.plans(&a, "").await;
    assert_eq!(
        r.body["branches"][0]["payment_method"],
        json!({"method": "upi"})
    );
    assert_eq!(h.only(&branch).await.mandate_detail, None);
    assert!(!r.body.to_string().contains("someone@okbank"));
}

#[tokio::test]
async fn every_state_of_the_subscription_reads_in_the_owners_words() {
    let (h, a, _branch, _id) = Harness::pending().await;
    h.charge("evt_1", PERIOD_END).await;
    let set = |h: &Harness, sql: &str| h.sql(&format!("UPDATE subscriptions SET {sql}"));
    let read = |r: Reply| r.body["branches"][0]["state"].as_str().unwrap().to_owned();
    assert_eq!(read(h.plans(&a, "").await), "active");
    set(&h, "cancel_at_period_end = 1");
    assert_eq!(read(h.plans(&a, "").await), "cancels");
    set(
        &h,
        "cancel_at_period_end = 0, state = 'renewal_needs_approval'",
    );
    assert_eq!(read(h.plans(&a, "").await), "needs_approval");
    set(&h, "state = 'grace'");
    assert_eq!(read(h.plans(&a, "").await), "grace");
    set(&h, "state = 'lapsed'");
    assert_eq!(read(h.plans(&a, "").await), "lapsed");
    set(&h, "state = 'pending_review'");
    let r = h.plans(&a, "").await;
    assert_eq!(r.body["branches"][0]["state"], "pending_review");
    assert!(
        r.body["branches"][0]["message"]
            .as_str()
            .unwrap()
            .contains("checking your payment")
    );
    // A subscription that is over leaves the branch on Free.
    set(&h, "state = 'cancelled'");
    assert_eq!(read(h.plans(&a, "").await), "free");
}

#[tokio::test]
async fn invoices_list_newest_first_filter_by_branch_and_year_and_credit_notes_are_negative() {
    let (h, a, branch, _id) = Harness::pending().await;
    h.charge("evt_1", PERIOD_END).await;
    h.charge("evt_2", PERIOD_END + 30 * 86_400).await;
    h.sql(
        "INSERT INTO billing_events (id, razorpay_event_id, type, payload_hash, source, received_at) \n         VALUES ('ev_cn', 'evt_cn', 'refund.processed', 'h', 'webhook', 1);",
    );
    h.sql(
        "INSERT INTO billing_invoices (id, number, fy, kind, seq, tenant_id, branch_id, \
         subscription_id, billing_event_id, issued_at, period_start, period_end, band, currency, \
         lines_json, subtotal_minor, cgst_minor, sgst_minor, igst_minor, total_minor, sac, \
         buyer_name, seller_name, seller_address, seller_gstin, original_number) \
         SELECT 'cn1', 'A360/2026-27/CN000001', fy, 'credit_note', 1, tenant_id, branch_id, \
         subscription_id, 'ev_cn', issued_at + 5, period_start, period_end, band, currency, \
         lines_json, subtotal_minor, cgst_minor, sgst_minor, igst_minor, total_minor, sac, \
         buyer_name, seller_name, seller_address, seller_gstin, number \
         FROM billing_invoices WHERE seq = 1",
    );
    let r = h.invoice_list(&a, "").await;
    assert_eq!(r.status, 200, "{}", r.body);
    let rows = r.body["invoices"].as_array().unwrap();
    assert_eq!(
        rows.iter()
            .map(|i| i["number"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "A360/2026-27/CN000001",
            "A360/2026-27/000002",
            "A360/2026-27/000001"
        ]
    );
    assert_eq!(r.body["years"], json!(["2026-27"]));
    let cn = &rows[0];
    assert_eq!(
        (cn["kind"].as_str(), cn["original_number"].as_str()),
        (Some("credit_note"), Some("A360/2026-27/000001"))
    );
    assert_eq!(
        cn["total_minor"].as_i64().unwrap(),
        -rows[2]["total_minor"].as_i64().unwrap()
    );
    assert!(rows[1]["total_minor"].as_i64().unwrap() > 0);
    assert_eq!(rows[1]["branch_name"], "Pixel Arena Gaming");
    assert_eq!(rows[1]["branch_id"], branch.as_str());
    assert_eq!(rows[1]["pdf_ready"], false);

    let r = h
        .invoice_list(&a, &format!("?branch={branch}&fy=2026-27"))
        .await;
    assert_eq!(r.body["invoices"].as_array().unwrap().len(), 3);
    let r = h.invoice_list(&a, "?fy=2025-26").await;
    assert_eq!(r.body["invoices"], json!([]));
    assert_eq!(
        r.body["years"],
        json!(["2026-27"]),
        "the filter list does not shrink with the filter"
    );
    for bad in ["?fy=26-27", "?fy=2026-27%27OR1"] {
        let r = h.invoice_list(&a, bad).await;
        assert_eq!((r.status, r.code()), (400, "BAD_REQUEST"), "{bad}");
    }
}

#[tokio::test]
async fn a_pdf_link_is_signed_and_short_lived_and_only_exists_once_rendered() {
    let (h, a, _branch, _id) = Harness::pending().await;
    h.charge("evt_1", PERIOD_END).await;
    let id =
        h.db.invoice("A360/2026-27/000001")
            .await
            .unwrap()
            .unwrap()
            .id;
    let path = format!("/v1/account/invoices/{id}/pdf");
    // Not signed in.
    assert_eq!(h.call("GET", &path, None, "").await.status, 401);
    h.with_presigner();
    // Not rendered yet.
    let r = h.call("GET", &path, None, &a).await;
    assert_eq!((r.status, r.code()), (404, "NOT_FOUND"));

    render_once(&h.st, &MemoryStore::new()).await.unwrap();
    let r = h.call("GET", &path, None, &a).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let url = r.body["url"].as_str().unwrap();
    assert!(
        url.contains("billing/invoices/2026-27/A360-2026-27-000001.pdf"),
        "{url}"
    );
    assert!(
        url.contains("X-Amz-Signature") && url.contains("X-Amz-Expires=600"),
        "{url}"
    );
    assert_eq!(r.body["expires_in_s"], 600);
    assert!(!url.contains("SAK-SECRET"));
    let list = h.invoice_list(&a, "").await;
    assert_eq!(list.body["invoices"][0]["pdf_ready"], true);
}

#[tokio::test]
async fn another_tenants_branch_invoice_or_plan_answers_forbidden_and_reads_nothing() {
    let (h, a, branch, _id) = Harness::pending().await;
    h.charge("evt_1", PERIOD_END).await;
    h.with_presigner();
    render_once(&h.st, &MemoryStore::new()).await.unwrap();
    let id =
        h.db.invoice("A360/2026-27/000001")
            .await
            .unwrap()
            .unwrap()
            .id;

    let b = h.owner(PHONE_B, "Other Cafe").await;
    let other = h.cafe_done(&b).await;
    assert_ne!(other, branch);
    for path in [
        format!("/v1/account/subscription?branch={branch}"),
        format!("/v1/account/invoices?branch={branch}"),
        format!("/v1/account/invoices/{id}/pdf"),
        "/v1/account/invoices/no-such-invoice/pdf".to_owned(),
    ] {
        let r = h.call("GET", &path, None, &b).await;
        assert_eq!(
            (r.status, r.code()),
            (403, "FORBIDDEN"),
            "{path}: {}",
            r.body
        );
    }
    // Their own lists hold only their own branch and no invoice of the first café.
    let r = h.plans(&b, "").await;
    let ids: Vec<_> = r.body["branches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["branch_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [other.as_str()]);
    let r = h.invoice_list(&b, "").await;
    assert_eq!(
        (r.body["invoices"].clone(), r.body["years"].clone()),
        (json!([]), json!([]))
    );
    // And the first owner still sees theirs.
    assert_eq!(
        h.invoice_list(&a, "").await.body["invoices"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

// ---- Story 10.9: change tier and add-ons mid-cycle with proration ----

impl Harness {
    /// An owner with an `active` Basic monthly subscription paid by card 4242 (period end
    /// `PERIOD_END`), on `add_ons`.
    async fn active_with(add_ons: Value) -> (Self, String, String) {
        let (h, a, branch, _id) = Harness::pending_with(json!({
            "tier": "basic", "cycle": "monthly", "billing_country": "IN", "add_ons": add_ons,
        }))
        .await;
        let r = h
            .send(
                "evt_first",
                &paid_event("sub_fake_001", PERIOD_END, card("4242")),
            )
            .await;
        assert_eq!(r.body["status"], "applied", "{}", r.body);
        (h, a, branch)
    }

    async fn change(&self, branch: &str, access: &str, step: &str, body: Value) -> Reply {
        self.call(
            "POST",
            &format!("/v1/branches/{branch}/subscription/change{step}"),
            Some(body),
            access,
        )
        .await
    }

    async fn rev(&self, branch: &str) -> i64 {
        self.only(branch).await.rev
    }

    fn charged(&self, event_id: &str, period_end: i64, amount: i64) -> (String, Value) {
        let mut payment = card("4242");
        payment["amount"] = json!(amount);
        (
            event_id.to_owned(),
            paid_event("sub_fake_001", period_end, payment),
        )
    }
}

// Basic monthly 20,000 to Premium monthly 199,900 with 30 of the period's 31 days left.
const UP_CHARGE: i64 = 174_097;
const UP_TAX: i64 = 31_337;

#[tokio::test]
async fn an_upgrade_is_previewed_prorated_charged_and_applied_when_razorpay_confirms() {
    let (h, a, branch) = Harness::active_with(json!({})).await;
    let rev = h.rev(&branch).await;
    let r = h
        .change(
            &branch,
            &a,
            "/preview",
            json!({"tier": "premium", "rev": rev}),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["kind"], "now");
    assert_eq!(
        (
            r.body["charge_amount_minor"].as_i64(),
            r.body["charge_tax_minor"].as_i64(),
            r.body["charge_total_minor"].as_i64(),
            r.body["days_left"].as_i64(),
        ),
        (
            Some(UP_CHARGE),
            Some(UP_TAX),
            Some(UP_CHARGE + UP_TAX),
            Some(30)
        )
    );
    // The new renewal: Premium monthly 199,900 + 18% GST.
    assert_eq!(r.body["renewal_total_minor"], 235_882);
    assert_eq!(r.body["needs_mandate"], false);
    // A preview writes nothing and calls no provider.
    assert!(h.fake().plan_changes().is_empty());
    assert_eq!(h.rev(&branch).await, rev);

    let r = h
        .change(&branch, &a, "", json!({"tier": "premium", "rev": rev}))
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["charge_total_minor"], UP_CHARGE + UP_TAX);
    let sent = h.fake().plan_changes();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].now);
    assert_eq!(
        (sent[0].renewal_minor, sent[0].charge_minor),
        (235_882, UP_CHARGE + UP_TAX)
    );
    // Nothing is entitled until the charge is confirmed, and the card shows the change.
    assert_eq!(
        h.db.entitlement(&branch).await.unwrap().unwrap().tier,
        arena_proto::Tier::Basic
    );
    let plans = h.plans(&a, "").await;
    assert_eq!(plans.body["branches"][0]["pending_change"]["kind"], "now");

    let (id, body) = h.charged("evt_up", PERIOD_END, UP_CHARGE + UP_TAX);
    let r = h.send(&id, &body).await;
    assert_eq!(r.body["status"], "applied", "{}", r.body);
    let sub = h.only(&branch).await;
    assert_eq!(sub.tier, arena_proto::Tier::Premium);
    assert_eq!(sub.state, SubscriptionState::Active);
    // The period did not move: it was a charge inside it.
    assert_eq!(sub.current_period_end, Some(PERIOD_END * 1000));
    assert_eq!(
        h.db.entitlement(&branch).await.unwrap().unwrap().tier,
        arena_proto::Tier::Premium
    );
    let inv = &h.db.invoices_for_event("evt_up").await.unwrap()[0];
    assert_eq!(
        (inv.subtotal_minor, inv.total_minor),
        (UP_CHARGE, UP_CHARGE + UP_TAX)
    );
    assert!(
        inv.lines_json.contains("plan change to Premium"),
        "{}",
        inv.lines_json
    );
    assert_eq!(h.invoices().await.len(), 2);
    assert_eq!(h.count("SELECT COUNT(*) FROM ops_tasks"), 0);
    assert!(h.db.open_plan_change(&sub.id).await.unwrap().is_none());
    let plans = h.plans(&a, "").await;
    assert!(plans.body["branches"][0].get("pending_change").is_none());
    // A repeat of the event changes nothing.
    let r = h.send(&id, &body).await;
    assert_eq!(r.body["status"], "duplicate");
}

#[tokio::test]
async fn a_charge_that_differs_from_the_clouds_figure_is_invoiced_as_charged_and_logged_to_ops() {
    let (h, a, branch) = Harness::active_with(json!({})).await;
    let rev = h.rev(&branch).await;
    let r = h
        .change(&branch, &a, "", json!({"tier": "premium", "rev": rev}))
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    // Razorpay prorates by seconds and charges 100 paise less than the cloud's whole days.
    let charged = UP_CHARGE + UP_TAX - 100;
    let (id, body) = h.charged("evt_up", PERIOD_END, charged);
    let r = h.send(&id, &body).await;
    assert_eq!(r.body["status"], "applied", "{}", r.body);
    let inv = &h.db.invoices_for_event("evt_up").await.unwrap()[0];
    assert_eq!(inv.total_minor, charged);
    assert_eq!(
        h.count("SELECT COUNT(*) FROM ops_tasks WHERE kind = 'proration_mismatch'"),
        1
    );
}

#[tokio::test]
async fn a_downgrade_waits_for_the_period_end_and_keeps_the_entitlement_until_then() {
    let (h, a, branch) = Harness::active_with(json!({"devices_20": 2})).await;
    let rev = h.rev(&branch).await;
    let body = json!({"tier": "basic", "add_ons": {"devices_20": 1}, "rev": rev});
    let r = h.change(&branch, &a, "/preview", body.clone()).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["kind"], "period_end");
    assert_eq!(
        (
            r.body["charge_total_minor"].as_i64(),
            r.body["effective_at"].as_i64()
        ),
        (Some(0), Some(PERIOD_END * 1000))
    );
    // Basic 20,000 + one pack 5,000 + GST.
    assert_eq!(r.body["renewal_total_minor"], 29_500);

    let r = h.change(&branch, &a, "", body).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(!h.fake().plan_changes()[0].now);
    let sub = h.only(&branch).await;
    assert_eq!(sub.addon_devices_20, 2);
    let e = h.db.entitlement(&branch).await.unwrap().unwrap();
    assert_eq!(e.addons, vec!["devices_20:2".to_owned()]);
    let plans = h.plans(&a, "").await;
    let pending = &plans.body["branches"][0]["pending_change"];
    assert_eq!(pending["kind"], "period_end");
    assert_eq!(pending["add_ons"]["devices_20"], 1);

    // A charge inside the period applies nothing.
    let (id, body) = h.charged("evt_inside", PERIOD_END, 1);
    h.send(&id, &body).await;
    assert_eq!(h.only(&branch).await.addon_devices_20, 2);

    // The renewal lands: the change takes effect and the renewal is invoiced at the new plan.
    let (id, body) = h.charged("evt_renew", PERIOD_END + 2_678_400, 29_500);
    let r = h.send(&id, &body).await;
    assert_eq!(r.body["status"], "applied", "{}", r.body);
    assert_eq!(h.only(&branch).await.addon_devices_20, 1);
    let e = h.db.entitlement(&branch).await.unwrap().unwrap();
    assert_eq!(e.addons, vec!["devices_20:1".to_owned()]);
    let inv = &h.db.invoices_for_event("evt_renew").await.unwrap()[0];
    assert_eq!(inv.total_minor, 29_500);
}

#[tokio::test]
async fn keep_current_plan_withdraws_a_scheduled_change_and_the_period_end_changes_nothing() {
    let (h, a, branch) = Harness::active_with(json!({"devices_20": 2})).await;
    let rev = h.rev(&branch).await;
    let r = h
        .change(
            &branch,
            &a,
            "",
            json!({"tier": "basic", "add_ons": {"devices_20": 1}, "rev": rev}),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let rev = h.rev(&branch).await;
    let r = h
        .change(&branch, &a, "/withdraw", json!({"rev": rev}))
        .await;
    assert_eq!(r.status, 204, "{}", r.body);
    // The provider was moved back to the current plan (2 packs: 30,000 + GST).
    let sent = h.fake().plan_changes();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1].renewal_minor, 35_400);
    let plans = h.plans(&a, "").await;
    assert!(plans.body["branches"][0].get("pending_change").is_none());
    let (id, body) = h.charged("evt_renew", PERIOD_END + 2_678_400, 35_400);
    h.send(&id, &body).await;
    assert_eq!(h.only(&branch).await.addon_devices_20, 2);
    // Withdrawing twice finds nothing to withdraw.
    let rev = h.rev(&branch).await;
    let r = h
        .change(&branch, &a, "/withdraw", json!({"rev": rev}))
        .await;
    assert_eq!(r.status, 422, "{}", r.body);
}

#[tokio::test]
async fn a_second_change_from_another_device_fails_stale_and_reaches_no_provider() {
    let (h, a, branch) = Harness::active_with(json!({})).await;
    let rev = h.rev(&branch).await;
    let body = json!({"tier": "premium", "rev": rev});
    let first = h.change(&branch, &a, "", body.clone()).await;
    assert_eq!(first.status, 200, "{}", first.body);
    let second = h
        .change(
            &branch,
            &a,
            "",
            json!({"tier": "premium", "add_ons": {"devices_20": 3}, "rev": rev}),
        )
        .await;
    assert_eq!((second.status, second.code()), (409, "STALE"));
    assert_eq!(h.fake().plan_changes().len(), 1);
    // With a change open, another one (even on the new `rev`) waits for it.
    let rev = h.rev(&branch).await;
    let r = h
        .change(
            &branch,
            &a,
            "",
            json!({"tier": "premium", "add_ons": {"devices_20": 3}, "rev": rev}),
        )
        .await;
    assert_eq!(r.status, 422, "{}", r.body);
    assert_eq!(h.fake().plan_changes().len(), 1);
}

#[tokio::test]
async fn a_provider_that_fails_leaves_no_change_open() {
    let (h, a, branch) = Harness::active_with(json!({})).await;
    h.fake().fail_changes(true);
    let rev = h.rev(&branch).await;
    let r = h
        .change(&branch, &a, "", json!({"tier": "premium", "rev": rev}))
        .await;
    assert_eq!(r.status, 503, "{}", r.body);
    let sub = h.only(&branch).await;
    assert!(h.db.open_plan_change(&sub.id).await.unwrap().is_none());
    assert_eq!(sub.tier, arena_proto::Tier::Basic);
    // The owner can try again once the provider is back.
    h.fake().fail_changes(false);
    let rev = h.rev(&branch).await;
    let r = h
        .change(&branch, &a, "", json!({"tier": "premium", "rev": rev}))
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
}

#[tokio::test]
async fn an_unsettled_renewal_a_mixed_change_and_no_change_are_refused_with_a_reason() {
    let (h, a, branch) = Harness::active_with(json!({"devices_20": 1})).await;
    let rev = h.rev(&branch).await;
    // Raising one part while lowering another is not one change.
    let r = h
        .change(
            &branch,
            &a,
            "/preview",
            json!({"tier": "premium", "add_ons": {"devices_20": 0}, "rev": rev}),
        )
        .await;
    assert_eq!(r.status, 422, "{}", r.body);
    // The same plan changes nothing.
    let r = h
        .change(
            &branch,
            &a,
            "/preview",
            json!({"tier": "basic", "add_ons": {"devices_20": 1}, "rev": rev}),
        )
        .await;
    assert_eq!(r.status, 422, "{}", r.body);
    for state in ["renewal_needs_approval", "grace", "pending_review"] {
        h.sql(&format!("UPDATE subscriptions SET state = '{state}'"));
        let r = h
            .change(
                &branch,
                &a,
                "/preview",
                json!({"tier": "premium", "rev": rev}),
            )
            .await;
        assert_eq!(r.status, 422, "{state}: {}", r.body);
        assert_eq!(r.body["message"], "Settle the pending renewal first");
        let r = h
            .change(&branch, &a, "", json!({"tier": "premium", "rev": rev}))
            .await;
        assert_eq!(r.status, 422, "{state}: {}", r.body);
    }
    assert!(h.fake().plan_changes().is_empty());
}

#[tokio::test]
async fn tunnel_cannot_be_added_on_an_android_hub_and_is_included_on_premium() {
    let (h, a, branch) = Harness::active_with(json!({})).await;
    h.hub_token(&a).await;
    let rev = h.rev(&branch).await;
    let r = h
        .change(
            &branch,
            &a,
            "/preview",
            json!({"tier": "basic", "add_ons": {"tunnel": 1}, "rev": rev}),
        )
        .await;
    assert_eq!(r.status, 422, "{}", r.body);
    assert_eq!(r.body["field"], "add_ons.tunnel");
    let plans = h.plans(&a, "").await;
    assert_eq!(plans.body["branches"][0]["hub_platform"], "android");
    // Premium with Tunnel is refused: it is included.
    let r = h
        .change(
            &branch,
            &a,
            "/preview",
            json!({"tier": "premium", "add_ons": {"tunnel": 1}, "rev": rev}),
        )
        .await;
    assert_eq!(r.status, 422, "{}", r.body);
}

#[tokio::test]
async fn an_upgrade_over_the_upi_limit_is_held_for_a_new_mandate_and_charges_nothing() {
    let (h, a, branch, _id) = Harness::pending_with(json!({
        "tier": "basic", "cycle": "yearly", "billing_country": "IN",
    }))
    .await;
    let upi = json!({"id": "pay_1", "method": "upi", "vpa": "someone@upi"});
    h.send("evt_first", &paid_event("sub_fake_001", PERIOD_END, upi))
        .await;
    assert_eq!(
        h.only(&branch).await.mandate_method,
        Some(arena_proto::MandateMethod::Upi)
    );
    let rev = h.rev(&branch).await;
    let body = json!({"tier": "premium", "rev": rev});
    // Premium yearly Rs 19,990 + GST = Rs 23,588.20: no UPI.
    let r = h.change(&branch, &a, "/preview", body.clone()).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["needs_mandate"], true);
    assert_eq!(r.body["renewal_total_minor"], 2_358_820);
    assert_eq!(r.body["methods"], json!(["card", "emandate"]));
    let charge = r.body["charge_total_minor"].as_i64().unwrap();

    let r = h.change(&branch, &a, "", body).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let checkout = &r.body["checkout"];
    assert_eq!(checkout["razorpay_subscription_id"], "sub_fake_m001");
    assert_eq!(checkout["methods"], json!(["card", "emandate"]));
    // The live subscription was not touched; only the mandate subscription was created.
    assert!(h.fake().plan_changes().is_empty());
    let mandates = h.fake().mandates();
    assert_eq!(mandates.len(), 1);
    assert_eq!(mandates[0].change.charge_minor, charge);
    assert_eq!(mandates[0].start_at_ms, PERIOD_END * 1000);
    let sub = h.only(&branch).await;
    assert_eq!(sub.tier, arena_proto::Tier::Basic);
    assert_eq!(
        sub.razorpay_subscription_id.as_deref(),
        Some("sub_fake_001")
    );
    assert_eq!(
        h.db.entitlement(&branch).await.unwrap().unwrap().tier,
        arena_proto::Tier::Basic
    );
    let plans = h.plans(&a, "").await;
    assert_eq!(
        plans.body["branches"][0]["pending_change"]["needs_mandate"],
        true
    );

    // Authorising the mandate charges the prorated amount, applies the change and swaps the
    // Razorpay subscription; the old one is cancelled so it never bills beside the new one.
    let mut payment = card("1111");
    payment["amount"] = json!(charge);
    let r = h
        .send(
            "evt_mandate",
            &paid_event("sub_fake_m001", PERIOD_END + 31_536_000, payment),
        )
        .await;
    assert_eq!(r.body["status"], "applied", "{}", r.body);
    let sub = h.only(&branch).await;
    assert_eq!(sub.tier, arena_proto::Tier::Premium);
    assert_eq!(
        sub.razorpay_subscription_id.as_deref(),
        Some("sub_fake_m001")
    );
    assert_eq!(sub.mandate_method, Some(arena_proto::MandateMethod::Card));
    assert_eq!(sub.mandate_detail.as_deref(), Some("1111"));
    assert_eq!(
        h.db.entitlement(&branch).await.unwrap().unwrap().tier,
        arena_proto::Tier::Premium
    );
    let inv = &h.db.invoices_for_event("evt_mandate").await.unwrap()[0];
    assert_eq!(inv.total_minor, charge);
    // The old subscription is cancelled in the background, and its task closed once it is.
    for _ in 0..100 {
        if h.fake().cancelled() == vec!["sub_fake_001".to_owned()] {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(h.fake().cancelled(), vec!["sub_fake_001".to_owned()]);
}

#[tokio::test]
async fn cancelling_checkout_leaves_a_held_change_inert_and_keep_current_plan_cancels_its_mandate()
{
    let (h, a, branch, _id) = Harness::pending_with(json!({
        "tier": "basic", "cycle": "yearly", "billing_country": "IN",
    }))
    .await;
    let upi = json!({"id": "pay_1", "method": "upi"});
    h.send("evt_first", &paid_event("sub_fake_001", PERIOD_END, upi))
        .await;
    let rev = h.rev(&branch).await;
    let r = h
        .change(&branch, &a, "", json!({"tier": "premium", "rev": rev}))
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    // Nobody pays: the plan is as it was.
    assert_eq!(h.only(&branch).await.tier, arena_proto::Tier::Basic);
    let rev = h.rev(&branch).await;
    let r = h
        .change(&branch, &a, "/withdraw", json!({"rev": rev}))
        .await;
    assert_eq!(r.status, 204, "{}", r.body);
    assert_eq!(h.fake().cancelled(), vec!["sub_fake_m001".to_owned()]);
    // A late charge on the withdrawn mandate applies nothing.
    let r = h
        .send(
            "evt_late",
            &paid_event("sub_fake_m001", PERIOD_END, card("1111")),
        )
        .await;
    assert_ne!(r.body["status"], "applied", "{}", r.body);
    assert_eq!(h.only(&branch).await.tier, arena_proto::Tier::Basic);
}

// ---- Story 10.10: switch billing cycle and payment method ----

impl Harness {
    /// An owner with an `active` subscription on `body`, paid with `payment`, period end `PERIOD_END`.
    async fn active_on(body: Value, payment: Value) -> (Self, String, String) {
        let (h, a, branch, _id) = Harness::pending_with(body).await;
        let r = h
            .send(
                "evt_first",
                &paid_event("sub_fake_001", PERIOD_END, payment),
            )
            .await;
        assert_eq!(r.body["status"], "applied", "{}", r.body);
        (h, a, branch)
    }

    async fn cycle(&self, branch: &str, access: &str, step: &str, body: Value) -> Reply {
        self.call(
            "POST",
            &format!("/v1/branches/{branch}/subscription/cycle{step}"),
            Some(body),
            access,
        )
        .await
    }

    async fn method(&self, branch: &str, access: &str, body: Value) -> Reply {
        self.call(
            "POST",
            &format!("/v1/branches/{branch}/subscription/payment-method"),
            Some(body),
            access,
        )
        .await
    }

    /// Waits for the background cancel that follows an event.
    async fn settle_cancels(&self, immediate: &[&str], at_cycle_end: &[&str]) {
        let want = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        for _ in 0..100 {
            if self.fake().cancelled() == want(immediate)
                && self.fake().cancelled_at_cycle_end() == want(at_cycle_end)
            {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(self.fake().cancelled(), want(immediate));
        assert_eq!(self.fake().cancelled_at_cycle_end(), want(at_cycle_end));
    }
}

fn authenticated(rid: &str, end_s: i64, payment: Value) -> Value {
    json!({
        "event": "subscription.authenticated",
        "created_at": 1_800_000_100,
        "payload": {
            "subscription": {"entity": {"id": rid, "status": "authenticated", "current_end": end_s}},
            "payment": {"entity": payment},
        },
    })
}

// A year out from the harness clock (2027-01-15 08:00 UTC to 2028-01-15), in seconds.
const YEAR_OUT: i64 = 1_800_000_000 + 365 * 86_400;
// Basic monthly 20,000 to yearly 200,000 with 30 of the month's 31 days left.
const YEAR_CREDIT: i64 = 19_355;
const YEAR_NET: i64 = 200_000 - YEAR_CREDIT;
const YEAR_TAX: i64 = 32_516;

#[tokio::test]
async fn monthly_to_yearly_shows_the_price_less_the_unused_days_and_applies_on_the_first_charge() {
    let (h, a, branch) = Harness::active_with(json!({})).await;
    let rev = h.rev(&branch).await;
    let body = json!({"cycle": "yearly", "rev": rev});
    let r = h.cycle(&branch, &a, "/preview", body.clone()).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let b = &r.body;
    assert_eq!(b["kind"], "now");
    // The yearly price is ten months: 200,000 + 18% GST.
    assert_eq!(
        (
            b["cycle_amount_minor"].as_i64(),
            b["cycle_tax_minor"].as_i64(),
            b["cycle_total_minor"].as_i64()
        ),
        (Some(200_000), Some(36_000), Some(236_000))
    );
    // The credit is the unused days of the paid month; the net is what is charged today.
    assert_eq!(b["days_left"], 30);
    assert_eq!(
        (
            b["charge_amount_minor"].as_i64(),
            b["charge_tax_minor"].as_i64(),
            b["charge_total_minor"].as_i64(),
            b["credit_total_minor"].as_i64()
        ),
        (
            Some(YEAR_NET),
            Some(YEAR_TAX),
            Some(YEAR_NET + YEAR_TAX),
            Some(236_000 - (YEAR_NET + YEAR_TAX))
        )
    );
    assert_eq!(b["renews_at"], YEAR_OUT * 1000);
    assert_eq!(b["needs_mandate"], true);
    // A preview writes nothing and calls no provider.
    assert!(h.fake().mandates().is_empty());
    assert_eq!(h.rev(&branch).await, rev);

    let r = h.cycle(&branch, &a, "", body).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        r.body["checkout"]["razorpay_subscription_id"],
        "sub_fake_m001"
    );
    assert_eq!(r.body["checkout"]["cycle"], "yearly");
    let mandates = h.fake().mandates();
    assert_eq!(mandates.len(), 1);
    // The new subscription renews a year out and carries today's net as its upfront charge.
    assert_eq!(mandates[0].start_at_ms, YEAR_OUT * 1000);
    assert_eq!(
        (
            mandates[0].change.charge_minor,
            mandates[0].change.renewal_minor
        ),
        (YEAR_NET + YEAR_TAX, 236_000)
    );
    // Nothing changed yet: the cycle and the Razorpay subscription are as they were.
    let sub = h.only(&branch).await;
    assert_eq!(sub.cycle, arena_proto::BillingCycle::Monthly);
    assert_eq!(
        sub.razorpay_subscription_id.as_deref(),
        Some("sub_fake_001")
    );
    let plans = h.plans(&a, "").await;
    let pending = &plans.body["branches"][0]["pending_change"];
    assert_eq!(
        (pending["subject"].as_str(), pending["cycle"].as_str()),
        (Some("cycle"), Some("yearly"))
    );
    assert_eq!(pending["needs_mandate"], true);

    // The owner authorises it: the upfront charge confirms the switch on the same row.
    let mut payment = card("1111");
    payment["amount"] = json!(YEAR_NET + YEAR_TAX);
    let r = h
        .send("evt_year", &paid_event("sub_fake_m001", YEAR_OUT, payment))
        .await;
    assert_eq!(r.body["status"], "applied", "{}", r.body);
    let after = h.only(&branch).await;
    assert_eq!(after.id, sub.id);
    assert_eq!(after.cycle, arena_proto::BillingCycle::Yearly);
    assert_eq!(after.band, sub.band);
    assert_eq!(after.state, SubscriptionState::Active);
    assert_eq!(
        after.razorpay_subscription_id.as_deref(),
        Some("sub_fake_m001")
    );
    assert_eq!(after.current_period_end, Some(YEAR_OUT * 1000));
    assert_eq!(after.mandate_detail.as_deref(), Some("1111"));
    // The entitlement never lapsed: still Basic, still paid.
    let ent = h.db.entitlement(&branch).await.unwrap().unwrap();
    assert_eq!(ent.tier, arena_proto::Tier::Basic);
    let inv = &h.db.invoices_for_event("evt_year").await.unwrap()[0];
    assert_eq!(
        (inv.subtotal_minor, inv.total_minor),
        (YEAR_NET, YEAR_NET + YEAR_TAX)
    );
    assert!(inv.lines_json.contains("yearly plan"), "{}", inv.lines_json);
    assert_eq!(
        h.count("SELECT COUNT(*) FROM ops_tasks WHERE kind = 'proration_mismatch'"),
        0
    );
    assert!(h.db.open_plan_change(&sub.id).await.unwrap().is_none());
    // The old monthly subscription is cancelled once the new one is paid.
    h.settle_cancels(&["sub_fake_001"], &[]).await;
    let plans = h.plans(&a, "").await;
    assert!(plans.body["branches"][0].get("pending_change").is_none());
    assert_eq!(plans.body["branches"][0]["cycle"], "yearly");

    // A late webhook for the replaced subscription is recorded and changes nothing.
    let r = h
        .send(
            "evt_old",
            &paid_event("sub_fake_001", PERIOD_END + 31 * 86_400, card("4242")),
        )
        .await;
    assert_ne!(r.body["status"], "applied", "{}", r.body);
    assert!(h.db.billing_event("evt_old").await.unwrap().is_some());
    let still = h.only(&branch).await;
    assert_eq!(still.cycle, arena_proto::BillingCycle::Yearly);
    assert_eq!(still.current_period_end, Some(YEAR_OUT * 1000));
    assert_eq!(
        still.razorpay_subscription_id.as_deref(),
        Some("sub_fake_m001")
    );
}

#[tokio::test]
async fn yearly_to_monthly_waits_for_the_paid_year_and_the_old_one_ends_only_once_authorised() {
    let (h, a, branch) = Harness::active_on(
        json!({"tier": "basic", "cycle": "yearly", "billing_country": "IN"}),
        card("4242"),
    )
    .await;
    let rev = h.rev(&branch).await;
    let body = json!({"cycle": "monthly", "rev": rev});
    let r = h.cycle(&branch, &a, "/preview", body.clone()).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["kind"], "period_end");
    assert_eq!(r.body["charge_total_minor"], 0);
    assert_eq!(r.body["effective_at"], PERIOD_END * 1000);
    assert_eq!(r.body["cycle_total_minor"], 23_600);
    let r = h.cycle(&branch, &a, "", body).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["kind"], "period_end");
    assert_eq!(r.body["charge_total_minor"], 0);
    let mandates = h.fake().mandates();
    assert_eq!(mandates[0].start_at_ms, PERIOD_END * 1000);
    assert_eq!(mandates[0].change.cycle, arena_proto::BillingCycle::Monthly);
    assert_eq!(mandates[0].change.charge_minor, 0);
    // Nothing is cancelled and nothing changed while the owner has not authorised.
    h.settle_cancels(&[], &[]).await;
    assert_eq!(
        h.only(&branch).await.cycle,
        arena_proto::BillingCycle::Yearly
    );

    // Authorising the new subscription ends the old one with its paid year, and cannot be undone.
    let r = h
        .send(
            "evt_auth",
            &authenticated("sub_fake_m001", PERIOD_END, card("1111")),
        )
        .await;
    assert_eq!(r.body["status"], "applied", "{}", r.body);
    h.settle_cancels(&[], &["sub_fake_001"]).await;
    let sub = h.only(&branch).await;
    assert_eq!(sub.cycle, arena_proto::BillingCycle::Yearly);
    assert_eq!(
        sub.razorpay_subscription_id.as_deref(),
        Some("sub_fake_001")
    );
    let plans = h.plans(&a, "").await;
    let pending = &plans.body["branches"][0]["pending_change"];
    assert_eq!(pending["subject"], "cycle");
    assert_eq!(pending["needs_mandate"], false);
    let rev = h.rev(&branch).await;
    let r = h
        .change(&branch, &a, "/withdraw", json!({"rev": rev}))
        .await;
    assert_ne!(r.status, 204, "{}", r.body);
    // A repeat of the authorisation ends nothing twice.
    let r = h
        .send(
            "evt_auth",
            &authenticated("sub_fake_m001", PERIOD_END, card("1111")),
        )
        .await;
    assert_eq!(r.body["status"], "duplicate");

    // The new monthly subscription's first charge, when the paid year ends, applies the switch.
    let month_end = PERIOD_END + 28 * 86_400;
    let r = h
        .send(
            "evt_month",
            &paid_event("sub_fake_m001", month_end, card("1111")),
        )
        .await;
    assert_eq!(r.body["status"], "applied", "{}", r.body);
    let sub = h.only(&branch).await;
    assert_eq!(sub.cycle, arena_proto::BillingCycle::Monthly);
    assert_eq!(sub.state, SubscriptionState::Active);
    assert_eq!(
        sub.razorpay_subscription_id.as_deref(),
        Some("sub_fake_m001")
    );
    assert_eq!(sub.current_period_end, Some(month_end * 1000));
    assert_eq!(sub.mandate_detail.as_deref(), Some("1111"));
    assert_eq!(
        h.db.entitlement(&branch).await.unwrap().unwrap().tier,
        arena_proto::Tier::Basic
    );
    // It is an ordinary renewal: invoiced at the monthly price, and nothing is cancelled again.
    let inv = &h.db.invoices_for_event("evt_month").await.unwrap()[0];
    assert_eq!(inv.total_minor, 23_600);
    h.settle_cancels(&[], &["sub_fake_001"]).await;
    assert!(h.db.open_plan_change(&sub.id).await.unwrap().is_none());
}

#[tokio::test]
async fn a_yearly_switch_over_the_upi_limit_opens_checkout_with_card_and_enach_only() {
    let upi = json!({"id": "pay_1", "method": "upi", "vpa": "someone@upi"});
    let (h, a, branch) = Harness::active_on(
        json!({"tier": "premium", "cycle": "monthly", "billing_country": "IN"}),
        upi,
    )
    .await;
    assert_eq!(
        h.only(&branch).await.mandate_method,
        Some(arena_proto::MandateMethod::Upi)
    );
    let rev = h.rev(&branch).await;
    // Premium yearly Rs 19,990 + GST = Rs 23,588.20: no UPI.
    let r = h
        .cycle(
            &branch,
            &a,
            "/preview",
            json!({"cycle": "yearly", "rev": rev}),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["cycle_total_minor"], 2_358_820);
    assert_eq!(r.body["methods"], json!(["card", "emandate"]));
    assert!(r.body["notice"].is_string());
    let r = h
        .cycle(&branch, &a, "", json!({"cycle": "yearly", "rev": rev}))
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["checkout"]["methods"], json!(["card", "emandate"]));
    assert_eq!(
        h.fake().mandates()[0].methods,
        vec![
            arena_proto::MandateMethod::Card,
            arena_proto::MandateMethod::Emandate
        ]
    );

    // Cancelling Checkout leaves the old cycle in force; keeping the plan cancels the unused mandate.
    assert_eq!(
        h.only(&branch).await.cycle,
        arena_proto::BillingCycle::Monthly
    );
    let rev = h.rev(&branch).await;
    let r = h
        .change(&branch, &a, "/withdraw", json!({"rev": rev}))
        .await;
    assert_eq!(r.status, 204, "{}", r.body);
    assert_eq!(h.fake().cancelled(), vec!["sub_fake_m001".to_owned()]);
    let r = h
        .send(
            "evt_late",
            &paid_event("sub_fake_m001", YEAR_OUT, card("1111")),
        )
        .await;
    assert_ne!(r.body["status"], "applied", "{}", r.body);
    let sub = h.only(&branch).await;
    assert_eq!(sub.cycle, arena_proto::BillingCycle::Monthly);
    assert_eq!(
        sub.razorpay_subscription_id.as_deref(),
        Some("sub_fake_001")
    );
}

#[tokio::test]
async fn a_cycle_switch_is_refused_for_the_same_cycle_a_stale_rev_a_pending_change_or_a_failing_provider()
 {
    let (h, a, branch) = Harness::active_with(json!({})).await;
    let rev = h.rev(&branch).await;
    let r = h
        .cycle(
            &branch,
            &a,
            "/preview",
            json!({"cycle": "monthly", "rev": rev}),
        )
        .await;
    assert_eq!(
        (r.status, r.code()),
        (422, "VALIDATION_FAILED"),
        "{}",
        r.body
    );
    // Another device moved the subscription first.
    let r = h
        .cycle(&branch, &a, "", json!({"cycle": "yearly", "rev": rev - 1}))
        .await;
    assert_eq!((r.status, r.code()), (409, "STALE"), "{}", r.body);
    assert!(h.fake().mandates().is_empty());
    // A provider that fails leaves no change open.
    h.fake().fail_changes(true);
    let r = h
        .cycle(&branch, &a, "", json!({"cycle": "yearly", "rev": rev}))
        .await;
    assert_eq!(r.status, 503, "{}", r.body);
    h.fake().fail_changes(false);
    let sub = h.only(&branch).await;
    assert!(h.db.open_plan_change(&sub.id).await.unwrap().is_none());
    // One open change at a time.
    let rev = h.rev(&branch).await;
    let r = h
        .cycle(&branch, &a, "", json!({"cycle": "yearly", "rev": rev}))
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let rev = h.rev(&branch).await;
    for r in [
        h.cycle(&branch, &a, "", json!({"cycle": "yearly", "rev": rev}))
            .await,
        h.method(&branch, &a, json!({"rev": rev})).await,
        h.change(&branch, &a, "", json!({"tier": "premium", "rev": rev}))
            .await,
    ] {
        assert_eq!(
            (r.status, r.code()),
            (422, "VALIDATION_FAILED"),
            "{}",
            r.body
        );
    }
}

#[tokio::test]
async fn a_new_payment_method_replaces_the_mandate_only_once_it_is_authorised() {
    let upi = json!({"id": "pay_1", "method": "upi", "vpa": "someone@upi"});
    let (h, a, branch) = Harness::active_on(
        json!({"tier": "basic", "cycle": "monthly", "billing_country": "IN"}),
        upi,
    )
    .await;
    let invoices_before = h.invoices().await.len();
    let rev = h.rev(&branch).await;
    let r = h.method(&branch, &a, json!({"rev": rev})).await;
    assert_eq!(r.status, 200, "{}", r.body);
    // The renewal is Rs 236, so every method is allowed; nothing is charged.
    assert_eq!(
        r.body["checkout"]["methods"],
        json!(["upi", "card", "emandate"])
    );
    assert_eq!(r.body["charge_total_minor"], 0);
    assert_eq!(
        r.body["checkout"]["razorpay_subscription_id"],
        "sub_fake_m001"
    );
    let mandates = h.fake().mandates();
    assert_eq!(mandates[0].start_at_ms, PERIOD_END * 1000);
    assert_eq!(mandates[0].change.charge_minor, 0);
    assert_eq!(mandates[0].change.renewal_minor, 23_600);
    // Until it is authorised the old mandate is in force and the card says so.
    let sub = h.only(&branch).await;
    assert_eq!(sub.mandate_method, Some(arena_proto::MandateMethod::Upi));
    assert_eq!(
        sub.razorpay_subscription_id.as_deref(),
        Some("sub_fake_001")
    );
    h.settle_cancels(&[], &[]).await;
    let plans = h.plans(&a, "").await;
    assert_eq!(
        plans.body["branches"][0]["pending_change"]["subject"],
        "method"
    );

    let r = h
        .send(
            "evt_method",
            &authenticated("sub_fake_m001", PERIOD_END, card("5678")),
        )
        .await;
    assert_eq!(r.body["status"], "applied", "{}", r.body);
    let after = h.only(&branch).await;
    assert_eq!(after.mandate_method, Some(arena_proto::MandateMethod::Card));
    assert_eq!(after.mandate_detail.as_deref(), Some("5678"));
    assert_eq!(
        after.razorpay_subscription_id.as_deref(),
        Some("sub_fake_m001")
    );
    // The plan, the period and the entitlement are untouched, and nothing was invoiced.
    assert_eq!(after.tier, sub.tier);
    assert_eq!(after.cycle, sub.cycle);
    assert_eq!(after.current_period_end, sub.current_period_end);
    assert_eq!(h.invoices().await.len(), invoices_before);
    h.settle_cancels(&["sub_fake_001"], &[]).await;
    let plans = h.plans(&a, "").await;
    assert!(plans.body["branches"][0].get("pending_change").is_none());
    assert_eq!(
        plans.body["branches"][0]["payment_method"]["detail"],
        "5678"
    );
}

#[tokio::test]
async fn an_abandoned_method_change_keeps_the_previous_method_and_a_big_renewal_offers_no_upi() {
    let upi = json!({"id": "pay_1", "method": "upi", "vpa": "someone@upi"});
    let (h, a, branch) = Harness::active_on(
        json!({"tier": "premium", "cycle": "yearly", "billing_country": "IN"}),
        upi,
    )
    .await;
    let rev = h.rev(&branch).await;
    let r = h.method(&branch, &a, json!({"rev": rev})).await;
    assert_eq!(r.status, 200, "{}", r.body);
    // Premium yearly renews at Rs 23,588.20: card or eNACH only.
    assert_eq!(r.body["checkout"]["methods"], json!(["card", "emandate"]));
    // The owner closes Checkout: the card goes back to the previous method.
    let rev = h.rev(&branch).await;
    let r = h
        .change(&branch, &a, "/withdraw", json!({"rev": rev}))
        .await;
    assert_eq!(r.status, 204, "{}", r.body);
    assert_eq!(h.fake().cancelled(), vec!["sub_fake_m001".to_owned()]);
    // A failed or late authorisation of the abandoned mandate applies nothing.
    let r = h
        .send(
            "evt_late",
            &authenticated("sub_fake_m001", PERIOD_END, card("5678")),
        )
        .await;
    assert_ne!(r.body["status"], "applied", "{}", r.body);
    let sub = h.only(&branch).await;
    assert_eq!(sub.mandate_method, Some(arena_proto::MandateMethod::Upi));
    assert_eq!(
        sub.razorpay_subscription_id.as_deref(),
        Some("sub_fake_001")
    );
    // A stale rev is refused before any provider call.
    let r = h.method(&branch, &a, json!({"rev": rev - 5})).await;
    assert_eq!((r.status, r.code()), (409, "STALE"), "{}", r.body);
    assert_eq!(h.fake().mandates().len(), 1);
}

// ---- Story 10.11: renewals that need approval go to grace, then lapse, on cloud time ----

use arena_cloud::lifecycle::{GRACE_DAYS, sweep_once};

const DAY_MS: i64 = 86_400_000;
/// The end of the first paid period, in epoch milliseconds.
const END_MS: i64 = PERIOD_END * 1000;

fn sub_event(kind: &str, rid: &str) -> Value {
    json!({
        "event": kind,
        "created_at": 1_800_000_100,
        "payload": {"subscription": {"entity": {"id": rid, "status": "pending"}}},
    })
}

impl Harness {
    fn set_now(&self, ms: i64) {
        self.clock.store(ms, Ordering::SeqCst);
        // Heartbeats are limited to one per 5 s per Hub and these steps jump days.
        self.sql("DELETE FROM rate_limits");
    }

    fn alerts_of(&self, kind: &str) -> i64 {
        self.count(&format!(
            "SELECT COUNT(*) FROM owner_alerts WHERE kind = '{kind}'"
        ))
    }

    async fn state_of(&self, branch: &str) -> SubscriptionState {
        self.only(branch).await.state
    }

    /// The tier the branch's next heartbeat licence carries.
    async fn licence_tier(&self, token: &str) -> arena_proto::Tier {
        self.sql("DELETE FROM rate_limits");
        let r = self.heartbeat(token).await;
        assert_eq!(r.status, 200, "{}", r.body);
        verify(
            r.body["licence"].as_str().unwrap(),
            DEV_LICENCE_VERIFY_KEYS,
            self.clock.load(Ordering::SeqCst),
        )
        .unwrap()
        .tier
    }

    /// An owner on an `active` Basic plan whose hub is paired, with its paid licence delivered.
    async fn paid_with_hub() -> (Self, String, String, String, String) {
        let (h, a, branch, id) = Harness::pending().await;
        h.choose_free(&a).await;
        let token = h.hub_token(&a).await;
        let r = h
            .send(
                "evt_first",
                &paid_event("sub_fake_001", PERIOD_END, card("4242")),
            )
            .await;
        assert_eq!(r.body["status"], "applied", "{}", r.body);
        assert_eq!(h.licence_tier(&token).await, arena_proto::Tier::Basic);
        (h, a, branch, id, token)
    }
}

#[tokio::test]
async fn a_renewal_that_needs_approval_keeps_the_entitlement_and_the_owner_hears_once() {
    let (h, _a, branch, id, token) = Harness::paid_with_hub().await;
    let r = h
        .send(
            "evt_approval",
            &sub_event("subscription.renewal_needs_approval", "sub_fake_001"),
        )
        .await;
    assert_eq!(r.body["status"], "applied", "{}", r.body);
    assert_eq!(
        h.state_of(&branch).await,
        SubscriptionState::RenewalNeedsApproval
    );
    // The entitlement and the period are untouched, and the hub keeps its paid licence.
    let e = h.db.entitlement(&branch).await.unwrap().unwrap();
    assert_eq!(
        (e.tier, e.source),
        (
            arena_proto::Tier::Basic,
            arena_proto::EntitlementSource::Paid
        )
    );
    assert_eq!(h.only(&branch).await.current_period_end, Some(END_MS));
    assert_eq!(h.licence_tier(&token).await, arena_proto::Tier::Basic);

    // The sweep raises the alert (the webhook may have raised it first: one alert either way),
    // and a repeat run raises nothing more.
    for _ in 0..3 {
        let out = sweep_once(&h.st).await.unwrap();
        assert_eq!((out.graced, out.lapsed, out.failed), (0, 0, 0));
    }
    assert_eq!(h.alerts_of("renewal_needs_approval"), 1);
    let tenant = h.only(&branch).await.tenant_id;
    let row =
        h.db.owner_alerts(&tenant, 0, None, 10)
            .await
            .unwrap()
            .into_iter()
            .find(|r| r.kind == "renewal_needs_approval")
            .unwrap();
    assert!(row.params.contains("Pixel Arena Gaming"), "{}", row.params);
    let history = h.db.subscription_history(&id).await.unwrap();
    assert_eq!(
        history
            .iter()
            .map(|r| (r.old_state.as_str(), r.new_state.as_str()))
            .collect::<Vec<_>>(),
        [("pending", "active"), ("active", "renewal_needs_approval")]
    );
    // A repeated event is a no-op.
    let r = h
        .send(
            "evt_approval",
            &sub_event("subscription.renewal_needs_approval", "sub_fake_001"),
        )
        .await;
    assert_eq!(r.body["status"], "duplicate");
}

#[tokio::test]
async fn a_renewal_not_approved_by_the_period_end_goes_to_grace_for_seven_days_on_cloud_time() {
    let (h, _a, branch, id, token) = Harness::paid_with_hub().await;
    h.send(
        "evt_approval",
        &sub_event("subscription.renewal_needs_approval", "sub_fake_001"),
    )
    .await;
    // One millisecond before the period ends nothing moves.
    h.set_now(END_MS - 1);
    let out = sweep_once(&h.st).await.unwrap();
    assert_eq!((out.graced, out.lapsed), (0, 0));
    assert_eq!(
        h.state_of(&branch).await,
        SubscriptionState::RenewalNeedsApproval
    );

    h.set_now(END_MS);
    let out = sweep_once(&h.st).await.unwrap();
    assert_eq!((out.graced, out.lapsed, out.failed), (1, 0, 0));
    let s = h.only(&branch).await;
    assert_eq!(s.state, SubscriptionState::Grace);
    assert_eq!(s.grace_until, Some(END_MS + GRACE_DAYS * DAY_MS));
    // The cloud keeps signing the paid licence through grace.
    assert_eq!(h.licence_tier(&token).await, arena_proto::Tier::Basic);
    assert_eq!(
        h.db.entitlement(&branch).await.unwrap().unwrap().source,
        arena_proto::EntitlementSource::Paid
    );
    let history = h.db.subscription_history(&id).await.unwrap();
    let last = history.last().unwrap();
    assert_eq!(
        (
            last.old_state.as_str(),
            last.new_state.as_str(),
            last.cause.as_str()
        ),
        ("renewal_needs_approval", "grace", "job:renewal-unpaid")
    );
    // A second run changes nothing.
    let out = sweep_once(&h.st).await.unwrap();
    assert_eq!((out.graced, out.lapsed), (0, 0));
}

#[tokio::test]
async fn a_failed_renewal_charge_goes_to_grace_when_the_period_ends() {
    // The failure is reported after the period ended: grace starts at once.
    let (h, _a, branch, _id, _token) = Harness::paid_with_hub().await;
    h.set_now(END_MS + 60_000);
    let r = h
        .send(
            "evt_pending",
            &sub_event("subscription.pending", "sub_fake_001"),
        )
        .await;
    assert_eq!(r.body["status"], "applied", "{}", r.body);
    let s = h.only(&branch).await;
    assert_eq!(s.state, SubscriptionState::Grace);
    assert_eq!(s.grace_until, Some(END_MS + GRACE_DAYS * DAY_MS));

    // The failure is reported before the period ended: it is marked, and the sweep waits.
    let (h, _a, branch, _id, _token) = Harness::paid_with_hub().await;
    h.set_now(END_MS - DAY_MS);
    let r = h
        .send(
            "evt_pending",
            &sub_event("subscription.pending", "sub_fake_001"),
        )
        .await;
    assert_eq!(r.body["status"], "applied", "{}", r.body);
    assert_eq!(h.state_of(&branch).await, SubscriptionState::Active);
    let out = sweep_once(&h.st).await.unwrap();
    assert_eq!(out.graced, 0);
    h.set_now(END_MS);
    let out = sweep_once(&h.st).await.unwrap();
    assert_eq!(out.graced, 1);
    assert_eq!(h.state_of(&branch).await, SubscriptionState::Grace);

    // Without a failure an unrenewed active subscription is not touched (another story ends it).
    let (h, _a, branch, _id, _token) = Harness::paid_with_hub().await;
    h.set_now(END_MS + 30 * DAY_MS);
    let out = sweep_once(&h.st).await.unwrap();
    assert_eq!((out.graced, out.lapsed), (0, 0));
    assert_eq!(h.state_of(&branch).await, SubscriptionState::Active);

    // A renewal that goes through clears the mark: a later period end does not move it.
    let (h, _a, branch, _id, _token) = Harness::paid_with_hub().await;
    h.set_now(END_MS - DAY_MS);
    h.send(
        "evt_pending",
        &sub_event("subscription.pending", "sub_fake_001"),
    )
    .await;
    let r = h
        .send(
            "evt_renewed",
            &paid_event("sub_fake_001", PERIOD_END + 30 * 86_400, card("4242")),
        )
        .await;
    assert_eq!(r.body["status"], "applied", "{}", r.body);
    h.set_now(END_MS + DAY_MS);
    assert_eq!(sweep_once(&h.st).await.unwrap().graced, 0);
    assert_eq!(h.state_of(&branch).await, SubscriptionState::Active);
}

#[tokio::test]
async fn the_owner_is_warned_on_days_one_five_and_seven_of_grace_and_never_twice() {
    let (h, _a, branch, _id, _token) = Harness::paid_with_hub().await;
    h.set_now(END_MS + 1);
    h.send(
        "evt_pending",
        &sub_event("subscription.pending", "sub_fake_001"),
    )
    .await;
    assert_eq!(h.state_of(&branch).await, SubscriptionState::Grace);

    for (offset_ms, want) in [
        (0, 1),
        (DAY_MS, 1),
        (3 * DAY_MS, 1),
        (4 * DAY_MS, 2),
        (5 * DAY_MS, 2),
        (6 * DAY_MS, 3),
        (6 * DAY_MS + 3_600_000, 3),
    ] {
        h.set_now(END_MS + 1 + offset_ms);
        for _ in 0..2 {
            sweep_once(&h.st).await.unwrap();
        }
        assert_eq!(
            h.alerts_of("renewal_grace"),
            want,
            "at +{offset_ms} ms into grace"
        );
    }
    // The grace alerts name the day.
    let tenant = h.only(&branch).await.tenant_id;
    let rows = h.db.owner_alerts(&tenant, 0, None, 20).await.unwrap();
    let mut days: Vec<String> = rows
        .iter()
        .filter(|r| r.kind == "renewal_grace")
        .map(|r| {
            serde_json::from_str::<Value>(&r.params).unwrap()["day"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    days.sort();
    assert_eq!(days, ["1", "5", "7"]);
}

#[tokio::test]
async fn grace_ending_lapses_the_branch_to_free_and_the_next_heartbeat_says_so() {
    let (h, _a, branch, id, token) = Harness::paid_with_hub().await;
    h.set_now(END_MS + 1);
    h.send(
        "evt_pending",
        &sub_event("subscription.pending", "sub_fake_001"),
    )
    .await;
    let until = END_MS + GRACE_DAYS * DAY_MS;
    h.set_now(until - 1);
    assert_eq!(sweep_once(&h.st).await.unwrap().lapsed, 0);
    assert_eq!(h.licence_tier(&token).await, arena_proto::Tier::Basic);

    h.set_now(until);
    let out = sweep_once(&h.st).await.unwrap();
    assert_eq!((out.graced, out.lapsed, out.failed), (0, 1, 0));
    let s = h.only(&branch).await;
    assert_eq!((s.state, s.grace_until), (SubscriptionState::Lapsed, None));
    let e = h.db.entitlement(&branch).await.unwrap().unwrap();
    assert_eq!(
        (e.tier, e.source, e.addons),
        (
            arena_proto::Tier::Free,
            arena_proto::EntitlementSource::Free,
            Vec::<String>::new()
        )
    );
    assert_eq!(h.licence_tier(&token).await, arena_proto::Tier::Free);
    let history = h.db.subscription_history(&id).await.unwrap();
    let last = history.last().unwrap();
    assert_eq!(
        (
            last.old_state.as_str(),
            last.new_state.as_str(),
            last.cause.as_str()
        ),
        ("grace", "lapsed", "job:grace-ended")
    );
    // A lapsed subscription is left alone by later runs and later charges.
    assert_eq!(sweep_once(&h.st).await.unwrap().lapsed, 0);
    let r = h
        .send(
            "evt_late",
            &paid_event("sub_fake_001", PERIOD_END + 60 * 86_400, card("4242")),
        )
        .await;
    assert_eq!(r.body["status"], "recorded");
    assert_eq!(h.state_of(&branch).await, SubscriptionState::Lapsed);
}

#[tokio::test]
async fn a_period_that_ended_long_ago_passes_through_grace_and_lapses_in_one_run() {
    let (h, _a, branch, id, _token) = Harness::paid_with_hub().await;
    h.send(
        "evt_approval",
        &sub_event("subscription.renewal_needs_approval", "sub_fake_001"),
    )
    .await;
    h.set_now(END_MS + 30 * DAY_MS);
    let out = sweep_once(&h.st).await.unwrap();
    assert_eq!((out.graced, out.lapsed), (1, 1));
    assert_eq!(h.state_of(&branch).await, SubscriptionState::Lapsed);
    let states: Vec<_> =
        h.db.subscription_history(&id)
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.new_state)
            .collect();
    assert_eq!(
        states,
        ["active", "renewal_needs_approval", "grace", "lapsed"]
    );
}

#[tokio::test]
async fn paying_in_grace_returns_to_active_from_the_original_renewal_date() {
    let (h, _a, branch, _id, token) = Harness::paid_with_hub().await;
    h.set_now(END_MS + 1);
    h.send(
        "evt_pending",
        &sub_event("subscription.pending", "sub_fake_001"),
    )
    .await;
    h.set_now(END_MS + 3 * DAY_MS);
    // Razorpay's retry succeeds: its next period runs from the original renewal date.
    let r = h
        .send(
            "evt_retry",
            &paid_event("sub_fake_001", PERIOD_END + 30 * 86_400, card("4242")),
        )
        .await;
    assert_eq!(r.body["status"], "applied", "{}", r.body);
    let s = h.only(&branch).await;
    assert_eq!(s.state, SubscriptionState::Active);
    assert_eq!(s.grace_until, None);
    assert_eq!(s.current_period_end, Some(END_MS + 30 * DAY_MS));
    // No lapse ever happened.
    let out = sweep_once(&h.st).await.unwrap();
    assert_eq!((out.graced, out.lapsed), (0, 0));
    assert_eq!(h.licence_tier(&token).await, arena_proto::Tier::Basic);
    assert_eq!(
        h.db.entitlement(&branch).await.unwrap().unwrap().source,
        arena_proto::EntitlementSource::Paid
    );
}

#[tokio::test]
async fn a_halted_subscription_lapses_at_once_and_a_comp_grant_is_left_alone() {
    let (h, _a, branch, _id, token) = Harness::paid_with_hub().await;
    let r = h
        .send(
            "evt_halted",
            &sub_event("subscription.halted", "sub_fake_001"),
        )
        .await;
    assert_eq!(r.body["status"], "applied", "{}", r.body);
    assert_eq!(h.state_of(&branch).await, SubscriptionState::Lapsed);
    assert_eq!(
        h.db.entitlement(&branch).await.unwrap().unwrap().source,
        arena_proto::EntitlementSource::Free
    );
    assert_eq!(h.licence_tier(&token).await, arena_proto::Tier::Free);

    // A halt reported while the branch holds a staff-granted comp entitlement leaves it.
    let (h, _a, branch, _id, _token) = Harness::paid_with_hub().await;
    h.sql(&format!(
        "UPDATE entitlements SET source = 'comp', tier = 'premium', until = {} \
         WHERE branch_id = '{branch}'",
        END_MS * 2
    ));
    h.send(
        "evt_halted",
        &sub_event("subscription.halted", "sub_fake_001"),
    )
    .await;
    assert_eq!(h.state_of(&branch).await, SubscriptionState::Lapsed);
    let e = h.db.entitlement(&branch).await.unwrap().unwrap();
    assert_eq!(
        (e.tier, e.source),
        (
            arena_proto::Tier::Premium,
            arena_proto::EntitlementSource::Comp
        )
    );
}

#[tokio::test]
async fn a_new_mandate_authorised_in_grace_and_its_charge_settle_the_renewal() {
    let (h, a, branch, id, _token) = Harness::paid_with_hub().await;
    // The owner's session is short-lived, so the clock stays and grace is put in place directly.
    h.sql(&format!(
        "UPDATE subscriptions SET state = 'grace', grace_until = {} WHERE id = '{id}'",
        END_MS + GRACE_DAYS * DAY_MS
    ));
    let rev = h.rev(&branch).await;
    // Change plan stays shut; the payment method does not.
    let r = h
        .change(
            &branch,
            &a,
            "/preview",
            json!({"tier": "premium", "rev": rev}),
        )
        .await;
    assert_eq!(r.status, 422, "{}", r.body);
    let r = h.method(&branch, &a, json!({"rev": rev})).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        r.body["checkout"]["razorpay_subscription_id"],
        "sub_fake_m001"
    );

    let r = h
        .send(
            "evt_mandate",
            &authenticated("sub_fake_m001", PERIOD_END + 30 * 86_400, card("5678")),
        )
        .await;
    assert_eq!(r.body["status"], "applied", "{}", r.body);
    let s = h.only(&branch).await;
    assert_eq!(
        s.state,
        SubscriptionState::Grace,
        "authorising is not paying"
    );
    assert_eq!(s.mandate_detail.as_deref(), Some("5678"));
    assert_eq!(s.razorpay_subscription_id.as_deref(), Some("sub_fake_m001"));

    let r = h
        .send(
            "evt_mandate_charge",
            &paid_event("sub_fake_m001", PERIOD_END + 30 * 86_400, card("5678")),
        )
        .await;
    assert_eq!(r.body["status"], "applied", "{}", r.body);
    let s = h.only(&branch).await;
    assert_eq!((s.state, s.grace_until), (SubscriptionState::Active, None));
}

#[tokio::test]
async fn a_new_mandate_whose_first_event_is_its_charge_settles_a_renewal_waiting_for_approval() {
    let (h, a, branch, _id, _token) = Harness::paid_with_hub().await;
    h.send(
        "evt_approval",
        &sub_event("subscription.renewal_needs_approval", "sub_fake_001"),
    )
    .await;
    let rev = h.rev(&branch).await;
    let r = h.method(&branch, &a, json!({"rev": rev})).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let r = h
        .send(
            "evt_mandate_charge",
            &paid_event("sub_fake_m001", PERIOD_END + 30 * 86_400, card("5678")),
        )
        .await;
    assert_eq!(r.body["status"], "applied", "{}", r.body);
    let s = h.only(&branch).await;
    assert_eq!(s.state, SubscriptionState::Active);
    assert_eq!(s.current_period_end, Some(END_MS + 30 * DAY_MS));
    assert_eq!(s.razorpay_subscription_id.as_deref(), Some("sub_fake_m001"));
}

#[tokio::test]
async fn subscriptions_are_counted_per_state_and_the_card_reads_grace() {
    let (h, a, branch, id, _token) = Harness::paid_with_hub().await;
    h.sql(&format!(
        "UPDATE subscriptions SET state = 'grace', grace_until = {} WHERE id = '{id}'",
        END_MS + GRACE_DAYS * DAY_MS
    ));
    let plans = h.plans(&a, "").await;
    let card = &plans.body["branches"][0];
    assert_eq!(card["state"], "grace", "{}", plans.body);
    assert_eq!(card["grace_until"], END_MS + GRACE_DAYS * DAY_MS);
    assert_eq!(
        h.db.subscription_state_counts().await.unwrap(),
        [("grace".to_owned(), 1)]
    );
    assert_eq!(h.only(&branch).await.state, SubscriptionState::Grace);
}

// ---- Story 10.14: cancel keeps the plan to the end of the period, then the branch drops to Free ----

impl Harness {
    async fn cancel_call(&self, branch: &str, access: &str, step: &str, rev: i64) -> Reply {
        self.call(
            "POST",
            &format!("/v1/branches/{branch}/subscription/cancel{step}"),
            Some(json!({ "rev": rev })),
            access,
        )
        .await
    }

    /// A step-up proof on the owner's session, as a passed second factor leaves it.
    fn stepped_up(&self) {
        let now = self.clock.load(Ordering::SeqCst);
        self.sql(&format!("UPDATE owner_sessions SET stepup_at = {now}"));
    }
}

#[tokio::test]
async fn cancelling_needs_a_fresh_step_up_and_changes_nothing_without_one() {
    let (h, a, branch, _id, _token) = Harness::paid_with_hub().await;
    let rev = h.only(&branch).await.rev;
    let r = h.cancel_call(&branch, &a, "", rev).await;
    assert_eq!((r.status, r.code()), (403, "MFA_REQUIRED"), "{}", r.body);
    assert!(h.fake().cancelled_at_cycle_end().is_empty());
    let s = h.only(&branch).await;
    assert!(!s.cancel_at_period_end);
    assert_eq!(s.rev, rev);
}

#[tokio::test]
async fn a_cancel_tells_razorpay_marks_the_card_and_leaves_the_entitlement_and_licence() {
    let (h, a, branch, id, token) = Harness::paid_with_hub().await;
    h.stepped_up();
    let before = h.only(&branch).await;
    let r = h.cancel_call(&branch, &a, "", before.rev).await;
    assert_eq!(r.status, 204, "{}", r.body);
    assert_eq!(h.fake().cancelled_at_cycle_end(), ["sub_fake_001"]);
    assert!(h.fake().cancelled().is_empty());
    let s = h.only(&branch).await;
    assert!(s.cancel_at_period_end);
    assert_eq!(s.state, SubscriptionState::Active);
    assert_eq!(s.current_period_end, Some(END_MS));
    let e = h.db.entitlement(&branch).await.unwrap().unwrap();
    assert_eq!(
        (e.tier, e.source),
        (
            arena_proto::Tier::Basic,
            arena_proto::EntitlementSource::Paid
        )
    );
    assert_eq!(h.licence_tier(&token).await, arena_proto::Tier::Basic);
    let plans = h.plans(&a, "").await;
    assert_eq!(
        plans.body["branches"][0]["state"], "cancels",
        "{}",
        plans.body
    );
    let history = h.db.subscription_history(&id).await.unwrap();
    assert_eq!(history.last().unwrap().cause, "owner:cancel");

    // A repeat of the same request is harmless; a stale one is refused.
    let again = h.cancel_call(&branch, &a, "", s.rev).await;
    assert_eq!(again.status, 204);
    assert_eq!(h.fake().cancelled_at_cycle_end(), ["sub_fake_001"]);
    let stale = h.cancel_call(&branch, &a, "", before.rev).await;
    assert_eq!(
        (stale.status, stale.code()),
        (409, "STALE"),
        "{}",
        stale.body
    );
}

#[tokio::test]
async fn keep_my_plan_withdraws_the_cancellation_with_the_same_renewal_date() {
    let (h, a, branch, _id, _token) = Harness::paid_with_hub().await;
    h.stepped_up();
    let rev = h.only(&branch).await.rev;
    assert_eq!(h.cancel_call(&branch, &a, "", rev).await.status, 204);
    let cancelled = h.only(&branch).await;
    let r = h.cancel_call(&branch, &a, "/withdraw", cancelled.rev).await;
    assert_eq!(r.status, 204, "{}", r.body);
    assert_eq!(h.fake().kept(), ["sub_fake_001"]);
    assert!(h.fake().cancelled_at_cycle_end().is_empty());
    let s = h.only(&branch).await;
    assert!(!s.cancel_at_period_end);
    assert_eq!(s.state, SubscriptionState::Active);
    assert_eq!(s.current_period_end, Some(END_MS));
    let plans = h.plans(&a, "").await;
    assert_eq!(
        plans.body["branches"][0]["state"], "active",
        "{}",
        plans.body
    );
    // Nothing ends at the period end.
    h.set_now(END_MS + 1);
    let out = sweep_once(&h.st).await.unwrap();
    assert_eq!(out.ended, 0);
}

#[tokio::test]
async fn a_cancelled_plan_ends_with_no_grace_at_the_period_end_and_the_branch_drops_to_free() {
    let (h, a, branch, id, token) = Harness::paid_with_hub().await;
    h.stepped_up();
    let rev = h.only(&branch).await.rev;
    assert_eq!(h.cancel_call(&branch, &a, "", rev).await.status, 204);

    h.set_now(END_MS - 1);
    let out = sweep_once(&h.st).await.unwrap();
    assert_eq!(
        (out.graced, out.lapsed, out.ended, out.failed),
        (0, 0, 0, 0)
    );
    assert_eq!(h.state_of(&branch).await, SubscriptionState::Active);
    assert_eq!(h.licence_tier(&token).await, arena_proto::Tier::Basic);

    h.set_now(END_MS);
    let out = sweep_once(&h.st).await.unwrap();
    assert_eq!(
        (out.graced, out.lapsed, out.ended, out.failed),
        (0, 0, 1, 0)
    );
    let s = h.only(&branch).await;
    assert_eq!(s.state, SubscriptionState::Ended);
    assert_eq!(s.grace_until, None);
    let e = h.db.entitlement(&branch).await.unwrap().unwrap();
    assert_eq!(
        (e.tier, e.source),
        (
            arena_proto::Tier::Free,
            arena_proto::EntitlementSource::Free
        )
    );
    // The next heartbeat delivers a Free licence.
    assert_eq!(h.licence_tier(&token).await, arena_proto::Tier::Free);
    let last = h.db.subscription_history(&id).await.unwrap();
    let last = last.last().unwrap();
    assert_eq!(
        (
            last.old_state.as_str(),
            last.new_state.as_str(),
            last.cause.as_str()
        ),
        ("active", "ended", "job:cancelled-period-ended")
    );
    // The card reads Lapsed; the branch may subscribe again; a second run does nothing.
    // (The access token has expired by now, so the card is read through its own function.)
    assert_eq!(
        arena_cloud::account_plan::plan_state(&s),
        arena_proto::PlanState::Lapsed
    );
    assert!(
        h.db.subscription_of_branch(&branch)
            .await
            .unwrap()
            .is_none()
    );
    let out = sweep_once(&h.st).await.unwrap();
    assert_eq!(out.ended, 0);
}

#[tokio::test]
async fn a_cancel_is_refused_while_a_renewal_waits_and_after_the_plan_is_over() {
    let (h, a, branch, id, _token) = Harness::paid_with_hub().await;
    h.stepped_up();
    h.send(
        "evt_approval",
        &sub_event("subscription.renewal_needs_approval", "sub_fake_001"),
    )
    .await;
    let rev = h.only(&branch).await.rev;
    let r = h.cancel_call(&branch, &a, "", rev).await;
    assert_eq!(r.status, 422, "{}", r.body);
    assert!(h.fake().cancelled_at_cycle_end().is_empty());
    h.sql(&format!(
        "UPDATE subscriptions SET state = 'lapsed' WHERE id = '{id}'"
    ));
    let r = h.cancel_call(&branch, &a, "", rev).await;
    assert_eq!(r.status, 404, "{}", r.body);
}
