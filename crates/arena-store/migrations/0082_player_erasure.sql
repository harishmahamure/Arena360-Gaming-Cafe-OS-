-- 0082: erasing a player's personal data by crypto-shredding (Story 9.1). A tombstoned player
-- keeps their row (so money, history and consent stay linked to the id) with every personal
-- column cleared and `erased_at` set. Many erased players share the tombstone username and an
-- empty phone lookup, so the unique indexes ignore erased rows. The wallet ledger gains the two
-- reasons of the row that zeroes a balance at erasure (`erase_refund` paid out at the counter,
-- `erase_forfeit` given up); SQLite cannot alter a CHECK, so the table is rebuilt with its rows
-- copied (the pattern of 0061).
ALTER TABLE players ADD COLUMN erased_at INTEGER NULL;

DROP INDEX players_username;
DROP INDEX players_phone;
CREATE UNIQUE INDEX players_username ON players (username)
    WHERE deleted_at IS NULL AND erased_at IS NULL;
CREATE UNIQUE INDEX players_phone ON players (phone_lookup)
    WHERE deleted_at IS NULL AND erased_at IS NULL;

CREATE TABLE wallet_txns_new (
    id           TEXT PRIMARY KEY,
    player_id    TEXT NOT NULL REFERENCES players (player_id),
    delta_minor  INTEGER NOT NULL CHECK (delta_minor <> 0),
    reason       TEXT NOT NULL CHECK (reason IN (
        'topup', 'bill', 'credit', 'import', 'erase_refund', 'erase_forfeit'
    )),
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
