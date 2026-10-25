CREATE TABLE commands (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    branch_id TEXT NOT NULL REFERENCES branches (id),
    idempotency_key TEXT NOT NULL,
    kind TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    payload TEXT NOT NULL,
    issued_by TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'fetched', 'applied', 'rejected')),
    error TEXT,
    applied_seq INTEGER,
    created_at INTEGER NOT NULL,
    fetched_at INTEGER,
    resolved_at INTEGER,
    UNIQUE (branch_id, idempotency_key)
);

CREATE INDEX commands_open ON commands (branch_id, id) WHERE status IN ('pending', 'fetched');
