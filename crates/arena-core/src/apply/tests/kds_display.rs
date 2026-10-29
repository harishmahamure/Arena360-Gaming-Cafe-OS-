//! Story 7.6: a kitchen display pairs with `kds.pair`, is revoked with `kds.revoke`, shows its
//! last-seen through `kds.seen` and takes a high-contrast setting through `settings.set`.

use super::kitchen_ticket::flags;
use super::prepaid::system;
use super::session::{int, refusal, submit, table, text};
use super::stock::{intent, owner, staff};
use super::*;
use crate::devices::{DeviceDirectory, format_token, hash_token_secret, new_paired_token_secret};
use crate::rbac::{ROLE_COUNTER, ROLE_MANAGER};
use arena_proto::{
    ClientClass, KDS_LIMIT_MESSAGE, KDS_MAX_DISPLAYS, KdsPair, KdsRevoke, KdsSeen, SettingsSet,
};

fn pair_payload(name: &str, by: &str) -> IntentPayload {
    IntentPayload::KdsPair(KdsPair {
        name: name.into(),
        token_hash: hash_token_secret("a secret").unwrap(),
        created_by: by.into(),
    })
}

fn displays(f: &Fixture) -> Vec<Row> {
    table(f, Table::KitchenDisplays)
}

/// A Basic branch and its Manager.
async fn basic_with_manager(f: &Fixture) -> Actor {
    flags(f, "kitchen").await;
    staff(f, "mgr", ROLE_MANAGER).await
}

/// Pairs a display as the system actor and returns its id.
async fn pair(f: &Fixture, name: &str, by: &str) -> String {
    let applied = submit(f, system(), pair_payload(name, by)).await.unwrap();
    let change = &applied.changes[0];
    assert_eq!(change.tbl, "kitchen_displays");
    change.row_id.clone()
}

async fn revoke(f: &Fixture, actor: Actor, id: &str) -> Result<Applied, AppError> {
    submit(
        f,
        actor,
        IntentPayload::KdsRevoke(KdsRevoke {
            display_id: id.into(),
        }),
    )
    .await
}

fn hc_key(id: &str) -> String {
    format!("display.hc.{id}")
}

async fn set_hc(
    f: &Fixture,
    actor: Actor,
    id: &str,
    value: &str,
    base_rev: Option<i64>,
) -> Result<Applied, AppError> {
    let mut i = intent(
        actor,
        IntentPayload::SettingsSet(SettingsSet {
            key: hc_key(id),
            value: value.into(),
        }),
    );
    i.base_rev = base_rev;
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

#[tokio::test]
async fn pairing_inserts_a_row_with_an_argon2id_hash_and_a_change_row() {
    let f = bootstrapped().await;
    let manager = basic_with_manager(&f).await;
    let before = table(&f, Table::Devices).len();
    let id = pair(&f, " Pass ".trim(), &manager.id).await;

    let rows = displays(&f);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(text(row, "id"), id);
    assert_eq!(text(row, "name"), "Pass");
    assert!(text(row, "token_hash").starts_with("$argon2id$"));
    assert_eq!(text(row, "created_by"), manager.id);
    assert_eq!(int(row, "high_contrast"), 0);
    assert_eq!(int(row, "rev"), 1);
    assert_eq!(row["revoked_at"], RowValue::Null);
    // Not a device: the devices table, the device cap and the HC settings are untouched.
    assert_eq!(table(&f, Table::Devices).len(), before);
    assert!(
        table(&f, Table::Settings)
            .iter()
            .all(|s| !text(s, "key").starts_with(&hc_key(&id)))
    );
}

#[tokio::test]
async fn pairing_is_refused_on_free_and_writes_nothing() {
    let f = bootstrapped().await;
    let manager = staff(&f, "mgr", ROLE_MANAGER).await;
    let e = submit(&f, system(), pair_payload("Pass", &manager.id))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::TierLocked);
    assert!(displays(&f).is_empty());
}

#[tokio::test]
async fn only_the_system_actor_pairs_and_the_row_is_checked() {
    let f = bootstrapped().await;
    let manager = basic_with_manager(&f).await;
    for actor in [owner(), manager.clone()] {
        let e = submit(&f, actor, pair_payload("Pass", &manager.id))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::PermissionDenied);
    }
    for (payload, field) in [
        (pair_payload("", &manager.id), "name"),
        (pair_payload("Line\nbreak", &manager.id), "name"),
        (pair_payload(&"x".repeat(41), &manager.id), "name"),
        (pair_payload("Pass", "no-such-staff"), "created_by"),
        (
            IntentPayload::KdsPair(KdsPair {
                name: "Pass".into(),
                token_hash: "plain".into(),
                created_by: manager.id.clone(),
            }),
            "token_hash",
        ),
    ] {
        assert_eq!(
            refusal(submit(&f, system(), payload).await),
            (ErrorCode::ValidationFailed, Some(field.into()))
        );
    }
    assert!(displays(&f).is_empty());
}

#[tokio::test]
async fn a_fifth_display_is_refused_until_one_is_revoked() {
    let f = bootstrapped().await;
    let manager = basic_with_manager(&f).await;
    let mut ids = Vec::new();
    for n in 0..KDS_MAX_DISPLAYS {
        ids.push(pair(&f, &format!("Screen {n}"), &manager.id).await);
    }
    let e = submit(&f, system(), pair_payload("Fifth", &manager.id))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::DeviceLimit);
    assert_eq!(e.message, KDS_LIMIT_MESSAGE);
    assert_eq!(displays(&f).len(), 4);

    revoke(&f, manager.clone(), &ids[0]).await.unwrap();
    pair(&f, "Fifth", &manager.id).await;
    assert_eq!(displays(&f).len(), 5);
}

#[tokio::test]
async fn displays_do_not_count_against_the_device_cap() {
    let f = bootstrapped().await;
    let manager = basic_with_manager(&f).await;
    for n in 0..KDS_MAX_DISPLAYS {
        pair(&f, &format!("Screen {n}"), &manager.id).await;
    }
    // The cap counts `devices_meta` rows, which a display never has.
    assert!(table(&f, Table::DevicesMeta).is_empty());
    assert!(table(&f, Table::Devices).is_empty());
}

#[tokio::test]
async fn revoke_needs_device_manage_and_works_after_a_lapse() {
    let f = bootstrapped().await;
    let manager = basic_with_manager(&f).await;
    let counter = staff(&f, "cnt", ROLE_COUNTER).await;
    let id = pair(&f, "Pass", &manager.id).await;

    assert_eq!(
        revoke(&f, counter, &id).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        revoke(&f, system(), &id).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    let missing = revoke(&f, manager.clone(), "01JNOPEAAAAAAAAAAAAAAAAAAA").await;
    assert_eq!(refusal(missing).0, ErrorCode::ValidationFailed);

    // A lapse to Free still lets a Manager revoke (existing work).
    flags(&f, "").await;
    revoke(&f, manager.clone(), &id).await.unwrap();
    let row = &displays(&f)[0];
    assert!(matches!(row["revoked_at"], RowValue::Integer(_)));
    assert_eq!(int(row, "rev"), 2);

    let again = revoke(&f, manager, &id).await;
    assert_eq!(
        refusal(again),
        (ErrorCode::ValidationFailed, Some("display_id".into()))
    );
}

#[tokio::test]
async fn seen_sets_last_seen_for_the_system_actor_only() {
    let f = bootstrapped().await;
    let manager = basic_with_manager(&f).await;
    let id = pair(&f, "Pass", &manager.id).await;
    assert_eq!(displays(&f)[0]["last_seen"], RowValue::Null);

    let seen = |actor: Actor| {
        let id = id.clone();
        let f = &f;
        async move { submit(f, actor, IntentPayload::KdsSeen(KdsSeen { display_id: id })).await }
    };
    assert_eq!(
        seen(manager.clone()).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    f.clock.advance(5_000);
    seen(system()).await.unwrap();
    assert_eq!(int(&displays(&f)[0], "last_seen"), f.clock.now_ms());

    revoke(&f, manager, &id).await.unwrap();
    assert_eq!(refusal(seen(system()).await).0, ErrorCode::ValidationFailed);
}

#[tokio::test]
async fn high_contrast_writes_the_column_through_settings_set() {
    let f = bootstrapped().await;
    let manager = basic_with_manager(&f).await;
    let counter = staff(&f, "cnt", ROLE_COUNTER).await;
    let id = pair(&f, "Pass", &manager.id).await;
    let settings_before = table(&f, Table::Settings).len();

    // base_rev is required, and a stale one is refused.
    assert_eq!(
        refusal(set_hc(&f, manager.clone(), &id, "true", None).await).0,
        ErrorCode::ValidationFailed
    );
    assert_eq!(
        refusal(set_hc(&f, manager.clone(), &id, "true", Some(9)).await).0,
        ErrorCode::Stale
    );
    assert_eq!(
        refusal(set_hc(&f, manager.clone(), &id, "yes", Some(1)).await).0,
        ErrorCode::ValidationFailed
    );
    // A Counter holds no `settings.edit`.
    assert_eq!(
        set_hc(&f, counter, &id, "true", Some(1))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );

    let applied = set_hc(&f, manager.clone(), &id, "true", Some(1))
        .await
        .unwrap();
    assert_eq!(applied.changes.len(), 1);
    assert_eq!(applied.changes[0].tbl, "kitchen_displays");
    assert_eq!(applied.changes[0].row_id, id);
    let row = &displays(&f)[0];
    assert_eq!(int(row, "high_contrast"), 1);
    assert_eq!(int(row, "rev"), 2);
    // No settings row is written or required for a display.
    assert_eq!(table(&f, Table::Settings).len(), settings_before);

    set_hc(&f, manager.clone(), &id, "false", Some(2))
        .await
        .unwrap();
    assert_eq!(int(&displays(&f)[0], "high_contrast"), 0);

    // An unknown display id is still not a setting.
    let unknown = set_hc(&f, manager, "01JNOPEAAAAAAAAAAAAAAAAAAA", "true", Some(0)).await;
    assert_eq!(
        refusal(unknown),
        (ErrorCode::ValidationFailed, Some("key".into()))
    );
}

#[tokio::test]
async fn a_display_token_verifies_against_kitchen_displays_only() {
    let f = bootstrapped().await;
    let manager = basic_with_manager(&f).await;
    let secret = new_paired_token_secret().unwrap();
    let applied = submit(
        &f,
        system(),
        IntentPayload::KdsPair(KdsPair {
            name: "Pass".into(),
            token_hash: hash_token_secret(&secret).unwrap(),
            created_by: manager.id.clone(),
        }),
    )
    .await
    .unwrap();
    let id = applied.changes[0].row_id.clone();
    let token = format_token(&id, &secret);
    let dir = DeviceDirectory::open(&f.path).unwrap();

    let ok = dir.verify(ClientClass::Kds, &token).unwrap();
    assert_eq!(
        (ok.device_id.as_str(), ok.class),
        (id.as_str(), ClientClass::Kds)
    );
    // A kds token is not a paired-device token, and the wrong secret is invalid.
    assert_eq!(
        dir.verify(ClientClass::Paired, &token).unwrap_err().code,
        ErrorCode::TokenInvalid
    );
    let wrong = format_token(&id, &new_paired_token_secret().unwrap());
    assert_eq!(
        dir.verify(ClientClass::Kds, &wrong).unwrap_err().code,
        ErrorCode::TokenInvalid
    );
    let unknown = format_token("01JNOPEAAAAAAAAAAAAAAAAAAA", &secret);
    assert_eq!(
        dir.verify(ClientClass::Kds, &unknown).unwrap_err().code,
        ErrorCode::TokenInvalid
    );

    revoke(&f, manager, &id).await.unwrap();
    assert_eq!(
        dir.verify(ClientClass::Kds, &token).unwrap_err().code,
        ErrorCode::TokenRevoked
    );
}

#[tokio::test]
async fn a_live_display_may_act_and_a_revoked_one_may_not() {
    let f = bootstrapped().await;
    let manager = basic_with_manager(&f).await;
    let id = pair(&f, "Pass", &manager.id).await;
    let display = Actor {
        kind: ActorKind::Device,
        id: id.clone(),
    };
    // It passes the permission step (a missing ticket is the refusal that follows).
    let advance = |actor: Actor| {
        submit(
            &f,
            actor,
            IntentPayload::KitchenAdvance(arena_proto::KitchenAdvance {
                ticket_id: "01JNOPEAAAAAAAAAAAAAAAAAAA".into(),
                to: "preparing".into(),
            }),
        )
    };
    assert_ne!(
        advance(display.clone()).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    revoke(&f, manager, &id).await.unwrap();
    assert_eq!(
        advance(display).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
}
