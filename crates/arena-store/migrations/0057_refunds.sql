-- Refunds with credit notes (Story 5.24). A refund pays part of a settled, invoiced order back and
-- is stamped with a credit note number from the invoice series (the same `(fy, segment)` counter).
-- `payments` now also holds the negative rows a refund writes (`amount_minor <> 0`), tied to their
-- refund by `refund_id`. The invoice's own payment rows are untouched.
CREATE TABLE payments_new (
    id           TEXT PRIMARY KEY,
    order_id     TEXT NOT NULL REFERENCES orders (id),
    method       TEXT NOT NULL,
    amount_minor INTEGER NOT NULL CHECK (amount_minor <> 0),
    reference    TEXT NULL,
    staff_id     TEXT NOT NULL,
    position     INTEGER NOT NULL,
    business_day TEXT NOT NULL,
    fy           TEXT NOT NULL,
    rev          INTEGER NOT NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    refund_id    TEXT NULL
);

INSERT INTO payments_new (id, order_id, method, amount_minor, reference, staff_id, position,
                          business_day, fy, rev, created_at, updated_at)
SELECT id, order_id, method, amount_minor, reference, staff_id, position, business_day, fy, rev,
       created_at, updated_at
FROM payments;

DROP TABLE payments;
ALTER TABLE payments_new RENAME TO payments;
CREATE INDEX payments_order ON payments (order_id, position);
CREATE INDEX payments_refund ON payments (refund_id) WHERE refund_id IS NOT NULL;

-- One row per refund: the credit note number, why, how the money went back, the total (tax
-- included), the part that cut the player's outstanding credit, the tax per rate of the refunded
-- lines and the lines themselves (a JSON array). `approved_by` is the elevating manager, else the
-- staff member who issued it. Rows are never edited: a refund has no Undo.
CREATE TABLE refunds (
    id             TEXT PRIMARY KEY,
    order_id       TEXT NOT NULL REFERENCES orders (id),
    credit_note_no TEXT NOT NULL UNIQUE,
    reason         TEXT NOT NULL CHECK (reason IN ('changed_mind', 'mistake', 'hardware_issue', 'other')),
    note           TEXT NULL,
    method         TEXT NULL,
    reference      TEXT NULL,
    amount_minor   INTEGER NOT NULL CHECK (amount_minor > 0),
    credit_minor   INTEGER NOT NULL DEFAULT 0 CHECK (credit_minor >= 0),
    tax_minor      INTEGER NOT NULL DEFAULT 0,
    tax_breakup    TEXT NULL,
    lines          TEXT NOT NULL,
    staff_id       TEXT NOT NULL,
    approved_by    TEXT NOT NULL,
    business_day   TEXT NOT NULL,
    fy             TEXT NOT NULL,
    rev            INTEGER NOT NULL,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
);

CREATE INDEX refunds_order ON refunds (order_id);
CREATE INDEX refunds_day ON refunds (business_day);

-- What has been refunded of each line so far: units, and money (net of coupon and discount).
ALTER TABLE order_lines ADD COLUMN refunded_qty INTEGER NOT NULL DEFAULT 0
    CHECK (refunded_qty >= 0);
ALTER TABLE order_lines ADD COLUMN refunded_minor INTEGER NOT NULL DEFAULT 0
    CHECK (refunded_minor >= 0);
