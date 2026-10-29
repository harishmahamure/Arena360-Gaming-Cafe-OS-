-- 0071: KOT rows on the side-effect outbox (Story 7.5). A ticket on a branch with a KOT printer
-- queues a `kot` row (its `ref_id` is the ticket id) in the transaction that opens it; the print
-- task marks it `sending` before the bytes go out, so a restart that finds a `sending` row records
-- it failed for Reprint and never sends it a second time. SQLite cannot change a CHECK in place,
-- so the table is rebuilt with the same columns and indexes.
CREATE TABLE side_effect_outbox_new (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL CHECK (kind IN ('print', 'drawer', 'cec', 'local_notify', 'kot')),
    ref_id      TEXT NOT NULL,
    payload     TEXT NOT NULL,
    state       TEXT NOT NULL CHECK (state IN ('queued', 'sending', 'done', 'failed')),
    at          INTEGER NOT NULL,
    finished_at INTEGER NULL,
    error       TEXT NULL
);

INSERT INTO side_effect_outbox_new (id, kind, ref_id, payload, state, at, finished_at, error)
    SELECT id, kind, ref_id, payload, state, at, finished_at, error FROM side_effect_outbox;

DROP TABLE side_effect_outbox;
ALTER TABLE side_effect_outbox_new RENAME TO side_effect_outbox;

CREATE INDEX side_effect_outbox_queued ON side_effect_outbox (at, id) WHERE state = 'queued';
CREATE INDEX side_effect_outbox_ref ON side_effect_outbox (ref_id, kind);
