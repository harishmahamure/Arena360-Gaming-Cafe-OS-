#![cfg(feature = "control")]

#[path = "support/sqld.rs"]
mod sqld;

use arena_proto::{EntitlementSource, StepState, Tier, WizardStep};
use arena_store::control::{
    BranchProfile, CompGrant, ControlDb, FreeTierOutcome, GrantOutcome, NewOtpChallenge, NewOwner,
    NewOwnerSession, NewTenantBranch, SignupCompletion, SignupOutcome,
};

async fn owner(db: &ControlDb, id: &str, phone: &str) {
    let o = NewOwner {
        name: "Asha Rao".into(),
        phone_e164: phone.into(),
        email: format!("{id}@example.in"),
        cafe_name: "Pixel Arena Gaming".into(),
        city: "Pune".into(),
    };
    let challenge = format!("c-{id}");
    db.insert_otp_challenge(&NewOtpChallenge {
        id: challenge.clone(),
        phone_e164: phone.into(),
        code_hash: "h".into(),
        payload: o.clone(),
        created_at: 1_000,
        expires_at: 301_000,
    })
    .await
    .unwrap();
    let out = db
        .complete_signup(&SignupCompletion {
            challenge_id: &challenge,
            code_hash: "h",
            max_attempts: 5,
            now: 2_000,
            new_owner_id: id,
            owner: &o,
            session: &NewOwnerSession {
                id: format!("s-{id}"),
                owner_id: String::new(),
                family_id: format!("f-{id}"),
                access_hash: format!("a-{id}"),
                access_expires_at: 900_000,
                refresh_hash: format!("r-{id}"),
                refresh_expires_at: 9_000_000,
                created_at: 2_000,
                device_id: None,
            },
        })
        .await
        .unwrap();
    assert!(matches!(out, SignupOutcome::Session(_)));
}

fn profile() -> BranchProfile {
    BranchProfile {
        name: "Pixel Arena Gaming".into(),
        city: "Pune".into(),
        country: "IN".into(),
        currency: "INR".into(),
        timezone: "Asia/Kolkata".into(),
        gst_registered: false,
        gstin: None,
    }
}

async fn tier_state(db: &ControlDb, branch: &str) -> (StepState, Option<i64>) {
    let p = db.onboarding_progress(branch).await.unwrap();
    let row = p.iter().find(|p| p.step == WizardStep::Tier).unwrap();
    (row.state, row.completed_at)
}

async fn exercise(db: &ControlDb, second_branch: impl AsyncFnOnce(), paid: impl AsyncFnOnce()) {
    owner(db, "o1", "+919876543210").await;
    let p = profile();
    assert!(
        db.create_tenant_with_branch(&NewTenantBranch {
            tenant_id: "t1",
            owner_id: "o1",
            tenant_name: "Pixel Arena Gaming",
            branch_id: "b1",
            branch_code: "PAG",
            profile: &p,
            now: 10_000,
        })
        .await
        .unwrap()
    );
    assert_eq!(db.entitlement("b1").await.unwrap(), None);
    assert_eq!(db.branch("b1").await.unwrap().unwrap().tenant_id, "t1");
    assert_eq!(db.branch("nope").await.unwrap(), None);

    assert_eq!(
        db.choose_free_tier("t2", "b1", 20_000).await.unwrap(),
        FreeTierOutcome::Missing
    );
    assert_eq!(tier_state(db, "b1").await, (StepState::Pending, None));

    assert_eq!(
        db.choose_free_tier("t1", "b1", 20_000).await.unwrap(),
        FreeTierOutcome::Written
    );
    let e = db.entitlement("b1").await.unwrap().unwrap();
    assert_eq!(
        (e.tier, e.source, e.addons.len(), e.issued_at, e.until),
        (Tier::Free, EntitlementSource::Free, 0, 20_000, None)
    );
    assert_eq!(tier_state(db, "b1").await, (StepState::Done, Some(20_000)));
    assert_eq!(
        db.choose_free_tier("t1", "b1", 21_000).await.unwrap(),
        FreeTierOutcome::AlreadyFree
    );
    assert_eq!(
        db.entitlement("b1").await.unwrap().unwrap().updated_at,
        20_000
    );

    second_branch().await;
    assert!(db.other_free_branch("t1", "b9", 30_000).await.unwrap());
    assert!(!db.other_free_branch("t1", "b1", 30_000).await.unwrap());
    assert_eq!(
        db.choose_free_tier("t1", "b9", 30_000).await.unwrap(),
        FreeTierOutcome::OtherFree
    );
    assert_eq!(db.entitlement("b9").await.unwrap(), None);

    let grant = CompGrant {
        branch_id: "b9",
        tier: Tier::Premium,
        until: 900_000,
        audit_id: "a1",
        operator: "harish",
        reason: "beta café",
        details_json: r#"{"tier":"premium"}"#,
        now: 40_000,
    };
    assert_eq!(
        db.grant_comp(&grant).await.unwrap(),
        GrantOutcome::Granted { previous: None }
    );
    let e = db.entitlement("b9").await.unwrap().unwrap();
    assert_eq!(
        (e.tier, e.source, e.until),
        (Tier::Premium, EntitlementSource::Comp, Some(900_000))
    );
    assert_eq!(e.tier_at(900_000), Tier::Premium);
    assert_eq!(e.tier_at(900_001), Tier::Free);
    assert_eq!(tier_state(db, "b9").await, (StepState::Done, Some(40_000)));
    assert!(matches!(
        db.choose_free_tier("t1", "b9", 41_000).await.unwrap(),
        FreeTierOutcome::OtherPlan(_)
    ));

    let GrantOutcome::Granted {
        previous: Some(previous),
    } = db
        .grant_comp(&CompGrant {
            tier: Tier::Basic,
            audit_id: "a2",
            now: 50_000,
            ..grant.clone()
        })
        .await
        .unwrap()
    else {
        panic!("not granted over the comp")
    };
    assert_eq!(
        (previous.tier, previous.source),
        (Tier::Premium, EntitlementSource::Comp)
    );
    let e = db.entitlement("b9").await.unwrap().unwrap();
    assert_eq!(
        (e.tier, e.issued_at, e.updated_at),
        (Tier::Basic, 50_000, 50_000)
    );
    assert_eq!(tier_state(db, "b9").await, (StepState::Done, Some(40_000)));

    assert_eq!(
        db.grant_comp(&CompGrant {
            branch_id: "ghost",
            audit_id: "a3",
            ..grant.clone()
        })
        .await
        .unwrap(),
        GrantOutcome::UnknownBranch
    );
    assert!(db.ops_audit_for("ghost").await.unwrap().is_empty());
    let audit = db.ops_audit_for("b9").await.unwrap();
    assert_eq!(audit.len(), 2);
    assert_eq!(
        (
            audit[0].operator.as_str(),
            audit[0].action.as_str(),
            audit[0].reason.as_str(),
            audit[0].at
        ),
        ("harish", "grant_comp", "beta café", 40_000)
    );
    let details =
        |i: usize| -> serde_json::Value { serde_json::from_str(&audit[i].details_json).unwrap() };
    assert_eq!(details(0)["tier"], "premium");
    assert_eq!(details(0)["previous"], serde_json::Value::Null);
    assert_eq!(details(1)["previous"]["tier"], "premium");
    assert_eq!(details(1)["previous"]["source"], "comp");
    assert_eq!(details(1)["previous"]["until"], 900_000);

    assert!(db.other_free_branch("t1", "b1", 900_001).await.unwrap());
    assert_eq!(
        db.choose_free_tier("t1", "b9", 900_001).await.unwrap(),
        FreeTierOutcome::OtherFree
    );
    paid().await;
    let refused = db
        .grant_comp(&CompGrant {
            audit_id: "a4",
            ..grant.clone()
        })
        .await
        .unwrap();
    assert!(matches!(refused, GrantOutcome::Paid(ref e) if e.tier == Tier::Basic));
    assert_eq!(db.ops_audit_for("b9").await.unwrap().len(), 2);
    assert_eq!(
        db.entitlement("b9").await.unwrap().unwrap().source,
        EntitlementSource::Paid
    );
}

async fn race(db: std::sync::Arc<ControlDb>) {
    let (a, b) = tokio::join!(
        db.choose_free_tier("t1", "b1", 10),
        db.choose_free_tier("t1", "b9", 10)
    );
    let outcomes = [a.unwrap(), b.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|o| **o == FreeTierOutcome::Written)
            .count(),
        1,
        "{outcomes:?}"
    );
    assert!(
        outcomes.contains(&FreeTierOutcome::OtherFree),
        "{outcomes:?}"
    );
}

const SECOND: &str = "INSERT INTO branches (id, tenant_id, name, city, country, currency, \
     timezone, gst_registered, gstin, branch_code, code_locked_at, created_at, updated_at) \
     VALUES ('b9', 't1', 'Second', 'Pune', 'IN', 'INR', 'Asia/Kolkata', 0, NULL, 'SEC', NULL, \
     25000, 25000)";

const PAID: &str = "UPDATE entitlements SET source = 'paid', until = NULL WHERE branch_id = 'b9'";

const SECOND_PROGRESS: &str = "INSERT INTO onboarding_progress (branch_id, step, state, \
     completed_at) VALUES ('b9', 'cafe', 'done', 25000), ('b9', 'tier', 'pending', NULL)";

#[tokio::test(flavor = "multi_thread")]
async fn entitlement_queries_on_the_local_backend() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gz_control.db");
    let db = ControlDb::open_local(&path).await.unwrap();
    db.migrate(1).await.unwrap();
    exercise(
        &db,
        async || {
            let c = rusqlite::Connection::open(&path).unwrap();
            c.execute(SECOND, []).unwrap();
            c.execute(SECOND_PROGRESS, []).unwrap();
        },
        async || {
            rusqlite::Connection::open(&path)
                .unwrap()
                .execute(PAID, [])
                .unwrap();
        },
    )
    .await;
}

async fn two_branches(db: &ControlDb) {
    owner(db, "o1", "+919876543210").await;
    let p = profile();
    assert!(
        db.create_tenant_with_branch(&NewTenantBranch {
            tenant_id: "t1",
            owner_id: "o1",
            tenant_name: "Pixel Arena Gaming",
            branch_id: "b1",
            branch_code: "PAG",
            profile: &p,
            now: 1,
        })
        .await
        .unwrap()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn two_simultaneous_free_picks_in_one_tenant_give_one_success_locally() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gz_control.db");
    let db = ControlDb::open_local(&path).await.unwrap();
    db.migrate(1).await.unwrap();
    two_branches(&db).await;
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute(SECOND, []).unwrap();
    c.execute(SECOND_PROGRESS, []).unwrap();
    race(std::sync::Arc::new(db)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn two_simultaneous_free_picks_in_one_tenant_give_one_success_remotely() {
    let Some(t) = sqld::fresh().await else { return };
    let db = ControlDb::connect(&t.url, "").await.unwrap();
    db.migrate(1).await.unwrap();
    two_branches(&db).await;
    sqld::exec(&t.url, SECOND).await;
    sqld::exec(&t.url, SECOND_PROGRESS).await;
    race(std::sync::Arc::new(db)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_schema_backs_up_one_free_branch_and_consistent_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gz_control.db");
    let db = ControlDb::open_local(&path).await.unwrap();
    db.migrate(1).await.unwrap();
    two_branches(&db).await;
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute(SECOND, []).unwrap();
    let insert = |branch: &str, tier: &str, source: &str, until: Option<i64>| {
        c.execute(
            "INSERT INTO entitlements (branch_id, tenant_id, tier, source, issued_at, until, \
             updated_at) VALUES (?, 't1', ?, ?, 1, ?, 1)",
            rusqlite::params![branch, tier, source, until],
        )
    };
    assert!(insert("b1", "free", "comp", Some(5)).is_err());
    assert!(insert("b1", "basic", "free", None).is_err());
    assert!(insert("b1", "basic", "comp", None).is_err());
    insert("b1", "free", "free", None).unwrap();
    let dup = insert("b9", "free", "free", None).unwrap_err().to_string();
    assert!(dup.contains("UNIQUE"), "{dup}");
    insert("b9", "basic", "paid", None).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn entitlement_queries_on_the_remote_backend() {
    let Some(t) = sqld::fresh().await else { return };
    let db = ControlDb::connect(&t.url, "").await.unwrap();
    db.migrate(1).await.unwrap();
    let url = t.url.clone();
    exercise(
        &db,
        async || {
            sqld::exec(&url, SECOND).await;
            sqld::exec(&url, SECOND_PROGRESS).await;
        },
        async || sqld::exec(&url, PAID).await,
    )
    .await;
}
