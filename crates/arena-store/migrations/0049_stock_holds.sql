-- Stock taken at "served" (Story 5.13). A placed line is served or not; a kitchen line placed
-- while the branch takes stock at served holds its stock instead of taking it. A hold is one
-- tracked item of one line: open until the line is served (the stock is taken) or cancelled
-- (the hold is released). `order_line_id` has no foreign key: the hold outlives a cancelled line.
ALTER TABLE order_lines ADD COLUMN served INTEGER NOT NULL DEFAULT 1 CHECK (served IN (0, 1));
ALTER TABLE order_lines ADD COLUMN served_at INTEGER NULL;
ALTER TABLE order_lines ADD COLUMN served_by TEXT NULL;

CREATE TABLE stock_holds (
    id            TEXT PRIMARY KEY,
    order_line_id TEXT NOT NULL,
    item_id       TEXT NOT NULL REFERENCES products (product_id),
    qty           INTEGER NOT NULL CHECK (qty > 0),
    released_at   INTEGER NULL,
    rev           INTEGER NOT NULL,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);

CREATE INDEX stock_holds_line ON stock_holds (order_line_id);
CREATE INDEX stock_holds_open ON stock_holds (item_id) WHERE released_at IS NULL;
