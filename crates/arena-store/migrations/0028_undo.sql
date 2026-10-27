ALTER TABLE sessions ADD COLUMN ended_by TEXT NULL;

CREATE TABLE irreversible_effects (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL CHECK (kind IN ('receipt_printed', 'drawer_kicked', 'invoice_number_assigned')),
    intent_id   TEXT NULL,
    order_id    TEXT NULL,
    recorded_at INTEGER NOT NULL,
    CHECK (intent_id IS NOT NULL OR order_id IS NOT NULL)
);

CREATE INDEX irreversible_effects_intent ON irreversible_effects (intent_id);
CREATE INDEX irreversible_effects_order ON irreversible_effects (order_id);
