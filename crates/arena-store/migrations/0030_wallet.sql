ALTER TABLE orders ADD COLUMN kind TEXT NOT NULL DEFAULT 'session' CHECK (kind IN ('session', 'topup'));

CREATE TABLE wallets (
    player_id     TEXT PRIMARY KEY REFERENCES players (player_id),
    balance_minor INTEGER NOT NULL CHECK (balance_minor >= 0),
    rev           INTEGER NOT NULL,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);

INSERT INTO wallets (player_id, balance_minor, rev, created_at, updated_at)
SELECT player_id, 0, 0, 0, 0 FROM players;

CREATE TABLE wallet_txns (
    id           TEXT PRIMARY KEY,
    player_id    TEXT NOT NULL REFERENCES players (player_id),
    delta_minor  INTEGER NOT NULL CHECK (delta_minor <> 0),
    reason       TEXT NOT NULL CHECK (reason IN ('topup', 'bill')),
    method       TEXT NULL,
    staff_id     TEXT NOT NULL,
    order_id     TEXT NOT NULL REFERENCES orders (id),
    business_day TEXT NOT NULL,
    fy           TEXT NOT NULL,
    rev          INTEGER NOT NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

CREATE INDEX wallet_txns_player ON wallet_txns (player_id, created_at DESC, id DESC);
CREATE INDEX wallet_txns_order ON wallet_txns (order_id);
