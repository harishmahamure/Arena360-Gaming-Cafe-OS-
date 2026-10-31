-- Cancel at the end of the period (Story 10.14). A subscription that ended (or was refunded) is over:
-- the branch may subscribe again, so it no longer holds the branch's one open-subscription slot.
-- The sweep that ends cancelled subscriptions reads the index.
DROP INDEX subscriptions_one_open_per_branch;

CREATE UNIQUE INDEX subscriptions_one_open_per_branch ON subscriptions (branch_id) WHERE state NOT IN ('abandoned', 'cancelled', 'lapsed', 'ended', 'refunded');

CREATE INDEX subscriptions_cancel_due ON subscriptions (current_period_end) WHERE state = 'active' AND cancel_at_period_end = 1;
