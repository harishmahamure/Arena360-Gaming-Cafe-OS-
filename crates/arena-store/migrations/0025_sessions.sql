CREATE TABLE sessions (
    id              TEXT PRIMARY KEY,
    player_id       TEXT NULL REFERENCES players (player_id),
    guest_no        INTEGER NULL,
    device_id       TEXT NOT NULL REFERENCES devices (device_id),
    group_id        TEXT NOT NULL REFERENCES device_groups (group_id),
    mode            TEXT NOT NULL CHECK (mode IN ('postpaid', 'pack', 'wallet', 'pass', 'block')),
    plan_id         TEXT NULL,
    pack_id         TEXT NULL,
    rate_id         TEXT NULL REFERENCES rates (rate_id),
    state           TEXT NOT NULL CHECK (
        state IN ('active', 'paused', 'grace', 'expired', 'ended', 'orphaned')
    ),
    started_at      INTEGER NOT NULL,
    ends_at         INTEGER NULL,
    paused_at       INTEGER NULL,
    paused_total_ms INTEGER NOT NULL DEFAULT 0,
    ended_at        INTEGER NULL,
    end_reason      TEXT NULL,
    group_bill_id   TEXT NULL,
    rounding_min    INTEGER NOT NULL,
    minimum_min     INTEGER NOT NULL,
    grace_min       INTEGER NOT NULL,
    time_up_mode    TEXT NOT NULL,
    epoch           TEXT NOT NULL,
    business_day    TEXT NOT NULL,
    fy              TEXT NOT NULL,
    rev             INTEGER NOT NULL,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);

CREATE UNIQUE INDEX sessions_one_live ON sessions (device_id)
    WHERE state IN ('active', 'paused', 'grace');
CREATE UNIQUE INDEX sessions_guest_no ON sessions (business_day, guest_no)
    WHERE guest_no IS NOT NULL;
CREATE INDEX sessions_state_ends ON sessions (state, ends_at);
CREATE INDEX sessions_business_day ON sessions (business_day);

CREATE TABLE session_segments (
    id                   TEXT PRIMARY KEY,
    session_id           TEXT NOT NULL REFERENCES sessions (id),
    from_at              INTEGER NOT NULL,
    to_at                INTEGER NULL,
    device_id            TEXT NOT NULL REFERENCES devices (device_id),
    rate_minor_per_hour  INTEGER NOT NULL,
    rule_id              TEXT NULL,
    rev                  INTEGER NOT NULL,
    created_at           INTEGER NOT NULL,
    updated_at           INTEGER NOT NULL
);

CREATE INDEX session_segments_session ON session_segments (session_id);
