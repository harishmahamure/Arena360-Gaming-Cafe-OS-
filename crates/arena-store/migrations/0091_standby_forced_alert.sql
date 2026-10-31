-- 0091: a forced promote raises the owner alert `standby_forced` (Story 9.22). `notify_outbox.kind`
-- gains it as a sixth kind a Hub sources; the rest of the table is as migration 0081 left it.
-- SQLite cannot alter a CHECK, so the table is rebuilt with its rows copied.

CREATE TABLE notify_outbox_new (
    id                     TEXT PRIMARY KEY,
    purpose                TEXT NOT NULL CHECK (purpose IN ('transactional', 'owner_alert')),
    kind                   TEXT NOT NULL CHECK (kind IN (
        'receipt', 'low_balance', 'session_ending', 'console_5min', 'credit_reminder',
        'cash_variance', 'low_stock', 'credit_overdue', 'large_refund', 'large_discount',
        'standby_forced'
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
