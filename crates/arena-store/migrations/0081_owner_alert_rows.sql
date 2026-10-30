-- 0081: owner alerts raised by the Hub (Story 8.34). `notify_outbox` gains purpose `owner_alert`:
-- a row that makes the cloud raise an alert for the café owner, not a message to a player. Its
-- kind is one of the five alerts a Hub sources (`cash_variance`, `low_stock`, `credit_overdue`,
-- `large_refund`, `large_discount`), `player_id` may be empty (only a `credit_overdue` names a
-- player), `channel` may be `app` (the alert reached the owner's app) and `source_ref` names the
-- event it was raised for (a shift, a Hub alert, `<player>:<day>`, a credit note, an order). A row
-- of purpose `owner_alert` needs `source_ref`, and at most one exists per `(kind, source_ref)`:
-- that is what raises an alert once per event. SQLite cannot alter a CHECK, so the table is
-- rebuilt with its rows copied.

CREATE TABLE notify_outbox_new (
    id                     TEXT PRIMARY KEY,
    purpose                TEXT NOT NULL CHECK (purpose IN ('transactional', 'owner_alert')),
    kind                   TEXT NOT NULL CHECK (kind IN (
        'receipt', 'low_balance', 'session_ending', 'console_5min', 'credit_reminder',
        'cash_variance', 'low_stock', 'credit_overdue', 'large_refund', 'large_discount'
    )),
    order_id               TEXT NULL,
    player_id              TEXT NULL REFERENCES players (player_id),
    consent_policy_version TEXT NOT NULL,
    not_before             INTEGER NOT NULL,
    state                  TEXT NOT NULL
        CHECK (state IN ('queued', 'sent', 'failed', 'skipped', 'expired')),
    rev                    INTEGER NOT NULL,
    created_at             INTEGER NOT NULL,
    updated_at             INTEGER NOT NULL,
    channel                TEXT NULL CHECK (channel IS NULL OR channel IN ('whatsapp', 'sms', 'app')),
    error_code             TEXT NULL,
    reason                 TEXT NULL,
    session_id             TEXT NULL,
    params                 TEXT NULL,
    rearmed_at             INTEGER NULL,
    reminder_day           TEXT NULL,
    source_ref             TEXT NULL,
    CHECK (
        (purpose = 'owner_alert' AND source_ref IS NOT NULL)
        OR (
            purpose = 'transactional'
            AND player_id IS NOT NULL
            AND (
                order_id IS NOT NULL OR session_id IS NOT NULL
                OR (kind = 'credit_reminder' AND reminder_day IS NOT NULL)
            )
        )
    )
);

INSERT INTO notify_outbox_new
    (id, purpose, kind, order_id, player_id, consent_policy_version, not_before, state, rev,
     created_at, updated_at, channel, error_code, reason, session_id, params, rearmed_at,
     reminder_day)
SELECT id, purpose, kind, order_id, player_id, consent_policy_version, not_before, state, rev,
       created_at, updated_at, channel, error_code, reason, session_id, params, rearmed_at,
       reminder_day
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
CREATE UNIQUE INDEX notify_outbox_alert_once ON notify_outbox (kind, source_ref)
    WHERE purpose = 'owner_alert';
