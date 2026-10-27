CREATE TABLE group_bills (
    id              TEXT PRIMARY KEY,
    payer_player_id TEXT NULL REFERENCES players (player_id),
    payer_guest_no  INTEGER NULL,
    state           TEXT NOT NULL CHECK (state IN ('open', 'closed')),
    rev             INTEGER NOT NULL,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);

ALTER TABLE orders ADD COLUMN group_bill_id TEXT NULL REFERENCES group_bills (id);

CREATE INDEX sessions_group_bill ON sessions (group_bill_id) WHERE group_bill_id IS NOT NULL;
