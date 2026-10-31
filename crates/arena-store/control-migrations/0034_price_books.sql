-- The price book (Story 10.1): every price Arena 360 charges, kept once per version. A book is a
-- `draft` until an operator publishes it, then `live`; publishing the next one retires it. A
-- book that is not a draft is immutable (a trigger refuses any change to its amounts), so a
-- later price change never reaches a subscriber who already pays an older book.
CREATE TABLE price_books (
    id TEXT PRIMARY KEY NOT NULL,
    version INTEGER NOT NULL UNIQUE,
    effective_from INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('draft', 'live', 'retired')),
    created_at INTEGER NOT NULL,
    published_at INTEGER
);

CREATE UNIQUE INDEX price_books_one_live ON price_books (status) WHERE status = 'live';

CREATE UNIQUE INDEX price_books_one_draft ON price_books (status) WHERE status = 'draft';

-- One price per (book, band, item, cycle). Amounts are minor units of `currency` (ISO 4217).
-- `razorpay_plan_id` is the Razorpay plan mirrored from the row; NULL until it is published
-- (or while `billing.provider = manual`).
CREATE TABLE price_book_items (
    price_book_id TEXT NOT NULL REFERENCES price_books (id),
    band TEXT NOT NULL CHECK (band IN ('IN', 'MY', 'SG', 'US')),
    currency TEXT NOT NULL CHECK (length(currency) = 3),
    item TEXT NOT NULL CHECK (item IN ('basic', 'premium', 'addon_devices_20', 'addon_tunnel', 'addon_owner_login')),
    cycle TEXT NOT NULL CHECK (cycle IN ('monthly', 'yearly')),
    amount_minor INTEGER NOT NULL CHECK (amount_minor > 0),
    gst_applicable INTEGER NOT NULL CHECK (gst_applicable IN (0, 1)),
    razorpay_plan_id TEXT,
    PRIMARY KEY (price_book_id, band, item, cycle)
);

CREATE TRIGGER price_book_items_no_edit BEFORE UPDATE OF price_book_id, band, currency, item, cycle, amount_minor, gst_applicable ON price_book_items
WHEN (SELECT status FROM price_books WHERE id = OLD.price_book_id) <> 'draft'
BEGIN
    SELECT RAISE(ABORT, 'PRICE_BOOK_IMMUTABLE');
END;

CREATE TRIGGER price_book_items_no_delete BEFORE DELETE ON price_book_items
WHEN (SELECT status FROM price_books WHERE id = OLD.price_book_id) <> 'draft'
BEGIN
    SELECT RAISE(ABORT, 'PRICE_BOOK_IMMUTABLE');
END;

CREATE TRIGGER price_book_items_no_insert BEFORE INSERT ON price_book_items
WHEN (SELECT status FROM price_books WHERE id = NEW.price_book_id) <> 'draft'
BEGIN
    SELECT RAISE(ABORT, 'PRICE_BOOK_IMMUTABLE');
END;

-- A book never goes back to a draft, and a published book keeps its id and version.
CREATE TRIGGER price_books_no_reopen BEFORE UPDATE ON price_books
WHEN OLD.status <> 'draft' AND (NEW.status = 'draft' OR NEW.id <> OLD.id OR NEW.version <> OLD.version)
BEGIN
    SELECT RAISE(ABORT, 'PRICE_BOOK_IMMUTABLE');
END;

-- Which bands are on sale. A band is switched on by an operator once Razorpay Subscriptions
-- coverage for its country is confirmed; until then pricing for it answers BAND_NOT_AVAILABLE.
CREATE TABLE billing_bands (
    band TEXT PRIMARY KEY NOT NULL CHECK (band IN ('IN', 'MY', 'SG', 'US')),
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    updated_at INTEGER NOT NULL
);

INSERT INTO billing_bands (band, enabled, updated_at) VALUES
    ('IN', 1, 0), ('MY', 0, 0), ('SG', 0, 0), ('US', 0, 0);

-- Work for an operator to do by hand: what a Razorpay call would have done while
-- `billing.provider = manual`.
CREATE TABLE ops_tasks (
    id TEXT PRIMARY KEY NOT NULL,
    at INTEGER NOT NULL,
    kind TEXT NOT NULL,
    target TEXT NOT NULL,
    details_json TEXT NOT NULL DEFAULT '{}',
    done_at INTEGER
);

CREATE INDEX ops_tasks_open ON ops_tasks (kind, at) WHERE done_at IS NULL;

-- The seed book (a draft while its rows go in, then live). Monthly amounts per band in minor units; every yearly
-- amount is exactly ten times its monthly one. GST applies to band IN only. Free is never billed.
INSERT INTO price_books (id, version, effective_from, status, created_at, published_at)
VALUES ('pb_v1', 1, 0, 'draft', 0, NULL);

WITH monthly (band, currency, item, amount_minor) AS (
    VALUES
    ('IN', 'INR', 'basic', 20000), ('IN', 'INR', 'premium', 199900),
    ('IN', 'INR', 'addon_devices_20', 5000), ('IN', 'INR', 'addon_tunnel', 20000),
    ('IN', 'INR', 'addon_owner_login', 5000),
    ('MY', 'MYR', 'basic', 2300), ('MY', 'MYR', 'premium', 22900),
    ('MY', 'MYR', 'addon_devices_20', 600), ('MY', 'MYR', 'addon_tunnel', 2300),
    ('MY', 'MYR', 'addon_owner_login', 600),
    ('SG', 'SGD', 'basic', 800), ('SG', 'SGD', 'premium', 7900),
    ('SG', 'SGD', 'addon_devices_20', 200), ('SG', 'SGD', 'addon_tunnel', 800),
    ('SG', 'SGD', 'addon_owner_login', 200),
    ('US', 'USD', 'basic', 600), ('US', 'USD', 'premium', 5900),
    ('US', 'USD', 'addon_devices_20', 150), ('US', 'USD', 'addon_tunnel', 600),
    ('US', 'USD', 'addon_owner_login', 150)
)
INSERT INTO price_book_items (price_book_id, band, currency, item, cycle, amount_minor, gst_applicable, razorpay_plan_id)
SELECT 'pb_v1', band, currency, item, 'monthly', amount_minor, band = 'IN', NULL FROM monthly
UNION ALL
SELECT 'pb_v1', band, currency, item, 'yearly', amount_minor * 10, band = 'IN', NULL FROM monthly;

UPDATE price_books SET status = 'live', published_at = 0 WHERE id = 'pb_v1';
