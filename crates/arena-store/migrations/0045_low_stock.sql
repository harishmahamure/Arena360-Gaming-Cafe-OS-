ALTER TABLE products ADD COLUMN low_stock_qty INTEGER NULL
    CHECK (low_stock_qty IS NULL OR low_stock_qty >= 0);

ALTER TABLE alerts ADD COLUMN product_id TEXT NULL REFERENCES products (product_id);
ALTER TABLE alerts ADD COLUMN qty INTEGER NULL;
ALTER TABLE alerts ADD COLUMN threshold INTEGER NULL;

DROP INDEX alerts_open_per_branch;
CREATE UNIQUE INDEX alerts_open_per_branch ON alerts (kind, IFNULL(product_id, ''))
    WHERE acked_at IS NULL AND device_id IS NULL;
