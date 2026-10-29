//! Story 6.10: a paired device applies `player.create` for a kiosk sign-up, and nothing else about
//! players.

use super::players::{add, create, form};
use super::session::pc;
use super::stock::{intent, owner};
use super::*;
use arena_proto::{DeviceKind, PLAYER_POLICY_VERSION, PlayerPasswordReset};

fn device(id: &str) -> Actor {
    Actor {
        kind: ActorKind::Device,
        id: id.into(),
    }
}

fn counts(f: &Fixture) -> (usize, usize, usize, usize) {
    let s = reopen(f);
    (
        s.rows(Table::Players).unwrap().len(),
        s.rows(Table::Consents).unwrap().len(),
        s.rows(Table::Wallets).unwrap().len(),
        s.changes().unwrap().len(),
    )
}

#[tokio::test]
async fn a_device_actor_creates_a_profile_with_consent_and_the_log_shows_the_device() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let before = counts(&f);
    let mut kiosk_form = form("Asha_9", "+91 98765-43210");
    kiosk_form.policy_version = PLAYER_POLICY_VERSION.into();
    let (id, applied) = add(&f, device(&d), &kiosk_form).await.unwrap();
    assert_eq!(
        applied
            .changes
            .iter()
            .map(|c| c.tbl.as_str())
            .collect::<Vec<_>>(),
        ["players", "consents", "wallets"]
    );
    let after = counts(&f);
    assert_eq!(
        (after.0, after.1, after.2, after.3),
        (before.0 + 1, before.1 + 1, before.2 + 1, before.3 + 3)
    );
    let store = reopen(&f);
    let consent = store.rows(Table::Consents).unwrap().pop().unwrap();
    assert_eq!(consent["player_id"], RowValue::Text(id.clone()));
    assert_eq!(consent["granted"], RowValue::Integer(1));
    assert_eq!(
        consent["policy_version"],
        RowValue::Text(PLAYER_POLICY_VERSION.into())
    );
    let changes = store.changes().unwrap();
    let player_change = changes.iter().rev().find(|c| c.tbl == "players").unwrap();
    assert_eq!(
        (
            player_change.actor_kind.as_str(),
            player_change.actor_id.as_str(),
            player_change.row_id.as_str()
        ),
        ("device", d.as_str(), id.as_str())
    );
    // The log carries ciphertext and a phone hash only.
    let log: Vec<u8> = changes.into_iter().flat_map(|c| c.row).collect();
    let log = String::from_utf8_lossy(&log).into_owned();
    for plain in ["Asha Rao", "9876543210", "secret1"] {
        assert!(!log.contains(plain), "{plain}");
    }
}

#[tokio::test]
async fn a_device_actor_may_not_create_a_profile_without_consent() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let before = counts(&f);
    let mut no_consent = form("asha", "+919876543210");
    no_consent.consent = false;
    let e = add(&f, device(&d), &no_consent).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    assert_eq!(counts(&f), before);
    // The same form from the counter (owner) is still allowed: only a device needs the box.
    add(&f, owner(), &no_consent).await.unwrap();
}

#[tokio::test]
async fn a_device_actor_holds_no_other_player_right() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let (id, _) = add(&f, owner(), &form("asha", "+919876543210"))
        .await
        .unwrap();
    let before = counts(&f);
    let (_, IntentPayload::PlayerCreate(sealed)) = create(&form("ravi", "+919000000001")) else {
        unreachable!()
    };
    let update = arena_proto::PlayerUpdate {
        player_id: id.clone(),
        phone_lookup: sealed.phone_lookup.clone(),
        name_enc: sealed.name_enc.clone(),
        phone_enc: sealed.phone_enc.clone(),
        email_enc: None,
        dob_enc: None,
        notes_enc: None,
        tags: Vec::new(),
        consent: true,
        policy_version: "1".into(),
    };
    let reset = PlayerPasswordReset {
        player_id: id,
        password_hash: "$argon2id$stub".into(),
    };
    for payload in [
        IntentPayload::PlayerUpdate(update),
        IntentPayload::PlayerPasswordReset(reset),
        IntentPayload::ImportPlayers(arena_proto::ImportPlayers { rows: Vec::new() }),
    ] {
        let mut i = intent(device(&d), payload);
        i.base_rev = Some(1);
        let e = f.writer.submit_intent(i).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::PermissionDenied);
    }
    assert_eq!(counts(&f), before);
}

#[tokio::test]
async fn a_device_actor_sign_up_with_a_taken_username_or_phone_writes_nothing() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    add(&f, owner(), &form("asha", "+91 98765 43210"))
        .await
        .unwrap();
    let before = counts(&f);
    let e = add(&f, device(&d), &form("ASHA", "+919000000002"))
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::UsernameTaken, Some("username"))
    );
    let e = add(&f, device(&d), &form("other", "+91-98765-43210"))
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::PhoneTaken, Some("phone"))
    );
    assert_eq!(counts(&f), before);
}
