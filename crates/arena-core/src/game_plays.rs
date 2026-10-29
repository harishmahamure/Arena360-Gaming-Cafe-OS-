//! "Most played here" (Story 6.27): the branch's top games by minutes played over the last
//! 7 days, recomputed at most every 15 minutes per Hub, and the live player's own last-played
//! time per game (never cached, and never another player's).

use std::sync::Mutex;

use arena_proto::{
    AppError, LastPlayed, MOST_PLAYED_DAYS, MOST_PLAYED_LIMIT, MostPlayed, MostPlayedGame,
};
use arena_store::Store;

/// How long a computed top list is reused, in milliseconds.
pub const MOST_PLAYED_TTL_MS: i64 = 15 * 60_000;

/// The most-played window, in milliseconds.
pub const MOST_PLAYED_WINDOW_MS: i64 = MOST_PLAYED_DAYS * 24 * 3_600_000;

/// The Hub's cached top list and when it was computed.
#[derive(Debug, Default)]
pub struct MostPlayedCache {
    entry: Mutex<Option<(i64, Vec<MostPlayedGame>)>>,
}

impl MostPlayedCache {
    /// The top list as of `now_ms`: the cached one while it is under 15 minutes old (a clock that
    /// moved back counts as expired), else recomputed with `compute` and kept.
    pub fn top(
        &self,
        now_ms: i64,
        compute: impl FnOnce(i64) -> Result<Vec<MostPlayedGame>, AppError>,
    ) -> Result<Vec<MostPlayedGame>, AppError> {
        let mut entry = self
            .entry
            .lock()
            .map_err(|_| AppError::new(arena_proto::ErrorCode::Internal, "most played lock"))?;
        if let Some((at, top)) = entry.as_ref()
            && (*at..*at + MOST_PLAYED_TTL_MS).contains(&now_ms)
        {
            return Ok(top.clone());
        }
        let top = compute(now_ms - MOST_PLAYED_WINDOW_MS)?;
        *entry = Some((now_ms, top.clone()));
        Ok(top)
    }
}

/// What a kiosk asking from `device_id` gets: the (cached) top list and the last-played times of
/// the player live on that PC (none for a guest or no session).
pub fn most_played(
    store: &Store,
    cache: &MostPlayedCache,
    device_id: &str,
    now_ms: i64,
) -> Result<MostPlayed, AppError> {
    let top = cache.top(now_ms, |since| {
        Ok(store
            .top_played(since, MOST_PLAYED_LIMIT)?
            .into_iter()
            .map(|(game_id, minutes)| MostPlayedGame { game_id, minutes })
            .collect())
    })?;
    let player = crate::player_account::device_session(store, device_id)?.and_then(|s| s.player_id);
    let last_played = match player {
        Some(player) => store
            .last_played(&player)?
            .into_iter()
            .map(|(game_id, at)| LastPlayed { game_id, at })
            .collect(),
        None => Vec::new(),
    };
    Ok(MostPlayed { top, last_played })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn game(id: &str, minutes: i64) -> MostPlayedGame {
        MostPlayedGame {
            game_id: id.into(),
            minutes,
        }
    }

    #[test]
    fn the_top_is_recomputed_at_most_every_fifteen_minutes() {
        let cache = MostPlayedCache::default();
        let runs = Cell::new(0);
        let ask = |now: i64, minutes: i64| {
            cache
                .top(now, |since| {
                    runs.set(runs.get() + 1);
                    assert_eq!(since, now - MOST_PLAYED_WINDOW_MS);
                    Ok(vec![game("A", minutes)])
                })
                .unwrap()
        };
        let t0 = 10_000_000_000;
        assert_eq!(ask(t0, 10), vec![game("A", 10)]);
        // Within 15 minutes: the first answer, whatever changed since.
        assert_eq!(ask(t0 + 14 * 60_000, 99), vec![game("A", 10)]);
        assert_eq!(ask(t0 + MOST_PLAYED_TTL_MS - 1, 99), vec![game("A", 10)]);
        assert_eq!(runs.get(), 1);
        // At 15 minutes: recomputed, and the new answer is the one kept.
        assert_eq!(ask(t0 + MOST_PLAYED_TTL_MS, 20), vec![game("A", 20)]);
        assert_eq!(
            ask(t0 + MOST_PLAYED_TTL_MS + 60_000, 99),
            vec![game("A", 20)]
        );
        assert_eq!(runs.get(), 2);
        // A clock that moved back is not trusted.
        assert_eq!(ask(t0, 30), vec![game("A", 30)]);
        assert_eq!(runs.get(), 3);
    }

    #[test]
    fn a_failed_computation_is_not_cached() {
        let cache = MostPlayedCache::default();
        let err = cache.top(1_000_000_000, |_| {
            Err(AppError::new(arena_proto::ErrorCode::Internal, "x"))
        });
        assert!(err.is_err());
        let ok = cache
            .top(1_000_000_001, |_| Ok(vec![game("A", 1)]))
            .unwrap();
        assert_eq!(ok, vec![game("A", 1)]);
    }
}
