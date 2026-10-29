-- 0065: play hours per game (Story 6.27). Each `game.play_recorded` intent a paired kiosk submits
-- (queued while the Hub was unreachable, replayed once) writes one row when a game started at the
-- PC stops running or the session ends. The Hub names the player and session itself from the
-- device's sessions; both are NULL for a play with no session (a guest has a session but no
-- player). `game_id` is the library entry id, deliberately without a foreign key: a play of an
-- entry deleted since is still a play. `started_at` and `ended_at` are the kiosk's clock;
-- `minutes` is the played time in whole minutes. Hours per game per branch are the sum over rows.
CREATE TABLE game_plays (
    id         TEXT PRIMARY KEY,
    branch_id  TEXT NOT NULL REFERENCES branch (branch_id),
    device_id  TEXT NOT NULL REFERENCES devices (device_id),
    session_id TEXT NULL REFERENCES sessions (id),
    player_id  TEXT NULL REFERENCES players (player_id),
    game_id    TEXT NOT NULL CHECK (game_id <> ''),
    started_at INTEGER NOT NULL,
    ended_at   INTEGER NOT NULL CHECK (ended_at >= started_at),
    minutes    INTEGER NOT NULL CHECK (minutes >= 0),
    rev        INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX game_plays_ended ON game_plays (ended_at);
CREATE INDEX game_plays_player ON game_plays (player_id, ended_at);
