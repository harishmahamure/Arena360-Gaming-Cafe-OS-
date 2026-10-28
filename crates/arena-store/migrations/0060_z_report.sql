-- The Z-report of a closed shift (Story 5.28), stored as JSON on the shift row the close writes,
-- so the screen, the print and every later view show the figures that were printed. NULL for an
-- open shift and for a shift closed before this story (those are built from the rows on read).
ALTER TABLE shifts ADD COLUMN z_report TEXT NULL;

-- The Z-report reads a shift's rows by time, so the time columns it filters on get partial indexes.
CREATE INDEX orders_settled_at ON orders (settled_at) WHERE settled_at IS NOT NULL;
CREATE INDEX orders_voided_at ON orders (voided_at) WHERE voided_at IS NOT NULL;
CREATE INDEX stock_moves_waste_at ON stock_moves (at) WHERE waste_reason IS NOT NULL;
