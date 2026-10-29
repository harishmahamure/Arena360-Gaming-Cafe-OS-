//! Launch times for the Hub (Story 6.25): the latest measured launch of each PC, and each game's
//! median measured launch time per PC over the last 7 days, read from the `game_launched`
//! device events (`launch_ms` is `null` when the game showed no window in time).

use arena_proto::{DeviceLaunchTimes, GameLaunchMedian, LastLaunch, LaunchTimesView, median_ms};
use arena_store::GameLaunchRow;

#[derive(serde::Deserialize)]
struct Detail {
    game_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    launch_ms: Option<i64>,
}

/// The view for `rows` (oldest first, as [`arena_store::list_game_launches`] returns them). A
/// row whose detail is not a launch record is skipped. PCs are ordered by name, games by name.
pub fn build(rows: Vec<GameLaunchRow>) -> LaunchTimesView {
    struct Game {
        name: String,
        launches: i64,
        times: Vec<i64>,
    }
    struct Device {
        device_id: String,
        name: String,
        last: Option<LastLaunch>,
        games: Vec<(String, Game)>,
    }
    let mut devices: Vec<Device> = Vec::new();
    for row in rows {
        let Ok(d) = serde_json::from_str::<Detail>(&row.detail) else {
            continue;
        };
        let at = devices.iter().position(|x| x.device_id == row.device_id);
        let device = match at {
            Some(i) => &mut devices[i],
            None => {
                devices.push(Device {
                    device_id: row.device_id.clone(),
                    name: row.label.clone(),
                    last: None,
                    games: Vec::new(),
                });
                devices.last_mut().expect("just pushed")
            }
        };
        let game = match device.games.iter().position(|(id, _)| *id == d.game_id) {
            Some(i) => &mut device.games[i].1,
            None => {
                device.games.push((
                    d.game_id.clone(),
                    Game {
                        name: String::new(),
                        launches: 0,
                        times: Vec::new(),
                    },
                ));
                &mut device.games.last_mut().expect("just pushed").1
            }
        };
        game.launches += 1;
        // The newest record names the game.
        game.name = d.name.clone();
        if let Some(ms) = d.launch_ms.filter(|ms| *ms >= 0) {
            game.times.push(ms);
            device.last = Some(LastLaunch {
                game_id: d.game_id,
                name: d.name,
                launch_ms: ms,
                at: row.at,
            });
        }
    }
    let mut out: Vec<DeviceLaunchTimes> = devices
        .into_iter()
        .map(|d| {
            let mut games: Vec<GameLaunchMedian> = d
                .games
                .into_iter()
                .map(|(game_id, mut g)| GameLaunchMedian {
                    game_id,
                    name: g.name,
                    launches: g.launches,
                    median_ms: median_ms(&mut g.times),
                })
                .collect();
            games.sort_by(|a, b| {
                a.name
                    .to_lowercase()
                    .cmp(&b.name.to_lowercase())
                    .then_with(|| a.game_id.cmp(&b.game_id))
            });
            DeviceLaunchTimes {
                device_id: d.device_id,
                name: d.name,
                last: d.last,
                games,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.device_id.cmp(&b.device_id))
    });
    LaunchTimesView { devices: out }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(
        device: &str,
        label: &str,
        at: i64,
        game: &str,
        name: &str,
        ms: Option<i64>,
    ) -> GameLaunchRow {
        GameLaunchRow {
            device_id: device.into(),
            label: label.into(),
            at,
            detail: serde_json::json!({
                "game_id": game, "name": name, "session_id": null, "player_id": null, "launch_ms": ms
            })
            .to_string(),
        }
    }

    #[test]
    fn medians_are_per_pc_and_game_and_the_last_launch_is_the_latest_measured() {
        let view = build(vec![
            row("P2", "PC 2", 10, "g1", "Valorant", Some(30_000)),
            row("P1", "PC 1", 20, "g1", "Valorant", Some(10_000)),
            row("P1", "PC 1", 30, "g1", "Valorant", Some(14_200)),
            row("P1", "PC 1", 40, "g1", "Valorant", Some(20_000)),
            row("P1", "PC 1", 50, "g2", "CS2", None),
            row("P1", "PC 1", 60, "g1", "Valorant", None),
        ]);
        assert_eq!(view.devices.len(), 2);
        let p1 = &view.devices[0];
        assert_eq!(p1.name, "PC 1");
        // The latest measured launch, not the latest launch.
        let last = p1.last.as_ref().unwrap();
        assert_eq!(
            (last.name.as_str(), last.launch_ms, last.at),
            ("Valorant", 20_000, 40)
        );
        assert_eq!(p1.games.len(), 2);
        assert_eq!(p1.games[0].name, "CS2");
        assert_eq!((p1.games[0].launches, p1.games[0].median_ms), (1, None));
        assert_eq!(p1.games[1].name, "Valorant");
        assert_eq!(
            (p1.games[1].launches, p1.games[1].median_ms),
            (4, Some(14_200))
        );
        let p2 = &view.devices[1];
        assert_eq!(p2.games[0].median_ms, Some(30_000));
    }

    #[test]
    fn a_pc_with_only_unmeasured_launches_has_no_last_launch_and_junk_rows_are_skipped() {
        let mut junk = row("P1", "PC 1", 5, "g1", "X", None);
        junk.detail = "not json".into();
        let view = build(vec![junk, row("P1", "PC 1", 6, "g1", "X", None)]);
        assert_eq!(view.devices.len(), 1);
        assert!(view.devices[0].last.is_none());
        assert_eq!(view.devices[0].games[0].launches, 1);
        assert!(build(Vec::new()).devices.is_empty());
    }
}
