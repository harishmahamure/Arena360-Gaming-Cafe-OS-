-- Matching scans to the catalogue and approving them into the library (Story 6.5).
--
-- `game_matches`: the catalogue matches the Hub found in a device's latest scan, one row per
-- device, replaced by every `game.matched`. `matches` is a JSON array of `{catalog_id, path}`;
-- `scanned_at` is the scan they were made from.
CREATE TABLE game_matches (
    device_id  TEXT PRIMARY KEY REFERENCES devices (device_id),
    scanned_at INTEGER NOT NULL CHECK (scanned_at > 0),
    matches    TEXT NOT NULL DEFAULT '[]',
    rev        INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- `library_installs`: where one device keeps a library entry, so the same game at different
-- paths on different PCs works. `match_key` is the key of the proposal the install was found as
-- (empty when recorded by hand): a later scan that finds the same key at a new path updates
-- `path`.
CREATE TABLE library_installs (
    entry_id   TEXT NOT NULL REFERENCES library_entries (id),
    device_id  TEXT NOT NULL REFERENCES devices (device_id),
    path       TEXT NOT NULL CHECK (path <> ''),
    match_key  TEXT NOT NULL DEFAULT '',
    rev        INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (entry_id, device_id)
);

-- `library_dismissals`: proposals the Manager dismissed ("Found on PCs"). Later scans that
-- report the same executables do not propose them again; "clear" deletes every row.
CREATE TABLE library_dismissals (
    key        TEXT PRIMARY KEY,
    rev        INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
