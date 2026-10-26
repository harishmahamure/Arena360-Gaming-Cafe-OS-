CREATE TABLE players (
    player_id     TEXT PRIMARY KEY,
    username      TEXT NOT NULL CHECK (
        username = lower(username) AND length(username) BETWEEN 3 AND 32
    ),
    password_hash TEXT NOT NULL,
    phone_lookup  TEXT NOT NULL,
    name_enc      TEXT NOT NULL,
    phone_enc     TEXT NOT NULL,
    email_enc     TEXT NULL,
    dob_enc       TEXT NULL,
    notes_enc     TEXT NULL,
    tags          TEXT NOT NULL,
    deleted_at    INTEGER NULL,
    rev           INTEGER NOT NULL,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);

CREATE UNIQUE INDEX players_username ON players (username) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX players_phone ON players (phone_lookup) WHERE deleted_at IS NULL;

CREATE TABLE consents (
    consent_id     TEXT PRIMARY KEY,
    player_id      TEXT NOT NULL REFERENCES players (player_id),
    purpose        TEXT NOT NULL CHECK (purpose IN ('transactional', 'marketing')),
    policy_version TEXT NOT NULL,
    granted        INTEGER NOT NULL CHECK (granted IN (0, 1)),
    at             INTEGER NOT NULL,
    rev            INTEGER NOT NULL,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
);

CREATE INDEX consents_player ON consents (player_id, at);

CREATE TRIGGER consents_no_delete BEFORE DELETE ON consents
BEGIN
    SELECT RAISE(ABORT, 'consents is append-only');
END;

CREATE TRIGGER consents_no_update BEFORE UPDATE OF
    consent_id, player_id, purpose, policy_version, granted, at
ON consents
BEGIN
    SELECT RAISE(ABORT, 'consents is append-only');
END;

CREATE TABLE player_keys (
    player_id   TEXT PRIMARY KEY REFERENCES players (player_id),
    dek_wrapped TEXT NOT NULL
);
