-- Imported opening balances (Story 5.34): a `wallet_txns` row with reason `import` belongs to no
-- order (no cash event, no invoice), so `order_id` may be NULL and the reason list gains `import`.
CREATE TABLE wallet_txns_new (
    id           TEXT PRIMARY KEY,
    player_id    TEXT NOT NULL REFERENCES players (player_id),
    delta_minor  INTEGER NOT NULL CHECK (delta_minor <> 0),
    reason       TEXT NOT NULL CHECK (reason IN ('topup', 'bill', 'credit', 'import')),
    method       TEXT NULL,
    staff_id     TEXT NOT NULL,
    order_id     TEXT NULL REFERENCES orders (id),
    business_day TEXT NOT NULL,
    fy           TEXT NOT NULL,
    rev          INTEGER NOT NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

INSERT INTO wallet_txns_new SELECT * FROM wallet_txns;
DROP TABLE wallet_txns;
ALTER TABLE wallet_txns_new RENAME TO wallet_txns;
CREATE INDEX wallet_txns_player ON wallet_txns (player_id, created_at DESC, id DESC);
CREATE INDEX wallet_txns_order ON wallet_txns (order_id);
