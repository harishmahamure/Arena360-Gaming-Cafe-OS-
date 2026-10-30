-- Command sweeper and epoch fencing (Story 8.20): `expired` joins the status values (SQLite cannot
-- alter a CHECK, so the table is rebuilt; nothing references it) and a result remembers the epoch
-- it was applied at, so a fence move can send commands applied past `fork_seq` back to `pending`.
CREATE TABLE commands_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    branch_id TEXT NOT NULL REFERENCES branches (id),
    idempotency_key TEXT NOT NULL,
    kind TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    payload TEXT NOT NULL,
    issued_by TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'fetched', 'applied', 'rejected', 'expired')),
    error TEXT,
    applied_seq INTEGER,
    created_at INTEGER NOT NULL,
    fetched_at INTEGER,
    resolved_at INTEGER,
    group_id TEXT,
    applied_epoch TEXT,
    UNIQUE (branch_id, idempotency_key)
);

INSERT INTO commands_new (id, branch_id, idempotency_key, kind, schema_version, payload,
    issued_by, status, error, applied_seq, created_at, fetched_at, resolved_at, group_id)
SELECT id, branch_id, idempotency_key, kind, schema_version, payload,
    issued_by, status, error, applied_seq, created_at, fetched_at, resolved_at, group_id
FROM commands;

DROP TABLE commands;
ALTER TABLE commands_new RENAME TO commands;

CREATE INDEX commands_open ON commands (branch_id, id) WHERE status IN ('pending', 'fetched');
CREATE INDEX commands_group ON commands (group_id) WHERE group_id IS NOT NULL;
CREATE INDEX commands_applied_epoch ON commands (branch_id, applied_epoch)
    WHERE applied_epoch IS NOT NULL;
