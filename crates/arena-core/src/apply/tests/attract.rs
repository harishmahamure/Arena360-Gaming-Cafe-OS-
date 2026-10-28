//! Story 6.7: the per-device attract view the Hub pushes to idle kiosks.

use super::prepaid::{group_of, ist};
use super::session::{pc, submit};
use super::stock::owner;
use super::*;
use crate::attract::{GameMediaHashes, attract_views};
use arena_proto::{
    AttractState, AttractView, DeviceAssign, DeviceKind, DeviceMaintenance, LibraryUpsert,
    MAX_ATTRACT_MEDIA, RuleUpsert,
};
use std::collections::HashMap;

/// Weekdays 16:00–18:00 at 20% off.
async fn happy_hour(f: &Fixture, group: &str) {
    submit(
        f,
        owner(),
        IntentPayload::RuleUpsert(RuleUpsert {
            id: None,
            name: "Happy hour".into(),
            group_id: group.into(),
            rule_kind: "weekly".into(),
            days_mask: Some(0b001_1111),
            start_min: Some(16 * 60),
            end_min: Some(18 * 60),
            date_from: None,
            date_to: None,
            adjust_kind: "percent_bp".into(),
            adjust_value: -2000,
            priority: 0,
            active: true,
        }),
    )
    .await
    .unwrap();
}

fn none(_: &str) -> GameMediaHashes {
    GameMediaHashes::default()
}

fn view(f: &Fixture, device: &str, now: i64) -> AttractView {
    let mut views = attract_views(&reopen(f), &[device.to_owned()], now, &none).unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].0, device);
    views.remove(0).1
}

#[tokio::test]
async fn a_ready_pc_shows_its_group_rate_and_no_chip_outside_a_rule() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(12_000)).await;
    happy_hour(&f, &group_of(&f, &d)).await;
    let v = view(&f, &d, ist("2027-01-15 15:00"));
    assert_eq!(v.state, AttractState::Ready);
    assert_eq!((v.rate_minor, v.rule.clone()), (Some(12_000), None));
    assert!(v.group_name.is_some());
    assert_eq!(v.branch_name, "Dev Branch");
    assert_eq!((v.locale.currency.as_str(), v.locale.exponent), ("INR", 2));
    assert!(v.media.is_empty());
    assert!(v.is_sane());
}

#[tokio::test]
async fn a_rule_shows_its_rate_and_when_it_ends_and_gives_way_after() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(12_000)).await;
    happy_hour(&f, &group_of(&f, &d)).await;
    let v = view(&f, &d, ist("2027-01-15 16:30"));
    assert_eq!(v.state, AttractState::Ready);
    assert_eq!(v.rate_minor, Some(9_600));
    let rule = v.rule.unwrap();
    assert_eq!(
        (
            rule.name.as_str(),
            rule.adjust_kind.as_str(),
            rule.adjust_value
        ),
        ("Happy hour", "percent_bp", -2000)
    );
    assert_eq!(rule.rate_minor, 9_600);
    assert_eq!(rule.until, Some(ist("2027-01-15 18:00")));
    let after = view(&f, &d, ist("2027-01-15 18:00"));
    assert_eq!((after.rate_minor, after.rule), (Some(12_000), None));
}

#[tokio::test]
async fn an_unassigned_maintenance_or_rateless_pc_asks_at_the_counter() {
    let f = bootstrapped().await;
    let now = ist("2027-01-15 16:30");
    let unassigned = pc(&f, DeviceKind::Pc, "pc", Some(12_000)).await;
    submit(
        &f,
        owner(),
        IntentPayload::DeviceAssign(DeviceAssign {
            device_id: unassigned.clone(),
            group_id: None,
        }),
    )
    .await
    .unwrap();
    let maint = pc(&f, DeviceKind::Pc, "pc", Some(12_000)).await;
    happy_hour(&f, &group_of(&f, &maint)).await;
    submit(
        &f,
        owner(),
        IntentPayload::DeviceMaintenance(DeviceMaintenance {
            device_id: maint.clone(),
            on: true,
            note: None,
        }),
    )
    .await
    .unwrap();
    let rateless = pc(&f, DeviceKind::Pc, "pc", None).await;
    for id in [
        unassigned.as_str(),
        maint.as_str(),
        rateless.as_str(),
        "NOPE",
    ] {
        let v = view(&f, id, now);
        assert_eq!(v.state, AttractState::AskCounter, "{id}");
        assert_eq!(
            (v.rate_minor, v.rule.clone(), v.group_name.clone()),
            (None, None, None),
            "{id}"
        );
        assert_eq!(v.branch_name, "Dev Branch");
        assert!(v.is_sane());
    }
}

#[tokio::test]
async fn views_come_back_in_the_order_asked_and_none_before_bootstrap() {
    let f = bootstrapped().await;
    let a = pc(&f, DeviceKind::Pc, "pc", Some(100)).await;
    let b = pc(&f, DeviceKind::Pc, "pc", Some(200)).await;
    let ids = [b.clone(), a.clone()];
    let views = attract_views(&reopen(&f), &ids, ist("2027-01-15 12:00"), &none).unwrap();
    let got: Vec<_> = views
        .iter()
        .map(|(d, v)| (d.as_str(), v.rate_minor))
        .collect();
    assert_eq!(got, [(b.as_str(), Some(200)), (a.as_str(), Some(100))]);
    let fresh = tempfile::tempdir().unwrap();
    let fresh = Store::open(fresh.path().join("fresh.db"), T0).unwrap();
    assert!(attract_views(&fresh, &ids, 0, &none).unwrap().is_empty());
}

fn linked(name: &str, game: Option<&str>, enabled: bool) -> LibraryUpsert {
    LibraryUpsert {
        id: None,
        entry_kind: "game".into(),
        name: name.into(),
        launcher: "steam".into(),
        executable_path: format!(r"C:\Games\{name}.exe"),
        launch_uri: String::new(),
        launch_args: String::new(),
        category: String::new(),
        tags: Vec::new(),
        age_rating: String::new(),
        enabled,
        catalog_id: game.map(str::to_owned),
        installs: Vec::new(),
    }
}

fn hashes(video: char, cover: Option<char>) -> GameMediaHashes {
    GameMediaHashes {
        video: Some(video.to_string().repeat(64)),
        cover: cover.map(|c| c.to_string().repeat(64)),
    }
}

#[tokio::test]
async fn the_media_is_the_stored_videos_of_enabled_linked_entries_in_name_order() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(100)).await;
    for u in [
        linked("Zed", Some("GZ"), true),
        linked("Alpha", Some("GA"), true),
        linked("Off", Some("GOFF"), false),
        linked("Manual", None, true),
        linked("NoVideo", Some("GNONE"), true),
        linked("Twin", Some("GTWIN"), true),
        linked("Mirror", Some("GMIRROR"), true),
    ] {
        submit(&f, owner(), IntentPayload::LibraryUpsert(u))
            .await
            .unwrap();
    }
    let sha = |c: char| c.to_string().repeat(64);
    let table: HashMap<&str, GameMediaHashes> = HashMap::from([
        ("GZ", hashes('b', Some('c'))),
        ("GA", hashes('a', None)),
        ("GOFF", hashes('e', None)),
        ("GTWIN", hashes('d', None)),
        ("GMIRROR", hashes('d', None)),
    ]);
    let lookup = |g: &str| table.get(g).cloned().unwrap_or_default();
    let views = attract_views(&reopen(&f), &[d], ist("2027-01-15 12:00"), &lookup).unwrap();
    let media = &views[0].1.media;
    // Alpha, Mirror (the twin's video, once), Zed; the disabled, manual and video-less skip.
    let videos: Vec<_> = media.iter().map(|m| m.video.clone()).collect();
    assert_eq!(videos, [sha('a'), sha('d'), sha('b')]);
    assert_eq!(media[0].cover, None);
    assert_eq!(media[2].cover, Some(sha('c')));
    assert!(views[0].1.is_sane());
}

#[tokio::test]
async fn the_media_is_capped_at_twenty_items() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(100)).await;
    for n in 0..MAX_ATTRACT_MEDIA + 5 {
        let name = format!("Game {n:02}");
        let game = format!("G{n:02}");
        submit(
            &f,
            owner(),
            IntentPayload::LibraryUpsert(linked(&name, Some(&game), true)),
        )
        .await
        .unwrap();
    }
    let lookup = |g: &str| {
        let n: u8 = g[1..].parse().unwrap();
        GameMediaHashes {
            video: Some(format!("{n:02x}").repeat(32)),
            cover: None,
        }
    };
    let views = attract_views(&reopen(&f), &[d], ist("2027-01-15 12:00"), &lookup).unwrap();
    let media = &views[0].1.media;
    assert_eq!(media.len(), MAX_ATTRACT_MEDIA);
    assert_eq!(media[0].video, "00".repeat(32));
    assert_eq!(media[19].video, "13".repeat(32));
}
