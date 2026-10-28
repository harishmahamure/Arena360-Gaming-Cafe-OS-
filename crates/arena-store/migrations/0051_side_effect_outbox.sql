-- The side-effect outbox (Story 5.17). A settle that prints, and a Reprint, queue a `print` row in
-- the same transaction as the sale; the Hub's print task sends it after commit and the result
-- intents (`receipt.printed`, `receipt.print_failed`) close the row. The table is local to the
-- Hub: it is not in the change log and is not replicated, so a standby never prints.
CREATE TABLE side_effect_outbox (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL CHECK (kind IN ('print', 'drawer', 'cec', 'local_notify')),
    ref_id      TEXT NOT NULL,
    payload     TEXT NOT NULL,
    state       TEXT NOT NULL CHECK (state IN ('queued', 'done', 'failed')),
    at          INTEGER NOT NULL,
    finished_at INTEGER NULL,
    error       TEXT NULL
);

CREATE INDEX side_effect_outbox_queued ON side_effect_outbox (at, id) WHERE state = 'queued';
CREATE INDEX side_effect_outbox_ref ON side_effect_outbox (ref_id, kind);
