-- 0078: the Hub's notification drain (Story 8.29). `notify_outbox.state` gains `expired` (a row
-- still unsent after 7 days is never sent), and a row keeps how its send went: `channel` for
-- `notify.sent`, `error_code` for `notify.failed`, `reason` for a skip. `irreversible_effects.kind`
-- gains `notification_sent`: once a message has gone to the player the forward action cannot be
-- undone. SQLite cannot alter a CHECK, so both tables are rebuilt with their rows copied.

CREATE TABLE notify_outbox_new (
    id                     TEXT PRIMARY KEY,
    purpose                TEXT NOT NULL CHECK (purpose IN ('transactional')),
    kind                   TEXT NOT NULL CHECK (kind IN ('receipt')),
    order_id               TEXT NOT NULL,
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
    reason                 TEXT NULL
);

INSERT INTO notify_outbox_new
    (id, purpose, kind, order_id, player_id, consent_policy_version, not_before, state, rev,
     created_at, updated_at)
SELECT id, purpose, kind, order_id, player_id, consent_policy_version, not_before, state, rev,
       created_at, updated_at
FROM notify_outbox;

DROP TABLE notify_outbox;
ALTER TABLE notify_outbox_new RENAME TO notify_outbox;

CREATE INDEX notify_outbox_due ON notify_outbox (not_before, id) WHERE state = 'queued';
CREATE INDEX notify_outbox_order ON notify_outbox (order_id, kind);

CREATE TABLE irreversible_effects_new (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL CHECK (kind IN (
        'receipt_printed', 'drawer_kicked', 'invoice_number_assigned', 'notification_sent'
    )),
    intent_id   TEXT NULL,
    order_id    TEXT NULL,
    recorded_at INTEGER NOT NULL,
    CHECK (intent_id IS NOT NULL OR order_id IS NOT NULL)
);

INSERT INTO irreversible_effects_new (id, kind, intent_id, order_id, recorded_at)
SELECT id, kind, intent_id, order_id, recorded_at FROM irreversible_effects;

DROP TABLE irreversible_effects;
ALTER TABLE irreversible_effects_new RENAME TO irreversible_effects;

CREATE INDEX irreversible_effects_intent ON irreversible_effects (intent_id);
CREATE INDEX irreversible_effects_order ON irreversible_effects (order_id);
