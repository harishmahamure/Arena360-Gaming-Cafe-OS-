-- 0080: credit reminders (Story 8.31). `notify_outbox.kind` gains `credit_reminder`: a message
-- about a player's unpaid credit, not about an order or a session, so such a row has neither
-- `order_id` nor `session_id`. `reminder_day` is the business day it was raised for: a player gets
-- at most one credit reminder per business day, whoever raised it (the daily check here, or an
-- owner's one-tap reminder in Story 8.32). SQLite cannot alter a CHECK, so the table is rebuilt
-- with its rows copied.

CREATE TABLE notify_outbox_new (
    id                     TEXT PRIMARY KEY,
    purpose                TEXT NOT NULL CHECK (purpose IN ('transactional')),
    kind                   TEXT NOT NULL CHECK (kind IN (
        'receipt', 'low_balance', 'session_ending', 'console_5min', 'credit_reminder'
    )),
    order_id               TEXT NULL,
    player_id              TEXT NOT NULL REFERENCES players (player_id),
    consent_policy_version TEXT NOT NULL,
    not_before             INTEGER NOT NULL,
    state                  TEXT NOT NULL
        CHECK (state IN ('queued', 'sent', 'failed', 'skipped', 'expired')),
    rev                    INTEGER NOT NULL,
    created_at             INTEGER NOT NULL,
    updated_at             INTEGER NOT NULL,
    channel                TEXT NULL CHECK (channel IS NULL OR channel IN ('whatsapp', 'sms')),
    error_code             TEXT NULL,
    reason                 TEXT NULL,
    session_id             TEXT NULL,
    params                 TEXT NULL,
    rearmed_at             INTEGER NULL,
    reminder_day           TEXT NULL,
    CHECK (
        order_id IS NOT NULL OR session_id IS NOT NULL
        OR (kind = 'credit_reminder' AND reminder_day IS NOT NULL)
    )
);

INSERT INTO notify_outbox_new
    (id, purpose, kind, order_id, player_id, consent_policy_version, not_before, state, rev,
     created_at, updated_at, channel, error_code, reason, session_id, params, rearmed_at)
SELECT id, purpose, kind, order_id, player_id, consent_policy_version, not_before, state, rev,
       created_at, updated_at, channel, error_code, reason, session_id, params, rearmed_at
FROM notify_outbox;

DROP TABLE notify_outbox;
ALTER TABLE notify_outbox_new RENAME TO notify_outbox;

CREATE INDEX notify_outbox_due ON notify_outbox (not_before, id) WHERE state = 'queued';
CREATE INDEX notify_outbox_order ON notify_outbox (order_id, kind);
CREATE UNIQUE INDEX notify_outbox_session_once ON notify_outbox (session_id, kind)
    WHERE kind IN ('session_ending', 'console_5min');
CREATE INDEX notify_outbox_low_balance ON notify_outbox (player_id, id) WHERE kind = 'low_balance';
CREATE UNIQUE INDEX notify_outbox_credit_day ON notify_outbox (player_id, reminder_day)
    WHERE kind = 'credit_reminder';
