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
    kind           TEXT NOT NULL DEFAULT 'session' CHECK (kind IN ('session', 'topup', 'credit_collect'))
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
