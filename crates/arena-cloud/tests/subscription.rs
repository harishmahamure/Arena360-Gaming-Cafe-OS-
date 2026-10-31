use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use arena_cloud::auth::AuthState;
use arena_cloud::billing::{BillingProvider, FakeRazorpay};
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

impl Harness {
    fn fake(&self) -> &FakeRazorpay {
        match self.st.billing.get() {
            Some(BillingProvider::Fake(f)) => f,
            _ => panic!("the test harness uses the fake billing provider"),
        }
    }

    fn provider(&self) -> &BillingProvider {
        self.st.billing.get().unwrap()
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

    async fn rows(&self, branch: &str) -> Vec<arena_store::control::Subscription> {
        self.db.subscriptions_of_branch(branch).await.unwrap()
    }

    /// An owner with a saved café; billing on.
    async fn owner_with_branch() -> (Self, String, String) {
        let h = Self::with_billing(true).await;
        let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
        let branch = h.cafe_done(&a).await;
        (h, a, branch)
    }
}

fn request(tier: &str, cycle: &str, country: &str) -> Value {
    json!({"tier": tier, "cycle": cycle, "billing_country": country})
}

fn methods(body: &Value) -> Vec<&str> {
    body["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn basic_yearly_in_india_is_pending_at_band_in_on_book_v1() {
    let (h, a, branch) = Harness::owner_with_branch().await;
    let r = h
        .subscribe(&branch, &a, request("basic", "yearly", "IN"))
        .await;
    assert_eq!(r.status, 201, "{}", r.body);
    assert_eq!(r.body["state"], "pending");
    assert_eq!(r.body["key_id"], "rzp_test_fake");
    assert_eq!(r.body["razorpay_subscription_id"], "sub_fake_001");
    assert_eq!(r.body["band"], "IN");
    assert_eq!(r.body["currency"], "INR");
    assert_eq!(r.body["amount_minor"], 200_000);
    assert_eq!(r.body["tax_minor"], 36_000);
    assert_eq!(r.body["total_minor"], 236_000);
    assert_eq!(methods(&r.body), ["upi", "card", "emandate"]);
    assert!(r.body.get("notice").is_none());

    let rows = h.rows(&branch).await;
    assert_eq!(rows.len(), 1);
    let s = &rows[0];
    assert_eq!(s.id, r.body["subscription_id"].as_str().unwrap());
    assert_eq!(s.state.as_str(), "pending");
    assert_eq!(s.band.as_str(), "IN");
    assert_eq!(s.price_book_id, "pb_v1");
    assert_eq!(s.razorpay_subscription_id.as_deref(), Some("sub_fake_001"));
    assert_eq!(s.first_paid_at, None);
    assert_eq!((s.created_at, s.updated_at), (T0, T0));

    let sent = h.fake().subscriptions();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].total_minor, 236_000);
    assert_eq!(sent[0].currency, "INR");
    assert_eq!(sent[0].subscription_id, s.id);
}

#[tokio::test]
async fn premium_yearly_in_india_needs_a_card_or_mandate() {
    let (h, a, branch) = Harness::owner_with_branch().await;
    let r = h
        .subscribe(&branch, &a, request("premium", "yearly", "IN"))
        .await;
    assert_eq!(r.status, 201, "{}", r.body);
    assert_eq!(r.body["total_minor"], 2_358_820);
    assert_eq!(r.body["tax_minor"], 359_820);
    assert_eq!(methods(&r.body), ["card", "emandate"]);
    assert_eq!(
        r.body["notice"],
        "Renewals over Rs 15,000 need a card or bank mandate"
    );
    assert_eq!(h.fake().subscriptions()[0].total_minor, 2_358_820);
}

#[tokio::test]
async fn add_ons_are_priced_into_the_locked_total() {
    let (h, a, branch) = Harness::owner_with_branch().await;
    let mut body = request("basic", "monthly", "IN");
    body["add_ons"] = json!({"devices_20": 2, "tunnel": 1});
    let r = h.subscribe(&branch, &a, body).await;
    assert_eq!(r.status, 201, "{}", r.body);
    // 20,000 + 2 x 5,000 + 20,000 paise, then 18% GST.
    assert_eq!(r.body["amount_minor"], 50_000);
    assert_eq!(r.body["tax_minor"], 9_000);
    assert_eq!(r.body["total_minor"], 59_000);
    let s = &h.rows(&branch).await[0];
    assert_eq!(
        (s.addon_devices_20, s.addon_tunnel, s.addon_owner_login),
        (2, 1, 0)
    );
}

#[tokio::test]
async fn a_country_without_a_band_or_a_band_off_sale_is_not_available() {
    let (h, a, branch) = Harness::owner_with_branch().await;
    for country in ["GB", "MY", "", "India"] {
        let r = h
            .subscribe(&branch, &a, request("basic", "monthly", country))
            .await;
        assert_eq!(
            (r.status, r.code(), r.field()),
            (409, "BAND_NOT_AVAILABLE", "billing_country"),
            "{country:?}"
        );
    }
    assert!(h.rows(&branch).await.is_empty());
    assert!(h.fake().subscriptions().is_empty());

    // Switching the band on is not enough: the band is the branch country's alone, so a request
    // naming another country is refused.
    h.sql("UPDATE billing_bands SET enabled = 1 WHERE band = 'MY'");
    let r = h
        .subscribe(&branch, &a, request("basic", "monthly", "my"))
        .await;
    assert_eq!((r.status, r.code()), (409, "BAND_NOT_AVAILABLE"));
    assert!(h.rows(&branch).await.is_empty());

    // A Malaysian branch opens it: card only, no GST, the band's currency.
    h.sql("UPDATE branches SET country = 'MY'");
    let r = h
        .subscribe(&branch, &a, request("basic", "monthly", "IN"))
        .await;
    assert_eq!((r.status, r.code()), (409, "BAND_NOT_AVAILABLE"));
    let r = h
        .subscribe(&branch, &a, request("basic", "monthly", "my"))
        .await;
    assert_eq!(r.status, 201, "{}", r.body);
    assert_eq!(r.body["band"], "MY");
    assert_eq!(r.body["currency"], "MYR");
    assert_eq!(r.body["tax_minor"], 0);
    assert_eq!(r.body["total_minor"], 2_300);
    assert_eq!(methods(&r.body), ["card"]);
}

#[tokio::test]
async fn a_branch_with_an_open_subscription_cannot_start_another() {
    let (h, a, branch) = Harness::owner_with_branch().await;
    let first = h
        .subscribe(&branch, &a, request("basic", "monthly", "IN"))
        .await;
    assert_eq!(first.status, 201, "{}", first.body);
    for state in [
        "active",
        "grace",
        "renewal_needs_approval",
        "pending_review",
    ] {
        h.sql(&format!("UPDATE subscriptions SET state = '{state}'"));
        let r = h
            .subscribe(&branch, &a, request("premium", "monthly", "IN"))
            .await;
        assert_eq!(
            (r.status, r.code()),
            (409, "SUBSCRIPTION_EXISTS"),
            "{state}"
        );
    }
    assert_eq!(h.fake().subscriptions().len(), 1);
    assert_eq!(h.rows(&branch).await.len(), 1);

    // An unpaid subscription is retried or replaced; a paid one never is.
    h.sql(&format!(
        "UPDATE subscriptions SET state = 'pending', first_paid_at = {T0}"
    ));
    let r = h
        .subscribe(&branch, &a, request("premium", "monthly", "IN"))
        .await;
    assert_eq!((r.status, r.code()), (409, "SUBSCRIPTION_EXISTS"));
    assert!(h.fake().cancelled().is_empty());

    // A lapsed (or cancelled, or abandoned) subscription is over: the owner can subscribe again.
    h.sql("UPDATE subscriptions SET state = 'lapsed'");
    let again = h
        .subscribe(&branch, &a, request("premium", "monthly", "IN"))
        .await;
    assert_eq!(again.status, 201, "{}", again.body);
    assert_eq!(h.rows(&branch).await.len(), 2);
}

#[tokio::test]
async fn two_racing_requests_reach_the_provider_once() {
    let (h, a, branch) = Harness::owner_with_branch().await;
    let (x, y) = tokio::join!(
        h.subscribe(&branch, &a, request("basic", "monthly", "IN")),
        h.subscribe(&branch, &a, request("basic", "monthly", "IN")),
    );
    // The loser either meets the row being created (409) or, a moment later, its Checkout again.
    assert!(
        [x.status, y.status].contains(&201)
            && [x.status, y.status].iter().all(|s| *s == 201 || *s == 409),
        "{} / {}",
        x.body,
        y.body
    );
    assert_eq!(h.fake().subscriptions().len(), 1);
    assert_eq!(h.rows(&branch).await.len(), 1);
}

#[tokio::test]
async fn another_owners_branch_is_forbidden_and_the_provider_untouched() {
    let (h, a, branch) = Harness::owner_with_branch().await;
    let b = h.owner(PHONE_B, "Other Den").await;
    h.cafe_done(&b).await;
    let r = h
        .subscribe(&branch, &b, request("basic", "monthly", "IN"))
        .await;
    assert_eq!((r.status, r.code()), (403, "FORBIDDEN"));
    let r = h
        .subscribe(
            "01JNOSUCHBRANCH0000000000000",
            &a,
            request("basic", "monthly", "IN"),
        )
        .await;
    assert_eq!((r.status, r.code()), (403, "FORBIDDEN"));
    let r = h
        .subscribe(&branch, "", request("basic", "monthly", "IN"))
        .await;
    assert_eq!(r.status, 401);
    assert!(h.rows(&branch).await.is_empty());
    assert!(h.fake().subscriptions().is_empty());
}

#[tokio::test]
async fn free_and_bad_add_ons_are_refused_before_the_provider() {
    let (h, a, branch) = Harness::owner_with_branch().await;
    let with = |tier: &str, add_ons: Value| {
        let mut b = request(tier, "monthly", "IN");
        b["add_ons"] = add_ons;
        b
    };
    for (body, field) in [
        (request("free", "monthly", "IN"), "tier"),
        (
            with("basic", json!({"owner_login": 1})),
            "add_ons.owner_login",
        ),
        (with("basic", json!({"tunnel": 2})), "add_ons.tunnel"),
        (with("premium", json!({"tunnel": 1})), "add_ons.tunnel"),
        (
            with("basic", json!({"devices_20": 11})),
            "add_ons.devices_20",
        ),
    ] {
        let r = h.subscribe(&branch, &a, body.clone()).await;
        assert_eq!(
            (r.status, r.code(), r.field()),
            (422, "VALIDATION_FAILED", field),
            "{body}"
        );
    }
    // A validation error wins over an unavailable band.
    let r = h
        .subscribe(&branch, &a, request("free", "monthly", "GB"))
        .await;
    assert_eq!((r.status, r.code()), (422, "VALIDATION_FAILED"));
    assert!(h.rows(&branch).await.is_empty());
    assert!(h.fake().subscriptions().is_empty());
}

#[tokio::test]
async fn billing_off_refuses_every_subscription() {
    let h = Harness::with_billing(false).await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let branch = h.cafe_done(&a).await;
    let r = h
        .subscribe(&branch, &a, request("basic", "monthly", "IN"))
        .await;
    assert_eq!((r.status, r.code()), (409, "BILLING_DISABLED"));
    assert!(h.rows(&branch).await.is_empty());
    assert!(h.fake().subscriptions().is_empty());
}

#[tokio::test]
async fn a_provider_failure_abandons_the_row_and_the_branch_can_retry() {
    let (h, a, branch) = Harness::owner_with_branch().await;
    h.fake().fail_subscriptions(true);
    let r = h
        .subscribe(&branch, &a, request("basic", "monthly", "IN"))
        .await;
    assert_eq!((r.status, r.code()), (503, "INTERNAL"));
    assert!(!r.body.to_string().contains("FAKE_SUBSCRIPTION_FAILED"));
    let rows = h.rows(&branch).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state.as_str(), "abandoned");
    assert_eq!(rows[0].razorpay_subscription_id, None);

    h.fake().fail_subscriptions(false);
    let r = h
        .subscribe(&branch, &a, request("basic", "monthly", "IN"))
        .await;
    assert_eq!(r.status, 201, "{}", r.body);
    let rows = h.rows(&branch).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].state.as_str(), "pending");
}

#[tokio::test]
async fn a_later_price_book_never_reprices_an_existing_subscription() {
    use arena_cloud::price_book;
    use arena_proto::{AddOns, Band, BillingCycle, PriceItem};

    let (h, a, branch) = Harness::owner_with_branch().await;
    let r = h
        .subscribe(&branch, &a, request("basic", "yearly", "IN"))
        .await;
    assert_eq!(r.status, 201, "{}", r.body);

    let (draft, version) = price_book::open_draft(&h.db, "harish", "raise", T0)
        .await
        .unwrap();
    assert_eq!(version, 2);
    price_book::set_amount(
        &h.db,
        &draft,
        Band::In,
        PriceItem::Basic,
        30_000,
        "harish",
        "raise",
        T0,
    )
    .await
    .unwrap();
    let provider = BillingProvider::Fake(FakeRazorpay::new());
    price_book::publish(&h.db, &provider, &draft, "harish", "go", T0 + 1)
        .await
        .unwrap();
    assert_eq!(h.db.live_price_book().await.unwrap().unwrap().id, draft);

    let sub = h.db.subscription_of_branch(&branch).await.unwrap().unwrap();
    assert_eq!(sub.price_book_id, "pb_v1");
    let locked = arena_cloud::subscription::quote_for(&h.db, &sub)
        .await
        .unwrap();
    assert_eq!(
        (locked.amount_minor, locked.tax_minor, locked.total_minor),
        (200_000, 36_000, 236_000)
    );
    // A quote from the live book is higher: the lock is what protects the subscriber.
    let live = arena_cloud::subscription::quote(
        &h.db,
        &draft,
        Band::In,
        PriceItem::Basic,
        BillingCycle::Yearly,
        &AddOns::default(),
    )
    .await
    .unwrap();
    assert_eq!(live.amount_minor, 300_000);

    // A second branch subscribing now gets the new book.
    let b = h.owner(PHONE_B, "Other Den").await;
    let other = h.cafe_done(&b).await;
    let r = h
        .subscribe(&other, &b, request("basic", "yearly", "IN"))
        .await;
    assert_eq!(r.status, 201, "{}", r.body);
    assert_eq!(r.body["amount_minor"], 300_000);
    let sub2 = h.db.subscription_of_branch(&other).await.unwrap().unwrap();
    assert_eq!(sub2.price_book_id, draft);
}

#[tokio::test]
async fn the_sweep_cancels_and_abandons_unpaid_pending_subscriptions_after_a_day() {
    use arena_cloud::subscription::{PENDING_TTL_MS, sweep_pending};

    let (h, a, branch) = Harness::owner_with_branch().await;
    let r = h
        .subscribe(&branch, &a, request("basic", "monthly", "IN"))
        .await;
    assert_eq!(r.status, 201, "{}", r.body);

    // Not yet a day old: untouched.
    let early = T0 + PENDING_TTL_MS;
    let out = sweep_pending(&h.db, h.provider(), early).await.unwrap();
    assert_eq!((out.abandoned, out.failed), (0, 0));
    assert_eq!(h.rows(&branch).await[0].state.as_str(), "pending");

    // Older than a day, but the provider fails: left for the next run.
    let late = T0 + PENDING_TTL_MS + 1;
    h.fake().fail_subscriptions(true);
    let out = sweep_pending(&h.db, h.provider(), late).await.unwrap();
    assert_eq!((out.abandoned, out.failed), (0, 1));
    assert_eq!(h.rows(&branch).await[0].state.as_str(), "pending");
    assert!(h.fake().cancelled().is_empty());

    // The provider is back: cancelled there, abandoned here, and the branch can subscribe again.
    h.fake().fail_subscriptions(false);
    let out = sweep_pending(&h.db, h.provider(), late).await.unwrap();
    assert_eq!((out.abandoned, out.failed), (1, 0));
    assert_eq!(h.fake().cancelled(), ["sub_fake_001"]);
    assert_eq!(h.rows(&branch).await[0].state.as_str(), "abandoned");
    assert_eq!(
        sweep_pending(&h.db, h.provider(), late).await.unwrap(),
        Default::default()
    );

    let again = h
        .subscribe(&branch, &a, request("basic", "monthly", "IN"))
        .await;
    assert_eq!(again.status, 201, "{}", again.body);
    assert_eq!(h.rows(&branch).await.len(), 2);
}

#[tokio::test]
async fn the_sweep_leaves_paid_and_active_subscriptions_alone() {
    use arena_cloud::subscription::{PENDING_TTL_MS, sweep_pending};

    let (h, a, branch) = Harness::owner_with_branch().await;
    let r = h
        .subscribe(&branch, &a, request("basic", "monthly", "IN"))
        .await;
    assert_eq!(r.status, 201, "{}", r.body);
    let late = T0 + PENDING_TTL_MS + 1;

    // Paid while still pending (the webhook has not caught up): never abandoned.
    h.sql(&format!("UPDATE subscriptions SET first_paid_at = {T0}"));
    let out = sweep_pending(&h.db, h.provider(), late).await.unwrap();
    assert_eq!((out.abandoned, out.failed), (0, 0));
    assert!(h.fake().cancelled().is_empty());

    h.sql("UPDATE subscriptions SET first_paid_at = NULL, state = 'active'");
    let out = sweep_pending(&h.db, h.provider(), late).await.unwrap();
    assert_eq!((out.abandoned, out.failed), (0, 0));
    assert_eq!(h.rows(&branch).await[0].state.as_str(), "active");
}

#[tokio::test]
async fn under_a_manual_provider_the_sweep_leaves_an_ops_task() {
    use arena_cloud::subscription::{PENDING_TTL_MS, TASK_CANCEL_SUBSCRIPTION, sweep_pending};

    let (h, a, branch) = Harness::owner_with_branch().await;
    let r = h
        .subscribe(&branch, &a, request("basic", "monthly", "IN"))
        .await;
    assert_eq!(r.status, 201, "{}", r.body);
    let id = h.rows(&branch).await[0].id.clone();
    let out = sweep_pending(&h.db, &BillingProvider::Manual, T0 + PENDING_TTL_MS + 1)
        .await
        .unwrap();
    assert_eq!((out.abandoned, out.failed), (1, 0));
    assert_eq!(h.rows(&branch).await[0].state.as_str(), "abandoned");
    let tasks = h.db.open_ops_tasks(TASK_CANCEL_SUBSCRIPTION).await.unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].target, id);
    assert!(tasks[0].details_json.contains("sub_fake_001"));
    assert!(h.fake().cancelled().is_empty());
}

#[tokio::test]
async fn a_row_that_never_reached_the_provider_is_abandoned_without_a_cancel() {
    use arena_cloud::subscription::{PENDING_TTL_MS, sweep_pending};

    let (h, a, branch) = Harness::owner_with_branch().await;
    let r = h
        .subscribe(&branch, &a, request("basic", "monthly", "IN"))
        .await;
    assert_eq!(r.status, 201, "{}", r.body);
    // A crash between reserving the row and storing the provider's id.
    h.sql("UPDATE subscriptions SET razorpay_subscription_id = NULL");
    let out = sweep_pending(&h.db, h.provider(), T0 + PENDING_TTL_MS + 1)
        .await
        .unwrap();
    assert_eq!((out.abandoned, out.failed), (1, 0));
    assert!(h.fake().cancelled().is_empty());
}

fn quote_path(branch: &str) -> String {
    format!("/v1/branches/{branch}/subscription/quote")
}

fn pick(tier: &str, cycle: &str) -> Value {
    json!({"tier": tier, "cycle": cycle})
}

#[tokio::test]
async fn the_same_unpaid_choice_again_answers_the_same_checkout_without_a_new_subscription() {
    let (h, a, branch) = Harness::owner_with_branch().await;
    let mut body = request("basic", "yearly", "IN");
    body["add_ons"] = json!({"devices_20": 1});
    let first = h.subscribe(&branch, &a, body.clone()).await;
    assert_eq!(first.status, 201, "{}", first.body);
    // A closed or declined Checkout, then Pay again.
    for _ in 0..2 {
        let again = h.subscribe(&branch, &a, body.clone()).await;
        assert_eq!(again.status, 201, "{}", again.body);
        assert_eq!(again.body, first.body);
    }
    assert_eq!(h.rows(&branch).await.len(), 1);
    assert_eq!(h.fake().subscriptions().len(), 1);
    assert!(h.fake().cancelled().is_empty());
}

#[tokio::test]
async fn another_choice_cancels_the_unpaid_subscription_and_starts_a_new_one() {
    let (h, a, branch) = Harness::owner_with_branch().await;
    let first = h
        .subscribe(&branch, &a, request("basic", "monthly", "IN"))
        .await;
    assert_eq!(first.status, 201, "{}", first.body);
    let mut other = request("basic", "monthly", "IN");
    other["add_ons"] = json!({"tunnel": 1});
    for changed in [
        other,
        request("premium", "monthly", "IN"),
        request("basic", "yearly", "IN"),
    ] {
        let before = h.rows(&branch).await.len();
        let r = h.subscribe(&branch, &a, changed.clone()).await;
        assert_eq!(r.status, 201, "{} {}", changed, r.body);
        assert_ne!(r.body["subscription_id"], first.body["subscription_id"]);
        let rows = h.rows(&branch).await;
        assert_eq!(rows.len(), before + 1);
        assert_eq!(rows[0].state.as_str(), "pending");
        assert_eq!(rows[1].state.as_str(), "abandoned");
    }
    assert_eq!(
        h.fake().cancelled(),
        ["sub_fake_001", "sub_fake_002", "sub_fake_003"]
    );
    assert_eq!(h.fake().subscriptions().len(), 4);
    let open: Vec<_> = h
        .rows(&branch)
        .await
        .into_iter()
        .filter(|s| !s.state.is_over())
        .collect();
    assert_eq!(open.len(), 1);
}

#[tokio::test]
async fn a_provider_failure_while_replacing_keeps_the_old_subscription() {
    let (h, a, branch) = Harness::owner_with_branch().await;
    let first = h
        .subscribe(&branch, &a, request("basic", "monthly", "IN"))
        .await;
    assert_eq!(first.status, 201, "{}", first.body);
    h.fake().fail_subscriptions(true);
    let r = h
        .subscribe(&branch, &a, request("premium", "monthly", "IN"))
        .await;
    assert_eq!((r.status, r.code()), (503, "INTERNAL"));
    assert!(!r.body.to_string().contains("FAKE_SUBSCRIPTION_FAILED"));
    let rows = h.rows(&branch).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state.as_str(), "pending");
    assert_eq!(rows[0].tier.as_str(), "basic");
    assert!(h.fake().cancelled().is_empty());

    // The provider is back: the old Checkout is still there to retry.
    h.fake().fail_subscriptions(false);
    let again = h
        .subscribe(&branch, &a, request("basic", "monthly", "IN"))
        .await;
    assert_eq!(again.body, first.body);
}

#[tokio::test]
async fn a_row_still_being_created_is_not_retried() {
    let (h, a, branch) = Harness::owner_with_branch().await;
    let r = h
        .subscribe(&branch, &a, request("basic", "monthly", "IN"))
        .await;
    assert_eq!(r.status, 201, "{}", r.body);
    h.sql("UPDATE subscriptions SET razorpay_subscription_id = NULL");
    for tier in ["basic", "premium"] {
        let r = h
            .subscribe(&branch, &a, request(tier, "monthly", "IN"))
            .await;
        assert_eq!((r.status, r.code()), (409, "SUBSCRIPTION_EXISTS"), "{tier}");
    }
    assert!(h.fake().cancelled().is_empty());
}

#[tokio::test]
async fn the_quote_is_made_by_the_cloud_for_the_branchs_band() {
    let (h, a, branch) = Harness::owner_with_branch().await;
    let r = h
        .call(
            "POST",
            &quote_path(&branch),
            Some(pick("basic", "yearly")),
            &a,
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["band"], "IN");
    assert_eq!(r.body["currency"], "INR");
    assert_eq!(r.body["tier"], "basic");
    assert_eq!(r.body["cycle"], "yearly");
    assert_eq!(r.body["amount_minor"], 200_000);
    assert_eq!(r.body["tax_minor"], 36_000);
    assert_eq!(r.body["total_minor"], 236_000);
    assert_eq!(r.body["gst_percent"], 18);
    assert_eq!(
        r.body["add_ons"],
        json!({"devices_20": 0, "tunnel": 0, "owner_login": 0})
    );
    // T0 is 2027-01-15 08:00 UTC: a year later is 2028-01-15 08:00 UTC.
    assert_eq!(r.body["renews_at"], 1_831_536_000_000_i64);
    assert_eq!(methods(&r.body), ["upi", "card", "emandate"]);
    assert!(r.body.get("notice").is_none());

    let mut monthly = pick("basic", "monthly");
    monthly["add_ons"] = json!({"devices_20": 2, "tunnel": 1});
    let r = h
        .call("POST", &quote_path(&branch), Some(monthly), &a)
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        (
            r.body["amount_minor"].as_i64(),
            r.body["tax_minor"].as_i64(),
            r.body["total_minor"].as_i64()
        ),
        (Some(50_000), Some(9_000), Some(59_000))
    );
    // 2027-02-15 08:00 UTC.
    assert_eq!(r.body["renews_at"], 1_802_678_400_000_i64);

    let r = h
        .call(
            "POST",
            &quote_path(&branch),
            Some(pick("premium", "yearly")),
            &a,
        )
        .await;
    assert_eq!(r.body["total_minor"], 2_358_820);
    assert_eq!(methods(&r.body), ["card", "emandate"]);
    assert_eq!(
        r.body["notice"],
        "Renewals over Rs 15,000 need a card or bank mandate"
    );

    // A quote writes nothing and calls no provider.
    assert!(h.rows(&branch).await.is_empty());
    assert!(h.fake().subscriptions().is_empty());
}

#[tokio::test]
async fn a_band_outside_india_quotes_without_gst() {
    let (h, a, branch) = Harness::owner_with_branch().await;
    h.sql(
        "UPDATE billing_bands SET enabled = 1 WHERE band = 'MY'; \
         UPDATE branches SET country = 'MY'",
    );
    let r = h
        .call(
            "POST",
            &quote_path(&branch),
            Some(pick("basic", "monthly")),
            &a,
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["band"], "MY");
    assert_eq!(r.body["currency"], "MYR");
    assert_eq!(
        (
            r.body["tax_minor"].as_i64(),
            r.body["gst_percent"].as_i64(),
            r.body["total_minor"].as_i64()
        ),
        (Some(0), Some(0), Some(2_300))
    );
    assert_eq!(methods(&r.body), ["card"]);
}

#[tokio::test]
async fn the_quote_refuses_what_checkout_would() {
    let (h, a, branch) = Harness::owner_with_branch().await;
    let with = |tier: &str, add_ons: Value| {
        let mut b = pick(tier, "monthly");
        b["add_ons"] = add_ons;
        b
    };
    for (body, field) in [
        (pick("free", "monthly"), "tier"),
        (
            with("basic", json!({"owner_login": 1})),
            "add_ons.owner_login",
        ),
        (with("premium", json!({"tunnel": 1})), "add_ons.tunnel"),
        (
            with("basic", json!({"devices_20": 11})),
            "add_ons.devices_20",
        ),
    ] {
        let r = h
            .call("POST", &quote_path(&branch), Some(body.clone()), &a)
            .await;
        assert_eq!(
            (r.status, r.code(), r.field()),
            (422, "VALIDATION_FAILED", field),
            "{body}"
        );
    }
    let r = h
        .call(
            "POST",
            &quote_path(&branch),
            Some(pick("basic", "monthly")),
            "",
        )
        .await;
    assert_eq!(r.status, 401);
    let b = h.owner(PHONE_B, "Other Den").await;
    h.cafe_done(&b).await;
    let r = h
        .call(
            "POST",
            &quote_path(&branch),
            Some(pick("basic", "monthly")),
            &b,
        )
        .await;
    assert_eq!((r.status, r.code()), (403, "FORBIDDEN"));

    // A country with no band, or a band off sale.
    for country in ["GB", "MY"] {
        h.sql(&format!(
            "UPDATE branches SET country = '{country}' WHERE id = '{branch}'"
        ));
        let r = h
            .call(
                "POST",
                &quote_path(&branch),
                Some(pick("basic", "monthly")),
                &a,
            )
            .await;
        assert_eq!(
            (r.status, r.code()),
            (409, "BAND_NOT_AVAILABLE"),
            "{country}"
        );
    }

    // A branch with a subscription that is not an unpaid pending one.
    h.sql(&format!(
        "UPDATE branches SET country = 'IN' WHERE id = '{branch}'"
    ));
    let made = h
        .subscribe(&branch, &a, request("basic", "monthly", "IN"))
        .await;
    assert_eq!(made.status, 201, "{}", made.body);
    h.sql("UPDATE subscriptions SET state = 'active'");
    let r = h
        .call(
            "POST",
            &quote_path(&branch),
            Some(pick("basic", "monthly")),
            &a,
        )
        .await;
    assert_eq!((r.status, r.code()), (409, "SUBSCRIPTION_EXISTS"));
    assert_eq!(h.fake().subscriptions().len(), 1);
}

#[tokio::test]
async fn billing_off_refuses_the_quote() {
    let h = Harness::with_billing(false).await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let branch = h.cafe_done(&a).await;
    let r = h
        .call(
            "POST",
            &quote_path(&branch),
            Some(pick("basic", "monthly")),
            &a,
        )
        .await;
    assert_eq!((r.status, r.code()), (409, "BILLING_DISABLED"));
}

#[tokio::test]
async fn the_summary_of_a_waiting_choice_uses_its_own_book_and_any_other_the_live_one() {
    use arena_cloud::price_book;
    use arena_proto::{Band, PriceItem};

    let (h, a, branch) = Harness::owner_with_branch().await;
    let made = h
        .subscribe(&branch, &a, request("basic", "yearly", "IN"))
        .await;
    assert_eq!(made.status, 201, "{}", made.body);

    let (draft, _) = price_book::open_draft(&h.db, "harish", "raise", T0)
        .await
        .unwrap();
    price_book::set_amount(
        &h.db,
        &draft,
        Band::In,
        PriceItem::Basic,
        30_000,
        "harish",
        "raise",
        T0,
    )
    .await
    .unwrap();
    let provider = BillingProvider::Fake(FakeRazorpay::new());
    price_book::publish(&h.db, &provider, &draft, "harish", "go", T0 + 1)
        .await
        .unwrap();

    // The same choice: what Checkout will charge, from the locked v1.
    let same = h
        .call(
            "POST",
            &quote_path(&branch),
            Some(pick("basic", "yearly")),
            &a,
        )
        .await;
    assert_eq!(same.body["total_minor"], 236_000);
    // Another choice replaces it, so it is priced from the live book.
    let other = h
        .call(
            "POST",
            &quote_path(&branch),
            Some(pick("basic", "monthly")),
            &a,
        )
        .await;
    assert_eq!(other.status, 200, "{}", other.body);
    assert_eq!(other.body["amount_minor"], 30_000);
}
