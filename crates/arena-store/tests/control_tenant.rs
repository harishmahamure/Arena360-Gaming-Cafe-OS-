#![cfg(feature = "control")]

#[path = "support/sqld.rs"]
mod sqld;

use arena_proto::{StepState, WizardStep};
use arena_store::control::{
    BranchCodeOutcome, BranchProfile, ControlDb, NewOtpChallenge, NewOwner, NewOwnerSession,
    NewTenantBranch, SignupCompletion, SignupOutcome,
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

fn profile(name: &str) -> BranchProfile {
    BranchProfile {
        name: name.into(),
        city: "Pune".into(),
        country: "IN".into(),
        currency: "INR".into(),
        timezone: "Asia/Kolkata".into(),
        gst_registered: false,
        gstin: None,
    }
}

async fn exercise(db: &ControlDb) {
    owner(db, "o1", "+919876543210").await;
    owner(db, "o2", "+919876543211").await;
    assert_eq!(db.tenant_of_owner("o1").await.unwrap(), None);

    let p = profile("Pixel Arena Gaming");
    let new = NewTenantBranch {
        tenant_id: "t1",
        owner_id: "o1",
        tenant_name: "Pixel Arena Gaming",
        branch_id: "b1",
        branch_code: "PAG",
        profile: &p,
        now: 10_000,
    };
    assert!(db.create_tenant_with_branch(&new).await.unwrap());
    let again = NewTenantBranch {
        tenant_id: "t1b",
        branch_id: "b1b",
        ..new.clone()
    };
    assert!(!db.create_tenant_with_branch(&again).await.unwrap());
    assert!(db.owned_branch("o1", "b1b").await.unwrap().is_none());

    let t = db.tenant_of_owner("o1").await.unwrap().unwrap();
    assert_eq!(
        (t.id.as_str(), t.name.as_str()),
        ("t1", "Pixel Arena Gaming")
    );
    let branches = db.branches_of_tenant("t1").await.unwrap();
    assert_eq!(branches.len(), 1);
    let b = &branches[0];
    assert_eq!(
        (
            b.branch_code.as_str(),
            b.country.as_str(),
            b.gst_registered,
            b.code_locked_at
        ),
        ("PAG", "IN", false, None)
    );

    let progress = db.onboarding_progress("b1").await.unwrap();
    assert_eq!(progress.len(), 10);
    assert_eq!(
        progress.iter().map(|p| p.step).collect::<Vec<_>>(),
        WizardStep::ALL.to_vec()
    );
    assert_eq!(
        (progress[0].state, progress[0].completed_at),
        (StepState::Done, Some(10_000))
    );
    assert!(
        progress[1..]
            .iter()
            .all(|p| p.state == StepState::Pending && p.completed_at.is_none())
    );

    let mut edited = profile("Pixel Arena Gaming Pune");
    edited.gst_registered = true;
    edited.gstin = Some("27AAPFU0939F1ZV".into());
    edited.timezone = "Asia/Calcutta".into();
    assert!(
        db.update_cafe("t1", "Pixel Arena", "b1", &edited, 20_000)
            .await
            .unwrap()
    );
    let b = db.owned_branch("o1", "b1").await.unwrap().unwrap();
    assert_eq!(b.name, "Pixel Arena Gaming Pune");
    assert_eq!(b.gstin.as_deref(), Some("27AAPFU0939F1ZV"));
    assert_eq!((b.created_at, b.updated_at), (10_000, 20_000));
    assert_eq!(
        db.tenant_of_owner("o1").await.unwrap().unwrap().name,
        "Pixel Arena"
    );
    let progress = db.onboarding_progress("b1").await.unwrap();
    assert_eq!(
        (progress[0].state, progress[0].completed_at),
        (StepState::Done, Some(10_000))
    );
    assert!(
        !db.update_cafe("t2", "X", "b1", &edited, 21_000)
            .await
            .unwrap()
    );

    assert!(db.owned_branch("o2", "b1").await.unwrap().is_none());
    assert!(db.owned_branch("o1", "nope").await.unwrap().is_none());

    let p2 = profile("Other");
    assert!(
        db.create_tenant_with_branch(&NewTenantBranch {
            tenant_id: "t2",
            owner_id: "o2",
            tenant_name: "Other",
            branch_id: "b2",
            branch_code: "PAG",
            profile: &p2,
            now: 30_000,
        })
        .await
        .unwrap()
    );
    assert_eq!(db.tenant_branch_codes("t2").await.unwrap(), vec!["PAG"]);

    assert_eq!(
        db.set_branch_code("t1", "b1", "ARENA1", 40_000)
            .await
            .unwrap(),
        BranchCodeOutcome::Updated
    );
    assert_eq!(
        db.set_branch_code("t2", "b1", "ZZZ", 40_000).await.unwrap(),
        BranchCodeOutcome::Missing
    );
    assert!(db.lock_branch_code("b1", 50_000).await.unwrap());
    assert!(!db.lock_branch_code("b1", 51_000).await.unwrap());
    assert_eq!(
        db.set_branch_code("t1", "b1", "NEW1", 52_000)
            .await
            .unwrap(),
        BranchCodeOutcome::Locked
    );
    assert_eq!(
        db.owned_branch("o1", "b1")
            .await
            .unwrap()
            .unwrap()
            .code_locked_at,
        Some(50_000)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn tenant_queries_on_the_local_backend() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gz_control.db");
    let db = ControlDb::open_local(&path).await.unwrap();
    db.migrate(1).await.unwrap();
    exercise(&db).await;
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO branches (id, tenant_id, name, city, country, currency, timezone, \
             gst_registered, gstin, branch_code, code_locked_at, created_at, updated_at) \
             VALUES ('b9', 't1', 'Second', 'Pune', 'IN', 'INR', 'Asia/Kolkata', 0, NULL, \
             'SEC', NULL, 60000, 60000)",
            [],
        )
        .unwrap();
    assert_eq!(
        db.set_branch_code("t1", "b9", "ARENA1", 61_000)
            .await
            .unwrap(),
        BranchCodeOutcome::Taken
    );
    assert_eq!(
        db.set_branch_code("t1", "b9", "SEC2", 61_000)
            .await
            .unwrap(),
        BranchCodeOutcome::Updated
    );
    let codes = db.tenant_branch_codes("t1").await.unwrap();
    assert!(codes.contains(&"ARENA1".to_owned()) && codes.contains(&"SEC2".to_owned()));
    assert_eq!(db.branches_of_tenant("t1").await.unwrap().len(), 2);

    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch(
        "DELETE FROM onboarding_progress WHERE branch_id = 'b2'; DELETE FROM branches WHERE id = 'b2';",
    )
    .unwrap();
    let p = profile("Fresh");
    assert!(
        db.create_first_branch("t2", "Fresh", "b2n", "FRE", &p, 70_000)
            .await
            .unwrap()
    );
    assert!(
        !db.create_first_branch("t2", "Fresh", "b2x", "FR2", &p, 70_001)
            .await
            .unwrap()
    );
    assert_eq!(db.branches_of_tenant("t2").await.unwrap().len(), 1);
    assert_eq!(
        db.tenant_of_owner("o2").await.unwrap().unwrap().name,
        "Fresh"
    );
    let progress = db.onboarding_progress("b2n").await.unwrap();
    assert_eq!(progress.len(), 10);
    assert_eq!(progress[0].state, StepState::Done);

    c.execute_batch(
        "CREATE TRIGGER no_code BEFORE UPDATE OF branch_code ON branches \
         BEGIN SELECT RAISE(IGNORE); END;",
    )
    .unwrap();
    assert!(
        db.set_branch_code("t2", "b2n", "QQQ", 71_000)
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn tenant_queries_on_the_remote_backend() {
    let Some(t) = sqld::fresh().await else { return };
    let db = ControlDb::connect(&t.url, "").await.unwrap();
    db.migrate(1).await.unwrap();
    exercise(&db).await;
}
