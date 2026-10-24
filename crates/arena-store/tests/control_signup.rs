//! Owner sign-up queries (Story 2.4) on both `gz_control` backends: the local rusqlite file
//! and a real libsql server (see `support/sqld.rs`).

#![cfg(feature = "control")]

#[path = "support/sqld.rs"]
mod sqld;

use arena_proto::MfaState;
use arena_store::control::{
    ControlDb, NewOtpChallenge, NewOwner, NewOwnerSession, REFRESH_RACE_MS, RateRule, RateWindow,
    RefreshOutcome, SignupCompletion, SignupOutcome,
};

const MAX: u32 = 5;

fn owner(phone: &str) -> NewOwner {
    NewOwner {
        name: "Asha Rao".into(),
        phone_e164: phone.into(),
        email: "asha@example.in".into(),
        cafe_name: "Level Up".into(),
        city: "Pune".into(),
    }
}

fn challenge(id: &str, phone: &str, hash: &str, now: i64) -> NewOtpChallenge {
    NewOtpChallenge {
        id: id.into(),
        phone_e164: phone.into(),
        code_hash: hash.into(),
        payload: owner(phone),
        created_at: now,
        expires_at: now + 300_000,
    }
}

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

async fn complete(
    db: &ControlDb,
    cid: &str,
    hash: &str,
    now: i64,
    owner_id: &str,
    phone: &str,
    sid: &str,
) -> SignupOutcome {
    let o = owner(phone);
    let s = session(sid, now);
    db.complete_signup(&SignupCompletion {
        challenge_id: cid,
        code_hash: hash,
        max_attempts: MAX,
        now,
        new_owner_id: owner_id,
        owner: &o,
        session: &s,
    })
    .await
    .unwrap()
}

/// Every behaviour, against one migrated database.
async fn exercise(db: &ControlDb) {
    let t0 = 1_700_000_000_000;
    let phone = "+919876543210";

    // A challenge creates no owner.
    db.insert_otp_challenge(&challenge("c1", phone, "h1", t0))
        .await
        .unwrap();
    assert_eq!(db.owner_count().await.unwrap(), 0);
    let c = db.otp_challenge("c1").await.unwrap().unwrap();
    assert_eq!(
        (c.attempts, c.consumed_at, c.expires_at),
        (0, None, t0 + 300_000)
    );
    assert_eq!(c.payload, Some(owner(phone)));
    assert_eq!(c.purpose, "signup");

    // Wrong codes count until the limit, then nothing more is counted; the voiding one clears
    // the stored form, and a void challenge can be neither completed nor reissued.
    db.insert_otp_challenge(&challenge("cv", phone, "hv", t0))
        .await
        .unwrap();
    for n in 1..MAX {
        assert_eq!(db.record_otp_failure("cv", MAX).await.unwrap(), Some(n));
        assert!(
            db.otp_challenge("cv")
                .await
                .unwrap()
                .unwrap()
                .payload
                .is_some()
        );
    }
    assert_eq!(db.record_otp_failure("cv", MAX).await.unwrap(), Some(MAX));
    assert_eq!(db.otp_challenge("cv").await.unwrap().unwrap().payload, None);
    assert_eq!(db.record_otp_failure("cv", MAX).await.unwrap(), None);
    assert_eq!(
        complete(db, "cv", "hv", t0 + 1, "o1", phone, "s1").await,
        SignupOutcome::NotConsumed
    );
    assert!(
        !db.reissue_otp_challenge("cv", t0 + 300_000, "hv2", t0 + 400_000)
            .await
            .unwrap()
    );

    // A resend resets attempts and expiry with a new hash, only if the expiry is still the
    // one the caller read; the old hash no longer completes.
    db.record_otp_failure("c1", MAX).await.unwrap();
    assert!(
        !db.reissue_otp_challenge("c1", t0 + 1, "h2", t0 + 400_000)
            .await
            .unwrap()
    );
    assert!(
        db.reissue_otp_challenge("c1", t0 + 300_000, "h2", t0 + 400_000)
            .await
            .unwrap()
    );
    assert!(
        !db.reissue_otp_challenge("c1", t0 + 300_000, "hx", t0 + 400_000)
            .await
            .unwrap(),
        "a second resend that read the same expiry loses"
    );
    assert_eq!(db.otp_challenge("c1").await.unwrap().unwrap().attempts, 0);
    assert_eq!(
        complete(db, "c1", "h1", t0 + 2, "o1", phone, "s1").await,
        SignupOutcome::NotConsumed
    );
    // A failed send undoes the reissue.
    let before = db.otp_challenge("c1").await.unwrap().unwrap();
    assert!(
        db.reissue_otp_challenge("c1", t0 + 400_000, "h3", t0 + 500_000)
            .await
            .unwrap()
    );
    db.revert_otp_reissue("c1", "h3", t0 + 500_000, &before)
        .await
        .unwrap();
    assert_eq!(db.otp_challenge("c1").await.unwrap().unwrap(), before);
    // Expired.
    assert_eq!(
        complete(db, "c1", "h2", t0 + 400_000, "o1", phone, "s1").await,
        SignupOutcome::NotConsumed
    );
    assert_eq!(db.owner_count().await.unwrap(), 0);

    // Correct: the owner (pending) and the session, the challenge consumed.
    let SignupOutcome::Session(o) = complete(db, "c1", "h2", t0 + 3, "o1", phone, "s1").await
    else {
        panic!("expected a session");
    };
    assert_eq!((o.id.as_str(), o.mfa_state), ("o1", MfaState::Pending));
    assert_eq!(o.name, "Asha Rao");
    assert_eq!(db.owner_count().await.unwrap(), 1);
    let consumed = db.otp_challenge("c1").await.unwrap().unwrap();
    assert_eq!(
        (consumed.consumed_at, consumed.payload),
        (Some(t0 + 3), None)
    );
    // Consumed: never again, not resendable, no failure counted.
    assert_eq!(
        complete(db, "c1", "h2", t0 + 4, "o9", phone, "s9").await,
        SignupOutcome::NotConsumed
    );
    assert!(
        !db.reissue_otp_challenge("c1", t0 + 400_000, "h3", t0 + 5)
            .await
            .unwrap()
    );
    assert_eq!(db.record_otp_failure("c1", MAX).await.unwrap(), None);
    assert!(db.owner_session("s9").await.unwrap().is_none());

    // A second sign-up for the pending owner: a new session for the same owner, no new row,
    // and the stored details are kept.
    let mut other = challenge("c2", phone, "k1", t0 + 10);
    other.payload.name = "Someone Else".into();
    db.insert_otp_challenge(&other).await.unwrap();
    let SignupOutcome::Session(o2) = db
        .complete_signup(&SignupCompletion {
            challenge_id: "c2",
            code_hash: "k1",
            max_attempts: MAX,
            now: t0 + 11,
            new_owner_id: "o2",
            owner: &other.payload,
            session: &session("s2", t0 + 11),
        })
        .await
        .unwrap()
    else {
        panic!("expected a session");
    };
    assert_eq!((o2.id.as_str(), o2.name.as_str()), ("o1", "Asha Rao"));
    assert_eq!(db.owner_count().await.unwrap(), 1);
    assert_eq!(
        db.owner_session("s2").await.unwrap().unwrap().owner_id,
        "o1"
    );

    // Enrolled: consumed, but no session.
    assert!(
        db.set_owner_mfa_state("o1", MfaState::Enrolled)
            .await
            .unwrap()
    );
    db.insert_otp_challenge(&challenge("c3", phone, "e1", t0 + 20))
        .await
        .unwrap();
    let SignupOutcome::Enrolled(o3) = complete(db, "c3", "e1", t0 + 21, "o3", phone, "s3").await
    else {
        panic!("expected enrolled");
    };
    assert_eq!(o3.id, "o1");
    assert!(db.owner_session("s3").await.unwrap().is_none());
    assert!(
        db.otp_challenge("c3")
            .await
            .unwrap()
            .unwrap()
            .consumed_at
            .is_some()
    );
    assert!(db.owner_by_phone(phone).await.unwrap().is_some());
    assert!(db.owner_by_id("o1").await.unwrap().is_some());

    // Sessions: lookup by access hash, rotation, reuse revokes the family.
    let live = db
        .owner_session_by_access("acc-s1", t0 + 100)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (live.session.id.as_str(), live.owner.id.as_str()),
        ("s1", "o1")
    );
    assert!(
        db.owner_session_by_access("acc-s1", t0 + 3 + 900_000)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.owner_session_by_access("nope", t0)
            .await
            .unwrap()
            .is_none()
    );

    let next = session("s1b", t0 + 200);
    assert_eq!(
        db.rotate_owner_session("ref-s1", &next, t0 + 200)
            .await
            .unwrap(),
        RefreshOutcome::Rotated(db.owner_by_id("o1").await.unwrap().unwrap())
    );
    let fam = db.owner_session_family("fam-s1").await.unwrap();
    assert_eq!(fam.len(), 2);
    assert_eq!(fam[0].rotated_at, Some(t0 + 200));
    assert_eq!(
        (fam[1].id.as_str(), fam[1].owner_id.as_str()),
        ("s1b", "o1")
    );
    assert!(
        db.owner_session_by_access("acc-s1b", t0 + 300)
            .await
            .unwrap()
            .is_some()
    );
    // The rotated session's own access token stops working at once.
    assert!(
        db.owner_session_by_access("acc-s1", t0 + 300)
            .await
            .unwrap()
            .is_none()
    );
    // The rotated token again within 10 s (a second tab): refused, nothing revoked.
    assert_eq!(
        db.rotate_owner_session("ref-s1", &session("s1r", t0 + 300), t0 + 300)
            .await
            .unwrap(),
        RefreshOutcome::RecentlyRotated
    );
    assert!(db.owner_session("s1r").await.unwrap().is_none());
    assert!(
        db.owner_session_by_access("acc-s1b", t0 + 300)
            .await
            .unwrap()
            .is_some()
    );
    // Reuse of the rotated token later: the family is revoked, the new access fails.
    let t1 = t0 + 200 + REFRESH_RACE_MS;
    assert_eq!(
        db.rotate_owner_session("ref-s1", &session("s1c", t1), t1)
            .await
            .unwrap(),
        RefreshOutcome::Reused
    );
    assert!(db.owner_session("s1c").await.unwrap().is_none());
    assert!(
        db.owner_session_by_access("acc-s1b", t1 + 1)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.owner_session_family("fam-s1")
            .await
            .unwrap()
            .iter()
            .all(|s| s.revoked_at == Some(t1))
    );
    // The revoked newer token refreshes nothing; an unknown one neither.
    assert_eq!(
        db.rotate_owner_session("ref-s1b", &session("s1d", t1 + 100), t1 + 100)
            .await
            .unwrap(),
        RefreshOutcome::Invalid
    );
    assert_eq!(
        db.rotate_owner_session("nope", &session("s1e", t1 + 100), t1 + 100)
            .await
            .unwrap(),
        RefreshOutcome::Invalid
    );
    // Another family is untouched.
    assert!(
        db.owner_session_by_access("acc-s2", t1 + 100)
            .await
            .unwrap()
            .is_some()
    );
    // An expired refresh token is invalid.
    assert_eq!(
        db.rotate_owner_session("ref-s2", &session("s2b", t0), t0 + 31 * 86_400_000)
            .await
            .unwrap(),
        RefreshOutcome::Invalid
    );

    // Rate limits: 5 per rolling hour in minute buckets, reserved atomically.
    let m = 60_000;
    let h = 60 * m;
    let rule = |key| RateRule {
        key,
        prune_prefix: "otp_send:",
        bucket_ms: m,
        window_ms: h,
        limit: 5,
    };
    let key = "otp_send:+919876543210";
    let base = 1_000 * h;
    assert_eq!(db.rate_window(key, 0).await.unwrap(), RateWindow::default());
    // Sends at 10:30 (bucket 10:00), 12:00, 12:59.9, 13:00, 40:00 into the hour.
    for at in [10 * m + 30_000, 12 * m, 13 * m - 100, 13 * m, 40 * m] {
        assert!(
            db.rate_reserve(&[rule(key)], base + at)
                .await
                .unwrap()
                .is_ok()
        );
    }
    assert_eq!(
        db.rate_buckets(key).await.unwrap(),
        vec![
            (base + 10 * m, 1),
            (base + 12 * m, 2),
            (base + 13 * m, 1),
            (base + 40 * m, 1)
        ]
    );
    // The 6th at 50:00 is refused until the 10:00 bucket ends an hour later (71:00): 21 min,
    // counted from the bucket end, not its start.
    let now = base + 50 * m;
    assert_eq!(
        db.rate_reserve(&[rule(key)], now).await.unwrap(),
        Err(21 * m)
    );
    assert_eq!(
        db.rate_retry_after_ms(&rule(key), now).await.unwrap(),
        21 * m
    );
    // Still counted 30 s before the bucket leaves, free right after.
    assert!(
        db.rate_reserve(&[rule(key)], base + 71 * m - 30_000)
            .await
            .unwrap()
            .is_err()
    );
    let r = db
        .rate_reserve(&[rule(key)], base + 71 * m)
        .await
        .unwrap()
        .unwrap();
    // A released reservation frees its slot.
    assert!(
        db.rate_reserve(&[rule(key)], base + 71 * m)
            .await
            .unwrap()
            .is_err()
    );
    db.rate_release(&r).await.unwrap();
    assert!(
        db.rate_reserve(&[rule(key)], base + 71 * m)
            .await
            .unwrap()
            .is_ok()
    );
    // All or nothing across rules: a full second rule counts nothing in the first.
    let other = rule("otp_send:+14155550100");
    let before = db.rate_buckets(other.key).await.unwrap();
    assert!(
        db.rate_reserve(&[other, rule(key)], base + 72 * m)
            .await
            .unwrap()
            .is_err()
    );
    assert_eq!(db.rate_buckets(other.key).await.unwrap(), before);
    // Pruning: only this limiter's keys, only buckets older than two windows.
    let foreign = RateRule {
        key: "login:x",
        prune_prefix: "login:",
        bucket_ms: m,
        window_ms: 24 * h,
        limit: 100,
    };
    db.rate_reserve(&[foreign], base).await.unwrap().unwrap();
    db.rate_reserve(&[rule(key)], base + 10 * h)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        db.rate_buckets(key).await.unwrap(),
        vec![(base + 10 * h, 1)]
    );
    assert_eq!(db.rate_buckets("login:x").await.unwrap(), vec![(base, 1)]);
}

/// Old challenges and sessions are pruned when new ones are written.
async fn pruning(db: &ControlDb) {
    let t0 = 1_700_000_000_000;
    let day = 86_400_000;
    let phone = "+919876543210";
    db.insert_otp_challenge(&challenge("old", phone, "h", t0))
        .await
        .unwrap();
    db.insert_otp_challenge(&challenge("recent", phone, "h", t0 + day))
        .await
        .unwrap();
    // `old` expired at t0 + 5 min: just over 24 h before this insert.
    db.insert_otp_challenge(&challenge("new", phone, "h", t0 + 300_001 + day))
        .await
        .unwrap();
    assert!(db.otp_challenge("old").await.unwrap().is_none());
    assert!(db.otp_challenge("recent").await.unwrap().is_some());
    assert!(db.otp_challenge("new").await.unwrap().is_some());

    let SignupOutcome::Session(o) =
        complete(db, "new", "h", t0 + 300_002 + day, "o1", phone, "s1").await
    else {
        panic!("expected a session");
    };
    let mut s = session("s2", t0);
    s.owner_id = o.id.clone();
    s.refresh_expires_at = t0 + 10;
    db.insert_owner_session(&s).await.unwrap();
    let mut t = session("s3", t0 + 10 + day);
    t.owner_id = o.id.clone();
    db.insert_owner_session(&t).await.unwrap();
    assert!(
        db.owner_session("s2").await.unwrap().is_some(),
        "exactly a day: kept"
    );
    let mut u = session("s4", t0 + 11 + day);
    u.owner_id = o.id.clone();
    db.insert_owner_session(&u).await.unwrap();
    assert!(
        db.owner_session("s2").await.unwrap().is_none(),
        "over a day: pruned"
    );
    assert!(db.owner_session("s1").await.unwrap().is_some());
    assert!(db.owner_session("s3").await.unwrap().is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn old_challenges_and_sessions_are_pruned_locally() {
    let dir = tempfile::tempdir().unwrap();
    let db = ControlDb::open_local(&dir.path().join("gz_control.db"))
        .await
        .unwrap();
    db.migrate(1).await.unwrap();
    pruning(&db).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn old_challenges_and_sessions_are_pruned_remotely() {
    let Some(t) = sqld::fresh().await else { return };
    let db = ControlDb::connect(&t.url, "").await.unwrap();
    db.migrate(1).await.unwrap();
    pruning(&db).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn signup_queries_on_the_local_backend() {
    let dir = tempfile::tempdir().unwrap();
    let db = ControlDb::open_local(&dir.path().join("gz_control.db"))
        .await
        .unwrap();
    db.migrate(1).await.unwrap();
    exercise(&db).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn signup_queries_on_the_remote_backend() {
    let Some(t) = sqld::fresh().await else { return };
    let db = ControlDb::connect(&t.url, "").await.unwrap();
    db.migrate(1).await.unwrap();
    exercise(&db).await;
}

/// Two verifications of the same correct code at once: exactly one consumes it.
async fn race(db: std::sync::Arc<ControlDb>) {
    let phone = "+14155550100";
    db.insert_otp_challenge(&challenge("r1", phone, "h", 1_000))
        .await
        .unwrap();
    let mut tasks = Vec::new();
    for i in 0..4 {
        let db = std::sync::Arc::clone(&db);
        tasks.push(tokio::spawn(async move {
            let o = owner(phone);
            let s = session(&format!("rs{i}"), 2_000);
            db.complete_signup(&SignupCompletion {
                challenge_id: "r1",
                code_hash: "h",
                max_attempts: MAX,
                now: 2_000,
                new_owner_id: &format!("ro{i}"),
                owner: &o,
                session: &s,
            })
            .await
            .unwrap()
        }));
    }
    let mut sessions = 0;
    for t in tasks {
        if matches!(t.await.unwrap(), SignupOutcome::Session(_)) {
            sessions += 1;
        }
    }
    assert_eq!(sessions, 1);
    assert_eq!(db.owner_count().await.unwrap(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_code_verified_twice_at_once_is_consumed_once_locally() {
    let dir = tempfile::tempdir().unwrap();
    let db = ControlDb::open_local(&dir.path().join("gz_control.db"))
        .await
        .unwrap();
    db.migrate(1).await.unwrap();
    race(std::sync::Arc::new(db)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_code_verified_twice_at_once_is_consumed_once_remotely() {
    let Some(t) = sqld::fresh().await else { return };
    let db = ControlDb::connect(&t.url, "").await.unwrap();
    db.migrate(1).await.unwrap();
    race(std::sync::Arc::new(db)).await;
}
