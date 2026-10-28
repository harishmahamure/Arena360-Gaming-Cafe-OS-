//! Story 6.8: what the Hub reads to check a kiosk login, and the footer fields of the attract view.

use super::players::{add, form};
use super::session::{end, end_intent, pc, sessions, start, submit, text};
use super::stock::owner;
use super::*;
use crate::attract::{GameMediaHashes, attract_views};
use crate::devices::DeviceDirectory;
use arena_proto::{DeviceKind, DevicePlace, ZoneUpsert};

fn none(_: &str) -> GameMediaHashes {
    GameMediaHashes::default()
}

fn directory(f: &Fixture) -> DeviceDirectory {
    DeviceDirectory::open(&f.path).unwrap()
}

#[tokio::test]
async fn an_unknown_username_has_no_candidate_and_a_known_one_carries_its_hash_and_key() {
    let f = bootstrapped().await;
    let (id, _) = add(&f, owner(), &form("Asha_9", "+919876543210"))
        .await
        .unwrap();
    let dir = directory(&f);
    assert_eq!(dir.login_candidate("nobody").unwrap(), None);
    // The caller normalises: the stored form is lower case.
    assert_eq!(dir.login_candidate("Asha_9").unwrap(), None);
    let c = dir.login_candidate("asha_9").unwrap().unwrap();
    assert_eq!(c.player_id, id);
    assert!(c.password_hash.starts_with("$argon2id$"));
    assert!(!c.wrapped_key.is_empty() && !c.name_enc.is_empty());
    assert_eq!(c.busy_device_name, None);
    let shown = format!("{c:?}");
    assert!(!shown.contains(&c.password_hash) && !shown.contains(&c.wrapped_key));
}

#[tokio::test]
async fn a_live_session_elsewhere_names_that_pc_until_it_ends() {
    let f = bootstrapped().await;
    let (player, _) = add(&f, owner(), &form("asha", "+919876543210"))
        .await
        .unwrap();
    let d = pc(&f, DeviceKind::Pc, "pc", Some(12_000)).await;
    let applied = start(&f, &d, Some(&player)).await.unwrap();
    let session = applied
        .changes
        .iter()
        .find(|c| c.tbl == "sessions")
        .unwrap()
        .row_id
        .clone();
    let label = text(
        reopen(&f)
            .rows(Table::DevicesMeta)
            .unwrap()
            .iter()
            .find(|r| text(r, "device_id") == d)
            .unwrap(),
        "label",
    );
    let dir = directory(&f);
    let c = dir.login_candidate("asha").unwrap().unwrap();
    assert_eq!(c.busy_device_name, Some(label));
    let rev = sessions(&f)
        .iter()
        .find(|r| text(r, "id") == session)
        .map(|r| match &r["rev"] {
            RowValue::Integer(n) => *n,
            other => panic!("{other:?}"),
        });
    end(&f, end_intent(&session, rev)).await.unwrap();
    assert_eq!(
        dir.login_candidate("asha")
            .unwrap()
            .unwrap()
            .busy_device_name,
        None,
        "an ended session no longer holds the player"
    );
}

#[tokio::test]
async fn an_imported_player_has_the_placeholder_and_never_verifies() {
    let f = bootstrapped().await;
    let id = ulid::Ulid::generate().to_string();
    let kek = [9u8; crate::players::KEK_LEN];
    let row =
        crate::players::seal_import(&kek, &id, "ravi", "+919000000001", "Ravi", 0, 0).unwrap();
    submit(
        &f,
        owner(),
        IntentPayload::ImportPlayers(arena_proto::ImportPlayers { rows: vec![row] }),
    )
    .await
    .unwrap();
    let c = directory(&f).login_candidate("ravi").unwrap().unwrap();
    assert_eq!(c.password_hash, arena_proto::IMPORT_NO_PASSWORD);
    assert!(!crate::players::check_password("!", Some(&c.password_hash)));
}

async fn zone_named(f: &Fixture, name: &str) -> String {
    submit(
        f,
        owner(),
        IntentPayload::ZoneUpsert(ZoneUpsert {
            id: None,
            name: name.into(),
            sort_order: 1,
        }),
    )
    .await
    .unwrap()
    .changes[0]
        .row_id
        .clone()
}

#[tokio::test]
async fn the_attract_view_names_the_pc_and_its_zone() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(12_000)).await;
    let ids = [d.clone()];
    let view = |f: &Fixture| {
        attract_views(&reopen(f), &ids, 0, &none)
            .unwrap()
            .remove(0)
            .1
    };
    let label = text(
        reopen(&f)
            .rows(Table::DevicesMeta)
            .unwrap()
            .iter()
            .find(|r| text(r, "device_id") == d)
            .unwrap(),
        "label",
    );
    let v = view(&f);
    assert_eq!(
        (v.device_name.as_str(), v.zone.as_str()),
        (label.as_str(), "")
    );
    let z = zone_named(&f, "Main hall").await;
    submit(
        &f,
        owner(),
        IntentPayload::DevicePlace(DevicePlace {
            device_id: d.clone(),
            zone_id: Some(z.clone()),
            floor_x: Some(0),
            floor_y: Some(0),
        }),
    )
    .await
    .unwrap();
    assert_eq!(view(&f).zone, "Main hall");
    // A device with no rate still names itself (the screen is "Ask at the counter").
    let bare = pc(&f, DeviceKind::Pc, "pc", None).await;
    let v = attract_views(&reopen(&f), &[bare], 0, &none)
        .unwrap()
        .remove(0)
        .1;
    assert!(!v.device_name.is_empty());
}
