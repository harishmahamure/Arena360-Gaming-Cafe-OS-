CREATE TABLE products (
    product_id   TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    category     TEXT NOT NULL,
    price_minor  INTEGER NOT NULL CHECK (price_minor >= 0),
    tax_rate_bp  INTEGER NOT NULL CHECK (tax_rate_bp BETWEEN 0 AND 10000),
    hsn          TEXT NOT NULL,
    track_stock  INTEGER NOT NULL CHECK (track_stock IN (0, 1)),
    image        TEXT NOT NULL,
    barcode      TEXT NULL,
    kitchen      INTEGER NOT NULL CHECK (kitchen IN (0, 1)),
    favourite    INTEGER NOT NULL CHECK (favourite IN (0, 1)),
    active       INTEGER NOT NULL CHECK (active IN (0, 1)),
    rev          INTEGER NOT NULL,
    deleted_at   INTEGER NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

CREATE UNIQUE INDEX products_barcode ON products (barcode)
    WHERE barcode IS NOT NULL AND active = 1 AND deleted_at IS NULL;
