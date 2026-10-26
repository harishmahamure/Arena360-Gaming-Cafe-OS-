CREATE TABLE rates (
    rate_id                TEXT PRIMARY KEY,
    group_id               TEXT NOT NULL REFERENCES device_groups (group_id),
    price_per_hour_minor   INTEGER NOT NULL CHECK (price_per_hour_minor > 0),
    active                 INTEGER NOT NULL CHECK (active IN (0, 1)),
    rev                    INTEGER NOT NULL,
    created_at             INTEGER NOT NULL,
    updated_at             INTEGER NOT NULL
);

CREATE UNIQUE INDEX rates_one_active ON rates (group_id) WHERE active = 1;
