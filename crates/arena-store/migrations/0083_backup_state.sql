-- 0083: the Hub's backup bookkeeping (Story 9.2, AD-24).
--
-- `backup_state` (single row) is local bookkeeping outside the change log, like `upload_cursor`
-- and `snapshot_log`; only maintenance writes and the triggers below touch it, never `apply()`'s
-- change rows.
--
-- * `keys_version` counts changes to `player_keys` (a trigger bumps it on every insert, update and
--   delete, in the transaction that made the change). It starts at 1 so the first key backup of a
--   Hub that already holds keys is always taken. `keys_backed_up_version` is the version the last
--   successful key backup PUT held.
-- * `keys_backed_up_at` / `keys_error` / `keys_error_at`: when the last key backup succeeded (Hub
--   clock, ms) and why the last attempt failed (a code, never player data).
-- * `kek_escrowed_at`: when the cloud accepted this Hub's branch key wrapped with the escrow key
--   (NULL until then, and again after a re-pair mints a new key).
CREATE TABLE backup_state (
    id                     INTEGER PRIMARY KEY CHECK (id = 1),
    keys_version           INTEGER NOT NULL DEFAULT 1 CHECK (keys_version >= 0),
    keys_backed_up_version INTEGER NOT NULL DEFAULT 0 CHECK (keys_backed_up_version >= 0),
    keys_backed_up_at      INTEGER NULL,
    keys_error             TEXT NULL,
    keys_error_at          INTEGER NULL,
    kek_escrowed_at        INTEGER NULL
);

INSERT INTO backup_state (id) VALUES (1);

CREATE TRIGGER player_keys_version_insert AFTER INSERT ON player_keys
BEGIN
    UPDATE backup_state SET keys_version = keys_version + 1 WHERE id = 1;
END;

CREATE TRIGGER player_keys_version_update AFTER UPDATE ON player_keys
BEGIN
    UPDATE backup_state SET keys_version = keys_version + 1 WHERE id = 1;
END;

CREATE TRIGGER player_keys_version_delete AFTER DELETE ON player_keys
BEGIN
    UPDATE backup_state SET keys_version = keys_version + 1 WHERE id = 1;
END;
