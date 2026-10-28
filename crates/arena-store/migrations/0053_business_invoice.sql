-- Business (B2B) invoices (Story 5.20). A settle on a GST branch may name the business the invoice
-- is made out to: its GSTIN and legal name are stored on the order at settle. An order with a
-- `buyer_gstin` is B2B for GSTR-1 (Story 5.29); both columns are set together or not at all, and
-- stay NULL for a consumer (B2C) sale and on every order settled before this migration.
ALTER TABLE orders ADD COLUMN buyer_gstin TEXT NULL;
ALTER TABLE orders ADD COLUMN buyer_name TEXT NULL
    CHECK (buyer_name IS NULL OR buyer_gstin IS NOT NULL);

CREATE INDEX orders_b2b ON orders (settled_at) WHERE buyer_gstin IS NOT NULL;
