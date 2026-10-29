-- 0068: kitchen notes (Story 7.2). A kitchen product holds its preset notes as a JSON array of
-- strings; an order line holds the notes chosen for it (NULL when it has none). Notes never
-- change price, tax or stock.
ALTER TABLE products ADD COLUMN kitchen_notes TEXT NOT NULL DEFAULT '[]';
ALTER TABLE order_lines ADD COLUMN kitchen_notes TEXT NULL;
