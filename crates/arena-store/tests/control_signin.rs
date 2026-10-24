#![cfg(feature = "control")]

#[path = "support/sqld.rs"]
mod sqld;

use arena_proto::MfaState;
use arena_store::control::{
    ControlDb, NewBackupCode, NewOtpChallenge, NewOwner, NewOwnerDevice, NewOwnerSession,
    NewSigninChallenge, SigninCompletion, SigninFactor, SigninFailure, SignupCompletion,
    SignupOutcome,
};

const PHONE: &str = "+919876543210";
const LOCK_MS: i64 = 15 * 60_000;

fn session(id: &str, now: i64) -> NewOwnerSession {
    NewOwnerSession {
        id: id.into(),
        owner_id: String::new(),
        family_id: format!("fam-{id}"),
        access_hash: format!("acc-{id}"),
        access_expires_at: now + 900_000,
        refresh_hash: format!("ref-{id}"),
        refresh_expires_at: now + 30 * 86_400_000,
        created_at: now,
        device_id: None,
    }
}

async fn enrolled_owner(db: &ControlDb) -> String {
    let owner = NewOwner {
        name: "Asha Rao".into(),
        phone_e164: PHONE.into(),
        email: "Asha.Rao@Example.in".into(),
        cafe_name: "Level Up".into(),
        city: "Pune".into(),
    };
    db.insert_otp_challenge(&NewOtpChallenge {
        id: "c1".into(),
        phone_e164: PHONE.into(),
        code_hash: "h".into(),
        payload: owner.clone(),
        created_at: 1_000,
        expires_at: 301_000,
    })
    .await
    .unwrap();
    let out = db
        .complete_signup(&SignupCompletion {
            challenge_id: "c1",
            code_hash: "h",
            max_attempts: 5,
            now: 2_000,
            new_owner_id: "o1",
            owner: &owner,
            session: &session("s0", 2_000),
        })
        .await
        .unwrap();
    assert!(matches!(out, SignupOutcome::Session(_)));
    assert!(
        db.set_owner_password("o1", "$argon2id$x", 3_000)
            .await
            .unwrap()
    );
    assert!(db.start_owner_mfa("o1", b"ct", b"n", 4_000).await.unwrap());
    let codes: Vec<NewBackupCode> = (0..10)
        .map(|i| NewBackupCode {
            id: format!("b{i:02}"),
            code_hash: format!("hash-{i}"),
        })
        .collect();
    assert!(
        db.confirm_owner_mfa("o1", b"n", 100, &codes, 5_000)
            .await
            .unwrap()
    );
    "o1".to_owned()
}

fn failure(challenge: Option<&str>, now: i64) -> SigninFailure<'_> {
    SigninFailure {
        owner_id: "o1",
        challenge_id: challenge,
        max_attempts: 5,
        max_failures: 5,
        lock_ms: LOCK_MS,
        now,
    }
}

async fn challenge(db: &ControlDb, id: &str, now: i64) {
    db.insert_signin_challenge(&NewSigninChallenge {
        id: id.into(),
        owner_id: "o1".into(),
        created_at: now,
        expires_at: now + 300_000,
    })
    .await
    .unwrap();
}

async fn exercise(db: &ControlDb) {
    let id = enrolled_owner(db).await;
    let o = db.owner_by_id(&id).await.unwrap().unwrap();
    assert_eq!(o.mfa_state, MfaState::Enrolled);
    assert_eq!(
        (o.sms_fallback, o.failed_signins, o.locked_until),
        (false, 0, None)
    );
    let by_email = db.owner_by_email("asha.rao@example.IN").await.unwrap();
    assert_eq!(by_email.map(|o| o.id), Some(id.clone()));
    assert_eq!(db.owner_by_email("nobody@example.in").await.unwrap(), None);

    let now = 10_000;
    for n in 1..5 {
        assert_eq!(
            db.record_signin_failure(&failure(None, now)).await.unwrap(),
            None
        );
        let o = db.owner_by_id(&id).await.unwrap().unwrap();
        assert_eq!(o.failed_signins, n);
    }
    let locked = db.record_signin_failure(&failure(None, now)).await.unwrap();
    assert_eq!(locked, Some(now + LOCK_MS));
    let o = db.owner_by_id(&id).await.unwrap().unwrap();
    assert_eq!((o.failed_signins, o.locked_until), (0, Some(now + LOCK_MS)));
    assert_eq!(
        db.record_signin_failure(&failure(None, now + 1))
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        db.owner_by_id(&id).await.unwrap().unwrap().failed_signins,
        0
    );
    assert!(
        !db.signin_with_password(&id, &session("s-locked", now), None, now + 1)
            .await
            .unwrap()
    );

    let now = now + LOCK_MS;
    challenge(db, "sc1", now).await;
    db.record_signin_failure(&failure(Some("sc1"), now))
        .await
        .unwrap();
    let c = db.signin_challenge("sc1").await.unwrap().unwrap();
    assert_eq!(c.attempts, 1);
    assert_eq!(
        db.owner_by_id(&id).await.unwrap().unwrap().failed_signins,
        1
    );

    let totp = |step| SigninCompletion {
        challenge_id: "sc1",
        owner_id: "o1",
        max_attempts: 5,
        now: now + 1_000,
        factor: SigninFactor::Totp { step },
        device: None,
        session: Box::leak(Box::new(session("s1", now + 1_000))),
    };
    assert!(!db.complete_signin(&totp(100)).await.unwrap());
    let device = NewOwnerDevice {
        id: "d1".into(),
        token_hash: "tok-1".into(),
        label: "Chrome on Android".into(),
        expires_at: now + 30 * 86_400_000,
        refresh: false,
    };
    let mut ok = totp(101);
    ok.device = Some(device);
    assert!(db.complete_signin(&ok).await.unwrap());
    assert!(!db.complete_signin(&totp(102)).await.unwrap());
    let o = db.owner_by_id(&id).await.unwrap().unwrap();
    assert_eq!((o.failed_signins, o.locked_until), (0, None));
    assert_eq!(
        db.owner_mfa(&id).await.unwrap().unwrap().last_step,
        Some(101)
    );
    let s1 = db.owner_session("s1").await.unwrap().unwrap();
    assert_eq!(s1.device_id.as_deref(), Some("d1"));

    let d = db.trusted_device(&id, "tok-1", now + 2_000).await.unwrap();
    assert_eq!(
        d.as_ref().map(|d| d.label.as_str()),
        Some("Chrome on Android")
    );
    assert!(
        db.trusted_device("o2", "tok-1", now)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.trusted_device(&id, "tok-1", now + 31 * 86_400_000)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.signin_with_password(&id, &session("s2", now + 3_000), Some("d1"), now + 3_000)
            .await
            .unwrap()
    );
    let devices = db.owner_devices(&id, now + 3_000).await.unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].last_seen_at, now + 3_000);
    let s2 = db.owner_session("s2").await.unwrap().unwrap();
    assert_eq!(s2.device_id.as_deref(), Some("d1"));

    challenge(db, "sc2", now).await;
    let backup = SigninCompletion {
        challenge_id: "sc2",
        owner_id: "o1",
        max_attempts: 5,
        now: now + 4_000,
        factor: SigninFactor::BackupCode { id: "b03".into() },
        device: None,
        session: &session("s3", now + 4_000),
    };
    assert!(db.complete_signin(&backup).await.unwrap());
    assert_eq!(db.backup_codes_remaining(&id).await.unwrap(), 9);
    challenge(db, "sc3", now).await;
    let again = SigninCompletion {
        challenge_id: "sc3",
        session: &session("s4", now + 4_000),
        ..backup.clone()
    };
    assert!(!db.complete_signin(&again).await.unwrap());
    assert!(
        db.signin_challenge("sc3")
            .await
            .unwrap()
            .unwrap()
            .consumed_at
            .is_none()
    );

    assert!(
        db.set_signin_sms("sc3", None, "sms-hash", now + 5_000, now + 305_000, 5)
            .await
            .unwrap()
    );
    assert!(
        !db.set_signin_sms("sc3", None, "sms-other", now + 5_000, now + 305_000, 5)
            .await
            .unwrap()
    );
    let c = db.signin_challenge("sc3").await.unwrap().unwrap();
    assert_eq!(
        (c.sms_code_hash.as_deref(), c.sms_sent_at, c.expires_at),
        (Some("sms-hash"), Some(now + 5_000), now + 305_000)
    );
    assert!(
        db.set_signin_sms(
            "sc3",
            Some(now + 5_000),
            "sms-2",
            now + 40_000,
            now + 340_000,
            5
        )
        .await
        .unwrap()
    );
    db.revert_signin_sms("sc3", "sms-2", &c).await.unwrap();
    let c = db.signin_challenge("sc3").await.unwrap().unwrap();
    assert_eq!(c.sms_code_hash.as_deref(), Some("sms-hash"));
    let sms = |hash: &str| SigninCompletion {
        challenge_id: "sc3",
        owner_id: "o1",
        max_attempts: 5,
        now: now + 6_000,
        factor: SigninFactor::Sms {
            code_hash: hash.into(),
            sent_after: now + 6_000 - 300_000,
        },
        device: None,
        session: Box::leak(Box::new(session("s5", now + 6_000))),
    };
    assert!(!db.complete_signin(&sms("wrong")).await.unwrap());
    assert!(db.complete_signin(&sms("sms-hash")).await.unwrap());

    challenge(db, "sc4", now).await;
    for _ in 0..5 {
        db.record_signin_failure(&SigninFailure {
            max_failures: 100,
            ..failure(Some("sc4"), now + 7_000)
        })
        .await
        .unwrap();
    }
    let mut voided = totp(200);
    voided.challenge_id = "sc4";
    voided.session = Box::leak(Box::new(session("s6", now + 7_000)));
    voided.now = now + 7_000;
    assert!(!db.complete_signin(&voided).await.unwrap());
    assert_eq!(
        db.signin_challenge("sc4").await.unwrap().unwrap().attempts,
        5
    );

    assert!(db.set_sms_fallback(&id, true).await.unwrap());
    assert!(db.owner_by_id(&id).await.unwrap().unwrap().sms_fallback);
    assert!(!db.set_sms_fallback("nobody", true).await.unwrap());

    challenge(db, "sc5", now + 8_000).await;
    let mut again = totp(300);
    again.challenge_id = "sc5";
    again.now = now + 8_000;
    again.session = Box::leak(Box::new(session("s7", now + 8_000)));
    again.device = Some(NewOwnerDevice {
        id: "d1".into(),
        token_hash: "tok-2".into(),
        label: "Firefox on Linux".into(),
        expires_at: now + 40 * 86_400_000,
        refresh: true,
    });
    assert!(db.complete_signin(&again).await.unwrap());
    let devices = db.owner_devices(&id, now + 8_000).await.unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(
        (devices[0].label.as_str(), devices[0].expires_at),
        ("Firefox on Linux", now + 40 * 86_400_000)
    );
    assert!(
        db.trusted_device(&id, "tok-1", now + 8_000)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.trusted_device(&id, "tok-2", now + 8_000)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        db.owner_session("s7")
            .await
            .unwrap()
            .unwrap()
            .device_id
            .as_deref(),
        Some("d1")
    );

    assert!(!db.revoke_owner_device("o2", "d1", now).await.unwrap());
    assert!(
        db.revoke_owner_device(&id, "d1", now + 8_000)
            .await
            .unwrap()
    );
    assert!(
        !db.revoke_owner_device(&id, "d1", now + 8_000)
            .await
            .unwrap()
    );
    assert!(db.owner_devices(&id, now + 8_000).await.unwrap().is_empty());
    assert!(
        db.trusted_device(&id, "tok-2", now + 8_000)
            .await
            .unwrap()
            .is_none()
    );
    for s in ["s1", "s2", "s7"] {
        let row = db.owner_session(s).await.unwrap().unwrap();
        assert!(row.revoked_at.is_some(), "{s}");
    }
    for s in ["s0", "s3", "s5"] {
        let row = db.owner_session(s).await.unwrap().unwrap();
        assert!(row.revoked_at.is_none(), "{s}");
    }

    challenge(db, "sc6", now + 8_500).await;
    challenge(db, "sc7", now + 8_600).await;
    let sc6 = db.signin_challenge("sc6").await.unwrap().unwrap();
    assert_eq!(sc6.consumed_at, Some(now + 8_600));
    assert!(
        db.signin_challenge("sc7")
            .await
            .unwrap()
            .unwrap()
            .consumed_at
            .is_none()
    );

    db.sign_out_everywhere(&id, now + 9_000).await.unwrap();
    for s in ["s0", "s1", "s2", "s3", "s5"] {
        let row = db.owner_session(s).await.unwrap().unwrap();
        assert!(row.revoked_at.is_some(), "{s}");
    }
    let sc7 = db.signin_challenge("sc7").await.unwrap().unwrap();
    assert_eq!(sc7.consumed_at, Some(now + 9_000));

    let other = NewOwner {
        name: "Ravi".into(),
        phone_e164: "+919811111111".into(),
        email: "ASHA.RAO@example.in".into(),
        cafe_name: "Other".into(),
        city: "Pune".into(),
    };
    db.insert_otp_challenge(&NewOtpChallenge {
        id: "c2".into(),
        phone_e164: other.phone_e164.clone(),
        code_hash: "h2".into(),
        payload: other.clone(),
        created_at: now,
        expires_at: now + 300_000,
    })
    .await
    .unwrap();
    db.complete_signup(&SignupCompletion {
        challenge_id: "c2",
        code_hash: "h2",
        max_attempts: 5,
        now: now + 1,
        new_owner_id: "o2",
        owner: &other,
        session: &session("s-o2", now + 1),
    })
    .await
    .unwrap();
    assert_eq!(
        db.owner_by_email("asha.rao@example.in")
            .await
            .unwrap()
            .map(|o| o.id),
        Some(id.clone())
    );
    assert!(
        db.set_owner_password("o2", "$argon2id$z", now + 2)
            .await
            .unwrap()
    );
    assert_eq!(
        db.owner_by_email("asha.rao@example.in").await.unwrap(),
        None
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn signin_queries_on_the_local_backend() {
    let dir = tempfile::tempdir().unwrap();
    let db = ControlDb::open_local(&dir.path().join("gz_control.db"))
        .await
        .unwrap();
    db.migrate(1).await.unwrap();
    exercise(&db).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn signin_queries_on_the_remote_backend() {
    let Some(t) = sqld::fresh().await else { return };
    let db = ControlDb::connect(&t.url, "").await.unwrap();
    db.migrate(1).await.unwrap();
    exercise(&db).await;
}
