ALTER TABLE orders ADD COLUMN settled_at INTEGER NULL;
ALTER TABLE orders ADD COLUMN settled_by TEXT NULL;

CREATE TABLE payments (
    id           TEXT PRIMARY KEY,
    order_id     TEXT NOT NULL REFERENCES orders (id),
    method       TEXT NOT NULL,
    amount_minor INTEGER NOT NULL CHECK (amount_minor > 0),
    reference    TEXT NULL,
    staff_id     TEXT NOT NULL,
    position     INTEGER NOT NULL,
    business_day TEXT NOT NULL,
    fy           TEXT NOT NULL,
    rev          INTEGER NOT NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

CREATE INDEX payments_order ON payments (order_id, position);
