//! `POST /v1/notify` (Story 8.28): the send ledger, the WhatsApp-then-SMS fallback, the
//! entitlement gate and the templates, against the recorded fake adapter.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use arena_cloud::auth::AuthState;
use arena_cloud::config::{AuthConfig, Role, SmsProviderConfig};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
use arena_cloud::messaging::{Behaviour, Channel, FakeNotifier, FakeSms, Messenger, Notifier};
use arena_cloud::{Clock, app_with_auth, auth_state};
use arena_proto::Tier;
use arena_store::control::{
    CompGrant, ControlDb, LATEST_CONTROL_VERSION, NewBackupCode, NewTemplate,
};
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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

    async fn notify_with(&self, token: &str, body: Value) -> Reply {
        let mut req = Request::builder()
            .method("POST")
            .uri("/v1/notify")
            .header("content-type", "application/json");
        if !token.is_empty() {
            req = req.header("authorization", format!("Bearer {token}"));
        }
        let req = req.body(Body::from(body.to_string())).unwrap();
        reply(self.app.clone().oneshot(req).await.unwrap()).await
    }

    async fn notify(&self, token: &str, outbox_id: &str, kind: &str) -> Reply {
        self.notify_with(token, request(outbox_id, kind)).await
    }

    fn fake(&self) -> &FakeNotifier {
        self.st
            .notifier
            .get()
            .and_then(Notifier::fake)
            .expect("the recorded fake")
    }

    /// The ledger row of `outbox_id` as one raw string, to look for what it must not hold.
    fn ledger_text(&self, outbox_id: &str) -> String {
        rusqlite::Connection::open(self.dir.path().join("gz_control.db"))
            .unwrap()
            .query_row(
                "SELECT id || tenant_id || branch_id || outbox_id || kind || purpose || \
                 consent_policy_version || recipient_hash || attempt_log || \
                 COALESCE(error_code, '') || COALESCE(provider_message_id, '') \
                 FROM send_ledger WHERE outbox_id = ?1",
                [outbox_id],
                |r| r.get(0),
            )
            .unwrap()
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

/// The player the messages are for.
const PLAYER: &str = "+919811122233";

fn request(outbox_id: &str, kind: &str) -> Value {
    let params = match kind {
        "receipt" => {
            json!({"name": "Ravi", "receipt_no": "INV-7", "amount": "Rs 120", "cafe": "Pixel Arena"})
        }
        "low_balance" => json!({"name": "Ravi", "balance": "Rs 20", "cafe": "Pixel Arena"}),
        _ => json!({}),
    };
    json!({
        "outbox_id": outbox_id,
        "kind": kind,
        "purpose": "transactional",
        "consent_policy_version": "2026-10",
        "recipient": PLAYER,
        "params": params,
    })
}

fn next_version(
    t: &arena_store::control::MessageTemplate,
    n: u32,
    approved: bool,
) -> NewTemplate<'_> {
    let _ = n;
    NewTemplate {
        kind: "receipt",
        whatsapp_template_id: "arena360_receipt_next",
        sms_dlt_template_id: "DLT-ARENA360-RECEIPT-NEXT",
        sms_text: &t.sms_text,
        param_names: &t.param_names,
        approved_at: approved.then_some(T0),
    }
}

#[tokio::test]
async fn a_paid_hub_sends_once_on_whatsapp_and_the_ledger_keeps_only_a_hash() {
    let h = Harness::new().await;
    let a = h.paid_cafe(None).await;
    let r = h.notify(&a.token, "OUT1", "receipt").await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["state"], "sent");
    assert_eq!(r.body["channel"], "whatsapp");
    assert_eq!(r.body["attempts"], 1);
    let id = r.body["provider_message_id"].as_str().unwrap().to_owned();

    let sent = h.fake().sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].channel, Channel::Whatsapp);
    assert_eq!(sent[0].to, PLAYER);
    assert_eq!(sent[0].template_id, "arena360_receipt_v1");
    assert_eq!(sent[0].params, ["Ravi", "INV-7", "Rs 120", "Pixel Arena"]);

    let row = h.db.ledger_by_outbox("OUT1").await.unwrap().unwrap();
    assert_eq!(
        (row.state.as_str(), row.channel.as_str()),
        ("sent", "whatsapp")
    );
    assert_eq!(row.template_version, 1);
    assert_eq!(row.provider_message_id.as_deref(), Some(id.as_str()));
    assert_eq!(row.branch_id, a.branch);
    assert_eq!(row.tenant_id, a.tenant);
    assert_eq!(row.recipient_hash.len(), 64);
    assert_eq!(row.recipient_hash, h.st.recipient_hash(PLAYER));
    assert_ne!(row.recipient_hash, h.st.recipient_hash(PHONE_B));
    let text = h.ledger_text("OUT1");
    assert!(
        !text.contains("98111") && !text.contains("Ravi") && !text.contains("INV-7"),
        "{text}"
    );
}

#[tokio::test]
async fn the_same_outbox_id_again_sends_nothing_and_returns_the_original_result() {
    let h = Harness::new().await;
    let a = h.paid_cafe(None).await;
    let first = h.notify(&a.token, "OUT1", "receipt").await;
    h.now.fetch_add(HOUR, Ordering::SeqCst);
    // Even a body that differs gets the stored answer.
    let again = h.notify(&a.token, "OUT1", "low_balance").await;
    assert_eq!((again.status, &again.body), (200, &first.body));
    assert_eq!(h.fake().sent().len(), 1);
}

#[tokio::test]
async fn a_whatsapp_rejection_falls_back_to_sms_and_the_ledger_records_both_attempts() {
    let h = Harness::new().await;
    let a = h.paid_cafe(None).await;
    h.fake().set_behaviour(Channel::Whatsapp, Behaviour::Reject);
    let r = h.notify(&a.token, "OUT1", "low_balance").await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["state"], "sent");
    assert_eq!(r.body["channel"], "sms");
    assert_eq!(r.body["attempts"], 2);

    let sent = h.fake().sent();
    assert_eq!(sent.len(), 1, "the rejected WhatsApp was not delivered");
    assert_eq!(sent[0].channel, Channel::Sms);
    assert_eq!(sent[0].template_id, "DLT-ARENA360-LOWBAL-1");
    assert_eq!(
        sent[0].text,
        "Hi Ravi, your wallet at Pixel Arena is down to Rs 20. Top up at the counter. - Arena 360"
    );
    let row = h.db.ledger_by_outbox("OUT1").await.unwrap().unwrap();
    let log: Vec<Value> = serde_json::from_str(&row.attempt_log).unwrap();
    assert_eq!(log.len(), 2);
    assert_eq!(log[0]["channel"], "whatsapp");
    assert_eq!(log[0]["outcome"], "rejected");
    assert_eq!(log[1]["channel"], "sms");
    assert_eq!(log[1]["outcome"], "sent");
    assert_eq!(row.channel, "sms");
}

#[tokio::test]
async fn both_channels_failing_ends_failed_and_a_repeat_does_not_try_again() {
    let h = Harness::new().await;
    let a = h.paid_cafe(None).await;
    h.fake().set_behaviour(Channel::Whatsapp, Behaviour::Reject);
    h.fake().set_behaviour(Channel::Sms, Behaviour::Reject);
    let r = h.notify(&a.token, "OUT1", "receipt").await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["state"], "failed");
    assert_eq!(r.body["error_code"], "FAKE_REJECTED");
    assert_eq!(r.body["attempts"], 2);

    h.fake()
        .set_behaviour(Channel::Whatsapp, Behaviour::Deliver);
    h.fake().set_behaviour(Channel::Sms, Behaviour::Deliver);
    let again = h.notify(&a.token, "OUT1", "receipt").await;
    assert_eq!(again.body, r.body);
    assert!(h.fake().sent().is_empty());
}

#[tokio::test]
async fn a_transient_failure_stays_queued_and_the_hub_can_post_it_again() {
    let h = Harness::new().await;
    let a = h.paid_cafe(None).await;
    h.fake()
        .set_behaviour(Channel::Whatsapp, Behaviour::Transient);
    let r = h.notify(&a.token, "OUT1", "receipt").await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["state"], "queued");
    assert_eq!(r.body["error_code"], "FAKE_UNAVAILABLE");
    assert!(h.fake().sent().is_empty());

    h.fake()
        .set_behaviour(Channel::Whatsapp, Behaviour::Deliver);
    let r = h.notify(&a.token, "OUT1", "receipt").await;
    assert_eq!(r.body["state"], "sent");
    assert_eq!(r.body["channel"], "whatsapp");
    assert_eq!(r.body["attempts"], 2);
    assert_eq!(h.fake().sent().len(), 1);
}

#[tokio::test]
async fn an_unknown_outcome_is_never_sent_again() {
    let h = Harness::new().await;
    let a = h.paid_cafe(None).await;
    h.fake()
        .set_behaviour(Channel::Whatsapp, Behaviour::Uncertain);
    let r = h.notify(&a.token, "OUT1", "receipt").await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["state"], "queued");

    h.fake()
        .set_behaviour(Channel::Whatsapp, Behaviour::Deliver);
    let again = h.notify(&a.token, "OUT1", "receipt").await;
    assert_eq!(again.body, r.body);
    assert!(
        h.fake().sent().is_empty(),
        "a row whose outcome is unknown is not resent"
    );
}

#[tokio::test]
async fn two_posts_at_once_send_one_message() {
    let h = Harness::new().await;
    let a = h.paid_cafe(None).await;
    let (x, y) = tokio::join!(
        h.notify(&a.token, "OUT1", "receipt"),
        h.notify(&a.token, "OUT1", "receipt")
    );
    assert_eq!((x.status, y.status), (200, 200));
    assert_eq!(h.fake().sent().len(), 1);
}

#[tokio::test]
async fn a_free_branch_is_refused_and_nothing_is_recorded_or_sent() {
    let h = Harness::new().await;
    let a = h.cafe(PHONE_A, "Pixel Arena", "198.51.100.4").await;
    let r = h.notify(&a.token, "OUT1", "receipt").await;
    assert_eq!((r.status, r.code()), (403, "FEATURE_NOT_ENTITLED"));
    assert!(h.db.ledger_by_outbox("OUT1").await.unwrap().is_none());
    assert!(h.fake().sent().is_empty());

    // A paid plan that has run out is Free again.
    h.upgrade(&a.branch, Tier::Basic, T0 + DAY).await;
    assert_eq!(h.notify(&a.token, "OUT1", "receipt").await.status, 200);
    h.now.fetch_add(2 * DAY, Ordering::SeqCst);
    let r = h.notify(&a.token, "OUT2", "receipt").await;
    assert_eq!((r.status, r.code()), (403, "FEATURE_NOT_ENTITLED"));
    assert_eq!(h.fake().sent().len(), 1);
}

#[tokio::test]
async fn only_a_valid_hub_token_may_post() {
    let h = Harness::new().await;
    let a = h.paid_cafe(None).await;
    for token in ["", "arena_hub_nope.x"] {
        let r = h.notify(token, "OUT1", "receipt").await;
        assert_eq!((r.status, r.code()), (401, "TOKEN_INVALID"));
    }
    h.revoke_hubs(&a.branch);
    let r = h.notify(&a.token, "OUT1", "receipt").await;
    assert_eq!((r.status, r.code()), (401, "TOKEN_REVOKED"));
    assert!(h.fake().sent().is_empty());
}

#[tokio::test]
async fn bad_shapes_are_validation_failures_that_record_and_send_nothing() {
    let h = Harness::new().await;
    let a = h.paid_cafe(None).await;
    let with = |f: &dyn Fn(&mut Value)| {
        let mut b = request("OUT1", "receipt");
        f(&mut b);
        b
    };
    let cases = [
        with(&|b| b["recipient"] = json!("9811122233")),
        with(&|b| b["recipient"] = json!("+0811122233")),
        with(&|b| b["outbox_id"] = json!("a/b")),
        with(&|b| b["outbox_id"] = json!("")),
        with(&|b| b["kind"] = json!("newsletter")),
        // The owner alert is the cloud's own message: a Hub cannot post it.
        with(&|b| b["kind"] = json!("owner_alert")),
        with(&|b| b["purpose"] = json!("")),
        with(&|b| b["consent_policy_version"] = json!("")),
        with(&|b| {
            b["params"].as_object_mut().unwrap().remove("amount");
        }),
        with(&|b| b["params"]["extra"] = json!("x")),
        with(&|b| b["params"]["name"] = json!("line\nbreak")),
        with(&|b| b["params"]["name"] = json!("x".repeat(201))),
    ];
    for body in cases {
        let r = h.notify_with(&a.token, body.clone()).await;
        assert_eq!(
            (r.status, r.code()),
            (422, "VALIDATION_FAILED"),
            "{body}: {}",
            r.body
        );
    }
    let extra = with(&|b| b["surprise"] = json!(1));
    assert!(matches!(
        h.notify_with(&a.token, extra).await.status,
        400 | 422
    ));
    assert!(h.db.ledger_by_outbox("OUT1").await.unwrap().is_none());
    assert!(h.fake().sent().is_empty());
}

#[tokio::test]
async fn an_outbox_id_of_another_branch_conflicts() {
    let h = Harness::new().await;
    let a = h.paid_cafe(None).await;
    let b = h.cafe(PHONE_B, "Other Arena", "198.51.100.5").await;
    h.upgrade(&b.branch, Tier::Basic, T0 + 30 * DAY).await;
    assert_eq!(h.notify(&a.token, "OUT1", "receipt").await.status, 200);
    let r = h.notify(&b.token, "OUT1", "receipt").await;
    assert_eq!((r.status, r.code()), (409, "IDEMPOTENCY_CONFLICT"));
    assert_eq!(h.fake().sent().len(), 1);
}

#[tokio::test]
async fn the_five_kinds_have_approved_templates_and_a_change_is_a_new_version() {
    let h = Harness::new().await;
    let a = h.paid_cafe(None).await;
    for kind in [
        "credit_reminder",
        "low_balance",
        "session_ending",
        "console_5min",
        "receipt",
    ] {
        let t = h.db.current_template(kind).await.unwrap().expect(kind);
        assert_eq!(t.version, 1, "{kind}");
        assert!(
            t.approved_at.is_some() && !t.whatsapp_template_id.is_empty(),
            "{kind}"
        );
        assert!(
            !t.sms_text.is_empty() && !t.sms_dlt_template_id.is_empty(),
            "{kind}"
        );
        for p in &t.param_names {
            assert!(t.sms_text.contains(&format!("{{{p}}}")), "{kind} {p}");
        }
    }
    assert!(h.db.current_template("newsletter").await.unwrap().is_none());

    assert_eq!(h.notify(&a.token, "OUT1", "receipt").await.status, 200);
    let v1 = h.db.current_template("receipt").await.unwrap().unwrap();
    let v2 =
        h.db.add_template_version(&next_version(&v1, 2, true))
            .await
            .unwrap();
    assert_eq!(v2, 2);
    // An unapproved draft is not current.
    let draft =
        h.db.add_template_version(&next_version(&v1, 3, false))
            .await
            .unwrap();
    assert_eq!(draft, 3);

    assert_eq!(h.notify(&a.token, "OUT2", "receipt").await.status, 200);
    let sent = h.fake().sent();
    assert_eq!(sent[0].template_id, "arena360_receipt_v1");
    assert_eq!(sent[1].template_id, "arena360_receipt_next");
    // The earlier send keeps its version.
    let first = h.db.ledger_by_outbox("OUT1").await.unwrap().unwrap();
    assert_eq!(first.template_version, 1);
    let second = h.db.ledger_by_outbox("OUT2").await.unwrap().unwrap();
    assert_eq!(second.template_version, 2);
}

#[tokio::test]
async fn a_queued_row_is_sent_with_the_template_version_it_was_created_with() {
    let h = Harness::new().await;
    let a = h.paid_cafe(None).await;
    h.fake()
        .set_behaviour(Channel::Whatsapp, Behaviour::Transient);
    assert_eq!(
        h.notify(&a.token, "OUT1", "receipt").await.body["state"],
        "queued"
    );
    let v1 = h.db.current_template("receipt").await.unwrap().unwrap();
    h.db.add_template_version(&next_version(&v1, 2, true))
        .await
        .unwrap();
    h.fake()
        .set_behaviour(Channel::Whatsapp, Behaviour::Deliver);
    assert_eq!(
        h.notify(&a.token, "OUT1", "receipt").await.body["state"],
        "sent"
    );
    assert_eq!(h.fake().sent()[0].template_id, "arena360_receipt_v1");
}

#[tokio::test]
async fn without_an_adapter_the_cloud_says_it_is_unavailable() {
    let h = Harness::new().await;
    let a = h.paid_cafe(None).await;
    // A state built the same way but with no adapter set.
    let bare = Arc::new(AuthState::new(
        Arc::clone(&h.db),
        Messenger::Fake(FakeSms::new(Arc::clone(&h.st.clock))),
        Arc::clone(&h.st.clock),
        &config(),
    ));
    let app = router(&bare);
    let req = Request::builder()
        .method("POST")
        .uri("/v1/notify")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", a.token))
        .body(Body::from(request("OUT1", "receipt").to_string()))
        .unwrap();
    let r = reply(app.oneshot(req).await.unwrap()).await;
    assert_eq!(r.status, 503, "{}", r.body);
    assert!(h.db.ledger_by_outbox("OUT1").await.unwrap().is_none());
}

#[tokio::test]
async fn a_retry_for_another_recipient_or_kind_conflicts_and_sends_nothing() {
    let h = Harness::new().await;
    let a = h.paid_cafe(None).await;
    h.fake()
        .set_behaviour(Channel::Whatsapp, Behaviour::Transient);
    assert_eq!(
        h.notify(&a.token, "OUT1", "receipt").await.body["state"],
        "queued"
    );
    h.fake()
        .set_behaviour(Channel::Whatsapp, Behaviour::Deliver);

    let mut other = request("OUT1", "receipt");
    other["recipient"] = json!("+919800000000");
    let r = h.notify_with(&a.token, other).await;
    assert_eq!((r.status, r.code()), (409, "IDEMPOTENCY_CONFLICT"));
    let r = h.notify(&a.token, "OUT1", "low_balance").await;
    assert_eq!((r.status, r.code()), (409, "IDEMPOTENCY_CONFLICT"));
    assert!(h.fake().sent().is_empty());

    let r = h.notify(&a.token, "OUT1", "receipt").await;
    assert_eq!(r.body["state"], "sent");
}
