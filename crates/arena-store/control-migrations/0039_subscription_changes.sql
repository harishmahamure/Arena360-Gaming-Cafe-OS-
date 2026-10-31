-- A change of tier or add-ons on a paid subscription (Story 10.9). An upgrade starts at once and is
-- paid with a prorated charge: `charging` until Razorpay confirms the charge, then `applied`. A
-- downgrade or a removal waits for the period end: `scheduled`. A change whose new renewal debit
-- needs a card or eNACH mandate the subscription does not have is `held` until that mandate exists
-- (`razorpay_subscription_id` is the new Razorpay subscription the owner authorises). The
-- entitlement and the subscription's own tier and add-ons change only when the change is applied.
-- One change is open per subscription; a new one supersedes the old.
CREATE TABLE subscription_changes (
    id TEXT PRIMARY KEY NOT NULL,
    subscription_id TEXT NOT NULL REFERENCES subscriptions (id),
    kind TEXT NOT NULL CHECK (kind IN ('now', 'period_end')),
    status TEXT NOT NULL CHECK (status IN ('held', 'charging', 'scheduled', 'applied', 'withdrawn', 'superseded')),
    tier TEXT NOT NULL CHECK (tier IN ('basic', 'premium')),
    addon_devices_20 INTEGER NOT NULL CHECK (addon_devices_20 BETWEEN 0 AND 10),
    addon_tunnel INTEGER NOT NULL CHECK (addon_tunnel IN (0, 1)),
    addon_owner_login INTEGER NOT NULL DEFAULT 0 CHECK (addon_owner_login >= 0),
    -- The prorated charge, GST included, in minor units; 0 for a change at the period end.
    charge_minor INTEGER NOT NULL CHECK (charge_minor >= 0),
    -- What each renewal debits after the change, GST included.
    renewal_minor INTEGER NOT NULL CHECK (renewal_minor >= 0),
    -- When the change takes effect: now for an upgrade, the period end otherwise.
    effective_at INTEGER NOT NULL,
    razorpay_subscription_id TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX subscription_changes_one_open ON subscription_changes (subscription_id) WHERE status IN ('held', 'charging', 'scheduled');

CREATE UNIQUE INDEX subscription_changes_razorpay_id ON subscription_changes (razorpay_subscription_id) WHERE razorpay_subscription_id IS NOT NULL;
