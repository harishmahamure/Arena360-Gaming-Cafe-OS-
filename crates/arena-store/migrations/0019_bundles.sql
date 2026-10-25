ALTER TABLE products ADD COLUMN bundle INTEGER NOT NULL DEFAULT 0 CHECK (bundle IN (0, 1));

CREATE TABLE bundle_components (
    bundle_id    TEXT NOT NULL REFERENCES products (product_id),
    component_id TEXT NOT NULL REFERENCES products (product_id),
    qty          INTEGER NOT NULL CHECK (qty > 0),
    PRIMARY KEY (bundle_id, component_id),
    CHECK (bundle_id <> component_id)
) WITHOUT ROWID;

CREATE INDEX bundle_components_component ON bundle_components (component_id);
