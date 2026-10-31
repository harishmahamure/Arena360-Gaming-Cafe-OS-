//! The price book (Story 10.1): the seed book, `GET /v1/pricing`, the audited ops path and the
//! billing provider seam.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::sync::Arc;

use arena_cloud::billing::{BillingProvider, FakeRazorpay};
use arena_cloud::config::{
    self, AuthConfig, Command, PriceBookAction, PriceBookConfig, Role, SmsProviderConfig,
};
use arena_cloud::crypto::MfaKey;
use arena_cloud::health::Reachability;
use arena_cloud::price_book::{self, BookError, TASK_CREATE_PLANS};
use arena_cloud::{Clock, app_with_auth, auth_state, ops};
use arena_proto::{Band, BillingCycle, PriceItem, Pricing};
use arena_store::control::{ControlDb, LATEST_CONTROL_VERSION, PriceBookStatus};
use axum::body::{Body, to_bytes};
use axum::http::Request;
use serde_json::Value;
use tokio::sync::watch;
use tower::ServiceExt;

const T0: i64 = 1_800_000_000_000;

struct Harness {
    app: axum::Router,
    db: Arc<ControlDb>,
    dir: tempfile::TempDir,
}

impl Harness {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = ControlDb::open_local(&dir.path().join("gz_control.db"))
            .await
            .unwrap();
        db.migrate(T0).await.unwrap();
        let db = Arc::new(db);
        let clock: Clock = Arc::new(|| T0);
        let cfg = AuthConfig {
            pepper: b"test-pepper-0123456789abcdef0123456789".to_vec(),
            web_origins: vec!["https://start.arena360.cloud".to_owned()],
            sms: SmsProviderConfig::Fake,
            sms_global_per_min: 10_000,
            mfa_key: MfaKey::from_bytes([0xa5; 32]),
            licence: arena_cloud::licence::LicenceSigner::dev(),
            billing_enabled: false,
            escrow_key: None,
        };
        let st = auth_state(Arc::clone(&db), &cfg, clock).unwrap();
        let (_tx, rx) = watch::channel(Reachability {
            reachable: true,
            checked_at: None,
        });
        let app = app_with_auth(Role::All, true, rx, LATEST_CONTROL_VERSION, Some(st));
        Self { app, db, dir }
    }

    fn sql(&self, sql: &str) -> Result<(), rusqlite::Error> {
        rusqlite::Connection::open(self.dir.path().join("gz_control.db"))
            .unwrap()
            .execute_batch(sql)
    }

    async fn get(&self, path: &str) -> (u16, Value) {
        let res = self
            .app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = res.status().as_u16();
        let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
}

fn fake() -> BillingProvider {
    BillingProvider::Fake(FakeRazorpay::new())
}

fn minor(p: &Pricing, item: PriceItem, cycle: BillingCycle) -> i64 {
    p.lines
        .iter()
        .find(|l| l.item == item && l.cycle == cycle)
        .unwrap()
        .amount_minor
}

#[tokio::test]
async fn the_seed_book_holds_the_published_prices_for_every_band() {
    let h = Harness::new().await;
    let live = h.db.live_price_book().await.unwrap().unwrap();
    assert_eq!((live.id.as_str(), live.version), ("pb_v1", 1));
    let items = h.db.price_book_items("pb_v1").await.unwrap();
    assert_eq!(items.len(), 4 * 5 * 2);
    // Monthly amounts in minor units: basic, premium, +20 devices, tunnel, owner login.
    let want: [(Band, [i64; 5]); 4] = [
        (Band::In, [20_000, 199_900, 5_000, 20_000, 5_000]),
        (Band::My, [2_300, 22_900, 600, 2_300, 600]),
        (Band::Sg, [800, 7_900, 200, 800, 200]),
        (Band::Us, [600, 5_900, 150, 600, 150]),
    ];
    for (band, amounts) in want {
        for (item, amount) in PriceItem::ALL.into_iter().zip(amounts) {
            let row = |cycle| {
                items
                    .iter()
                    .find(|i| i.band == band && i.item == item && i.cycle == cycle)
                    .unwrap()
            };
            let monthly = row(BillingCycle::Monthly);
            let yearly = row(BillingCycle::Yearly);
            assert_eq!(monthly.amount_minor, amount, "{band:?} {item:?}");
            assert_eq!(yearly.amount_minor, amount * 10, "{band:?} {item:?} yearly");
            assert_eq!(monthly.currency, band.currency());
            assert_eq!(monthly.gst_applicable, band == Band::In);
            assert_eq!(yearly.gst_applicable, band == Band::In);
            assert_eq!(monthly.razorpay_plan_id, None);
        }
    }
}

#[tokio::test]
async fn pricing_serves_an_enabled_band_and_refuses_the_others() {
    let h = Harness::new().await;
    let (status, body) = h.get("/v1/pricing?band=IN").await;
    assert_eq!(status, 200);
    let p: Pricing = serde_json::from_value(body).unwrap();
    assert_eq!(
        (p.band, p.currency.as_str(), p.price_book_version),
        (Band::In, "INR", 1)
    );
    assert_eq!(p.lines.len(), 10);
    assert_eq!(minor(&p, PriceItem::Basic, BillingCycle::Monthly), 20_000);
    assert_eq!(
        minor(&p, PriceItem::Premium, BillingCycle::Yearly),
        1_999_000
    );
    // No band means the home band.
    assert_eq!(h.get("/v1/pricing").await.0, 200);

    let (status, body) = h.get("/v1/pricing?band=MY").await;
    assert_eq!(status, 409);
    assert_eq!(body["code"], "BAND_NOT_AVAILABLE");
    assert_eq!(body["field"], "band");
    let (status, body) = h.get("/v1/pricing?band=XX").await;
    assert_eq!((status, body["code"].as_str()), (400, Some("BAD_REQUEST")));
    assert_eq!(h.get("/v1/pricing?band=in").await.0, 400);

    // An operator switches the band on, and it is served in its own currency.
    price_book::set_band(&h.db, Band::My, true, "harish", "coverage confirmed", T0)
        .await
        .unwrap();
    let (status, body) = h.get("/v1/pricing?band=MY").await;
    assert_eq!(status, 200);
    let p: Pricing = serde_json::from_value(body).unwrap();
    assert_eq!(p.currency, "MYR");
    assert_eq!(minor(&p, PriceItem::Basic, BillingCycle::Monthly), 2_300);
    assert!(p.lines.iter().all(|l| !l.gst_applicable));
    let audit = h.db.ops_audit_for("MY").await.unwrap();
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].action, "billing_band");
}

#[tokio::test]
async fn a_draft_is_copied_edited_and_published_with_a_plan_per_row() {
    let h = Harness::new().await;
    let provider = fake();
    let (draft, version) = price_book::open_draft(&h.db, "harish", "july prices", T0)
        .await
        .unwrap();
    assert_eq!(version, 2);
    // A second draft is refused while one is open.
    assert!(matches!(
        price_book::open_draft(&h.db, "harish", "again", T0).await,
        Err(BookError::DraftExists(id)) if id == draft
    ));
    price_book::set_amount(
        &h.db,
        &draft,
        Band::In,
        PriceItem::Basic,
        25_000,
        "harish",
        "raise",
        T0,
    )
    .await
    .unwrap();
    let rows = h.db.price_book_items(&draft).await.unwrap();
    assert_eq!(rows.len(), 40);
    let monthly = rows
        .iter()
        .find(|i| {
            i.band == Band::In && i.item == PriceItem::Basic && i.cycle == BillingCycle::Monthly
        })
        .unwrap();
    let yearly = rows
        .iter()
        .find(|i| {
            i.band == Band::In && i.item == PriceItem::Basic && i.cycle == BillingCycle::Yearly
        })
        .unwrap();
    assert_eq!(
        (monthly.amount_minor, yearly.amount_minor),
        (25_000, 250_000)
    );
    // The live book is untouched until the draft is published.
    let (_, body) = h.get("/v1/pricing?band=IN").await;
    assert_eq!(body["price_book_version"], 1);

    let summary = price_book::publish(&h.db, &provider, &draft, "harish", "go live", T0 + 1)
        .await
        .unwrap();
    assert_eq!(
        (
            summary.plans_created,
            summary.plans_deferred,
            summary.synced_only
        ),
        (40, 0, false)
    );
    let books = h.db.price_books().await.unwrap();
    let status = |id: &str| books.iter().find(|b| b.id == id).unwrap().status;
    assert_eq!(status(&draft), PriceBookStatus::Live);
    assert_eq!(status("pb_v1"), PriceBookStatus::Retired);
    let rows = h.db.price_book_items(&draft).await.unwrap();
    let ids: Vec<_> = rows
        .iter()
        .map(|r| r.razorpay_plan_id.clone().unwrap())
        .collect();
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        40
    );
    let BillingProvider::Fake(f) = &provider else {
        unreachable!()
    };
    let made = f.created();
    assert_eq!(made.len(), 40);
    let basic_plan = made
        .iter()
        .find(|p| {
            p.band == Band::In && p.item == PriceItem::Basic && p.cycle == BillingCycle::Monthly
        })
        .unwrap();
    assert_eq!(
        (basic_plan.amount_minor, basic_plan.currency.as_str()),
        (25_000, "INR")
    );
    let (_, body) = h.get("/v1/pricing?band=IN").await;
    assert_eq!(body["price_book_version"], 2);
    let audit = h.db.ops_audit_for(&draft).await.unwrap();
    let actions: Vec<_> = audit.iter().map(|a| a.action.as_str()).collect();
    assert_eq!(
        actions,
        ["price_book_draft", "price_book_set", "price_book_publish"]
    );
    assert_eq!(audit[2].reason, "go live");
}

#[tokio::test]
async fn a_provider_error_leaves_the_draft_unpublished_and_nothing_half_written() {
    let h = Harness::new().await;
    let f = FakeRazorpay::new();
    f.fail_after(7);
    let provider = BillingProvider::Fake(f);
    let (draft, _) = price_book::open_draft(&h.db, "harish", "next", T0)
        .await
        .unwrap();
    let err = price_book::publish(&h.db, &provider, &draft, "harish", "go", T0 + 1)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, BookError::Provider(e) if e.0 == "FAKE_PLAN_FAILED"),
        "{err}"
    );
    let books = h.db.price_books().await.unwrap();
    assert_eq!(
        books.iter().find(|b| b.id == draft).unwrap().status,
        PriceBookStatus::Draft
    );
    assert_eq!(h.db.live_price_book().await.unwrap().unwrap().id, "pb_v1");
    let rows = h.db.price_book_items(&draft).await.unwrap();
    assert!(rows.iter().all(|r| r.razorpay_plan_id.is_none()));
    assert!(
        h.db.ops_audit_for(&draft)
            .await
            .unwrap()
            .iter()
            .all(|a| a.action != "price_book_publish")
    );

    // Run again once the provider is healthy: the same rows get the same plans and it publishes.
    let BillingProvider::Fake(f) = &provider else {
        unreachable!()
    };
    f.heal();
    price_book::publish(&h.db, &provider, &draft, "harish", "go", T0 + 2)
        .await
        .unwrap();
    assert_eq!(f.created().len(), 40);
    assert_eq!(h.db.live_price_book().await.unwrap().unwrap().id, draft);
}

#[tokio::test]
async fn a_published_book_is_immutable() {
    let h = Harness::new().await;
    let err = price_book::set_amount(
        &h.db,
        "pb_v1",
        Band::In,
        PriceItem::Basic,
        1,
        "harish",
        "x",
        T0,
    )
    .await
    .unwrap_err();
    assert_eq!(err, BookError::Immutable("pb_v1".to_owned()));
    assert!(err.to_string().starts_with("PRICE_BOOK_IMMUTABLE"));
    // Not even raw SQL can change a published amount, delete a row or add one.
    for sql in [
        "UPDATE price_book_items SET amount_minor = 1 WHERE price_book_id = 'pb_v1'",
        "DELETE FROM price_book_items WHERE price_book_id = 'pb_v1'",
        "INSERT INTO price_book_items VALUES ('pb_v1', 'IN', 'INR', 'basic', 'monthly', 9, 1, NULL)",
        "UPDATE price_books SET status = 'draft' WHERE id = 'pb_v1'",
        "UPDATE price_books SET version = 9 WHERE id = 'pb_v1'",
    ] {
        let e = h.sql(sql).unwrap_err().to_string();
        assert!(e.contains("PRICE_BOOK_IMMUTABLE"), "{sql}: {e}");
    }
    assert_eq!(
        h.db.price_book_items("pb_v1")
            .await
            .unwrap()
            .iter()
            .filter(|i| i.band == Band::In)
            .count(),
        10
    );
    // Absurd amounts are refused before the database sees them.
    let (draft, _) = price_book::open_draft(&h.db, "harish", "n", T0)
        .await
        .unwrap();
    // A draft missing a price cannot go live.
    h.sql(&format!(
        "DELETE FROM price_book_items WHERE price_book_id = '{draft}' AND band = 'US' AND item = 'basic' AND cycle = 'yearly'"
    ))
    .unwrap();
    assert_eq!(
        price_book::publish(&h.db, &fake(), &draft, "harish", "go", T0)
            .await
            .unwrap_err(),
        BookError::Incomplete { have: 39, want: 40 }
    );
    h.sql(&format!(
        "INSERT INTO price_book_items VALUES ('{draft}', 'US', 'USD', 'basic', 'yearly', 6000, 0, NULL)"
    ))
    .unwrap();
    for bad in [0, -5, price_book::MAX_MONTHLY_MINOR + 1] {
        assert_eq!(
            price_book::set_amount(&h.db, &draft, Band::In, PriceItem::Basic, bad, "h", "n", T0)
                .await
                .unwrap_err(),
            BookError::BadAmount(bad)
        );
    }
    // Once the draft is published it is immutable too.
    price_book::publish(&h.db, &fake(), &draft, "harish", "go", T0)
        .await
        .unwrap();
    assert_eq!(
        price_book::set_amount(
            &h.db,
            &draft,
            Band::In,
            PriceItem::Basic,
            30_000,
            "h",
            "n",
            T0
        )
        .await
        .unwrap_err(),
        BookError::Immutable(draft.clone())
    );
    // A retired book cannot be published again.
    assert_eq!(
        price_book::publish(&h.db, &fake(), "pb_v1", "h", "n", T0)
            .await
            .unwrap_err(),
        BookError::Retired("pb_v1".to_owned())
    );
}

#[tokio::test]
async fn the_seed_book_gets_its_plans_without_being_published_again() {
    let h = Harness::new().await;
    let provider = fake();
    let summary = price_book::publish(&h.db, &provider, "pb_v1", "harish", "mirror the seed", T0)
        .await
        .unwrap();
    assert!(summary.synced_only);
    assert_eq!(summary.plans_created, 40);
    assert_eq!(h.db.live_price_book().await.unwrap().unwrap().id, "pb_v1");
    assert_eq!(
        h.db.ops_audit_for("pb_v1")
            .await
            .unwrap()
            .last()
            .unwrap()
            .action,
        "price_book_sync"
    );
    assert_eq!(
        price_book::publish(&h.db, &provider, "pb_v1", "harish", "again", T0)
            .await
            .unwrap_err(),
        BookError::AlreadyPublished("pb_v1".to_owned())
    );
    assert_eq!(
        price_book::publish(&h.db, &provider, "nope", "harish", "x", T0)
            .await
            .unwrap_err(),
        BookError::UnknownBook("nope".to_owned())
    );
}

#[tokio::test]
async fn manual_mode_calls_nothing_and_leaves_an_ops_task() {
    let h = Harness::new().await;
    let (draft, _) = price_book::open_draft(&h.db, "harish", "n", T0)
        .await
        .unwrap();
    let summary = price_book::publish(&h.db, &BillingProvider::Manual, &draft, "harish", "go", T0)
        .await
        .unwrap();
    assert_eq!(
        (
            summary.plans_created,
            summary.plans_deferred,
            summary.provider
        ),
        (0, 40, "manual")
    );
    // The book is live without plan ids, and an operator has one task listing every plan.
    assert_eq!(h.db.live_price_book().await.unwrap().unwrap().id, draft);
    assert!(
        h.db.price_book_items(&draft)
            .await
            .unwrap()
            .iter()
            .all(|r| r.razorpay_plan_id.is_none())
    );
    let tasks = h.db.open_ops_tasks(TASK_CREATE_PLANS).await.unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].target, draft);
    let details: Value = serde_json::from_str(&tasks[0].details_json).unwrap();
    assert_eq!(details["plans"].as_array().unwrap().len(), 40);
    // Pricing is unaffected by the provider.
    assert_eq!(h.get("/v1/pricing?band=IN").await.0, 200);
    // When Razorpay is back, the same command gives the live book its plans.
    let healed = price_book::publish(&h.db, &fake(), &draft, "harish", "sync", T0 + 1)
        .await
        .unwrap();
    assert!(healed.synced_only);
    assert_eq!(healed.plans_created, 40);
}

fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let m: BTreeMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).into(), (*v).into()))
        .collect();
    move |k| m.get(k).map(OsString::from)
}

fn parse(args: &[&str]) -> Result<PriceBookConfig, String> {
    match config::parse(
        ["ops", "price-book"].iter().chain(args).map(OsString::from),
        env_of(&[]),
    )? {
        Command::PriceBook(c) => Ok(c),
        other => panic!("not a price-book command: {other:?}"),
    }
}

#[test]
fn the_price_book_commands_parse() {
    let c = parse(&["publish", "--id", "pb_x", "--reason", "go live"]).unwrap();
    assert_eq!(c.action, PriceBookAction::Publish { id: "pb_x".into() });
    assert_eq!(
        (c.operator, c.reason.as_str(), c.dev_data_dir),
        (None, "go live", None)
    );
    let c = parse(&[
        "set",
        "--id",
        "pb_x",
        "--band",
        "SG",
        "--item",
        "addon_tunnel",
        "--monthly-minor",
        "900",
        "--reason",
        "r",
        "--operator",
        "asha",
        "--dev",
    ])
    .unwrap();
    assert_eq!(
        c.action,
        PriceBookAction::Set {
            id: "pb_x".into(),
            band: Band::Sg,
            item: PriceItem::AddonTunnel,
            monthly_minor: 900
        }
    );
    assert_eq!(c.operator.as_deref(), Some("asha"));
    assert!(c.dev_data_dir.is_some());
    let c = parse(&[
        "band",
        "--band",
        "US",
        "--enabled",
        "false",
        "--reason",
        "r",
    ])
    .unwrap();
    assert_eq!(
        c.action,
        PriceBookAction::Band {
            band: Band::Us,
            enabled: false
        }
    );
    assert_eq!(parse(&["list"]).unwrap().action, PriceBookAction::List);
    for bad in [
        &["publish", "--reason", "r"][..],
        &["publish", "--id", "x"],
        &[
            "set",
            "--id",
            "x",
            "--band",
            "XX",
            "--item",
            "basic",
            "--monthly-minor",
            "1",
            "--reason",
            "r",
        ],
        &[
            "set",
            "--id",
            "x",
            "--band",
            "IN",
            "--item",
            "gold",
            "--monthly-minor",
            "1",
            "--reason",
            "r",
        ],
        &[
            "set",
            "--id",
            "x",
            "--band",
            "IN",
            "--item",
            "basic",
            "--monthly-minor",
            "1.5",
            "--reason",
            "r",
        ],
        &["band", "--band", "IN", "--reason", "r"],
        &["draft", "--reason", "r", "--data-dir", "d"],
        &["frobnicate"],
        &[],
    ] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
}

#[tokio::test]
async fn the_ops_command_drafts_sets_publishes_and_audits_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let run = |args: Vec<&'static str>, at: i64, env: Vec<(&'static str, &'static str)>| {
        let dir = dir.path().to_owned();
        async move {
            let mut c = parse(&args).unwrap();
            c.dev_data_dir = Some(dir);
            ops::price_book(&c, env_of(&env), at).await
        }
    };
    let out = run(vec!["list"], T0, vec![]).await.unwrap();
    assert!(
        out[0].contains("pb_v1") && out[0].contains("live"),
        "{out:?}"
    );
    // The operator comes from the environment when no flag names one.
    let out = run(
        vec!["draft", "--reason", "q4"],
        T0 + 1,
        vec![("ARENA_OPS_OPERATOR", "asha")],
    )
    .await
    .unwrap();
    let draft = out[0].split_whitespace().nth(4).unwrap().to_owned();
    assert!(draft.starts_with("pb_"), "{out:?}");
    let draft: &'static str = Box::leak(draft.into_boxed_str());
    run(
        vec![
            "set",
            "--id",
            draft,
            "--band",
            "IN",
            "--item",
            "premium",
            "--monthly-minor",
            "209900",
            "--reason",
            "q4",
        ],
        T0 + 2,
        vec![("ARENA_OPS_OPERATOR", "asha")],
    )
    .await
    .unwrap();
    let out = run(
        vec!["publish", "--id", draft, "--reason", "q4 go-live"],
        T0 + 3,
        vec![("ARENA_OPS_OPERATOR", "asha")],
    )
    .await
    .unwrap();
    assert!(
        out[0].contains("published") && out[0].contains("fake") && out[0].contains("40"),
        "{out:?}"
    );
    // The live book is now the draft; the old one cannot be edited through the command either.
    let err = run(
        vec![
            "set",
            "--id",
            "pb_v1",
            "--band",
            "IN",
            "--item",
            "basic",
            "--monthly-minor",
            "1",
            "--reason",
            "x",
        ],
        T0 + 4,
        vec![("ARENA_OPS_OPERATOR", "asha")],
    )
    .await
    .unwrap_err();
    assert!(err.to_string().starts_with("PRICE_BOOK_IMMUTABLE"), "{err}");
    let db = ControlDb::open_local(&dir.path().join(config::DEV_DB_FILE))
        .await
        .unwrap();
    let audit = db.ops_audit_for(draft).await.unwrap();
    assert_eq!(audit.len(), 3);
    assert!(audit.iter().all(|a| a.operator == "asha"));
    assert_eq!(db.live_price_book().await.unwrap().unwrap().version, 2);
}
