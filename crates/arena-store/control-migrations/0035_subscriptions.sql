-- Branch subscriptions (Story 10.2): what an owner signed up for. One row per attempt; a branch
-- holds at most one row that is not over. The PPP band and the price book are copied onto the row
-- when it is created and never change, so a later price book publish cannot reprice a subscriber
-- (a change of tier, cycle or add-ons is a later story). Amounts are never stored here: they are
-- always priced from this row's own book. `razorpay_subscription_id` is empty until the provider
-- has answered; `first_paid_at` is written by the webhook story, never by this one.
CREATE TABLE subscriptions (
    id TEXT PRIMARY KEY NOT NULL,
    tenant_id TEXT NOT NULL REFERENCES tenants (id),
    branch_id TEXT NOT NULL REFERENCES branches (id),
    state TEXT NOT NULL CHECK (state IN ('pending', 'pending_review', 'active', 'renewal_needs_approval', 'grace', 'lapsed', 'cancelled', 'ended', 'refunded', 'abandoned')),
    tier TEXT NOT NULL CHECK (tier IN ('basic', 'premium')),
    cycle TEXT NOT NULL CHECK (cycle IN ('monthly', 'yearly')),
    band TEXT NOT NULL CHECK (band IN ('IN', 'MY', 'SG', 'US')),
    currency TEXT NOT NULL CHECK (length(currency) = 3),
    price_book_id TEXT NOT NULL REFERENCES price_books (id),
    addon_devices_20 INTEGER NOT NULL DEFAULT 0 CHECK (addon_devices_20 BETWEEN 0 AND 10),
    addon_tunnel INTEGER NOT NULL DEFAULT 0 CHECK (addon_tunnel IN (0, 1)),
    addon_owner_login INTEGER NOT NULL DEFAULT 0 CHECK (addon_owner_login >= 0),
    mandate_method TEXT CHECK (mandate_method IS NULL OR mandate_method IN ('upi', 'card', 'emandate')),
    razorpay_subscription_id TEXT,
    rev INTEGER NOT NULL DEFAULT 1,
    current_period_end INTEGER,
    grace_until INTEGER,
    first_paid_at INTEGER,
    cancel_at_period_end INTEGER NOT NULL DEFAULT 0 CHECK (cancel_at_period_end IN (0, 1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- A branch holds one subscription that is not over; abandoned, cancelled and lapsed rows are over
-- (a lapsed owner can subscribe again).
CREATE UNIQUE INDEX subscriptions_one_open_per_branch ON subscriptions (branch_id) WHERE state NOT IN ('abandoned', 'cancelled', 'lapsed');

CREATE UNIQUE INDEX subscriptions_razorpay_id ON subscriptions (razorpay_subscription_id) WHERE razorpay_subscription_id IS NOT NULL;

CREATE INDEX subscriptions_pending_by_age ON subscriptions (created_at) WHERE state = 'pending';
