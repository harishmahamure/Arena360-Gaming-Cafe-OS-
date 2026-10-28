-- Products placed on a running session's bill order (Story 5.12). A placement is one `order.place`
-- intent: its id is stored on the lines it added and on the stock moves it wrote, so its Undo
-- takes back that placement alone.
ALTER TABLE order_lines ADD COLUMN placement_id TEXT NULL;
ALTER TABLE order_lines ADD COLUMN placed_by TEXT NULL;
ALTER TABLE order_lines ADD COLUMN placed_at INTEGER NULL;
CREATE INDEX order_lines_placement ON order_lines (placement_id) WHERE placement_id IS NOT NULL;

ALTER TABLE stock_moves ADD COLUMN placement_id TEXT NULL;
CREATE INDEX stock_moves_placement ON stock_moves (placement_id) WHERE placement_id IS NOT NULL;

DROP TRIGGER stock_moves_no_update;

CREATE TRIGGER stock_moves_no_update BEFORE UPDATE OF
    move_id, product_id, reason, delta, before_qty, before_avg_cost_minor, unit_cost_minor,
    supplier, lot_id, lots_used, staff_id, at, waste_reason, value_minor, order_id, note,
    elevated_by, placement_id
ON stock_moves
BEGIN
    SELECT RAISE(ABORT, 'stock_moves is append-only');
END;
