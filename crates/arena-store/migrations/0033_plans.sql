CREATE TABLE plans (
    plan_id          TEXT PRIMARY KEY,
    kind             TEXT NOT NULL CHECK (kind IN ('pack', 'block', 'pass')),
    name             TEXT NOT NULL,
    price_minor      INTEGER NOT NULL CHECK (price_minor > 0),
    minutes          INTEGER NULL CHECK (minutes IS NULL OR minutes > 0),
    validity_days    INTEGER NULL CHECK (validity_days IS NULL OR validity_days > 0),
    window_start_min INTEGER NULL CHECK (window_start_min IS NULL OR window_start_min BETWEEN 0 AND 1439),
    window_end_min   INTEGER NULL CHECK (window_end_min IS NULL OR window_end_min BETWEEN 0 AND 1439),
    active           INTEGER NOT NULL CHECK (active IN (0, 1)),
    rev              INTEGER NOT NULL,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);

CREATE TABLE plan_groups (
    plan_id  TEXT NOT NULL REFERENCES plans (plan_id),
    group_id TEXT NOT NULL REFERENCES device_groups (group_id),
    PRIMARY KEY (plan_id, group_id)
) WITHOUT ROWID;

CREATE INDEX plan_groups_group ON plan_groups (group_id);
