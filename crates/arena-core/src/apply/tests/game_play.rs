//! Story 6.27: `game.play_recorded` writes one `game_plays` row naming the player and session the
//! Hub knows on the device, and the rows feed the most-played list.

use super::session::{end, end_intent, int, pc, player, start, table, text};
use super::stock::intent;
use super::*;
use crate::DeviceDirectory;
use arena_proto::{DeviceKind, KioskGamePlayRecorded};

const MIN: i64 = 60_000;

fn play(device: &str, game: &str, started_at: i64, minutes: i64) -> Intent {
    let mut i = intent(
        Actor {
            kind: ActorKind::Device,
            id: device.into(),
        },
        IntentPayload::KioskGamePlayRecorded(KioskGamePlayRecorded {
            game_id: game.into(),
            started_at,
            ended_at: started_at + minutes * MIN,
            minutes,
        }),
    );
    i.issued_at = started_at + minutes * MIN;
    i
}

fn plays(f: &Fixture) -> Vec<Row> {
    table(f, Table::GamePlays)
}

#[tokio::test]
async fn a_play_writes_one_row_for_the_player_and_session_on_the_pc_and_replays_once() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    start(&f, &d, Some(&p)).await.unwrap();
    let session = text(&table(&f, Table::Sessions)[0], "id");
    f.clock.advance(40 * MIN);

    let first = play(&d, "01JGAME", T0 + 5 * MIN, 25);
    let applied = f.writer.submit_intent(first.clone()).await.unwrap();
    assert_eq!(
        applied
            .changes
            .iter()
            .map(|c| c.tbl.as_str())
            .collect::<Vec<_>>(),
        ["game_plays"]
    );
    // Replayed after a reconnect: the stored outcome, nothing new.
    assert_eq!(f.writer.submit_intent(first).await.unwrap(), applied);
    let rows = plays(&f);
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(text(r, "device_id"), d);
    assert_eq!(text(r, "player_id"), p);
    assert_eq!(text(r, "session_id"), session);
    assert_eq!(text(r, "game_id"), "01JGAME");
    assert_eq!(int(r, "started_at"), T0 + 5 * MIN);
    assert_eq!(int(r, "ended_at"), T0 + 30 * MIN);
    assert_eq!(int(r, "minutes"), 25);
    assert_eq!(int(r, "rev"), 1);
    let change = reopen(&f).changes().unwrap().pop().unwrap();
    assert_eq!(
        (change.intent_kind.as_str(), change.actor_kind.as_str()),
        ("game.play_recorded", "device")
    );
}

#[tokio::test]
async fn a_guest_play_has_a_session_and_no_player_and_no_session_has_neither() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    // No session yet.
    f.writer
        .submit_intent(play(&d, "G1", T0 - 10 * MIN, 5))
        .await
        .unwrap();
    start(&f, &d, None).await.unwrap();
    f.clock.advance(20 * MIN);
    f.writer
        .submit_intent(play(&d, "G2", T0 + MIN, 10))
        .await
        .unwrap();
    let rows = plays(&f);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["session_id"], RowValue::Null);
    assert_eq!(rows[0]["player_id"], RowValue::Null);
    assert!(matches!(rows[1]["session_id"], RowValue::Text(_)));
    assert_eq!(rows[1]["player_id"], RowValue::Null);
}

#[tokio::test]
async fn a_play_recorded_after_the_session_ended_still_names_that_session() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let a = player(&f, "asha", "+919876543210").await;
    let b = player(&f, "ben", "+919876543211").await;
    start(&f, &d, Some(&a)).await.unwrap();
    let first = text(&table(&f, Table::Sessions)[0], "id");
    f.clock.advance(40 * MIN);
    end(&f, end_intent(&first, Some(1))).await.unwrap();
    f.clock.advance(MIN);
    start(&f, &d, Some(&b)).await.unwrap();
    f.clock.advance(5 * MIN);
    // The kiosk reports the first player's game only now (the end of their session stopped it).
    f.writer
        .submit_intent(play(&d, "G1", T0 + 10 * MIN, 30))
        .await
        .unwrap();
    let rows = plays(&f);
    assert_eq!(text(&rows[0], "player_id"), a);
    assert_eq!(text(&rows[0], "session_id"), first);
}

#[tokio::test]
async fn a_bad_play_is_refused_and_nothing_is_written() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    for actor in [ActorKind::Owner, ActorKind::System, ActorKind::Staff] {
        let mut i = play(&d, "G1", T0, 5);
        i.actor.kind = actor;
        let e = f.writer.submit_intent(i).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::PermissionDenied, "{actor:?}");
    }
    let e = f
        .writer
        .submit_intent(play("01JNOPEAAAAAAAAAAAAAAAAAAA", "G1", T0, 5))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    for (i, field) in [
        (play(&d, "not a game", T0, 5), "game_id"),
        (play(&d, "G1", T0, -1), "ended_at"),
        (
            {
                let mut i = play(&d, "G1", T0, 5);
                if let IntentPayload::KioskGamePlayRecorded(p) = &mut i.payload {
                    p.minutes = 99;
                }
                i
            },
            "minutes",
        ),
        (
            {
                let mut i = play(&d, "G1", T0, 5);
                i.base_rev = Some(1);
                i
            },
            "base_rev",
        ),
    ] {
        let e = f.writer.submit_intent(i).await.unwrap_err();
        assert_eq!(e.field.as_deref(), Some(field));
    }
    assert!(plays(&f).is_empty());
}

#[tokio::test]
async fn most_played_ranks_the_week_by_minutes_caches_for_fifteen_minutes_and_names_only_the_live_player()
 {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let other = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let asha = player(&f, "asha", "+919876543210").await;
    let ben = player(&f, "ben", "+919876543211").await;
    start(&f, &other, Some(&ben)).await.unwrap();
    start(&f, &d, Some(&asha)).await.unwrap();
    f.clock.advance(60 * MIN);
    f.writer
        .submit_intent(play(&other, "B", T0 + MIN, 50))
        .await
        .unwrap();
    f.writer
        .submit_intent(play(&other, "A", T0 + 2 * MIN, 20))
        .await
        .unwrap();
    f.writer
        .submit_intent(play(&other, "Z", T0 + 3 * MIN, 0))
        .await
        .unwrap();
    f.writer
        .submit_intent(play(&d, "A", T0 + 5 * MIN, 30))
        .await
        .unwrap();
    let dir = DeviceDirectory::open(&f.path).unwrap();
    let now = f.clock.now_ms();

    let asked = dir.most_played(&d, now).unwrap();
    let top: Vec<(&str, i64)> = asked
        .top
        .iter()
        .map(|g| (g.game_id.as_str(), g.minutes))
        .collect();
    // A is 20 + 30 across both PCs; Z has no minutes.
    assert_eq!(top, [("A", 50), ("B", 50)]);
    // The player on this PC sees their own plays only.
    assert_eq!(asked.last_played.len(), 1);
    assert_eq!(
        (
            asked.last_played[0].game_id.as_str(),
            asked.last_played[0].at
        ),
        ("A", T0 + 35 * MIN)
    );
    let theirs = dir.most_played(&other, now).unwrap();
    assert_eq!(theirs.top, asked.top);
    assert_eq!(
        theirs
            .last_played
            .iter()
            .map(|l| l.game_id.as_str())
            .collect::<Vec<_>>(),
        ["B", "A", "Z"]
    );
    let serialized = serde_json::to_string(&asked).unwrap();
    assert!(!serialized.contains(&ben) && !serialized.contains(&asha));

    // A play recorded now is not in the list for 15 minutes.
    f.writer
        .submit_intent(play(&d, "C", T0 + MIN, 55))
        .await
        .unwrap();
    assert_eq!(dir.most_played(&d, now + 14 * MIN).unwrap().top, asked.top);
    let later = dir.most_played(&d, now + 15 * MIN).unwrap();
    assert_eq!(later.top[0].game_id, "C");
    // Plays older than 7 days fall out.
    let week = dir.most_played(&d, now + 8 * 24 * 60 * MIN).unwrap();
    assert!(week.top.is_empty());
    assert_eq!(week.last_played.len(), 2);
}

#[tokio::test]
async fn a_play_that_starts_before_every_session_falls_back_to_the_live_one() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    f.clock.advance(10 * MIN);
    start(&f, &d, Some(&p)).await.unwrap();
    let session = text(&table(&f, Table::Sessions)[0], "id");
    f.clock.advance(5 * MIN);
    // The kiosk's clock put the start before the session began (T0 + 10 min): none covers it.
    f.writer
        .submit_intent(play(&d, "G1", T0 + 5 * MIN, 8))
        .await
        .unwrap();
    let rows = plays(&f);
    assert_eq!(rows.len(), 1);
    assert_eq!(text(&rows[0], "session_id"), session);
    assert_eq!(text(&rows[0], "player_id"), p);
}

// ---- Story 6.29: the stored media of the library games, for the Home hero ----

async fn library_game(f: &Fixture, name: &str, kind: &str, catalog: Option<&str>, enabled: bool) {
    f.writer
        .submit_intent(intent(
            super::stock::owner(),
            IntentPayload::LibraryUpsert(arena_proto::LibraryUpsert {
                id: None,
                entry_kind: kind.into(),
                name: name.into(),
                launcher: "steam".into(),
                executable_path: format!(r"C:\Games\{name}.exe"),
                launch_uri: String::new(),
                launch_args: String::new(),
                category: String::new(),
                tags: Vec::new(),
                age_rating: String::new(),
                enabled,
                catalog_id: catalog.map(str::to_owned),
                installs: Vec::new(),
            }),
        ))
        .await
        .unwrap();
}

#[tokio::test]
async fn the_game_media_lists_enabled_catalogue_games_that_have_a_video_or_a_cover() {
    use crate::attract::GameMediaHashes;
    let f = bootstrapped().await;
    library_game(&f, "Apex", "game", Some("C-APEX"), true).await;
    library_game(&f, "Bare", "game", Some("C-BARE"), true).await;
    library_game(&f, "Off", "game", Some("C-OFF"), false).await;
    library_game(&f, "Custom", "game", None, true).await;
    library_game(&f, "Discord", "app", Some("C-APEX"), true).await;
    library_game(&f, "Cover", "game", Some("C-COVER"), true).await;
    let dir = DeviceDirectory::open(&f.path).unwrap();
    let hash = |c: char| c.to_string().repeat(64);
    let lookup = |catalog: &str| match catalog {
        "C-APEX" => GameMediaHashes {
            video: Some(hash('a')),
            cover: Some(hash('b')),
        },
        "C-COVER" => GameMediaHashes {
            video: None,
            cover: Some(hash('c')),
        },
        "C-OFF" => GameMediaHashes {
            video: Some(hash('d')),
            cover: None,
        },
        _ => GameMediaHashes::default(),
    };
    let media = dir.game_media(&lookup).unwrap();
    // Library order (by name): the disabled, app, unlinked and media-less entries are left out.
    assert_eq!(
        media
            .iter()
            .map(|m| (m.video.clone(), m.cover.clone()))
            .collect::<Vec<_>>(),
        [(Some(hash('a')), Some(hash('b'))), (None, Some(hash('c')))]
    );
    // The game id is the library entry's id.
    let id_of = |name: &str| {
        table(&f, Table::LibraryEntries)
            .iter()
            .find(|r| text(r, "name") == name)
            .map(|r| text(r, "id"))
            .unwrap()
    };
    assert_eq!(media[0].game_id, id_of("Apex"));
    assert_eq!(media[1].game_id, id_of("Cover"));
    assert!(
        dir.game_media(&|_: &str| GameMediaHashes::default())
            .unwrap()
            .is_empty()
    );
}
