CREATE TABLE stock (
    product_id     TEXT PRIMARY KEY REFERENCES products (product_id),
    qty            INTEGER NOT NULL CHECK (qty >= 0),
    avg_cost_minor INTEGER NOT NULL CHECK (avg_cost_minor >= 0),
    rev            INTEGER NOT NULL,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
);

CREATE TABLE stock_lots (
    lot_id          TEXT PRIMARY KEY,
    product_id      TEXT NOT NULL REFERENCES products (product_id),
    qty_in          INTEGER NOT NULL CHECK (qty_in > 0),
    qty_remaining   INTEGER NOT NULL CHECK (qty_remaining >= 0 AND qty_remaining <= qty_in),
    unit_cost_minor INTEGER NOT NULL CHECK (unit_cost_minor >= 0),
    expiry          TEXT NULL CHECK (
        expiry IS NULL OR expiry GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
    ),
    received_at     INTEGER NOT NULL,
    rev             INTEGER NOT NULL,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);

CREATE INDEX stock_lots_product ON stock_lots (product_id, expiry);

CREATE TABLE stock_moves (
    move_id               TEXT PRIMARY KEY,
    product_id            TEXT NOT NULL REFERENCES products (product_id),
    reason                TEXT NOT NULL,
    delta                 INTEGER NOT NULL CHECK (delta <> 0),
    before_qty            INTEGER NOT NULL CHECK (before_qty >= 0),
    before_avg_cost_minor INTEGER NOT NULL CHECK (before_avg_cost_minor >= 0),
    unit_cost_minor       INTEGER NOT NULL CHECK (unit_cost_minor >= 0),
    supplier              TEXT NOT NULL,
    lot_id                TEXT NULL,
    lots_used             TEXT NOT NULL,
    staff_id              TEXT NOT NULL,
    at                    INTEGER NOT NULL,
    rev                   INTEGER NOT NULL,
    created_at            INTEGER NOT NULL,
    updated_at            INTEGER NOT NULL
);

CREATE INDEX stock_moves_product_at ON stock_moves (product_id, at);

CREATE TRIGGER stock_moves_no_delete BEFORE DELETE ON stock_moves
BEGIN
    SELECT RAISE(ABORT, 'stock_moves is append-only');
END;

CREATE TRIGGER stock_moves_no_update BEFORE UPDATE OF
    move_id, product_id, reason, delta, before_qty, before_avg_cost_minor, unit_cost_minor,
    supplier, lot_id, lots_used, staff_id, at
ON stock_moves
BEGIN
    SELECT RAISE(ABORT, 'stock_moves is append-only');
END;
