-- 0003: idempotency records. One row per idempotency key whose outcome is final: the
-- MessagePack of the `Applied` summary or of the deterministic `AppError`, plus the SHA-256
-- fingerprint of the intent body it was recorded for (a reuse with another body conflicts). Rows older than the
-- 30-day retention are pruned by the Hub's housekeeping task (a maintenance write outside
-- apply(), with no change rows).

CREATE TABLE applied_intents (
    idempotency_key TEXT PRIMARY KEY,
    intent_id       TEXT NOT NULL,
    intent_kind     TEXT NOT NULL,
    fingerprint     TEXT NOT NULL,
    outcome         BLOB NOT NULL,
    applied_at      INTEGER NOT NULL
);

CREATE INDEX applied_intents_applied_at ON applied_intents (applied_at);
