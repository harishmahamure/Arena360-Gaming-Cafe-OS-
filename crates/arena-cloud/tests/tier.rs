use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use arena_cloud::auth::AuthState;
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
    async fn new() -> Self {
        Self::with_billing(false).await
    }

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

    fn entitlements(&self) -> Vec<(String, String, String)> {
        let path = self._dir.path().join("gz_control.db");
        let c = rusqlite::Connection::open(path).unwrap();
        let mut st = c
            .prepare("SELECT branch_id, tier, source FROM entitlements ORDER BY branch_id")
            .unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn tier_step(&self, branch: &str) -> (String, Option<i64>) {
        let path = self._dir.path().join("gz_control.db");
        rusqlite::Connection::open(path)
            .unwrap()
            .query_row(
                "SELECT state, completed_at FROM onboarding_progress \
                 WHERE branch_id = ? AND step = 'tier'",
                [branch],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }
}

fn options(body: &Value) -> Vec<(String, bool, Option<String>, u64)> {
    body["tiers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                t["key"].as_str().unwrap().to_owned(),
                t["available"].as_bool().unwrap(),
                t["reason"].as_str().map(str::to_owned),
                t["limits"]["devices"].as_u64().unwrap(),
            )
        })
        .collect()
}

fn opt(
    key: &str,
    available: bool,
    reason: Option<&str>,
    devices: u64,
) -> (String, bool, Option<String>, u64) {
    (
        key.to_owned(),
        available,
        reason.map(str::to_owned),
        devices,
    )
}

const SECOND_BRANCH: &str = "INSERT INTO branches (id, tenant_id, name, city, country, currency, \
     timezone, gst_registered, gstin, branch_code, code_locked_at, created_at, updated_at) \
     SELECT 'b-second', tenant_id, 'Second', 'Pune', 'IN', 'INR', 'Asia/Kolkata', 0, NULL, 'SEC', \
     NULL, 1, 1 FROM branches LIMIT 1; \
     INSERT INTO onboarding_progress (branch_id, step, state, completed_at) VALUES \
     ('b-second', 'cafe', 'done', 1), ('b-second', 'tier', 'pending', NULL);";

#[tokio::test]
async fn the_tier_list_offers_free_and_holds_paid_plans_behind_billing() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    h.cafe_done(&a).await;
    let r = h.call("GET", "/v1/wizard/tier", None, &a).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["billing_enabled"], false);
    assert!(r.body.get("current").is_none());
    assert_eq!(
        options(&r.body),
        [
            opt("free", true, None, 20),
            opt("basic", false, Some("billing_disabled"), 60),
            opt("premium", false, Some("billing_disabled"), 200),
        ]
    );
}

#[tokio::test]
async fn with_billing_on_the_paid_plans_list_as_available_but_still_write_nothing() {
    let h = Harness::with_billing(true).await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    h.cafe_done(&a).await;
    let r = h.call("GET", "/v1/wizard/tier", None, &a).await;
    assert_eq!(r.body["billing_enabled"], true);
    assert_eq!(
        options(&r.body),
        [
            opt("free", true, None, 20),
            opt("basic", true, None, 60),
            opt("premium", true, None, 200),
        ]
    );
    let r = h
        .call("PUT", "/v1/wizard/tier", Some(json!({"tier": "basic"})), &a)
        .await;
    assert_eq!((r.status, r.code()), (409, "BILLING_DISABLED"));
    assert!(h.entitlements().is_empty());
}

#[tokio::test]
async fn picking_free_writes_the_entitlement_and_completes_the_step_once() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let branch = h.cafe_done(&a).await;
    assert_eq!(h.tier_step(&branch), ("pending".to_owned(), None));
    let r = h
        .call("PUT", "/v1/wizard/tier", Some(json!({"tier": "free"})), &a)
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["current"], "install_hub");
    assert_eq!(r.body["steps"][1]["state"], "done");
    assert_eq!(
        h.entitlements(),
        [(branch.clone(), "free".to_owned(), "free".to_owned())]
    );
    assert_eq!(h.tier_step(&branch), ("done".to_owned(), Some(T0)));
    let again = h
        .call("PUT", "/v1/wizard/tier", Some(json!({"tier": "free"})), &a)
        .await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(again.body, r.body);
    assert_eq!(h.entitlements().len(), 1);
    let list = h.call("GET", "/v1/wizard/tier", None, &a).await;
    assert_eq!(list.body["current"], "free");
    assert_eq!(options(&list.body)[0], opt("free", true, None, 20));
}

#[tokio::test]
async fn a_paid_pick_while_billing_is_off_writes_nothing() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let branch = h.cafe_done(&a).await;
    for tier in ["basic", "premium"] {
        let r = h
            .call("PUT", "/v1/wizard/tier", Some(json!({"tier": tier})), &a)
            .await;
        assert_eq!(
            (r.status, r.code(), r.field()),
            (409, "BILLING_DISABLED", "tier"),
            "{tier}"
        );
    }
    assert!(h.entitlements().is_empty());
    assert_eq!(h.tier_step(&branch), ("pending".to_owned(), None));
    let r = h
        .call("PUT", "/v1/wizard/tier", Some(json!({"tier": "gold"})), &a)
        .await;
    assert_eq!((r.status, r.code()), (400, "BAD_REQUEST"));
}

#[tokio::test]
async fn a_second_free_branch_in_the_tenant_is_refused() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let first = h.cafe_done(&a).await;
    let r = h
        .call("PUT", "/v1/wizard/tier", Some(json!({"tier": "free"})), &a)
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    h.sql(SECOND_BRANCH);
    let list = h
        .call("GET", "/v1/wizard/tier?branch_id=b-second", None, &a)
        .await;
    assert_eq!(list.status, 200, "{}", list.body);
    assert_eq!(
        options(&list.body)[0],
        opt("free", false, Some("one_free_branch"), 20)
    );
    let r = h
        .call(
            "PUT",
            "/v1/wizard/tier",
            Some(json!({"tier": "free", "branch_id": "b-second"})),
            &a,
        )
        .await;
    assert_eq!((r.status, r.code()), (409, "ONE_FREE_BRANCH"));
    assert_eq!(
        h.entitlements(),
        [(first, "free".to_owned(), "free".to_owned())]
    );
    assert_eq!(h.tier_step("b-second"), ("pending".to_owned(), None));
}

#[tokio::test]
async fn a_comp_branch_is_not_downgraded_by_picking_free() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let branch = h.cafe_done(&a).await;
    h.sql(&format!(
        "INSERT INTO entitlements (branch_id, tenant_id, tier, source, issued_at, until, \
         updated_at) SELECT id, tenant_id, 'premium', 'comp', 1, {}, 1 FROM branches \
         WHERE id = '{branch}'",
        T0 + 86_400_000
    ));
    let list = h.call("GET", "/v1/wizard/tier", None, &a).await;
    assert_eq!(list.body["current"], "premium");
    let r = h
        .call("PUT", "/v1/wizard/tier", Some(json!({"tier": "free"})), &a)
        .await;
    assert_eq!((r.status, r.code()), (409, "ALREADY_SET"));
    assert_eq!(
        h.entitlements(),
        [(branch, "premium".to_owned(), "comp".to_owned())]
    );
}

#[tokio::test]
async fn before_the_cafe_step_there_is_no_branch_to_choose_for() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let r = h.call("GET", "/v1/wizard/tier", None, &a).await;
    assert_eq!((r.status, r.code()), (403, "FORBIDDEN"));
    let r = h
        .call("PUT", "/v1/wizard/tier", Some(json!({"tier": "free"})), &a)
        .await;
    assert_eq!((r.status, r.code()), (403, "FORBIDDEN"));
    assert!(h.entitlements().is_empty());
}

#[tokio::test]
async fn the_tier_step_needs_a_finished_account_setup() {
    let h = Harness::new().await;
    let a = h.sign_up(PHONE_A, "Pixel Arena Gaming").await;
    let r = h.call("GET", "/v1/wizard/tier", None, &a).await;
    assert_eq!((r.status, r.code()), (403, "SETUP_INCOMPLETE"));
    let r = h
        .call("PUT", "/v1/wizard/tier", Some(json!({"tier": "free"})), "")
        .await;
    assert_eq!((r.status, r.code()), (401, "AUTH_FAILED"));
}

#[tokio::test]
async fn another_tenants_branch_is_forbidden_and_untouched() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let b = h.owner(PHONE_B, "Other Den").await;
    let a_branch = h.cafe_done(&a).await;
    h.cafe_done(&b).await;
    for target in [a_branch.as_str(), "01JNOSUCHBRANCH0000000000000"] {
        let r = h
            .call(
                "GET",
                &format!("/v1/wizard/tier?branch_id={target}"),
                None,
                &b,
            )
            .await;
        assert_eq!((r.status, r.code()), (403, "FORBIDDEN"), "{target}");
        let r = h
            .call(
                "PUT",
                "/v1/wizard/tier",
                Some(json!({"tier": "free", "branch_id": target})),
                &b,
            )
            .await;
        assert_eq!((r.status, r.code()), (403, "FORBIDDEN"), "{target}");
    }
    assert!(h.entitlements().is_empty());
    assert_eq!(h.tier_step(&a_branch), ("pending".to_owned(), None));
}

#[tokio::test]
async fn after_free_the_wizard_moves_on_and_the_branch_licence_verifies_with_the_dev_key() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let branch_id = h.cafe_done(&a).await;
    let r = h
        .call("PUT", "/v1/wizard/tier", Some(json!({"tier": "free"})), &a)
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let w = h.call("GET", "/v1/wizard", None, &a).await;
    assert_eq!(w.body["current"], "install_hub");
    let branch = h.db.branch(&branch_id).await.unwrap().unwrap();
    let e = h.db.entitlement(&branch_id).await.unwrap();
    let token = arena_cloud::licence::sign_branch(&h.st.licence, &branch, e.as_ref(), T0).unwrap();
    let claims =
        arena_proto::licence::verify(&token, arena_proto::licence::DEV_LICENCE_VERIFY_KEYS, T0)
            .unwrap();
    assert_eq!(claims.branch_id, branch_id);
    assert_eq!(claims.tenant_id, branch.tenant_id);
    assert_eq!(claims.tier, arena_proto::Tier::Free);
    assert_eq!(claims.limits.devices, 20);
}

fn comp(branch: &str, until: i64) -> String {
    format!(
        "INSERT INTO entitlements (branch_id, tenant_id, tier, source, issued_at, until, \
         updated_at) SELECT id, tenant_id, 'premium', 'comp', 1, {until}, 1 FROM branches \
         WHERE id = '{branch}'"
    )
}

#[tokio::test]
async fn a_branch_whose_comp_has_lapsed_can_pick_free_and_moves_on() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let branch = h.cafe_done(&a).await;
    h.sql(&comp(&branch, T0 - 1));
    let list = h.call("GET", "/v1/wizard/tier", None, &a).await;
    assert_eq!(list.body["current"], "free");
    assert_eq!(options(&list.body)[0], opt("free", true, None, 20));
    let r = h
        .call("PUT", "/v1/wizard/tier", Some(json!({"tier": "free"})), &a)
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["current"], "install_hub");
    assert_eq!(
        h.entitlements(),
        [(branch.clone(), "free".to_owned(), "free".to_owned())]
    );
    assert_eq!(h.tier_step(&branch), ("done".to_owned(), Some(T0)));
}

#[tokio::test]
async fn a_sibling_whose_comp_has_lapsed_counts_as_the_free_branch() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let first = h.cafe_done(&a).await;
    h.sql(&comp(&first, T0 - 1));
    h.sql(SECOND_BRANCH);
    let list = h
        .call("GET", "/v1/wizard/tier?branch_id=b-second", None, &a)
        .await;
    assert_eq!(
        options(&list.body)[0],
        opt("free", false, Some("one_free_branch"), 20)
    );
    let r = h
        .call(
            "PUT",
            "/v1/wizard/tier",
            Some(json!({"tier": "free", "branch_id": "b-second"})),
            &a,
        )
        .await;
    assert_eq!((r.status, r.code()), (409, "ONE_FREE_BRANCH"));
    assert_eq!(h.entitlements().len(), 1);
    assert_eq!(h.tier_step("b-second"), ("pending".to_owned(), None));
}

#[tokio::test]
async fn re_picking_free_marks_a_step_left_pending_done() {
    let h = Harness::new().await;
    let a = h.owner(PHONE_A, "Pixel Arena Gaming").await;
    let branch = h.cafe_done(&a).await;
    let r = h
        .call("PUT", "/v1/wizard/tier", Some(json!({"tier": "free"})), &a)
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    h.sql(&format!(
        "UPDATE onboarding_progress SET state = 'pending', completed_at = NULL \
         WHERE branch_id = '{branch}' AND step = 'tier'"
    ));
    let r = h
        .call("PUT", "/v1/wizard/tier", Some(json!({"tier": "free"})), &a)
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["current"], "install_hub");
    assert_eq!(h.tier_step(&branch), ("done".to_owned(), Some(T0)));
}
