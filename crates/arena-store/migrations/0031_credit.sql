CREATE TABLE wallet_txns_new (
    id           TEXT PRIMARY KEY,
    player_id    TEXT NOT NULL REFERENCES players (player_id),
    delta_minor  INTEGER NOT NULL CHECK (delta_minor <> 0),
    reason       TEXT NOT NULL CHECK (reason IN ('topup', 'bill', 'credit')),
    method       TEXT NULL,
    staff_id     TEXT NOT NULL,
    order_id     TEXT NOT NULL REFERENCES orders (id),
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

CREATE TABLE credit_balances (
    player_id         TEXT PRIMARY KEY REFERENCES players (player_id),
    limit_minor       INTEGER NOT NULL CHECK (limit_minor >= 0),
    outstanding_minor INTEGER NOT NULL CHECK (outstanding_minor >= 0),
    reminder_days     TEXT NOT NULL,
    credit_paused     INTEGER NOT NULL DEFAULT 0 CHECK (credit_paused IN (0, 1)),
    last_override_day TEXT NULL,
    rev               INTEGER NOT NULL,
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL
);
