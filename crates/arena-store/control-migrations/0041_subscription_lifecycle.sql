-- Renewals that need approval, grace and lapse (Story 10.11). `grace_until` (cloud time) has been on
-- `subscriptions` since 0035; this adds the mark of a failed renewal charge on a subscription whose
-- paid period has not ended yet (it moves to `grace` when the period ends) and the index the
-- lifecycle sweep reads.
ALTER TABLE subscriptions ADD COLUMN renewal_failed_at INTEGER;

CREATE INDEX subscriptions_lifecycle ON subscriptions (state, current_period_end) WHERE state IN ('active', 'renewal_needs_approval', 'grace');
