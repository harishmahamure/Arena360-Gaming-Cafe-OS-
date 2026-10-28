-- Invoice numbers (Story 5.14). A sale's number is `<branch code>/<FY>/E<epoch counter><tag>/<seq>`
-- and is written to `orders.invoice_no` by the `invoice.issue` intent. `invoice_sequences` holds
-- the next sequence number of each `(fy, segment)`: a new financial year or a new epoch opens a new
-- segment that starts again at 1, and an old segment is never written to again.
ALTER TABLE orders ADD COLUMN invoice_no TEXT NULL;

CREATE UNIQUE INDEX orders_invoice_no ON orders (invoice_no) WHERE invoice_no IS NOT NULL;
CREATE INDEX orders_unnumbered ON orders (settled_at)
    WHERE status = 'settled' AND invoice_no IS NULL AND total_minor > 0;

CREATE TABLE invoice_sequences (
    fy         TEXT NOT NULL,
    segment    TEXT NOT NULL,
    next       INTEGER NOT NULL CHECK (next >= 1),
    rev        INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (fy, segment)
);
