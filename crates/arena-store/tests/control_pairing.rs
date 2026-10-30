#![cfg(feature = "control")]

#[path = "support/sqld.rs"]
mod sqld;

use arena_proto::{HubRole, Tier};
use arena_store::control::{
    BranchProfile, ControlDb, IssueOutcome, NewOtpChallenge, NewOwner, NewOwnerSession,
    NewPairingCode, NewTenantBranch, PAIRING_HISTORY_MS, RedeemInvalid, Redeemed, SignupCompletion,
    SignupOutcome,
};

const MIN15: i64 = 15 * 60 * 1000;

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

async fn branch(db: &ControlDb, owner_id: &str, tenant: &str, branch: &str, code: &str) {
    let p = BranchProfile {
        name: "Pixel Arena Gaming".into(),
        city: "Pune".into(),
        country: "IN".into(),
        currency: "INR".into(),
        timezone: "Asia/Kolkata".into(),
        gst_registered: false,
        gstin: None,
    };
    assert!(
        db.create_tenant_with_branch(&NewTenantBranch {
            tenant_id: tenant,
            owner_id,
            tenant_name: "Pixel Arena Gaming",
            branch_id: branch,
            branch_code: code,
            profile: &p,
            now: 10_000,
        })
        .await
        .unwrap()
    );
}

fn code<'a>(id: &'a str, branch_id: &'a str, hash: &'a str, now: i64) -> NewPairingCode<'a> {
    NewPairingCode {
        id,
        branch_id,
        code_hash: hash,
        role: HubRole::Main,
        now,
        expires_at: now + MIN15,
    }
}

async fn exercise(db: &ControlDb, second: &ControlDb) {
    owner(db, "o1", "+919876543210").await;
    owner(db, "o2", "+919876543211").await;
    branch(db, "o1", "t1", "b1", "PAG").await;
    branch(db, "o2", "t2", "b2", "OTH").await;

    assert!(!db.pairing_steps_ready("b1").await.unwrap());
    assert_eq!(
        db.issue_pairing_code(&code("p0", "b1", "h0", 20_000))
            .await
            .unwrap(),
        IssueOutcome::NotReady
    );
    assert!(db.pairing_codes_of("b1").await.unwrap().is_empty());

    db.choose_free_tier("t1", "b1", 20_000).await.unwrap();
    db.choose_free_tier("t2", "b2", 20_000).await.unwrap();
    assert!(db.pairing_steps_ready("b1").await.unwrap());

    let t = 100_000;
    assert_eq!(
        db.issue_pairing_code(&code("p1", "b1", "h1", t))
            .await
            .unwrap(),
        IssueOutcome::Issued
    );
    let rows = db.pairing_codes_of("b1").await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (
            rows[0].code_hash.as_str(),
            rows[0].tier_at_issue,
            rows[0].created_at,
            rows[0].expires_at
        ),
        ("h1", Tier::Free, t, t + MIN15)
    );
    assert_eq!(
        db.active_pairing_code("b1", t).await.unwrap().unwrap().id,
        "p1"
    );

    assert_eq!(
        db.issue_pairing_code(&code("p2", "b1", "h2", t + 1_000))
            .await
            .unwrap(),
        IssueOutcome::Issued
    );
    let rows = db.pairing_codes_of("b1").await.unwrap();
    assert_eq!(rows[0].invalidated_at, Some(t + 1_000));
    assert_eq!(rows[1].invalidated_at, None);
    assert_eq!(rows.iter().filter(|r| r.is_valid_at(t + 1_000)).count(), 1);
    assert_eq!(
        db.active_pairing_code("b1", t + 1_000)
            .await
            .unwrap()
            .unwrap()
            .id,
        "p2"
    );

    assert_eq!(
        db.redeem_pairing_code("h1", t + 2_000).await.unwrap(),
        Err(RedeemInvalid)
    );
    assert_eq!(
        db.issue_pairing_code(&code("p3", "b2", "h2", t + 2_000))
            .await
            .unwrap(),
        IssueOutcome::HashTaken
    );
    assert_eq!(db.active_pairing_code("b2", t + 2_000).await.unwrap(), None);

    let ok = Redeemed {
        branch_id: "b1".into(),
        tenant_id: "t1".into(),
        tier: Tier::Free,
    };
    assert_eq!(
        db.redeem_pairing_code("h2", t + 3_000).await.unwrap(),
        Ok(ok.clone())
    );
    let rows = db.pairing_codes_of("b1").await.unwrap();
    assert_eq!(rows[1].used_at, Some(t + 3_000));
    assert_eq!(
        db.redeem_pairing_code("h2", t + 4_000).await.unwrap(),
        Err(RedeemInvalid)
    );
    assert_eq!(db.active_pairing_code("b1", t + 4_000).await.unwrap(), None);
    assert_eq!(
        db.redeem_pairing_code("nope", t + 4_000).await.unwrap(),
        Err(RedeemInvalid)
    );

    assert_eq!(
        db.issue_pairing_code(&code("p4", "b1", "h4", t + 5_000))
            .await
            .unwrap(),
        IssueOutcome::Issued
    );
    assert_eq!(
        db.pairing_codes_of("b1").await.unwrap()[1].invalidated_at,
        None
    );
    let expiry = t + 5_000 + MIN15;
    assert!(
        db.active_pairing_code("b1", expiry - 1)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(db.active_pairing_code("b1", expiry).await.unwrap(), None);
    assert_eq!(
        db.redeem_pairing_code("h4", expiry).await.unwrap(),
        Err(RedeemInvalid)
    );
    assert_eq!(db.pairing_codes_of("b1").await.unwrap()[2].used_at, None);

    assert_eq!(
        db.issue_pairing_code(&code("p5", "b2", "h5", t + 6_000))
            .await
            .unwrap(),
        IssueOutcome::Issued
    );
    assert_eq!(
        db.issue_pairing_code(&code("p6", "b1", "h6", t + 6_000))
            .await
            .unwrap(),
        IssueOutcome::Issued
    );
    assert!(
        db.active_pairing_code("b2", t + 6_000)
            .await
            .unwrap()
            .is_some(),
        "issuing for b1 leaves b2's code alone"
    );

    let (a, b) = tokio::join!(
        db.redeem_pairing_code("h6", t + 7_000),
        second.redeem_pairing_code("h6", t + 7_000)
    );
    let outcomes = [a.unwrap(), b.unwrap()];
    assert_eq!(
        outcomes.iter().filter(|o| o.is_ok()).count(),
        1,
        "{outcomes:?}"
    );
    assert!(outcomes.contains(&Ok(ok)));
    assert!(outcomes.contains(&Err(RedeemInvalid)));

    assert_eq!(
        db.issue_pairing_code(&code("p7", "b2", "h1", t + 8_000))
            .await
            .unwrap(),
        IssueOutcome::HashTaken
    );
    let live = db
        .active_pairing_code("b2", t + 8_000)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((live.id.as_str(), live.invalidated_at), ("p5", None));
    assert!(
        db.pairing_codes_of("b2")
            .await
            .unwrap()
            .iter()
            .all(|r| r.id != "p7")
    );

    let (c8, c9) = (
        code("p8", "b2", "h8", t + 9_000),
        code("p9", "b2", "h9", t + 9_000),
    );
    let (a, b) = tokio::join!(db.issue_pairing_code(&c8), second.issue_pairing_code(&c9));
    let issued = [a, b]
        .into_iter()
        .filter(|o| o.as_ref().is_ok_and(|o| *o == IssueOutcome::Issued))
        .count();
    assert!(issued >= 1);
    let rows = db.pairing_codes_of("b2").await.unwrap();
    assert_eq!(
        rows.iter().filter(|r| r.is_valid_at(t + 9_000)).count(),
        1,
        "{rows:?}"
    );
    assert_eq!(
        rows.iter()
            .filter(|r| r.used_at.is_none() && r.invalidated_at.is_none())
            .count(),
        1
    );

    let later = t + MIN15 + PAIRING_HISTORY_MS + 10_000;
    assert_eq!(
        db.issue_pairing_code(&code("p10", "b1", "h10", later))
            .await
            .unwrap(),
        IssueOutcome::Issued
    );
    let ids: Vec<String> = db
        .pairing_codes_of("b1")
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(ids, ["p10"]);
    assert!(db.pairing_codes_of("b2").await.unwrap().is_empty());

    // Story 9.18: a standby code lives next to the Hub code of the same branch, and each role
    // replaces only its own.
    let s = later + 1_000;
    let standby = |id, hash, now| NewPairingCode {
        role: HubRole::Standby,
        ..code(id, "b1", hash, now)
    };
    assert_eq!(
        db.issue_pairing_code(&standby("p11", "h11", s))
            .await
            .unwrap(),
        IssueOutcome::Issued
    );
    let hub_code = db.active_pairing_code("b1", s).await.unwrap().unwrap();
    assert_eq!(
        (hub_code.id.as_str(), hub_code.role),
        ("p10", HubRole::Main)
    );
    let standby_code = db.active_standby_code("b1", s).await.unwrap().unwrap();
    assert_eq!(
        (standby_code.id.as_str(), standby_code.role),
        ("p11", HubRole::Standby)
    );
    assert_eq!(
        db.issue_pairing_code(&standby("p12", "h12", s + 1_000))
            .await
            .unwrap(),
        IssueOutcome::Issued
    );
    assert_eq!(
        db.active_standby_code("b1", s + 1_000)
            .await
            .unwrap()
            .unwrap()
            .id,
        "p12"
    );
    assert_eq!(
        db.active_pairing_code("b1", s + 1_000)
            .await
            .unwrap()
            .unwrap()
            .id,
        "p10",
        "a new standby code leaves the Hub code alone"
    );
    assert_eq!(
        db.issue_pairing_code(&code("p13", "b1", "h13", s + 2_000))
            .await
            .unwrap(),
        IssueOutcome::Issued
    );
    assert_eq!(
        db.active_standby_code("b1", s + 2_000)
            .await
            .unwrap()
            .unwrap()
            .id,
        "p12",
        "a new Hub code leaves the standby code alone"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn pairing_codes_on_the_local_backend() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gz_control.db");
    let db = ControlDb::open_local(&path).await.unwrap();
    db.migrate(1).await.unwrap();
    let second = ControlDb::open_local(&path).await.unwrap();
    exercise(&db, &second).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pairing_codes_on_the_remote_backend() {
    let Some(t) = sqld::fresh().await else { return };
    let db = ControlDb::connect(&t.url, "").await.unwrap();
    db.migrate(1).await.unwrap();
    let second = ControlDb::connect(&t.url, "").await.unwrap();
    exercise(&db, &second).await;
}
