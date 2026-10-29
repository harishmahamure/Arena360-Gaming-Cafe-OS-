//! Play hours per game (Story 6.27): the `game_plays` change-log table. Only `apply()` writes it,
//! through [`insert_game_play`]; the readers feed the most-played list and each player's
//! last-played times.

use arena_proto::AppError;
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

/// What `apply()` writes: one finished play (`rev` and the timestamps are set by the stamp step).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewGamePlay<'a> {
    pub id: &'a str,
    pub branch_id: &'a str,
    pub device_id: &'a str,
    pub session_id: Option<&'a str>,
    pub player_id: Option<&'a str>,
    pub game_id: &'a str,
    pub started_at: i64,
    pub ended_at: i64,
    pub minutes: i64,
}

/// Inserts a play with `rev = 0` and zero timestamps, for the stamp step to set.
pub fn insert_game_play(conn: &Connection, p: &NewGamePlay<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO game_plays (id, branch_id, device_id, session_id, player_id, game_id,
                                 started_at, ended_at, minutes, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, 0, 0)",
        params![
            p.id,
            p.branch_id,
            p.device_id,
            p.session_id,
            p.player_id,
            p.game_id,
            p.started_at,
            p.ended_at,
            p.minutes
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// The session (and its player, `None` for a guest) the device was running at `at`: the one that
/// had started by then and had not ended before it, the latest started if several. A play is
/// recorded when its game stops, which can be after the Hub has already ended the session (the
/// session end that stopped it), so the live session alone would miss exactly those plays.
pub fn session_on_device_at(
    conn: &Connection,
    device_id: &str,
    at: i64,
) -> Result<Option<(String, Option<String>)>, AppError> {
    conn.query_row(
        "SELECT id, player_id FROM sessions
         WHERE device_id = ?1 AND started_at <= ?2 AND (ended_at IS NULL OR ended_at >= ?2)
           AND state <> 'orphaned'
         ORDER BY started_at DESC, id DESC LIMIT 1",
        params![device_id, at],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(db_error)
}

/// The branch's games by minutes played, for plays that ended at or after `since`: at most
/// `limit`, games with minutes above zero only, most minutes first and ties by game id.
pub fn top_played(
    conn: &Connection,
    since: i64,
    limit: usize,
) -> Result<Vec<(String, i64)>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT game_id, SUM(minutes) AS total FROM game_plays
             WHERE ended_at >= ?1
             GROUP BY game_id HAVING total > 0
             ORDER BY total DESC, game_id ASC LIMIT ?2",
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map(params![since, limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(db_error)?;
    rows.collect::<Result<_, _>>().map_err(db_error)
}

/// When `player_id` last finished playing each game, in any window: newest first, ties by game id.
pub fn last_played(conn: &Connection, player_id: &str) -> Result<Vec<(String, i64)>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT game_id, MAX(ended_at) AS at FROM game_plays
             WHERE player_id = ?1
             GROUP BY game_id
             ORDER BY at DESC, game_id ASC",
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map([player_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(db_error)?;
    rows.collect::<Result<_, _>>().map_err(db_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE sessions (id TEXT PRIMARY KEY, player_id TEXT NULL,
                 device_id TEXT NOT NULL, state TEXT NOT NULL, started_at INTEGER NOT NULL,
                 ended_at INTEGER NULL);",
        )
        .unwrap();
        c.execute_batch(
            &include_str!("../migrations/0065_game_plays.sql")
                // The real tables the keys point at are not needed to test the queries.
                .replace("REFERENCES branch (branch_id)", "")
                .replace("REFERENCES devices (device_id)", "")
                .replace("REFERENCES sessions (id)", "")
                .replace("REFERENCES players (player_id)", ""),
        )
        .unwrap();
        c
    }

    fn play(c: &Connection, id: &str, player: Option<&str>, game: &str, ended: i64, minutes: i64) {
        insert_game_play(
            c,
            &NewGamePlay {
                id,
                branch_id: "B",
                device_id: "D",
                session_id: None,
                player_id: player,
                game_id: game,
                started_at: ended - minutes * 60_000,
                ended_at: ended,
                minutes,
            },
        )
        .unwrap();
    }

    #[test]
    fn the_top_sums_minutes_in_the_window_and_breaks_ties_by_game_id() {
        let c = conn();
        play(&c, "1", Some("P1"), "B-game", 10_000_000, 30);
        play(&c, "2", None, "B-game", 10_000_100, 30);
        play(&c, "3", Some("P2"), "A-game", 10_000_200, 60);
        play(&c, "4", Some("P2"), "C-game", 10_000_300, 90);
        // Out of the window, and a play with no minutes.
        play(&c, "5", Some("P2"), "A-game", 1_000, 600);
        play(&c, "6", Some("P2"), "Z-game", 10_000_400, 0);
        let top = top_played(&c, 5_000_000, 10).unwrap();
        assert_eq!(
            top,
            vec![
                ("C-game".to_owned(), 90),
                ("A-game".to_owned(), 60),
                ("B-game".to_owned(), 60)
            ]
        );
        assert_eq!(top_played(&c, 5_000_000, 2).unwrap().len(), 2);
        assert!(top_played(&c, 99_000_000, 10).unwrap().is_empty());
    }

    #[test]
    fn a_players_last_play_of_each_game_is_in_any_window() {
        let c = conn();
        play(&c, "1", Some("P1"), "A", 1_000_000, 5);
        play(&c, "2", Some("P1"), "A", 2_000_000, 5);
        play(&c, "3", Some("P1"), "B", 1_500_000, 5);
        play(&c, "4", Some("P2"), "C", 9_000_000, 5);
        play(&c, "5", None, "D", 9_000_000, 5);
        assert_eq!(
            last_played(&c, "P1").unwrap(),
            vec![("A".to_owned(), 2_000_000), ("B".to_owned(), 1_500_000)]
        );
        assert!(last_played(&c, "nobody").unwrap().is_empty());
    }

    #[test]
    fn the_session_is_the_one_running_at_the_time_even_if_it_ended_since() {
        let c = conn();
        for (id, player, state, start, end) in [
            ("S1", Some("P1"), "ended", 1_000, Some(5_000)),
            ("S2", None, "active", 6_000, None),
            ("S3", Some("P3"), "orphaned", 7_000, None),
        ] {
            c.execute(
                "INSERT INTO sessions VALUES (?1, ?2, 'D', ?3, ?4, ?5)",
                params![id, player, state, start, end],
            )
            .unwrap();
        }
        let at = |t| session_on_device_at(&c, "D", t).unwrap();
        assert_eq!(at(3_000), Some(("S1".to_owned(), Some("P1".to_owned()))));
        assert_eq!(at(5_000), Some(("S1".to_owned(), Some("P1".to_owned()))));
        assert_eq!(at(9_000), Some(("S2".to_owned(), None)));
        assert_eq!(at(500), None);
        assert_eq!(session_on_device_at(&c, "other", 3_000).unwrap(), None);
    }
}
