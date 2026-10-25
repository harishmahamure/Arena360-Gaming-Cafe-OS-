CREATE TABLE hub_identity (
    id        INTEGER PRIMARY KEY CHECK (id = 1),
    tenant_id TEXT NOT NULL,
    branch_id TEXT NOT NULL,
    hub_id    TEXT NOT NULL,
    tier      TEXT NOT NULL CHECK (tier IN ('free', 'basic', 'premium')),
    paired_at INTEGER NOT NULL
);

CREATE TABLE licence_cache (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    jwt         TEXT NOT NULL,
    received_at INTEGER NOT NULL
);
