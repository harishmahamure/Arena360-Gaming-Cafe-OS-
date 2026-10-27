CREATE TABLE orders (
    id             TEXT PRIMARY KEY,
    session_id     TEXT NULL REFERENCES sessions (id),
    player_id      TEXT NULL REFERENCES players (player_id),
    guest_no       INTEGER NULL,
    device_id      TEXT NULL REFERENCES devices (device_id),
    status         TEXT NOT NULL CHECK (status IN ('open', 'settled', 'void')),
    subtotal_minor INTEGER NOT NULL,
    total_minor    INTEGER NOT NULL,
    business_day   TEXT NOT NULL,
    fy             TEXT NOT NULL,
    rev            INTEGER NOT NULL,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
);

CREATE UNIQUE INDEX orders_one_per_session ON orders (session_id) WHERE session_id IS NOT NULL;
CREATE INDEX orders_status ON orders (status);

CREATE TABLE order_lines (
    id                 TEXT PRIMARY KEY,
    order_id           TEXT NOT NULL REFERENCES orders (id),
    session_segment_id TEXT NULL REFERENCES session_segments (id),
    device_id          TEXT NULL REFERENCES devices (device_id),
    from_at            INTEGER NOT NULL,
    to_at              INTEGER NOT NULL,
    minutes            INTEGER NOT NULL,
    rate_minor_per_hour INTEGER NOT NULL,
    amount_minor       INTEGER NOT NULL,
    position           INTEGER NOT NULL,
    rev                INTEGER NOT NULL,
    created_at         INTEGER NOT NULL,
    updated_at         INTEGER NOT NULL
);

CREATE INDEX order_lines_order ON order_lines (order_id, position);
