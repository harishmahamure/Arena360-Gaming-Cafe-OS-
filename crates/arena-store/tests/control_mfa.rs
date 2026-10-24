//! The owner's password, TOTP secret and backup codes (Story 2.5) on both `gz_control`
//! backends: the local rusqlite file and a real libsql server (see `support/sqld.rs`).

#![cfg(feature = "control")]

#[path = "support/sqld.rs"]
mod sqld;

use arena_proto::MfaState;
use arena_store::control::{
    ControlDb, NewBackupCode, NewOtpChallenge, NewOwner, NewOwnerSession, SignupCompletion,
    SignupOutcome,
};

const PHONE: &str = "+919876543210";

/// Signs an owner up (pending) and answers their id.
async fn pending_owner(db: &ControlDb) -> String {
    let owner = NewOwner {
        name: "Asha Rao".into(),
        phone_e164: PHONE.into(),
        email: "asha@example.in".into(),
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
    let session = NewOwnerSession {
        id: "s1".into(),
        owner_id: String::new(),
        family_id: "f1".into(),
        access_hash: "a1".into(),
        access_expires_at: 900_000,
        refresh_hash: "r1".into(),
        refresh_expires_at: 9_000_000,
        created_at: 2_000,
        device_id: None,
    };
    let out = db
        .complete_signup(&SignupCompletion {
            challenge_id: "c1",
            code_hash: "h",
            max_attempts: 5,
            now: 2_000,
            new_owner_id: "o1",
            owner: &owner,
            session: &session,
        })
        .await
        .unwrap();
    assert!(matches!(out, SignupOutcome::Session(_)));
    "o1".to_owned()
}

fn codes(tag: &str) -> Vec<NewBackupCode> {
    (0..10)
        .map(|i| NewBackupCode {
            id: format!("{tag}{i:02}"),
            code_hash: format!("hash-{tag}-{i}"),
        })
        .collect()
}

async fn exercise(db: &ControlDb) {
    let id = pending_owner(db).await;
    let o = db.owner_by_id(&id).await.unwrap().unwrap();
    assert!(!o.has_password());
    assert_eq!(db.owner_password_hash(&id).await.unwrap(), None);

    // No TOTP before a password.
    assert!(!db.start_owner_mfa(&id, b"ct0", b"n0", 3_000).await.unwrap());
    assert_eq!(db.owner_mfa(&id).await.unwrap(), None);

    // The password is set once.
    assert!(
        db.set_owner_password(&id, "$argon2id$x", 4_000)
            .await
            .unwrap()
    );
    assert!(
        !db.set_owner_password(&id, "$argon2id$y", 4_500)
            .await
            .unwrap()
    );
    assert!(
        !db.set_owner_password("nobody", "$argon2id$y", 4_500)
            .await
            .unwrap()
    );
    let o = db.owner_by_id(&id).await.unwrap().unwrap();
    assert_eq!(o.password_set_at, Some(4_000));
    assert_eq!(
        db.owner_password_hash(&id).await.unwrap().as_deref(),
        Some("$argon2id$x")
    );

    // Each start replaces the unconfirmed secret.
    assert!(db.start_owner_mfa(&id, b"ct1", b"n1", 5_000).await.unwrap());
    assert!(db.start_owner_mfa(&id, b"ct2", b"n2", 6_000).await.unwrap());
    let m = db.owner_mfa(&id).await.unwrap().unwrap();
    assert_eq!(
        (
            m.secret_ct.as_slice(),
            m.secret_nonce.as_slice(),
            m.created_at
        ),
        (&b"ct2"[..], &b"n2"[..], 6_000)
    );
    assert_eq!(
        (m.confirmed_at, m.last_step, m.backup_ack_at),
        (None, None, None)
    );

    // Acking or regenerating needs a confirmed secret.
    assert!(!db.ack_backup_codes(&id, 6_500).await.unwrap());
    assert!(
        !db.regenerate_backup_codes(&id, 10, &codes("x"), 6_500)
            .await
            .unwrap()
    );

    // Confirming against a replaced secret changes nothing.
    assert!(
        !db.confirm_owner_mfa(&id, b"n1", 100, &codes("a"), 7_000)
            .await
            .unwrap()
    );
    assert!(db.backup_codes(&id).await.unwrap().is_empty());
    assert_eq!(
        db.owner_by_id(&id).await.unwrap().unwrap().mfa_state,
        MfaState::Pending
    );

    // Confirming the current one enrols the owner with 10 codes.
    assert!(
        db.confirm_owner_mfa(&id, b"n2", 100, &codes("a"), 7_000)
            .await
            .unwrap()
    );
    let m = db.owner_mfa(&id).await.unwrap().unwrap();
    assert_eq!((m.confirmed_at, m.last_step), (Some(7_000), Some(100)));
    assert_eq!(
        db.owner_by_id(&id).await.unwrap().unwrap().mfa_state,
        MfaState::Enrolled
    );
    let stored = db.backup_codes(&id).await.unwrap();
    assert_eq!(stored.len(), 10);
    assert!(
        stored
            .iter()
            .all(|c| c.code_hash.starts_with("hash-a-") && c.used_at.is_none())
    );

    // Once confirmed, the secret is never replaced or confirmed again.
    assert!(!db.start_owner_mfa(&id, b"ct3", b"n3", 8_000).await.unwrap());
    assert!(
        !db.confirm_owner_mfa(&id, b"n2", 101, &codes("b"), 8_000)
            .await
            .unwrap()
    );
    assert_eq!(
        db.owner_mfa(&id).await.unwrap().unwrap().secret_nonce,
        b"n2"
    );

    // Regenerating needs a later step and replaces every code.
    assert!(
        !db.regenerate_backup_codes(&id, 100, &codes("c"), 9_000)
            .await
            .unwrap()
    );
    assert!(
        !db.regenerate_backup_codes(&id, 99, &codes("c"), 9_000)
            .await
            .unwrap()
    );
    assert!(
        db.backup_codes(&id).await.unwrap()[0]
            .code_hash
            .starts_with("hash-a-")
    );
    assert!(
        db.regenerate_backup_codes(&id, 101, &codes("c"), 9_000)
            .await
            .unwrap()
    );
    let stored = db.backup_codes(&id).await.unwrap();
    assert_eq!(stored.len(), 10);
    assert!(
        stored
            .iter()
            .all(|c| c.code_hash.starts_with("hash-c-") && c.created_at == 9_000)
    );
    assert_eq!(
        db.owner_mfa(&id).await.unwrap().unwrap().last_step,
        Some(101)
    );

    // A failed insert rolls the whole regeneration back (a duplicate id).
    let mut dup = codes("d");
    dup[9].id = dup[0].id.clone();
    assert!(
        db.regenerate_backup_codes(&id, 102, &dup, 9_500)
            .await
            .is_err()
    );
    assert!(
        db.backup_codes(&id).await.unwrap()[0]
            .code_hash
            .starts_with("hash-c-")
    );
    assert_eq!(
        db.owner_mfa(&id).await.unwrap().unwrap().last_step,
        Some(101)
    );

    // The ack is recorded.
    assert!(db.ack_backup_codes(&id, 10_000).await.unwrap());
    assert_eq!(
        db.owner_mfa(&id).await.unwrap().unwrap().backup_ack_at,
        Some(10_000)
    );

    assert!(
        db.regenerate_backup_codes(&id, 103, &codes("e"), 11_000)
            .await
            .unwrap()
    );
    assert_eq!(
        db.owner_mfa(&id).await.unwrap().unwrap().backup_ack_at,
        None
    );
    assert!(db.ack_backup_codes(&id, 12_000).await.unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn mfa_queries_on_the_local_backend() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gz_control.db");
    let db = ControlDb::open_local(&path).await.unwrap();
    db.migrate(1).await.unwrap();
    exercise(&db).await;

    let id = db.owner_by_phone(PHONE).await.unwrap().unwrap().id;
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE owner_backup_codes SET used_at = 1 WHERE owner_id = ?",
            [&id],
        )
        .unwrap();
    assert!(!db.ack_backup_codes(&id, 13_000).await.unwrap());
    assert_eq!(
        db.owner_mfa(&id).await.unwrap().unwrap().backup_ack_at,
        Some(12_000)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn mfa_queries_on_the_remote_backend() {
    let Some(t) = sqld::fresh().await else { return };
    let db = ControlDb::connect(&t.url, "").await.unwrap();
    db.migrate(1).await.unwrap();
    exercise(&db).await;
}
