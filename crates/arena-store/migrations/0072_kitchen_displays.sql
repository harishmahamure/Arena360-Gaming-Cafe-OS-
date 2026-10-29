-- 0072: kitchen displays (Story 7.6). A tablet pairs by a 6-digit code at the Hub and gets a
-- scoped `kds` token; only its Argon2id hash is kept here. Displays are not `devices`: they never
-- count against the device cap, never appear on the floor and carry no PC fields. `created_by` is
-- the Manager or Owner who issued the pairing code. `last_seen` is refreshed at most once a minute.
-- `high_contrast` is the manager's per-display setting (`display.hc.<id>`), 0 or 1.
CREATE TABLE kitchen_displays (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    token_hash    TEXT NOT NULL,
    created_by    TEXT NULL,
    last_seen     INTEGER NULL,
    revoked_at    INTEGER NULL,
    high_contrast INTEGER NOT NULL DEFAULT 0 CHECK (high_contrast IN (0, 1)),
    rev           INTEGER NOT NULL,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);

CREATE INDEX kitchen_displays_live ON kitchen_displays (id) WHERE revoked_at IS NULL;
