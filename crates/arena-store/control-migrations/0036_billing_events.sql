-- Razorpay events and the subscription life they drive (Story 10.3). `billing_events` holds every
-- event once: the unique Razorpay event id is what makes a repeated delivery (or a reconcile that
-- sees the same state again) a no-op, and the row is written in the same transaction as the
-- change it caused. `subscription_history` records every transition with its cause (the event id
-- or a job) and the cloud time. `payment_country` is the country Razorpay reported for the first
-- successful payment, kept so a later event without a payment can still be checked.
ALTER TABLE subscriptions ADD COLUMN payment_country TEXT;

CREATE TABLE billing_events (
    id TEXT PRIMARY KEY NOT NULL,
    razorpay_event_id TEXT NOT NULL UNIQUE,
    type TEXT NOT NULL,
    subscription_id TEXT REFERENCES subscriptions (id),
    payload_hash TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('webhook', 'reconcile')),
    received_at INTEGER NOT NULL,
    applied_at INTEGER
);

CREATE INDEX billing_events_by_subscription ON billing_events (subscription_id, received_at);

CREATE TABLE subscription_history (
    id TEXT PRIMARY KEY NOT NULL,
    subscription_id TEXT NOT NULL REFERENCES subscriptions (id),
    old_state TEXT NOT NULL,
    new_state TEXT NOT NULL,
    cause TEXT NOT NULL,
    cloud_time INTEGER NOT NULL
);

CREATE INDEX subscription_history_by_subscription ON subscription_history (subscription_id, cloud_time);
