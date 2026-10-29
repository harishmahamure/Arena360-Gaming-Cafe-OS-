//! Story 6.35: a paired device saves PC settings for the player on its own PC and no one else's;
//! the branch's `kiosk.display_modes` list is a validated setting.

use super::prepaid::wallet_session;
use super::session::{pc, player, submit, topped_up};
use super::stock::owner;
use super::*;
use arena_proto::{DeviceKind, PcSettings, PlayerPcSettingsSet};

fn device(id: &str) -> Actor {
    Actor {
        kind: ActorKind::Device,
        id: id.into(),
    }
}

fn save(player: &str, settings: PcSettings) -> IntentPayload {
    IntentPayload::PlayerPcSettingsSet(PlayerPcSettingsSet {
        player_id: player.into(),
        settings,
    })
}

fn mouse(speed: u8) -> PcSettings {
    PcSettings {
        mouse_speed: Some(speed),
        ..Default::default()
    }
}

fn stored(f: &Fixture, player: &str) -> Option<PcSettings> {
    reopen(f).pc_settings(player).unwrap()
}

#[tokio::test]
async fn a_device_saves_settings_for_the_player_on_its_own_pc_and_no_one_elses() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let other = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let asha = player(&f, "asha", "+919876543210").await;
    let ravi = player(&f, "ravi", "+919000000001").await;
    topped_up(&f, &asha, 10_000).await;
    // No session on the PC yet: denied.
    let e = submit(&f, device(&d), save(&asha, mouse(8)))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    wallet_session(&f, &d, &asha).await;
    // Another PC's device, and another player's settings, are denied.
    for (actor, target) in [(&other, &asha), (&d, &ravi)] {
        let e = submit(&f, device(actor), save(target, mouse(8)))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::PermissionDenied);
    }
    assert_eq!(stored(&f, &asha), None);
    assert_eq!(stored(&f, &ravi), None);

    let applied = submit(&f, device(&d), save(&asha, mouse(8))).await.unwrap();
    assert_eq!(applied.changes.len(), 1);
    assert_eq!(stored(&f, &asha), Some(mouse(8)));
    let logged = reopen(&f)
        .changes()
        .unwrap()
        .into_iter()
        .rev()
        .find(|c| c.tbl == "player_pc_settings")
        .unwrap();
    assert_eq!(logged.row_id, asha);
    assert_eq!(
        (logged.actor_kind.as_str(), logged.actor_id.as_str()),
        ("device", d.as_str())
    );
}

#[tokio::test]
async fn a_later_save_changes_only_what_it_chose() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let asha = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &asha, 10_000).await;
    wallet_session(&f, &d, &asha).await;
    submit(&f, device(&d), save(&asha, mouse(8))).await.unwrap();
    submit(
        &f,
        device(&d),
        save(
            &asha,
            PcSettings {
                display_mode: Some("1920x1080@144".into()),
                keyboard_layout: Some("00000409".into()),
                ..Default::default()
            },
        ),
    )
    .await
    .unwrap();
    submit(&f, device(&d), save(&asha, mouse(12)))
        .await
        .unwrap();
    assert_eq!(
        stored(&f, &asha),
        Some(PcSettings {
            mouse_speed: Some(12),
            display_mode: Some("1920x1080@144".into()),
            audio_output: None,
            keyboard_layout: Some("00000409".into()),
        })
    );
    // The row's rev counts the saves.
    let rows = reopen(&f).rows(Table::PlayerPcSettings).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["rev"], RowValue::Integer(3));
}

#[tokio::test]
async fn a_bad_value_is_refused_and_nothing_is_stored() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let asha = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &asha, 10_000).await;
    wallet_session(&f, &d, &asha).await;
    for (bad, field) in [
        (mouse(0), "mouse_speed"),
        (mouse(21), "mouse_speed"),
        (
            PcSettings {
                display_mode: Some("1920x1080".into()),
                ..Default::default()
            },
            "display_mode",
        ),
        (
            PcSettings {
                keyboard_layout: Some("US".into()),
                ..Default::default()
            },
            "keyboard_layout",
        ),
        (
            PcSettings {
                audio_output: Some(String::new()),
                ..Default::default()
            },
            "audio_output",
        ),
        (PcSettings::default(), "settings"),
    ] {
        let e = submit(&f, device(&d), save(&asha, bad)).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::ValidationFailed, "{field}");
        assert_eq!(e.field.as_deref(), Some(field));
    }
    assert_eq!(stored(&f, &asha), None);
    // A staff-style intent for a player that does not exist is refused too.
    let e = submit(&f, owner(), save("nobody", mouse(5)))
        .await
        .unwrap_err();
    assert_eq!(e.field.as_deref(), Some("player_id"));
}

#[tokio::test]
async fn the_display_modes_setting_exists_empty_and_refuses_bad_lists() {
    let f = bootstrapped().await;
    assert_eq!(
        setting(&reopen(&f), "kiosk.display_modes")["value"],
        RowValue::Text(String::new())
    );
    f.writer
        .submit_intent(set(
            "kiosk.display_modes",
            "1920x1080@144, 1920x1080@60",
            Some(1),
        ))
        .await
        .unwrap();
    for value in ["1920x1080", "1920x1080@60,1920x1080@60", "a, b"] {
        let err = expect_err(&f, set("kiosk.display_modes", value, Some(2))).await;
        assert_eq!(err.code, ErrorCode::ValidationFailed, "{value}");
        assert_eq!(err.field.as_deref(), Some("kiosk.display_modes"));
    }
    assert_eq!(
        setting(&reopen(&f), "kiosk.display_modes")["value"],
        RowValue::Text("1920x1080@144, 1920x1080@60".into())
    );
}
