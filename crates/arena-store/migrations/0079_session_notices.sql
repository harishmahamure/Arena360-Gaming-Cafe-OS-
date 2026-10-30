-- 0079: messages raised by a running session (Story 8.30). `notify_outbox.kind` gains
-- `low_balance`, `session_ending` and `console_5min`. Such a row is about a session and a player,
-- not an order, so `order_id` may be empty and `session_id` names the session; `params` keeps the
-- raw values the drain writes into the template (a JSON object of text). A `low_balance` row also
-- keeps `rearmed_at`: set once the wallet rose above the low-balance level again, so the next fall
-- below it is a new crossing. At most one `session_ending` and one `console_5min` row exists per
-- session, which is why an Extend never sends the message twice. SQLite cannot alter a CHECK, so
-- the table is rebuilt with its rows copied.

CREATE TABLE notify_outbox_new (
    id                     TEXT PRIMARY KEY,
    purpose                TEXT NOT NULL CHECK (purpose IN ('transactional')),
    kind                   TEXT NOT NULL CHECK (kind IN (
        'receipt', 'low_balance', 'session_ending', 'console_5min'
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
    CHECK (order_id IS NOT NULL OR session_id IS NOT NULL)
);

INSERT INTO notify_outbox_new
    (id, purpose, kind, order_id, player_id, consent_policy_version, not_before, state, rev,
     created_at, updated_at, channel, error_code, reason)
SELECT id, purpose, kind, order_id, player_id, consent_policy_version, not_before, state, rev,
       created_at, updated_at, channel, error_code, reason
FROM notify_outbox;

DROP TABLE notify_outbox;
ALTER TABLE notify_outbox_new RENAME TO notify_outbox;

CREATE INDEX notify_outbox_due ON notify_outbox (not_before, id) WHERE state = 'queued';
CREATE INDEX notify_outbox_order ON notify_outbox (order_id, kind);
CREATE UNIQUE INDEX notify_outbox_session_once ON notify_outbox (session_id, kind)
    WHERE kind IN ('session_ending', 'console_5min');
CREATE INDEX notify_outbox_low_balance ON notify_outbox (player_id, id) WHERE kind = 'low_balance';
