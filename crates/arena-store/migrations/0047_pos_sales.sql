-- A counter sale is an order of kind `pos_sale`: rebuild `orders` to widen the kind check and to
-- carry the per-rate tax breakup. The child rows are kept aside while the table is replaced.
CREATE TEMP TABLE keep_order_lines AS SELECT * FROM order_lines;
CREATE TEMP TABLE keep_payments AS SELECT * FROM payments;
CREATE TEMP TABLE keep_wallet_txns AS SELECT * FROM wallet_txns;
CREATE TEMP TABLE keep_player_packs AS SELECT * FROM player_packs;
DELETE FROM order_lines;
DELETE FROM payments;
DELETE FROM wallet_txns;
DELETE FROM player_packs;

CREATE TABLE orders_new (
    id                 TEXT PRIMARY KEY,
    session_id         TEXT NULL REFERENCES sessions (id),
    player_id          TEXT NULL REFERENCES players (player_id),
    guest_no           INTEGER NULL,
    device_id          TEXT NULL REFERENCES devices (device_id),
    status             TEXT NOT NULL CHECK (status IN ('open', 'settled', 'void')),
    subtotal_minor     INTEGER NOT NULL,
    total_minor        INTEGER NOT NULL,
    business_day       TEXT NOT NULL,
    fy                 TEXT NOT NULL,
    rev                INTEGER NOT NULL,
    created_at         INTEGER NOT NULL,
    updated_at         INTEGER NOT NULL,
    settled_at         INTEGER NULL,
    settled_by         TEXT NULL,
    kind               TEXT NOT NULL DEFAULT 'session'
                       CHECK (kind IN ('session', 'topup', 'credit_collect', 'plan_sale',
                                       'pos_sale')),
    extends_session_id TEXT NULL REFERENCES sessions (id),
    group_bill_id      TEXT NULL REFERENCES group_bills (id),
    tax_minor          INTEGER NOT NULL DEFAULT 0,
    tax_breakup        TEXT NULL
);

INSERT INTO orders_new (id, session_id, player_id, guest_no, device_id, status, subtotal_minor,
                        total_minor, business_day, fy, rev, created_at, updated_at, settled_at,
                        settled_by, kind, extends_session_id, group_bill_id)
SELECT id, session_id, player_id, guest_no, device_id, status, subtotal_minor, total_minor,
       business_day, fy, rev, created_at, updated_at, settled_at, settled_by, kind,
       extends_session_id, group_bill_id
FROM orders;

DROP TABLE orders;
ALTER TABLE orders_new RENAME TO orders;

CREATE UNIQUE INDEX orders_one_per_session ON orders (session_id) WHERE session_id IS NOT NULL;
CREATE INDEX orders_status ON orders (status);
CREATE INDEX orders_kind_day ON orders (kind, business_day);

INSERT INTO order_lines SELECT * FROM keep_order_lines;
INSERT INTO payments SELECT * FROM keep_payments;
INSERT INTO wallet_txns SELECT * FROM keep_wallet_txns;
INSERT INTO player_packs SELECT * FROM keep_player_packs;
DROP TABLE keep_order_lines;
DROP TABLE keep_payments;
DROP TABLE keep_wallet_txns;
DROP TABLE keep_player_packs;

-- A product line: what was sold, at what price and rate.
ALTER TABLE order_lines ADD COLUMN product_id TEXT NULL REFERENCES products (product_id);
ALTER TABLE order_lines ADD COLUMN qty INTEGER NULL CHECK (qty IS NULL OR qty > 0);
ALTER TABLE order_lines ADD COLUMN unit_price_minor INTEGER NULL
    CHECK (unit_price_minor IS NULL OR unit_price_minor >= 0);
ALTER TABLE order_lines ADD COLUMN tax_rate_bp INTEGER NULL;

-- A stock move names the sale it belongs to (no foreign key: the ledger outlives an undone
-- sale); an override's raise carries a note and the manager who approved it.
ALTER TABLE stock_moves ADD COLUMN order_id TEXT NULL;
ALTER TABLE stock_moves ADD COLUMN note TEXT NULL;
ALTER TABLE stock_moves ADD COLUMN elevated_by TEXT NULL;
CREATE INDEX stock_moves_order ON stock_moves (order_id) WHERE order_id IS NOT NULL;

DROP TRIGGER stock_moves_no_update;

CREATE TRIGGER stock_moves_no_update BEFORE UPDATE OF
    move_id, product_id, reason, delta, before_qty, before_avg_cost_minor, unit_cost_minor,
    supplier, lot_id, lots_used, staff_id, at, waste_reason, value_minor, order_id, note,
    elevated_by
ON stock_moves
BEGIN
    SELECT RAISE(ABORT, 'stock_moves is append-only');
END;
