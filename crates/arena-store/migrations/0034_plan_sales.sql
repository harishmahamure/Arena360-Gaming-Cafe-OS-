CREATE TEMP TABLE keep_order_lines AS SELECT * FROM order_lines;
CREATE TEMP TABLE keep_payments AS SELECT * FROM payments;
CREATE TEMP TABLE keep_wallet_txns AS SELECT * FROM wallet_txns;
DELETE FROM order_lines;
DELETE FROM payments;
DELETE FROM wallet_txns;

CREATE TABLE orders_new (
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
    updated_at     INTEGER NOT NULL,
    settled_at     INTEGER NULL,
    settled_by     TEXT NULL,
    kind           TEXT NOT NULL DEFAULT 'session'
                   CHECK (kind IN ('session', 'topup', 'credit_collect', 'plan_sale'))
);

INSERT INTO orders_new (id, session_id, player_id, guest_no, device_id, status, subtotal_minor,
                        total_minor, business_day, fy, rev, created_at, updated_at, settled_at,
                        settled_by, kind)
SELECT id, session_id, player_id, guest_no, device_id, status, subtotal_minor, total_minor,
       business_day, fy, rev, created_at, updated_at, settled_at, settled_by, kind
FROM orders;

DROP TABLE orders;
ALTER TABLE orders_new RENAME TO orders;

CREATE UNIQUE INDEX orders_one_per_session ON orders (session_id) WHERE session_id IS NOT NULL;
CREATE INDEX orders_status ON orders (status);

INSERT INTO order_lines SELECT * FROM keep_order_lines;
INSERT INTO payments SELECT * FROM keep_payments;
INSERT INTO wallet_txns SELECT * FROM keep_wallet_txns;
DROP TABLE keep_order_lines;
DROP TABLE keep_payments;
DROP TABLE keep_wallet_txns;

ALTER TABLE order_lines ADD COLUMN plan_id TEXT NULL REFERENCES plans (plan_id);
ALTER TABLE order_lines ADD COLUMN label TEXT NULL;

CREATE TABLE player_packs (
    id               TEXT PRIMARY KEY,
    player_id        TEXT NOT NULL REFERENCES players (player_id),
    plan_id          TEXT NOT NULL REFERENCES plans (plan_id),
    kind             TEXT NOT NULL CHECK (kind IN ('pack', 'pass')),
    name             TEXT NOT NULL,
    price_minor      INTEGER NOT NULL CHECK (price_minor > 0),
    minutes_total    INTEGER NULL CHECK (minutes_total IS NULL OR minutes_total > 0),
    minutes_left     INTEGER NULL CHECK (minutes_left IS NULL OR minutes_left >= 0),
    validity_days    INTEGER NOT NULL CHECK (validity_days > 0),
    window_start_min INTEGER NULL CHECK (window_start_min IS NULL OR window_start_min BETWEEN 0 AND 1439),
    window_end_min   INTEGER NULL CHECK (window_end_min IS NULL OR window_end_min BETWEEN 0 AND 1439),
    group_ids        TEXT NOT NULL,
    bought_at        INTEGER NOT NULL,
    expires_at       INTEGER NOT NULL,
    order_id         TEXT NOT NULL REFERENCES orders (id),
    rev              INTEGER NOT NULL,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);

CREATE INDEX player_packs_player ON player_packs (player_id, expires_at DESC, id DESC);
CREATE INDEX player_packs_order ON player_packs (order_id);
