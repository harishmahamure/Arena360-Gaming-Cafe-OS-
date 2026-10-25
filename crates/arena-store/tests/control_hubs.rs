#![cfg(feature = "control")]

#[path = "support/sqld.rs"]
mod sqld;

use arena_proto::{HubPlatform, Tier};
use arena_store::control::{
    BranchProfile, CompGrant, ControlDb, IssueOutcome, NewHub, NewOtpChallenge, NewOwner,
    NewOwnerSession, NewPairingCode, NewSetupCode, NewTenantBranch, PairOutcome, RateRule,
    SignupCompletion, SignupOutcome,
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
        now,
        expires_at: now + MIN15,
    }
}

fn hub<'a>(id: &'a str, code_hash: &'a str, platform: HubPlatform, now: i64) -> NewHub<'a> {
    NewHub {
        id,
        branch_id: "b1",
        platform,
        hub_instance_id: "inst-1",
        version: "0.1.0",
        token_hash: "$argon2id$hash",
        code_hash,
        setup: NewSetupCode {
            code_hash: "$argon2id$setup",
            hub_expires_at: now + 72 * 60 * 60 * 1000,
            ciphertext: b"ct",
            nonce: b"nonce",
        },
        now,
    }
}

async fn issued(db: &ControlDb, id: &str, hash: &str, now: i64) {
    assert_eq!(
        db.issue_pairing_code(&code(id, "b1", hash, now))
            .await
            .unwrap(),
        IssueOutcome::Issued
    );
}

async fn exercise(db: &ControlDb, second: &ControlDb) {
    owner(db, "o1", "+919876543210").await;
    branch(db, "o1", "t1", "b1", "PAG").await;
    db.choose_free_tier("t1", "b1", 20_000).await.unwrap();
    let t = 100_000;

    issued(db, "p1", "h1", t).await;
    assert_eq!(
        db.pair_hub(&hub("hw", "h1", HubPlatform::Windows, t + 1))
            .await
            .unwrap(),
        PairOutcome::CodeRefused,
        "a Windows Hub on Free is refused"
    );
    let row = db.pairing_code_by_hash("h1").await.unwrap().unwrap();
    assert!(row.is_valid_at(t + 1), "the refused code stays unconsumed");
    assert!(db.hubs_of_branch("b1").await.unwrap().is_empty());
    assert_eq!(db.branch("b1").await.unwrap().unwrap().code_locked_at, None);

    assert_eq!(
        db.pair_hub(&hub("ha", "h1", HubPlatform::Android, t + 2))
            .await
            .unwrap(),
        PairOutcome::Paired { revoked: vec![] }
    );
    let used = db.pairing_code_by_hash("h1").await.unwrap().unwrap();
    assert_eq!(used.used_at, Some(t + 2));
    assert_eq!(
        db.branch("b1").await.unwrap().unwrap().code_locked_at,
        Some(t + 2)
    );
    let active = db.active_hub("b1").await.unwrap().unwrap();
    assert_eq!(
        (
            active.id.as_str(),
            active.platform,
            active.hub_instance_id.as_str(),
            active.version.as_str(),
            active.token_hash.as_str(),
            active.paired_at,
            active.last_seen_at,
            active.revoked_at
        ),
        (
            "ha",
            HubPlatform::Android,
            "inst-1",
            "0.1.0",
            "$argon2id$hash",
            t + 2,
            None,
            None
        )
    );
    assert_eq!(
        db.pair_hub(&hub("hx", "h1", HubPlatform::Android, t + 3))
            .await
            .unwrap(),
        PairOutcome::CodeRefused,
        "a used code never pairs again"
    );

    issued(db, "p2", "h2", t + 10).await;
    assert_eq!(
        db.pair_hub(&hub("hb", "h2", HubPlatform::Android, t + 11))
            .await
            .unwrap(),
        PairOutcome::Paired {
            revoked: vec!["ha".to_owned()]
        }
    );
    let all = db.hubs_of_branch("b1").await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].revoked_at, Some(t + 11));
    assert_eq!(db.active_hub("b1").await.unwrap().unwrap().id, "hb");
    assert_eq!(
        db.branch("b1").await.unwrap().unwrap().code_locked_at,
        Some(t + 2),
        "the first lock stays"
    );
    assert_eq!(
        db.hub("ha").await.unwrap().unwrap().revoked_at,
        Some(t + 11)
    );

    issued(db, "p3", "h3", t + 20).await;
    assert_eq!(
        db.pair_hub(&hub(
            "hy",
            "h3",
            HubPlatform::Android,
            t + 20 + 15 * 60 * 1000
        ))
        .await
        .unwrap(),
        PairOutcome::CodeRefused,
        "an expired code never pairs"
    );

    let until = t + 10_000_000;
    db.grant_comp(&CompGrant {
        branch_id: "b1",
        tier: Tier::Basic,
        until,
        audit_id: "a1",
        operator: "ops",
        reason: "beta",
        details_json: "{}",
        now: t + 30,
    })
    .await
    .unwrap();
    issued(db, "p4", "h4", t + 40).await;
    assert_eq!(
        db.pair_hub(&hub("hc", "h4", HubPlatform::Windows, t + 41))
            .await
            .unwrap(),
        PairOutcome::Paired {
            revoked: vec!["hb".to_owned()]
        },
        "a Windows Hub on comp Basic pairs"
    );
    issued(db, "p5", "h5", until).await;
    assert_eq!(
        db.pair_hub(&hub("hd", "h5", HubPlatform::Windows, until + 1))
            .await
            .unwrap(),
        PairOutcome::CodeRefused,
        "an expired comp is Free again"
    );

    issued(db, "p6", "h6", until + 2).await;
    let (r1, r2) = (
        hub("r1", "h6", HubPlatform::Android, until + 3),
        hub("r2", "h6", HubPlatform::Android, until + 3),
    );
    let (a, b) = tokio::join!(db.pair_hub(&r1), second.pair_hub(&r2));
    let outcomes = [a.unwrap(), b.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|o| matches!(o, PairOutcome::Paired { .. }))
            .count(),
        1,
        "{outcomes:?}"
    );
    assert_eq!(
        db.hubs_of_branch("b1")
            .await
            .unwrap()
            .iter()
            .filter(|h| h.revoked_at.is_none())
            .count(),
        1
    );

    issued(db, "p7", "h7", until + 10).await;
    let key = "pair_code:h7";
    let rule = RateRule {
        key,
        prune_prefix: "pair_code:",
        bucket_ms: 60_000,
        window_ms: 3_600_000,
        limit: 5,
    };
    for i in 0..4 {
        db.rate_reserve(&[rule], until + 11 + i)
            .await
            .unwrap()
            .unwrap();
        assert!(
            !db.invalidate_failed_pairing_code("h7", key, until - 3_660_000, 5, until + 11 + i)
                .await
                .unwrap()
        );
    }
    db.rate_reserve(&[rule], until + 20).await.unwrap().unwrap();
    assert!(
        db.invalidate_failed_pairing_code("h7", key, until - 3_660_000, 5, until + 20)
            .await
            .unwrap()
    );
    let gone = db.pairing_code_by_hash("h7").await.unwrap().unwrap();
    assert_eq!(gone.invalidated_at, Some(until + 20));
    assert_eq!(
        db.pair_hub(&hub("hz", "h7", HubPlatform::Android, until + 21))
            .await
            .unwrap(),
        PairOutcome::CodeRefused
    );
    assert_eq!(db.pairing_code_by_hash("nope").await.unwrap(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn hubs_on_the_local_backend() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gz_control.db");
    let db = ControlDb::open_local(&path).await.unwrap();
    db.migrate(1).await.unwrap();
    let second = ControlDb::open_local(&path).await.unwrap();
    exercise(&db, &second).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn hubs_on_the_remote_backend() {
    let Some(t) = sqld::fresh().await else { return };
    let db = ControlDb::connect(&t.url, "").await.unwrap();
    db.migrate(1).await.unwrap();
    let second = ControlDb::connect(&t.url, "").await.unwrap();
    exercise(&db, &second).await;
}
