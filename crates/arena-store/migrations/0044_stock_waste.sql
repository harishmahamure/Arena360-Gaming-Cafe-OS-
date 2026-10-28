ALTER TABLE stock_moves ADD COLUMN waste_reason TEXT NULL;
ALTER TABLE stock_moves ADD COLUMN value_minor INTEGER NOT NULL DEFAULT 0;

DROP TRIGGER stock_moves_no_update;

CREATE TRIGGER stock_moves_no_update BEFORE UPDATE OF
    move_id, product_id, reason, delta, before_qty, before_avg_cost_minor, unit_cost_minor,
    supplier, lot_id, lots_used, staff_id, at, waste_reason, value_minor
ON stock_moves
BEGIN
    SELECT RAISE(ABORT, 'stock_moves is append-only');
END;
