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
    assert!(c.flags.iter().any(|f| f == "tunnel"));
    assert!(c.flags.iter().any(|f| f == "uploads"));
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
