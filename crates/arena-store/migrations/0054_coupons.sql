-- Coupons applied at settle (Story 5.21). A coupon is a percent (`percent_bp`) or a fixed amount
-- (`amount_minor`), never both, stored under an upper-cased code that is unique on the branch.
-- `valid_from` / `valid_to` are local `YYYY-MM-DD` dates (inclusive), `used` counts the settles
-- that took it and is raised and lowered only inside a settle and its Undo. `coupon_targets` lists
-- the device groups, plans and products it is limited to (none = the whole bill); the target ids
-- have no foreign key so a retired product or group never blocks the list. Neither table carries
-- anything about the customer.
CREATE TABLE coupons (
    coupon_id      TEXT PRIMARY KEY,
    code           TEXT NOT NULL UNIQUE CHECK (code = upper(code) AND length(code) BETWEEN 3 AND 24),
    percent_bp     INTEGER NULL CHECK (percent_bp IS NULL OR percent_bp BETWEEN 1 AND 10000),
    amount_minor   INTEGER NULL CHECK (amount_minor IS NULL OR amount_minor > 0),
    valid_from     TEXT NULL,
    valid_to       TEXT NULL,
    usage_limit    INTEGER NULL CHECK (usage_limit IS NULL OR usage_limit >= 1),
    used           INTEGER NOT NULL DEFAULT 0 CHECK (used >= 0),
    min_bill_minor INTEGER NOT NULL DEFAULT 0 CHECK (min_bill_minor >= 0),
    active         INTEGER NOT NULL CHECK (active IN (0, 1)),
    rev            INTEGER NOT NULL,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    CHECK ((percent_bp IS NULL) <> (amount_minor IS NULL))
);

CREATE TABLE coupon_targets (
    coupon_id   TEXT NOT NULL REFERENCES coupons (coupon_id),
    target_kind TEXT NOT NULL CHECK (target_kind IN ('group', 'plan', 'product')),
    target_id   TEXT NOT NULL,
    PRIMARY KEY (coupon_id, target_kind, target_id)
) WITHOUT ROWID;

-- The coupon a settled order took and the amount it took off (the order total is already net of
-- it; a session or plan order's `subtotal_minor` too). Both NULL when no coupon was used.
ALTER TABLE orders ADD COLUMN coupon_code TEXT NULL;
ALTER TABLE orders ADD COLUMN coupon_minor INTEGER NULL
    CHECK (coupon_minor IS NULL OR coupon_minor > 0);
