CREATE TABLE pricing_rules (
    rule_id      TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    group_id     TEXT NOT NULL REFERENCES device_groups (group_id),
    rule_kind    TEXT NOT NULL CHECK (rule_kind IN ('weekly', 'date')),
    days_mask    INTEGER NULL CHECK (days_mask IS NULL OR days_mask BETWEEN 1 AND 127),
    start_min    INTEGER NULL CHECK (start_min IS NULL OR start_min BETWEEN 0 AND 1439),
    end_min      INTEGER NULL CHECK (end_min IS NULL OR end_min BETWEEN 0 AND 1439),
    date_from    TEXT NULL,
    date_to      TEXT NULL,
    adjust_kind  TEXT NOT NULL CHECK (adjust_kind IN ('override_minor', 'percent_bp')),
    adjust_value INTEGER NOT NULL,
    priority     INTEGER NOT NULL DEFAULT 0,
    active       INTEGER NOT NULL CHECK (active IN (0, 1)),
    rev          INTEGER NOT NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

CREATE INDEX pricing_rules_group ON pricing_rules (group_id, active);
